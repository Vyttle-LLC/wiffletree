//! Processes as the kernel reports them: reading one, watching what a provider starts, and
//! stopping a process group only once it is confirmed to be the one that was recorded.
//!
//! Both Claude Code and Codex run each tool command as the leader of its own session and
//! process group, so nothing a tool starts is in its provider's group. A process handed to
//! launchd when its shell exits loses its parent but keeps its group, and macOS never gives a
//! new process an id that is still a live group's id. A group that still holds a recorded
//! process has therefore never emptied, and everything in it belongs to it.
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex, MutexGuard},
    time::{Duration, Instant},
};

/// How long a process group may take to exit after SIGTERM before it is killed.
const TERMINATION_GRACE: Duration = Duration::from_secs(1);
/// How often the watcher lists the process table while any provider runs. A Claude Code tool
/// shell that starts a background process and returns lives about 10 ms; the watcher must list
/// it once while it lives, and follows its group from then on. Listing after each sleep costs
/// roughly 2 to 4% of one core in a release build, more the more processes the machine runs,
/// whatever the number of turns; a debug build measured 18 to 20%.
pub(crate) const SCAN_INTERVAL: Duration = Duration::from_millis(4);
/// What one scan learned still holds at the next only while too little time has passed for a
/// process id to have been reused, which takes a full lap of the id counter: that a group whose
/// last known member has gone is still the same group, and that an id listed before is still
/// the same process.
const CONTINUITY: Duration = Duration::from_secs(1);
/// How long a finished turn waits for the other processes of its tree, such as tool-server
/// helpers exiting with their provider, before they count as left running.
const SETTLE: Duration = Duration::from_millis(300);
/// Bounds one provider's tree, so a runaway fork cannot grow the watcher's work without limit.
const MAX_TREE: usize = 4096;
/// How long a provider's group may take to stop on SIGSTOP before the stop looks anyway.
const FREEZE_WAIT: Duration = Duration::from_millis(100);

/// One live process. Process ids are reused; an id and its start time together are not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: i32,
    pub parent: i32,
    pub group: i32,
    /// Microseconds since the epoch.
    pub started: i64,
    pub command: String,
}

#[cfg(target_os = "macos")]
mod os {
    use super::ProcessInfo;
    use std::ffi::{CStr, c_char, c_int, c_void};

    /// The kernel's account of a live process. A zombie has exited.
    fn info(pid: i32) -> Option<libc::proc_bsdinfo> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as c_int;
        let read = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
        };
        (read == size && info.pbi_status != libc::SZOMB).then_some(info)
    }

    /// Whether the process has stopped, as on SIGSTOP. A process stops on its way back to
    /// user mode, so one that has stopped is not part way through a fork.
    pub(crate) fn stopped(pid: i32) -> bool {
        info(pid).is_some_and(|info| info.pbi_status == libc::SSTOP)
    }

    /// The process, unless it has exited or belongs to another user.
    pub(crate) fn read(pid: i32) -> Option<ProcessInfo> {
        let info = info(pid)?;
        let name = |text: &[c_char]| {
            // SAFETY: the kernel ends both names with a NUL inside their arrays.
            let text = unsafe { CStr::from_ptr(text.as_ptr()) };
            text.to_string_lossy().into_owned()
        };
        let command = match name(&info.pbi_name) {
            long if !long.is_empty() => long,
            _ => name(&info.pbi_comm),
        };
        Some(ProcessInfo {
            pid: info.pbi_pid as i32,
            parent: info.pbi_ppid as i32,
            group: info.pbi_pgid as i32,
            started: (info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec) as i64,
            command,
        })
    }

    /// Fills a growing buffer through one of the `proc_list*` calls, which return how many
    /// ids they wrote.
    fn list(call: impl Fn(*mut c_void, c_int) -> c_int) -> Vec<i32> {
        let mut pids = vec![0i32; 1024];
        loop {
            let bytes = (pids.len() * std::mem::size_of::<i32>()) as c_int;
            let count = call(pids.as_mut_ptr().cast(), bytes).max(0) as usize;
            if count < pids.len() {
                pids.truncate(count);
                return pids;
            }
            pids.resize(pids.len() * 2, 0);
        }
    }

    /// Every process id, in one call.
    pub(crate) fn all() -> Vec<i32> {
        list(|buffer, size| unsafe { libc::proc_listallpids(buffer, size) })
    }

    pub(crate) fn children(pid: i32) -> Vec<i32> {
        list(|buffer, size| unsafe { libc::proc_listchildpids(pid, buffer, size) })
    }

    pub(crate) fn in_group(group: i32) -> Vec<i32> {
        list(|buffer, size| unsafe { libc::proc_listpgrppids(group, buffer, size) })
    }
}

/// Without a process table to read, nothing is ever confirmed, so nothing is ever signalled.
#[cfg(not(target_os = "macos"))]
mod os {
    use super::ProcessInfo;

    pub(crate) fn read(_pid: i32) -> Option<ProcessInfo> {
        None
    }
    pub(crate) fn stopped(_pid: i32) -> bool {
        false
    }
    pub(crate) fn all() -> Vec<i32> {
        vec![]
    }
    pub(crate) fn children(_pid: i32) -> Vec<i32> {
        vec![]
    }
    pub(crate) fn in_group(_group: i32) -> Vec<i32> {
        vec![]
    }
}

pub(crate) use os::read;

/// A process's start time in microseconds since the epoch.
pub(crate) fn process_start(pid: i32) -> Option<i64> {
    read(pid).map(|process| process.started)
}

/// The live processes now in the group.
pub(crate) fn group_members(group: i32) -> Vec<ProcessInfo> {
    os::in_group(group)
        .into_iter()
        .filter_map(read)
        .filter(|process| process.group == group)
        .collect()
}

/// Whether the group has a live process this user may signal; the probe delivers nothing. A
/// group whose only members are zombies answers EPERM.
pub(crate) fn group_runs(group: i32) -> bool {
    unsafe { libc::killpg(group, 0) == 0 }
}

/// Sends SIGTERM and waits out the grace period while live members remain. A frozen process
/// acts on SIGTERM only once continued, so the group is continued after it: one frozen by a
/// host that died before it could stop it is stopped like any other.
pub(crate) fn terminate(group: i32) {
    unsafe {
        libc::killpg(group, libc::SIGTERM);
    }
    signal(group, libc::SIGCONT);
}

pub(crate) fn kill(group: i32) {
    signal(group, libc::SIGKILL);
}

fn signal(group: i32, signal: i32) {
    unsafe {
        libc::killpg(group, signal);
    }
    let deadline = Instant::now() + TERMINATION_GRACE;
    while group_runs(group) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A process group stopped with SIGSTOP. Dropping it continues the group, so a stop that fails
/// part way does not leave the group frozen.
struct Frozen(i32);

impl Frozen {
    /// Returns once every member has stopped, or after `FREEZE_WAIT` if one is slow to. A
    /// running group whose members cannot be read, as during an exec, has not stopped.
    fn new(group: i32) -> Self {
        unsafe {
            libc::killpg(group, libc::SIGSTOP);
        }
        let deadline = Instant::now() + FREEZE_WAIT;
        let all_stopped = || match group_members(group) {
            members if members.is_empty() => !group_runs(group),
            members => members.iter().all(|p| os::stopped(p.pid)),
        };
        while !all_stopped() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_micros(100));
        }
        Self(group)
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        unsafe {
            libc::killpg(self.0, libc::SIGCONT);
        }
    }
}

/// Stops a provider's own group, whose leader the caller keeps unreaped: SIGTERM, the grace
/// period, then SIGKILL. A tool the provider forked a moment ago into a session of its own
/// would be orphaned by the stop before the watcher had listed it. So the group is frozen
/// first and `look` called while it is: anything a member forked before the freeze is still
/// that member's child, and the look adopts it into the run's tree.
pub(crate) fn stop_provider_group(group: i32, look: impl FnOnce()) {
    {
        let _frozen = Frozen::new(group);
        look();
        terminate(group);
    }
    unsafe {
        libc::killpg(group, libc::SIGKILL);
    }
}

/// The group as a signal target, while it runs. Ids 0 and 1 never name an agent's group.
fn running_group(group: i64) -> Option<i32> {
    i32::try_from(group)
        .ok()
        .filter(|group| *group > 1 && group_runs(*group))
}

/// Whether a recorded process group still has a live process.
pub(crate) fn recorded_group_runs(group: i64) -> bool {
    running_group(group).is_some()
}

/// A recorded member of a process group: its process id and start time.
pub(crate) type Member = (i64, i64);

#[cfg(test)]
thread_local! {
    /// How many recorded members this thread has probed, for tests that bound the work.
    pub(crate) static PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The recorded member's process, while it lives. One that has exited never comes back: its
/// id can be given out again, but never with its start time.
pub(crate) fn recorded_alive((pid, started): Member) -> Option<ProcessInfo> {
    #[cfg(test)]
    PROBES.with(|probes| probes.set(probes.get() + 1));
    i32::try_from(pid)
        .ok()
        .and_then(read)
        .filter(|process| process.started == started)
}

/// The processes now in the group, if one of the recorded members is still alive in it.
/// Everything in a group that never emptied belongs to it.
pub(crate) fn confirmed_members(group: i64, recorded: &[Member]) -> Option<Vec<ProcessInfo>> {
    let group = running_group(group)?;
    let alive = |member: &Member| recorded_alive(*member).is_some_and(|p| p.group == group);
    let members = recorded.iter().any(alive).then(|| group_members(group));
    // The member seen alive can have exited before the group was listed.
    members.filter(|members| !members.is_empty())
}

/// What became of a recorded process group the host set out to stop.
#[derive(Debug, PartialEq)]
pub(crate) enum Stop {
    /// Nothing of the group ran any more; nothing was signalled.
    Gone,
    /// It was confirmed and has been stopped; these were its processes.
    Stopped(Vec<ProcessInfo>),
    /// It was confirmed and signalled, but part of it still runs: processes that joined it
    /// after it was confirmed, which nothing confirms a second signal for, or ones that
    /// outlast SIGKILL.
    Signalled,
    /// It still runs. No recorded member was alive in it, so it was not signalled.
    Running,
}

/// Stops a recorded process group: SIGTERM, the grace period, then SIGKILL. Nothing keeps a
/// group's id from being reused once the group has emptied, so each signal goes out only
/// while a recorded member, or a process confirmed with it, is still alive in the group. The
/// leader need not be.
pub(crate) fn stop_group(group: i64, recorded: &[Member]) -> Stop {
    let Some(target) = running_group(group) else {
        return Stop::Gone;
    };
    let Some(processes) = confirmed_members(group, recorded) else {
        // It may have emptied since the first look.
        return match group_runs(target) {
            true => Stop::Running,
            false => Stop::Gone,
        };
    };
    terminate(target);
    let confirmed: Vec<Member> = processes
        .iter()
        .map(|process| (i64::from(process.pid), process.started))
        .collect();
    if group_runs(target) && confirmed_members(group, &confirmed).is_some() {
        kill(target);
    }
    if group_runs(target) {
        Stop::Signalled
    } else {
        Stop::Stopped(processes)
    }
}

/// What the watcher saw of one provider's tree since it was last asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sighting {
    /// A process of the tree, when first seen and whenever its group or command changes.
    Member(ProcessInfo),
    /// Every process of this group has exited.
    GroupExited(i32),
}

/// One provider's process and everything descended from it that is still alive, whatever
/// process group each is in and whether or not its parent has exited.
#[derive(Default)]
struct Tree {
    /// The provider's process id.
    root: i32,
    /// The run the provider belongs to, once a `Watch` holds the tree.
    run: String,
    /// The run's thread has taken its last sightings. The tree stays, so `followed_now` still
    /// names what the run started, until the host has stored those sightings and releases it.
    handed_over: bool,
    members: HashMap<i32, ProcessInfo>,
    pending: Vec<Sighting>,
    /// Groups a taken sighting has named and no taken sighting has ended.
    told: HashSet<i32>,
}

impl Tree {
    fn of(root: i32) -> Self {
        let mut tree = Self {
            root,
            ..Self::default()
        };
        let mut next = vec![root];
        while let Some(pid) = next.pop() {
            if let Some(process) = read(pid)
                && tree.adopt(process)
            {
                next.extend(os::children(pid));
            }
        }
        tree
    }

    /// Whether the process is now in the tree.
    fn adopt(&mut self, process: ProcessInfo) -> bool {
        if self.members.contains_key(&process.pid) {
            return true;
        }
        if self.members.len() >= MAX_TREE {
            return false;
        }
        self.pending.push(Sighting::Member(process.clone()));
        self.members.insert(process.pid, process);
        true
    }

    fn holds_group(&self, group: i32) -> bool {
        self.members.values().any(|member| member.group == group)
    }

    /// Adopts a process whose parent is in the tree, or whose group is one a member is in.
    fn adopt_relative(&mut self, process: &ProcessInfo) -> bool {
        (self.members.contains_key(&process.parent) || self.holds_group(process.group))
            && self.adopt(process.clone())
    }

    /// Reads every member again: one may have exited, and a child often changes its group or
    /// its command just after it is forked. A group that lost its last known member is listed
    /// once, when `continuous`: whatever is still in it never left the tree. Otherwise its id
    /// may have been given out again, so a group that still runs is neither adopted nor said
    /// to have exited: its record stays for the host to stop or to ask the human about.
    fn refresh(&mut self, continuous: bool) {
        let mut left = HashSet::new();
        self.members.retain(|pid, known| match read(*pid) {
            Some(now) if now.started == known.started => {
                if now.group != known.group {
                    left.insert(known.group);
                }
                // An orphan's new parent is nothing the record keeps.
                if (now.group, &now.command) != (known.group, &known.command) {
                    self.pending.push(Sighting::Member(now.clone()));
                }
                *known = now;
                true
            }
            _ => {
                left.insert(known.group);
                false
            }
        });
        for group in left {
            if self.holds_group(group) {
                continue;
            }
            if !group_runs(group) {
                self.pending.push(Sighting::GroupExited(group));
            } else if continuous {
                for process in group_members(group) {
                    self.adopt(process);
                }
            }
        }
    }

    /// The sightings since the last take. A group that came and went in between, as most
    /// commands do, is not worth telling.
    fn take(&mut self) -> Vec<Sighting> {
        let mut sightings = std::mem::take(&mut self.pending);
        let exited: HashSet<i32> = sightings
            .iter()
            .filter_map(|sighting| match sighting {
                Sighting::GroupExited(group) if !self.holds_group(*group) => Some(*group),
                _ => None,
            })
            .collect();
        sightings.retain(|sighting| match sighting {
            Sighting::Member(process) => !exited.contains(&process.group),
            Sighting::GroupExited(group) => self.told.remove(group),
        });
        for sighting in &sightings {
            if let Sighting::Member(process) = sighting {
                self.told.insert(process.group);
            }
        }
        sightings
    }
}

/// Follows every running provider's tree from one listing of the process table per scan.
#[derive(Default)]
struct Watcher {
    trees: HashMap<u64, Tree>,
    next_key: u64,
    /// The process ids of the last listing, in order; only ids not in it are read.
    listed: Vec<i32>,
    scanned_at: Option<Instant>,
    /// How many times the process table has been listed.
    listings: u64,
}

impl Watcher {
    fn add(&mut self, root: i32) -> u64 {
        self.next_key += 1;
        self.trees.insert(self.next_key, Tree::of(root));
        self.next_key
    }

    fn scan(&mut self) {
        let continuous = self.scanned_at.is_some_and(|at| at.elapsed() < CONTINUITY);
        self.scanned_at = Some(Instant::now());
        if !continuous {
            // After a pause, such as no run being watched, any id may name a new process.
            self.listed.clear();
        }
        // Members first, so a new process is matched against what is alive now.
        for tree in self.trees.values_mut() {
            tree.refresh(continuous);
        }
        let mut pids = os::all();
        pids.sort_unstable();
        self.listings += 1;
        let mut fresh: Vec<ProcessInfo> = pids
            .iter()
            .filter(|pid| self.listed.binary_search(pid).is_err())
            .filter_map(|pid| read(*pid))
            .collect();
        // A parent and its child can both be new; the child is adopted on the pass after.
        loop {
            let unplaced = fresh.len();
            fresh.retain(|process| {
                !self
                    .trees
                    .values_mut()
                    .any(|tree| tree.adopt_relative(process))
            });
            if fresh.len() == unplaced {
                break;
            }
        }
        self.listed = pids;
        // Nobody takes a handed-over tree's sightings, and one with nothing left to follow has
        // nothing left to name.
        self.trees.retain(|_, tree| {
            if tree.handed_over {
                tree.pending.clear();
            }
            !(tree.handed_over && tree.members.is_empty())
        });
    }

    fn take(&mut self, key: u64) -> Vec<Sighting> {
        self.trees.get_mut(&key).map_or_else(Vec::new, Tree::take)
    }
}

/// The one watcher every run shares, and whether its thread is running.
static SHARED: LazyLock<Mutex<(Watcher, bool)>> = LazyLock::new(Mutex::default);

fn shared() -> MutexGuard<'static, (Watcher, bool)> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

/// Scans on a clock while any tree is watched, however many are.
fn watch_until_idle() {
    loop {
        std::thread::sleep(SCAN_INTERVAL);
        let mut shared = shared();
        if shared.0.trees.is_empty() {
            shared.1 = false;
            return;
        }
        shared.0.scan();
    }
}

/// Has the watcher look now, whatever its clock. What it finds goes out with each run's next
/// sightings.
pub(crate) fn look() {
    shared().0.scan();
}

/// Every process the watcher follows for the provider of `run` after a look made now, apart
/// from those in the provider's own group, which its turn stops itself. This is for a caller
/// that cannot wait for the run's thread, such as a stop. It is everything followed, not only
/// what is new, and it answers until the host releases the run: a sighting the thread has
/// taken may not have reached the store, and neither may the run's last ones.
pub(crate) fn followed_now(run: &str) -> Vec<ProcessInfo> {
    let mut shared = shared();
    shared.0.scan();
    let mut trees = shared.0.trees.values_mut();
    let Some(tree) = trees.find(|tree| tree.run == run) else {
        return vec![];
    };
    // The provider leads its group, so the group's id is the provider's process id.
    let started = |process: &&ProcessInfo| process.group != tree.root;
    let followed: Vec<ProcessInfo> = tree.members.values().filter(started).cloned().collect();
    // The caller now knows these groups, so it is told when one of them has exited.
    tree.told
        .extend(followed.iter().map(|process| process.group));
    followed
}

/// Lets go of a run's tree. The host calls this once it has stored everything the run's
/// thread sent it, the last sightings included: from then on the store answers for the run.
pub(crate) fn release(run: &str) {
    shared().0.trees.retain(|_, tree| tree.run != run);
}

/// One provider run's place in the shared watcher, held by the run's thread. The tree itself
/// outlives it: the watcher keeps it until the host releases the run.
pub(crate) struct Watch(u64);

impl Watch {
    pub(crate) fn start(root: i32, run: &str) -> Self {
        let mut shared = shared();
        let key = shared.0.add(root);
        if let Some(tree) = shared.0.trees.get_mut(&key) {
            tree.run = run.into();
        }
        if !shared.1 {
            shared.1 = true;
            std::thread::spawn(watch_until_idle);
        }
        Self(key)
    }

    /// What the watcher has seen of this tree since it was last asked.
    pub(crate) fn seen(&self) -> Vec<Sighting> {
        shared().0.take(self.0)
    }

    /// The last sightings, after one more look. With `settle`, first gives the tree's
    /// remaining processes a moment to exit, so only what stays is reported as running.
    pub(crate) fn finish(self, settle: bool) -> Vec<Sighting> {
        let deadline = Instant::now() + SETTLE;
        loop {
            {
                let mut shared = shared();
                shared.0.scan();
                let empty = |tree: &Tree| tree.members.is_empty();
                if !settle
                    || Instant::now() >= deadline
                    || shared.0.trees.get(&self.0).is_none_or(empty)
                {
                    return shared.0.take(self.0);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Watch {
    /// Hands the tree over: the thread takes no more of its sightings, and whoever holds the
    /// last ones has yet to store them.
    fn drop(&mut self) {
        if let Some(tree) = shared().0.trees.get_mut(&self.0) {
            tree.handed_over = true;
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
pub(crate) mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write as _},
        os::unix::process::{CommandExt, ExitStatusExt},
        process::{Child, ChildStdin, Command, Stdio},
    };

    /// Kills the processes a test started, by their saved ids, also when it fails.
    pub(crate) struct Started(pub(crate) Vec<i32>);
    impl Drop for Started {
        fn drop(&mut self) {
            for pid in &self.0 {
                unsafe {
                    libc::kill(*pid, libc::SIGKILL);
                }
            }
        }
    }

    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn member(process: &ProcessInfo) -> Member {
        (i64::from(process.pid), process.started)
    }

    /// The ids of the processes a stop stopped; anything else fails the test.
    fn stopped(stop: Stop) -> Vec<i32> {
        match stop {
            Stop::Stopped(processes) => processes.iter().map(|p| p.pid).collect(),
            other => panic!("not stopped: {other:?}"),
        }
    }

    /// A process in its own group, as a tool shell runs, that says `ready` once `script` has
    /// set it up.
    fn in_own_group(script: &str) -> Child {
        let mut child = Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut ready = [0; 6];
        child
            .stdout
            .as_mut()
            .unwrap()
            .read_exact(&mut ready)
            .unwrap();
        assert_eq!(&ready, b"ready\n");
        child
    }

    #[test]
    fn a_process_is_read_with_its_parent_group_start_and_command() {
        let mut child = in_own_group("echo ready; exec sleep 30");
        let _started = Started(vec![child.id() as i32]);
        let pid = child.id() as i32;
        until("the exec", || {
            read(pid).is_some_and(|p| p.command == "sleep")
        });
        let process = read(pid).unwrap();
        assert_eq!(process.parent, std::process::id() as i32);
        assert_eq!(process.group, pid);
        assert!(process.started > 0);
        assert_eq!(group_members(pid), [process]);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(read(pid), None);
        assert_eq!(group_members(pid), []);
    }

    #[test]
    fn a_group_is_signalled_only_while_a_recorded_member_is_alive_in_it() {
        let mut child = in_own_group("echo ready; exec sleep 30");
        let _started = Started(vec![child.id() as i32]);
        let group = i64::from(child.id());
        let process = read(child.id() as i32).unwrap();

        // The same id with another start time is another process; so is a group with no
        // recorded member, and a recorded member that is in another group.
        assert_eq!(
            stop_group(group, &[(group, process.started + 1)]),
            Stop::Running
        );
        assert_eq!(stop_group(group, &[]), Stop::Running);
        let own = read(std::process::id() as i32).unwrap();
        assert_eq!(stop_group(group, &[member(&own)]), Stop::Running);
        assert_eq!(child.try_wait().unwrap(), None);
        assert!(recorded_group_runs(group));

        assert_eq!(
            stopped(stop_group(group, &[member(&process)])),
            [process.pid]
        );
        assert!(!recorded_group_runs(group));
        assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGTERM));
        assert_eq!(stop_group(group, &[member(&process)]), Stop::Gone);
    }

    #[test]
    fn a_group_that_ignores_sigterm_is_killed_after_the_grace_period() {
        let mut child = in_own_group("trap '' TERM; echo ready; exec sleep 30");
        let _started = Started(vec![child.id() as i32]);
        let process = read(child.id() as i32).unwrap();

        let killed = stop_group(i64::from(child.id()), &[member(&process)]);
        assert_eq!(stopped(killed), [process.pid]);
        assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    /// A provider that exits on SIGTERM beside a helper that ignores it, in one group from
    /// the start. The helper needs no signal to end: it lives until the child's input closes,
    /// as when the child is dropped.
    fn with_survivor() -> Child {
        in_own_group("exec 3<&0; (trap '' TERM; echo ready; exec cat <&3) & exec sleep 30")
    }

    /// A provider that, when told to stop, starts a helper that ignores SIGTERM and exits: the
    /// helper joins the group after the host confirmed it. It lives until the child's input
    /// closes. Returns once the provider waits on its own child, so a signal finds it settled.
    pub(crate) fn with_late_survivor() -> Child {
        let child = in_own_group(
            "exec 3<&0; trap '(trap \"\" TERM; exec cat <&3) >/dev/null & exit 0' TERM; sleep 30 & echo ready; wait",
        );
        until("the provider's child to be exec'd", || {
            let members = group_members(child.id() as i32);
            members.iter().any(|member| member.command == "sleep")
        });
        child
    }

    #[test]
    fn a_member_that_ignores_sigterm_is_killed_though_the_leader_exited_on_it() {
        let mut child = with_survivor();
        let _started = Started(vec![child.id() as i32]);
        let group = i64::from(child.id());
        let leader = read(child.id() as i32).unwrap();
        let members = group_members(child.id() as i32);
        assert_eq!(members.len(), 2, "{members:?}");

        // Only the leader is recorded, as an earlier version recorded it. It exits on SIGTERM,
        // but the helper was in the group when the leader confirmed it, so it is the agent's.
        let stop = stop_group(group, &[member(&leader)]);
        assert_eq!(stopped(stop).len(), 2);
        assert!(!recorded_group_runs(group));
        assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGTERM));
    }

    #[test]
    fn a_group_joined_after_it_was_confirmed_is_signalled_once_and_reported_so() {
        let mut child = with_late_survivor();
        let _started = Started(vec![child.id() as i32]);
        let group = i64::from(child.id());
        let leader = read(child.id() as i32).unwrap();

        // Everything the host confirmed exits on SIGTERM; nothing confirms what joined after.
        assert_eq!(stop_group(group, &[member(&leader)]), Stop::Signalled);
        assert!(recorded_group_runs(group));
        // The record still names only processes that have gone, so nothing is signalled again.
        assert_eq!(stop_group(group, &[member(&leader)]), Stop::Running);

        drop(child.stdin.take());
        until("the survivor to end with its input", || {
            !recorded_group_runs(group)
        });
        child.wait().unwrap();
    }

    #[test]
    fn a_group_left_frozen_is_stopped_with_sigterm() {
        // A provider that handles SIGTERM, as Claude Code does. macOS ends a stopped process
        // on a SIGTERM it does not handle, but a handler runs only once the process continues.
        let mut child = in_own_group("trap 'exit 3' TERM; echo ready; sleep 30 & wait");
        let pid = child.id() as i32;
        let _started = Started(vec![pid]);
        until("the provider's child to be exec'd", || {
            group_members(pid)
                .iter()
                .any(|member| member.command == "sleep")
        });
        // As when a host dies between freezing a provider's group and stopping it: the next
        // startup stops the group it recorded.
        unsafe {
            libc::killpg(pid, libc::SIGSTOP);
        }
        until("the group to stop", || {
            group_members(pid)
                .iter()
                .all(|member| os::stopped(member.pid))
        });

        let leader = read(pid).unwrap();
        assert_eq!(
            stopped(stop_group(i64::from(pid), &[member(&leader)])).len(),
            2
        );
        assert_eq!(child.wait().unwrap().code(), Some(3));
    }

    /// A stand-in provider that, once it reads a line, forks a tool that leads a session of
    /// its own, as Claude Code and Codex start a tool command, says the tool's id and waits.
    const FORKS_A_TOOL: &str = "\
import os, sys
sys.stdin.readline()
tool = os.fork()
if tool == 0:
    os.setsid()
    os.execvp('sleep', ['sleep', '30'])
print(tool, flush=True)
sys.stdin.readline()
";

    #[test]
    fn a_tool_forked_just_before_a_cancel_is_found_by_the_stops_look() {
        let mut provider = Command::new("python3")
            .args(["-c", FORKS_A_TOOL])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let group = provider.id() as i32;
        let mut started = Started(vec![group]);
        let mut watcher = Watcher::default();
        let key = watcher.add(group);
        watcher.take(key);

        writeln!(provider.stdin.as_mut().unwrap(), "fork").unwrap();
        let mut line = String::new();
        BufReader::new(provider.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let tool: i32 = line.trim().parse().unwrap();
        started.0.push(tool);
        until("the tool to lead its own session", || {
            read(tool).is_some_and(|p| p.group == tool && p.command == "sleep")
        });

        // The cancel lands before the watcher has listed the tool. The stop orphans it, but
        // its look comes while the provider is frozen and still the tool's parent.
        let mut frozen = false;
        stop_provider_group(group, || {
            frozen = os::stopped(group);
            watcher.scan();
        });
        assert!(frozen, "the look came while the provider was frozen");
        assert_eq!(provider.wait().unwrap().signal(), Some(libc::SIGTERM));
        assert!(read(tool).is_some_and(|p| p.parent == 1), "orphaned");
        // The provider itself may be named again: a framework Python, as `/usr/bin/python3`
        // starts, re-execs itself under the same id, which changes its command.
        let seen = members(&watcher.take(key));
        let started: Vec<_> = seen.into_iter().filter(|(g, _)| *g != group).collect();
        assert_eq!(started, [(tool, tool)]);
    }

    #[test]
    fn a_stop_whose_look_fails_leaves_the_group_running() {
        let mut child = in_own_group("echo ready; exec sleep 30");
        let pid = child.id() as i32;
        let _started = Started(vec![pid]);
        until("the exec", || {
            read(pid).is_some_and(|p| p.command == "sleep")
        });

        let mut frozen = false;
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stop_provider_group(pid, || {
                frozen = os::stopped(pid);
                panic!("the look failed");
            })
        }));
        assert!(failed.is_err());
        assert!(frozen);
        until("the group to be continued", || !os::stopped(pid));
        assert_eq!(child.try_wait().unwrap(), None);
    }

    /// A tool shell that started `sleep` in the background, said its id and exited: the
    /// orphan keeps the shell's group, which now has no leader.
    fn orphan() -> (i32, ProcessInfo, Started) {
        let mut shell = Command::new("/bin/sh")
            .args(["-c", "sleep 30 >/dev/null 2>&1 & echo $!"])
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let group = shell.id() as i32;
        let mut pid = String::new();
        BufReader::new(shell.stdout.take().unwrap())
            .read_line(&mut pid)
            .unwrap();
        let pid: i32 = pid.trim().parse().unwrap();
        let started = Started(vec![pid]);
        shell.wait().unwrap();
        let orphan = read(pid).unwrap();
        assert_eq!((orphan.group, orphan.parent), (group, 1));
        assert_eq!(read(group), None, "the leader has exited");
        (group, orphan, started)
    }

    #[test]
    fn orphans_are_stopped_though_their_group_leader_has_exited() {
        let (group, orphan, _started) = orphan();

        let stop = stop_group(i64::from(group), &[member(&orphan)]);
        assert_eq!(stopped(stop), [orphan.pid]);
        assert!(!recorded_group_runs(i64::from(group)));
    }

    #[test]
    fn a_group_whose_recorded_members_are_gone_is_not_signalled() {
        // The recorded group has emptied, and its id now names an unrelated group.
        let mut unrelated = in_own_group("echo ready; exec sleep 30");
        let _started = Started(vec![unrelated.id() as i32]);
        let (_, gone, _orphan) = orphan();
        unsafe {
            libc::kill(gone.pid, libc::SIGKILL);
        }
        until("the recorded member to exit", || read(gone.pid).is_none());

        let group = i64::from(unrelated.id());
        assert_eq!(stop_group(group, &[member(&gone)]), Stop::Running);
        assert_eq!(unrelated.try_wait().unwrap(), None, "never signalled");
    }

    /// A stand-in provider: a shell that runs each line it reads, with job control, so every
    /// background command leads a process group of its own as a tool command does.
    pub(crate) struct Provider {
        child: Child,
        input: ChildStdin,
        output: BufReader<std::process::ChildStdout>,
    }
    impl Provider {
        pub(crate) fn start() -> Self {
            let mut child = Command::new("/bin/sh")
                .args(["-c", "set -m; while read -r line; do eval \"$line\"; done"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .unwrap();
            let input = child.stdin.take().unwrap();
            let output = BufReader::new(child.stdout.take().unwrap());
            let mut provider = Self {
                child,
                input,
                output,
            };
            // Once it answers it has been exec'd, so its command no longer changes.
            assert_eq!(provider.run("echo ready"), "ready");
            provider
        }
        pub(crate) fn pid(&self) -> i32 {
            self.child.id() as i32
        }
        /// Runs `command` and returns the line it printed.
        pub(crate) fn run(&mut self, command: &str) -> String {
            writeln!(self.input, "{command}").unwrap();
            let mut line = String::new();
            self.output.read_line(&mut line).unwrap();
            line.trim().to_owned()
        }
    }
    impl Drop for Provider {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// A named pipe a tool shell waits on, so the test decides when the shell goes on.
    struct Gate(tempfile::TempDir);
    impl Gate {
        fn new() -> Self {
            let gate = Self(tempfile::tempdir().unwrap());
            let path = std::ffi::CString::new(gate.path()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            gate
        }
        fn path(&self) -> String {
            self.0.path().join("gate").to_string_lossy().into_owned()
        }
        fn open(&self) {
            std::fs::write(self.path(), "go\n").unwrap();
        }
    }

    fn members(sightings: &[Sighting]) -> Vec<(i32, i32)> {
        sightings
            .iter()
            .filter_map(|sighting| match sighting {
                Sighting::Member(process) => Some((process.group, process.pid)),
                Sighting::GroupExited(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_tool_shell_in_its_own_group_is_followed_after_it_exits() {
        let mut provider = Provider::start();
        let gate = Gate::new();
        let mut watcher = Watcher::default();
        let key = watcher.add(provider.pid());
        assert_eq!(
            members(&watcher.take(key)),
            [(provider.pid(), provider.pid())]
        );

        // A tool shell in its own group starts `sleep` in the background, says both ids and
        // waits at the gate.
        let line = provider.run(&format!(
            "sh -c 'sleep 30 >/dev/null 2>&1 & echo $$ $!; read -r go < \"$0\"' {} & :",
            gate.path()
        ));
        let (shell, sleep) = line.split_once(' ').unwrap();
        let (shell, sleep): (i32, i32) = (shell.parse().unwrap(), sleep.parse().unwrap());
        let _started = Started(vec![shell, sleep]);
        until("the exec", || {
            read(sleep).is_some_and(|p| p.command == "sleep")
        });
        watcher.scan();
        let seen = members(&watcher.take(key));
        assert!(seen.contains(&(shell, shell)), "{seen:?}");
        assert!(seen.contains(&(shell, sleep)), "{seen:?}");

        // The shell exits; its orphan stays in the tree and in the group.
        gate.open();
        until("the tool shell to exit", || read(shell).is_none());
        watcher.scan();
        assert_eq!(watcher.take(key), []);
        assert!(watcher.trees[&key].members.contains_key(&sleep));

        // Once the orphan has gone too, the group is reported gone.
        unsafe {
            libc::kill(sleep, libc::SIGKILL);
        }
        until("the orphan to exit", || read(sleep).is_none());
        watcher.scan();
        assert_eq!(watcher.take(key), [Sighting::GroupExited(shell)]);
    }

    #[test]
    fn a_child_whose_shell_exited_between_two_scans_is_found_in_its_group() {
        let mut provider = Provider::start();
        let gate = Gate::new();
        let mut watcher = Watcher::default();
        let key = watcher.add(provider.pid());
        // A tool shell in its own group says its id and waits at the gate.
        let shell: i32 = provider
            .run(&format!(
                "sh -c 'echo $$; read -r go < \"$0\"; sleep 30 >/dev/null 2>&1 & echo $!' {} & :",
                gate.path()
            ))
            .parse()
            .unwrap();
        let mut started = Started(vec![shell]);
        watcher.scan();
        assert!(members(&watcher.take(key)).contains(&(shell, shell)));

        // Between two scans the shell starts its child and exits: the child was never anyone's
        // descendant at a scan, but it is in the group the shell was seen leading.
        gate.open();
        let mut line = String::new();
        provider.output.read_line(&mut line).unwrap();
        let sleep: i32 = line.trim().parse().unwrap();
        started.0.push(sleep);
        until("the tool shell to exit", || read(shell).is_none());
        watcher.scan();
        assert_eq!(members(&watcher.take(key)), [(shell, sleep)]);
    }

    #[test]
    fn a_group_that_runs_on_after_the_watcher_paused_is_not_said_to_have_exited() {
        let mut provider = Provider::start();
        let gate = Gate::new();
        let mut watcher = Watcher::default();
        let key = watcher.add(provider.pid());
        let shell: i32 = provider
            .run(&format!(
                "sh -c 'echo $$; read -r go < \"$0\"; sleep 30 >/dev/null 2>&1 & echo $!' {} & :",
                gate.path()
            ))
            .parse()
            .unwrap();
        let mut started = Started(vec![shell]);
        watcher.scan();
        assert!(members(&watcher.take(key)).contains(&(shell, shell)));

        // The watcher does not look for over a second, as when the machine stalls. Meanwhile
        // the shell starts a child nobody saw and exits.
        gate.open();
        let mut line = String::new();
        provider.output.read_line(&mut line).unwrap();
        let sleep: i32 = line.trim().parse().unwrap();
        started.0.push(sleep);
        until("the tool shell to exit", || read(shell).is_none());
        watcher.scanned_at = Instant::now().checked_sub(CONTINUITY * 2);
        watcher.scan();

        // After so long the group's id could be another group's, so its member is not taken
        // for the agent's. But something has that id, so the group is not said to have exited:
        // its record stays, for the host to stop or to ask the human about.
        assert_eq!(watcher.take(key), []);
        assert!(!watcher.trees[&key].members.contains_key(&sleep));

        // A group nothing is in any more has exited, however long the watcher was away.
        let gone = provider
            .run("sh -c 'echo $$; exec sleep 30' >&1 & :")
            .parse()
            .unwrap();
        started.0.push(gone);
        watcher.scan();
        assert!(members(&watcher.take(key)).contains(&(gone, gone)));
        unsafe {
            libc::kill(gone, libc::SIGKILL);
        }
        until("the command to exit", || read(gone).is_none());
        watcher.scanned_at = Instant::now().checked_sub(CONTINUITY * 2);
        watcher.scan();
        assert_eq!(watcher.take(key), [Sighting::GroupExited(gone)]);
    }

    #[test]
    fn a_look_made_for_the_asking_names_everything_followed_also_what_was_already_taken() {
        let mut provider = Provider::start();
        let run = format!("run-{}", provider.pid());
        let watch = Watch::start(provider.pid(), &run);
        let sleep: i32 = provider
            .run("sleep 30 >/dev/null 2>&1 & echo $!")
            .parse()
            .unwrap();
        let _started = Started(vec![sleep]);

        // No waiting for the watcher's clock or for the run's thread to ask.
        let followed = followed_now(&run);
        let names = |followed: &[ProcessInfo]| -> Vec<(i32, i32)> {
            followed.iter().map(|p| (p.group, p.pid)).collect()
        };
        assert_eq!(names(&followed), [(sleep, sleep)]);

        // The run's thread takes the sighting and has not handed it on: nothing stores it.
        let mut in_transit = vec![];
        until("the thread to take the sighting", || {
            in_transit.extend(members(&watch.seen()));
            in_transit.contains(&(sleep, sleep))
        });
        assert!(read(sleep).is_some());
        // The look still names the process: it is everything followed, not what is new. The
        // provider itself is not named: its own group is its turn's to stop.
        assert_eq!(names(&followed_now(&run)), [(sleep, sleep)]);
        assert_eq!(followed_now("no such run"), []);
    }

    #[test]
    fn a_shell_that_starts_and_exits_between_two_scans_is_never_seen() {
        // The known gap the scan interval is chosen against: nothing links the orphan to the
        // provider once its shell has exited unseen.
        let mut provider = Provider::start();
        let mut watcher = Watcher::default();
        let key = watcher.add(provider.pid());
        watcher.scan();
        watcher.take(key);

        let sleep: i32 = provider
            .run("sh -c 'sleep 30 >/dev/null 2>&1 & echo $!' & wait")
            .parse()
            .unwrap();
        let _started = Started(vec![sleep]);
        until("the shell to exit", || {
            read(sleep).is_some_and(|orphan| orphan.parent == 1)
        });
        assert_ne!(read(sleep).unwrap().group, provider.pid());
        watcher.scan();
        assert_eq!(watcher.take(key), []);

        // A tool shell lives about 10 ms; the watcher has to look at least twice in that time.
        assert!(SCAN_INTERVAL <= Duration::from_millis(5));
    }

    #[test]
    fn a_command_that_came_and_went_between_two_takes_is_not_told() {
        let mut provider = Provider::start();
        let mut watcher = Watcher::default();
        let key = watcher.add(provider.pid());
        watcher.take(key);

        let shell: i32 = provider
            .run("sh -c 'echo $$; exec sleep 30' >&1 & :")
            .parse()
            .unwrap();
        let _started = Started(vec![shell]);
        watcher.scan();
        assert!(watcher.trees[&key].members.contains_key(&shell));
        unsafe {
            libc::kill(shell, libc::SIGKILL);
        }
        until("the command to exit", || read(shell).is_none());
        watcher.scan();
        assert_eq!(watcher.take(key), []);
    }

    #[test]
    fn several_trees_share_one_listing_of_the_process_table() {
        let providers = [Provider::start(), Provider::start(), Provider::start()];
        let mut watcher = Watcher::default();
        let keys: Vec<u64> = providers.iter().map(|p| watcher.add(p.pid())).collect();
        watcher.scan();
        assert_eq!(watcher.listings, 1);
        for (provider, key) in providers.iter().zip(keys) {
            assert_eq!(
                members(&watcher.take(key)),
                [(provider.pid(), provider.pid())]
            );
        }
    }

    #[test]
    fn a_runs_tree_answers_until_the_host_releases_it() {
        let mut provider = Provider::start();
        let run = format!("run-{}", provider.pid());
        let watch = Watch::start(provider.pid(), &run);
        let sleep: i32 = provider
            .run("sleep 30 >/dev/null 2>&1 & echo $!")
            .parse()
            .unwrap();
        let _started = Started(vec![sleep]);
        // The shared watcher's own clock finds it. The run's thread takes the sighting and
        // stores nothing; then the provider exits and the thread makes its last look.
        let mut taken = vec![];
        until("the watcher to see the command", || {
            taken.extend(members(&watch.seen()));
            taken.contains(&(sleep, sleep))
        });
        drop(provider);
        taken.extend(members(&watch.finish(true)));
        assert!(read(sleep).is_some());

        // The thread is done and its last sightings are on their way to the store. Until the
        // host has them the watcher still answers for the run, and names what is running.
        let names = |followed: Vec<ProcessInfo>| -> Vec<(i32, i32)> {
            followed.iter().map(|p| (p.group, p.pid)).collect()
        };
        assert_eq!(names(followed_now(&run)), [(sleep, sleep)]);

        // The host has stored them: from here the store answers.
        release(&run);
        assert_eq!(followed_now(&run), []);
    }

    #[test]
    fn a_handed_over_tree_with_nothing_left_to_follow_is_dropped() {
        let provider = Provider::start();
        let run = format!("run-{}", provider.pid());
        let watch = Watch::start(provider.pid(), &run);
        let key = watch.0;
        watch.finish(false);
        assert!(shared().0.trees.contains_key(&key), "handed over, not gone");

        // Nobody releases this run. Once its last process has exited there is nothing to name.
        drop(provider);
        followed_now(&run);
        assert!(!shared().0.trees.contains_key(&key));
    }
}
