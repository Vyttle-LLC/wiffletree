//! Reclaiming build output: deleting what `build_output::scan` finds for a ticket, and its
//! scratch folder, while keeping its worktree and source. The next build there makes it again.
//!
//! A ticket's build output is reclaimed when the ticket is accepted or closed, when no agent of
//! it had a turn for the idle time, when the build output across tickets is over the budget
//! (least recently used first), when free space on the workspaces disk falls under the soft
//! floor, and when the human asks. A ticket with an agent in a turn, or whose build output a
//! process still uses, is never touched, and neither is anything `scan` calls ambiguous.
//!
//! Every folder goes through the durable deletes of `worktree_deletions`: it is recorded, then
//! renamed aside on its own filesystem, outside the worktree, so the worktree stays as clean as
//! it was. Scanning and measuring run off the host thread; their latest result is kept for a few
//! minutes and answers the disk use read. Under the hard floor, the build gate holds new builds
//! and the human's inbox says so until space returns.
//!
//! Nothing that uses the build output has it moved from under it, as far as can be seen. The
//! scan, at most a few seconds old, is followed on the host thread by a look at the ticket's
//! turns; the deletes are recorded and held; then a worker looks for processes with a file in
//! the folders. It then takes the build gate's lock, so the gate admits no build until the
//! renames are done, and the lock restores take; under both it looks for builds the gate
//! admitted or queued that run in the worktree and, through its own connection to the store,
//! for an agent turn that started, then renames. Once the folders moved it looks for processes
//! again, and moves them all back if one uses them. What remains is a process that is neither
//! an agent's turn nor a build through the gate, and that opens its first file in the folders
//! after the first look for processes and closes it before the second: it has them moved from
//! under it. Removing a ticket's worktree deletes its build output outside the worktree the
//! same way, by a cleanup recorded before the worktree moves, which is dropped if the worktree
//! is back at its path, and kept until every folder of it has moved: a failed attempt is
//! retried with a delete's backoff, told to the human's inbox as a delete's is, and retried
//! again at every start.
use crate::*;
use std::{
    collections::{HashMap, HashSet},
    os::unix::fs::MetadataExt,
    sync::{Arc, Mutex},
};

/// How often free space is looked at while nothing else asks.
const CHECK_MS: i64 = 3 * 60_000;
/// How long a measurement of every ticket's disk use is kept.
const SURVEY_MS: i64 = 5 * 60_000;
/// How old a ticket's scan may be when its build output is moved; an older one is redone.
const FRESH_SCAN_MS: i64 = 5_000;

/// A disk's free and total bytes, or `None` when it cannot be read.
pub type Space = Arc<dyn Fn(&Path) -> Option<(u64, u64)> + Send + Sync>;

/// Where the watch looks: Xcode's DerivedData folder, the folder holding each ticket's scratch
/// folder, and a disk's free space. Tests give their own, so nothing reads the machine's.
#[derive(Clone)]
pub struct DiskRoots {
    pub derived_data: Option<PathBuf>,
    pub scratch_root: PathBuf,
    pub space: Space,
}
impl DiskRoots {
    /// This machine's.
    pub fn machine() -> Self {
        Self {
            derived_data: build_output::xcode_derived_data(),
            scratch_root: std::env::temp_dir(),
            space: Arc::new(disk::space),
        }
    }
    /// Scratch folders in `folder`, no DerivedData, and a disk with 1,000 GB free of 2,000 GB.
    pub fn isolated(folder: &Path) -> Self {
        Self {
            derived_data: None,
            scratch_root: folder.join("scratch"),
            space: Arc::new(|_| Some((1_000 * GB, 2_000 * GB))),
        }
    }
}

/// Why a ticket's build output is reclaimed, as its notice says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    Done,
    Idle,
    /// Over the budget, with the build output across tickets.
    Budget(u64),
    /// Under the soft floor, with the free space.
    SoftFloor(u64),
    Asked,
}

/// One ticket's disk use as last measured.
#[derive(Clone, Debug, Default)]
struct Measured {
    source: u64,
    parts: Vec<DiskPart>,
    left_alone: Vec<DiskPart>,
}
impl Measured {
    fn build_output(&self) -> u64 {
        self.parts.iter().map(|part| part.size).sum()
    }
}

/// Every ticket's disk use, measured at `at`.
#[derive(Clone, Debug, Default)]
struct Survey {
    at: i64,
    tickets: HashMap<String, Measured>,
}

/// What a ticket's scan off the host thread found to reclaim, each folder with its size, or why
/// nothing may be reclaimed now.
type Inspected = Result<Vec<(PathBuf, u64)>, String>;

enum Finished {
    Survey(Survey),
    /// A ticket's scan to reclaim it, with when it ended.
    Inspected(String, Why, Inspected, i64),
    /// A removed ticket's build output outside its worktree, each folder with its size.
    Outside(String, Vec<(PathBuf, u64)>),
    Moved(Move, Moved),
}

/// A ticket's folders, as the threads off the host thread look at them.
#[derive(Clone)]
struct Folders {
    ticket: String,
    worktree: PathBuf,
    scratch: PathBuf,
    derived_data: Option<PathBuf>,
}

/// The build output watch: what the host knows of disk use, and the work it runs off its thread.
pub(crate) struct Watch {
    /// Xcode's DerivedData folder, where builds of the worktree's Apple projects also go.
    pub(crate) derived_data: Option<PathBuf>,
    /// The folder holding each ticket's scratch folder; see `scratch`.
    pub(crate) scratch_root: PathBuf,
    pub(crate) space: Space,
    /// Off until the service starts it, so a host that only answers reads never measures.
    started: bool,
    /// When free space is next looked at.
    check_at: i64,
    survey: Option<Survey>,
    surveying: bool,
    /// The tickets whose build output is being scanned to reclaim it.
    inspecting: HashSet<String>,
    /// The removed tickets whose build output outside their worktree is being found.
    cleaning: HashSet<String>,
    /// A removal recorded such a cleanup since the store was last read for them.
    cleanups_due: bool,
    /// When the earliest cleanup waiting after a failed attempt comes due.
    cleanup_retry_at: Option<i64>,
    /// Held while a move takes its last look and renames, and while worktrees are restored, so
    /// a removed ticket's cleanup never moves what a restore just gave back to it.
    pub(crate) moving: Arc<Mutex<()>>,
    /// Free space is under the hard floor; whether the human's inbox says so.
    held: Option<bool>,
    finished: Arc<Mutex<Vec<Finished>>>,
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Default for Watch {
    fn default() -> Self {
        // Unit tests never reach the user's real DerivedData, temporary folder or disk.
        let roots = if cfg!(test) {
            DiskRoots::isolated(&std::env::temp_dir().join("wiffletree-test-unused"))
        } else {
            DiskRoots::machine()
        };
        Self {
            derived_data: roots.derived_data,
            scratch_root: roots.scratch_root,
            space: roots.space,
            started: false,
            check_at: 0,
            survey: None,
            surveying: false,
            inspecting: HashSet::new(),
            cleaning: HashSet::new(),
            cleanups_due: false,
            cleanup_retry_at: None,
            moving: Arc::default(),
            held: None,
            finished: Arc::default(),
            notify: None,
        }
    }
}
impl Watch {
    /// Has the next settle look at free space, as before an agent turn or after a settings change.
    pub(crate) fn check_now(&mut self) {
        self.check_at = 0;
    }
    /// Runs `work` on a thread of its own, leaving what it finished here before `notify`.
    fn spawn(&self, work: impl FnOnce() -> Finished + Send + 'static) {
        let (finished, notify) = (self.finished.clone(), self.notify.clone());
        std::thread::spawn(move || {
            let done = work();
            finished
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(done);
            if let Some(notify) = notify {
                notify();
            }
        });
    }
}

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS build_output_reclaims (
            ticket_id TEXT PRIMARY KEY,
            reclaimed_at INTEGER NOT NULL,
            bytes INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS outside_cleanups (
            ticket_id TEXT PRIMARY KEY,
            worktree TEXT NOT NULL)",
    )?;
    // A cleanup's failed attempts, retried and told as a delete's are.
    for (column, kind) in [
        ("attempts", "INTEGER NOT NULL DEFAULT 0"),
        ("last_error", "TEXT"),
        ("next_attempt_at", "INTEGER NOT NULL DEFAULT 0"),
        ("reported_at", "INTEGER"),
    ] {
        let present: i64 = db.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('outside_cleanups') WHERE name=?1",
            [column],
            |r| r.get(0),
        )?;
        if present == 0 {
            db.execute_batch(&format!(
                "ALTER TABLE outside_cleanups ADD COLUMN {column} {kind}"
            ))?;
        }
    }
    Ok(())
}

/// Bytes allocated under `path`, leaving out each of `skipped` and what it holds.
fn size_outside(path: &Path, skipped: &[&Path]) -> u64 {
    if skipped.contains(&path) {
        return 0;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    let own = metadata.blocks() * 512;
    if !metadata.is_dir() {
        return own;
    }
    let entries = fs::read_dir(path).into_iter().flatten().flatten();
    own + entries
        .map(|entry| size_outside(&entry.path(), skipped))
        .sum::<u64>()
}

fn is_real_folder(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn part(path: &Path, size: u64, note: &str) -> DiskPart {
    DiskPart {
        path: path.to_string_lossy().into_owned(),
        size,
        note: note.into(),
    }
}

/// Measures one ticket's source and build output. Off the host thread.
fn measure(folders: &Folders) -> Measured {
    let mut measured = Measured::default();
    if folders.worktree.is_dir() {
        match build_output::scan(&folders.worktree, folders.derived_data.as_deref()) {
            Ok(output) => {
                let found: Vec<&Path> = output.found.iter().map(|f| f.path.as_path()).collect();
                measured.source = size_outside(&folders.worktree, &found);
                measured.parts = (output.found.iter())
                    .map(|found| part(&found.path, found.size, ""))
                    .collect();
                measured.left_alone = (output.ambiguous.iter())
                    .map(|left| part(&left.path, 0, left_alone_note(left.reason)))
                    .collect();
            }
            Err(error) => {
                eprintln!("Measure build output of {}: {error:#}", folders.ticket);
                measured.source = size_outside(&folders.worktree, &[]);
            }
        }
    }
    if is_real_folder(&folders.scratch) {
        let size = worktree_deletions::allocated_size(&folders.scratch);
        measured.parts.push(part(&folders.scratch, size, ""));
    }
    measured
}

fn left_alone_note(reason: build_output::Reason) -> &'static str {
    match reason {
        build_output::Reason::HoldsTracked => "Left alone: it holds tracked files",
        build_output::Reason::NotIgnored => "Left alone: it holds files Git does not ignore",
        build_output::Reason::HoldsRepository => "Left alone: it holds a Git repository",
    }
}

/// What may be reclaimed of one ticket now: the build output `scan` finds and the scratch
/// folder, unless a process uses a file in one of them. Off the host thread.
fn inspect(folders: &Folders) -> Inspected {
    let mut found = vec![];
    if folders.worktree.is_dir() {
        let output = build_output::scan(&folders.worktree, folders.derived_data.as_deref())
            .map_err(|error| format!("{error:#}"))?;
        found.extend(output.found.into_iter().map(|f| (f.path, f.size)));
    }
    if is_real_folder(&folders.scratch) {
        let size = worktree_deletions::allocated_size(&folders.scratch);
        found.push((folders.scratch.clone(), size));
    }
    let canonical: Vec<PathBuf> = (found.iter())
        .filter_map(|(folder, _)| fs::canonicalize(folder).ok())
        .collect();
    if canonical.is_empty() {
        return Ok(found);
    }
    let holders = worktree_deletions::holders(&canonical)?;
    match holders.is_empty() {
        true => Ok(found),
        false => Err(format!(
            "processes use files in its build output: {}",
            holders.join(", ")
        )),
    }
}

/// What a move aside is for.
#[derive(Clone, Copy, Debug)]
enum Purpose {
    Reclaim(Why),
    /// A removed ticket's build output outside its worktree.
    Outside,
}

/// Folders of one ticket to rename aside off the host thread, their deletes recorded and held.
struct Move {
    ticket: String,
    purpose: Purpose,
    worktree: PathBuf,
    /// Each folder, where it is to wait, and its size.
    folders: Vec<(PathBuf, PathBuf, u64)>,
    /// The host's data folder, holding its store and the build gate's state.
    home: PathBuf,
    /// Held for the last look and the renames; see `Watch::moving`.
    moving: Arc<Mutex<()>>,
}

/// What a move did.
#[derive(Debug)]
enum Moved {
    /// Nothing moved, and why.
    Kept(String),
    /// A process used a folder once it moved, so every folder was moved back, and why. Lists
    /// where each that could not go back waits.
    PutBack(String, Vec<PathBuf>),
    /// Where each folder that moved waits, with its size, and why each that could not move.
    Done(Vec<(PathBuf, u64)>, Vec<String>),
}

/// Renames the folders aside unless something uses them. Everything that would stop it is
/// looked at last, just before the renames: processes with files in the folders, builds the
/// gate admitted or queued that run in the worktree, and, through its own connection to the
/// store, an agent of the ticket that started a turn. Once moved, the folders are looked at for
/// processes again, and moved back if one uses them. Off the host thread.
fn run_move(job: &Move) -> Moved {
    let folders: Vec<PathBuf> = (job.folders.iter())
        .filter_map(|(folder, _, _)| fs::canonicalize(folder).ok())
        .collect();
    if let Some(reason) = holders_of(&folders) {
        return Moved::Kept(reason);
    }
    // The last look and the renames happen while no restore runs and the gate admits no build.
    let restoring = job
        .moving
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let renamed = build_gate::while_admitting_none(&job.home, |builds| {
        if let Some(reason) = last_look(job, builds) {
            return Err(reason);
        }
        let (mut moved, mut failed) = (vec![], vec![]);
        for (folder, aside, size) in &job.folders {
            match fs::rename(folder, aside) {
                Ok(()) => moved.push((folder, aside, *size)),
                Err(error) => failed.push(format!("{}: {error}", folder.display())),
            }
        }
        Ok((moved, failed))
    });
    drop(restoring);
    let (moved, failed) = match renamed {
        Ok(Ok(renamed)) => renamed,
        Ok(Err(reason)) => return Moved::Kept(reason),
        Err(error) => return Moved::Kept(format!("could not lock the build gate: {error}")),
    };
    if let Some(put_back) = put_back_if_used(&moved) {
        return put_back;
    }
    let moved = (moved.into_iter())
        .map(|(_, aside, size)| (aside.clone(), size))
        .collect();
    Moved::Done(moved, failed)
}

/// Moves every folder back from where it waits if a process uses one there now. Each `moved`
/// is a folder, where it waits, and its size.
fn put_back_if_used(moved: &[(&PathBuf, &PathBuf, u64)]) -> Option<Moved> {
    let asides: Vec<PathBuf> = (moved.iter())
        .filter_map(|(_, aside, _)| fs::canonicalize(aside).ok())
        .collect();
    let reason = holders_of(&asides)?;
    let stuck = (moved.iter())
        .filter(|(folder, aside, _)| fs::rename(aside, folder).is_err())
        .map(|(_, aside, _)| aside.to_path_buf())
        .collect();
    Some(Moved::PutBack(reason, stuck))
}

/// Why the move's folders must stay where they are now, if anything says so, given the
/// `builds` the gate admitted or queued: a removed ticket's worktree that is back, a gated
/// build that runs in the worktree, or an agent turn that started.
fn last_look(job: &Move, builds: &[u32]) -> Option<String> {
    if matches!(job.purpose, Purpose::Outside) && fs::symlink_metadata(&job.worktree).is_ok() {
        return Some("its worktree is back".into());
    }
    let worktree = fs::canonicalize(&job.worktree).unwrap_or_else(|_| job.worktree.clone());
    if working_folders(builds)
        .iter()
        .any(|folder| folder.starts_with(&worktree))
    {
        return Some("a build the gate admitted or queued runs in its worktree".into());
    }
    let store = job.home.join(STORE_FILE);
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
    let in_turn = Connection::open_with_flags(store, flags)
        .and_then(|db| worktrees::ticket_run_open(&db, &job.ticket));
    match in_turn {
        Ok(false) => None,
        Ok(true) => Some("an agent started a turn".into()),
        Err(error) => Some(format!("could not read the store: {error}")),
    }
}

/// Why processes keep `folders` (canonical) where they are, if any use a file in them.
fn holders_of(folders: &[PathBuf]) -> Option<String> {
    if folders.is_empty() {
        return None;
    }
    match worktree_deletions::holders(folders) {
        Ok(holders) if holders.is_empty() => None,
        Ok(holders) => Some(format!("processes use it: {}", holders.join(", "))),
        Err(reason) => Some(reason),
    }
}

/// The working folders of the processes `pids`, as far as they can be read.
#[cfg(target_os = "macos")]
fn working_folders(pids: &[u32]) -> Vec<PathBuf> {
    if pids.is_empty() {
        return vec![];
    }
    let pids: Vec<String> = pids.iter().map(u32::to_string).collect();
    let listed = std::process::Command::new("lsof")
        .args(["-a", "-nPw", "-d", "cwd", "-F", "n", "-p", &pids.join(",")])
        .output();
    let listed = listed.map(|o| o.stdout).unwrap_or_default();
    (String::from_utf8_lossy(&listed).lines())
        .filter_map(|line| line.strip_prefix('n').map(PathBuf::from))
        .collect()
}
#[cfg(not(target_os = "macos"))]
fn working_folders(pids: &[u32]) -> Vec<PathBuf> {
    (pids.iter())
        .filter_map(|pid| fs::read_link(format!("/proc/{pid}/cwd")).ok())
        .collect()
}

/// A ticket whose build output may be reclaimed now: no agent of it is in a turn.
struct Candidate {
    ticket: String,
    size: u64,
    /// When it was created or an agent of it last had a turn, whichever is later.
    last_active: i64,
    /// Accepted or closed.
    done: bool,
}

/// Whose build output to reclaim at `at`, and why: done tickets and those idle past the idle
/// time, then the least recently used of the rest while the build output across every ticket,
/// `total`, is over the budget or the `free` space under the soft floor.
fn choose(
    mut candidates: Vec<Candidate>,
    total: u64,
    free: Option<u64>,
    disk: &DiskSettings,
    at: i64,
) -> Vec<(String, Why)> {
    candidates.sort_by_key(|c| c.last_active);
    let (mut chosen, mut rest) = (vec![], vec![]);
    let mut left = total;
    for candidate in candidates {
        let idle = at - candidate.last_active >= disk.idle_ms();
        match (candidate.done, idle) {
            (true, _) => chosen.push((candidate.ticket, Why::Done)),
            (false, true) => chosen.push((candidate.ticket, Why::Idle)),
            (false, false) => {
                rest.push(candidate);
                continue;
            }
        }
        left -= candidate.size;
    }
    // As it will be once the chosen are deleted.
    let mut freed = free.map(|free| free + (total - left));
    for candidate in rest {
        let why = if left > disk.budget() {
            Why::Budget(total)
        } else if freed.is_some_and(|freed| freed < disk.soft_floor()) {
            Why::SoftFloor(free.unwrap_or(0))
        } else {
            break;
        };
        left -= candidate.size;
        freed = freed.map(|freed| freed + candidate.size);
        chosen.push((candidate.ticket, why));
    }
    chosen
}

impl Host {
    /// Starts watching disk use in `roots`, telling `notify` as each measurement or scan finishes.
    pub(crate) fn watch_build_output(
        &mut self,
        roots: DiskRoots,
        notify: impl Fn() + Send + Sync + 'static,
    ) {
        let watch = &mut self.build_output;
        (watch.derived_data, watch.scratch_root, watch.space) =
            (roots.derived_data, roots.scratch_root, roots.space);
        watch.notify = Some(Arc::new(notify));
        watch.started = true;
        watch.check_at = 0;
        // Resumes the cleanups of removals a stop cut off, and retries the failed ones now.
        watch.cleanups_due = true;
        if let Err(error) = (self.db).execute("UPDATE outside_cleanups SET next_attempt_at=0", []) {
            eprintln!("Retry removed tickets' cleanups: {error:#}");
        }
    }
    /// The scratch folder of `ticket`'s agents, under the watch's scratch root.
    fn scratch_folder(&self, ticket: &str) -> PathBuf {
        let name = scratch::ticket_scratch(ticket);
        self.build_output
            .scratch_root
            .join(name.file_name().unwrap_or_default())
    }
    fn folders(&self, ticket: &Ticket) -> Folders {
        Folders {
            ticket: ticket.id.clone(),
            worktree: PathBuf::from(&ticket.worktree),
            scratch: self.scratch_folder(&ticket.id),
            derived_data: self.build_output.derived_data.clone(),
        }
    }
    /// Free and total bytes on the disk holding the workspaces folder.
    fn workspaces_space(&self) -> Option<(u64, u64)> {
        (self.build_output.space)(Path::new(&self.settings().workspaces_dir))
    }
    /// The tickets whose worktree or scratch folder exists, which are all that hold build output.
    fn tickets_on_disk(&self) -> Result<Vec<Ticket>> {
        Ok((self.tickets()?.into_iter())
            .filter(|t| {
                Path::new(&t.worktree).is_dir() || is_real_folder(&self.scratch_folder(&t.id))
            })
            .collect())
    }
    /// Whether the ticket's build output must be left alone now: an agent of it is in a turn, or
    /// its worktree is being checked out.
    fn building(&self, ticket: &Ticket) -> Result<bool> {
        Ok(self.checkout_reservations.contains(&ticket.id) || self.agent_in_turn(ticket)?)
    }
    /// When an agent of each ticket last started or ended a turn.
    fn last_turns(&self) -> Result<HashMap<String, i64>> {
        Ok(self
            .db
            .prepare(
                "SELECT json_extract(t.data,'$.ticket_id'),MAX(COALESCE(r.finished_at,r.started_at))
                 FROM provider_runs r JOIN runtimes t ON t.session_id=r.session_id
                 WHERE json_extract(t.data,'$.ticket_id') IS NOT NULL
                 GROUP BY json_extract(t.data,'$.ticket_id')",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// When each ticket was created, as its activity recorded it.
    fn ticket_creations(&self) -> Result<HashMap<String, i64>> {
        Ok(self
            .db
            .prepare("SELECT detail,MAX(created_at) FROM activity WHERE kind='ticket_created' GROUP BY detail")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    fn last_reclaims(&self) -> Result<HashMap<String, (i64, u64)>> {
        Ok(self
            .db
            .prepare("SELECT ticket_id,reclaimed_at,bytes FROM build_output_reclaims")?
            .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Records what finished off the host thread, looks at free space when due, and starts the
    /// reclaims and measurement that are due. Returns when it next wants to look. The service
    /// calls this after every event; failures are logged, never propagated.
    pub fn settle_build_output(&mut self, at: i64) -> Option<i64> {
        if !self.build_output.started {
            return None;
        }
        let finished = std::mem::take(
            &mut *(self.build_output.finished)
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        for done in finished {
            let settled = match done {
                Finished::Survey(survey) => {
                    self.build_output.surveying = false;
                    self.build_output.survey = Some(survey);
                    self.plan_reclaims(at)
                }
                Finished::Inspected(ticket, why, inspected, scanned_at) => {
                    self.build_output.inspecting.remove(&ticket);
                    self.move_reclaimed(&ticket, why, inspected, scanned_at)
                }
                Finished::Outside(ticket, folders) => self.move_outside(&ticket, folders, at),
                Finished::Moved(job, moved) => self.finish_move_aside(job, moved, at),
            };
            if let Err(error) = settled {
                eprintln!("Reclaim build output: {error:#}");
            }
        }
        if self
            .build_output
            .cleanup_retry_at
            .is_some_and(|due| at >= due)
        {
            self.build_output.cleanups_due = true;
        }
        if std::mem::take(&mut self.build_output.cleanups_due)
            && let Err(error) = self.start_outside_cleanups(at)
        {
            eprintln!("Delete removed tickets' build output: {error:#}");
            self.build_output.cleanups_due = true;
        }
        if at >= self.build_output.check_at {
            self.build_output.check_at = at + CHECK_MS;
            if let Err(error) = self.check_disk(at) {
                eprintln!("Check disk space: {error:#}");
            }
        }
        let check_at = self.build_output.check_at;
        Some((self.build_output.cleanup_retry_at).map_or(check_at, |due| due.min(check_at)))
    }

    /// Looks at free space: raises or clears the hard-floor card, reclaims from the measurement
    /// at hand when under the soft floor, and measures again when the last is old.
    fn check_disk(&mut self, at: i64) -> Result<()> {
        let disk = self.settings().disk;
        let free = self.workspaces_space().map(|(free, _)| free);
        self.guard_hard_floor(free.is_some_and(|free| free < disk.hard_floor()), free, at)?;
        let stale = (self.build_output.survey.as_ref()).is_none_or(|s| at - s.at >= SURVEY_MS);
        if stale && !self.build_output.surveying {
            self.start_survey(at)?;
        } else if free.is_some_and(|free| free < disk.soft_floor()) {
            self.plan_reclaims(at)?;
        }
        Ok(())
    }
    fn start_survey(&mut self, at: i64) -> Result<()> {
        let folders: Vec<Folders> = (self.tickets_on_disk()?.iter())
            .map(|ticket| self.folders(ticket))
            .collect();
        self.build_output.surveying = true;
        self.build_output.spawn(move || {
            let tickets = (folders.iter())
                .map(|folders| (folders.ticket.clone(), measure(folders)))
                .collect();
            Finished::Survey(Survey { at, tickets })
        });
        Ok(())
    }

    /// Chooses whose build output to reclaim from the latest measurement, and starts each: done
    /// tickets and those idle past the idle time, then the least recently used of the rest while
    /// the total is over the budget or the free space under the soft floor.
    fn plan_reclaims(&mut self, at: i64) -> Result<()> {
        let Some(survey) = self.build_output.survey.clone() else {
            return Ok(());
        };
        let disk = self.settings().disk;
        let (turns, reclaims) = (self.last_turns()?, self.last_reclaims()?);
        let creations = self.ticket_creations()?;
        let mut total = 0;
        let mut tickets = HashMap::new();
        let mut candidates = vec![];
        for ticket in self.tickets()? {
            let Some(measured) = survey.tickets.get(&ticket.id) else {
                continue;
            };
            let size = measured.build_output();
            total += size;
            let reclaimed_since = reclaims
                .get(&ticket.id)
                .is_some_and(|(r, _)| *r >= survey.at);
            if size == 0
                || reclaimed_since
                || self.build_output.inspecting.contains(&ticket.id)
                || self.building(&ticket)?
            {
                continue;
            }
            candidates.push(Candidate {
                ticket: ticket.id.clone(),
                size,
                last_active: [&turns, &creations]
                    .iter()
                    .filter_map(|times| times.get(&ticket.id).copied())
                    .max()
                    .unwrap_or(0),
                done: !ticket.state.is_open(),
            });
            tickets.insert(ticket.id.clone(), ticket);
        }
        let free = self.workspaces_space().map(|(free, _)| free);
        for (id, why) in choose(candidates, total, free, &disk, at) {
            self.start_reclaim(&tickets[&id], why);
        }
        Ok(())
    }
    fn start_reclaim(&mut self, ticket: &Ticket, why: Why) {
        let folders = self.folders(ticket);
        if !self.build_output.inspecting.insert(ticket.id.clone()) {
            return;
        }
        self.build_output.spawn(move || {
            let inspected = std::panic::catch_unwind(|| inspect(&folders))
                .unwrap_or_else(|_| Err("the scan panicked".into()));
            Finished::Inspected(folders.ticket, why, inspected, now())
        });
    }
    /// Reclaims one ticket's build output, or with `None` every ticket's but those with an
    /// agent in a turn. Each finishes off the host thread.
    pub fn reclaim_build_output(&mut self, ticket: Option<&str>) -> Result<()> {
        if let Some(id) = ticket {
            let ticket = self.ticket(id)?;
            ensure!(
                !self.building(&ticket)?,
                "An agent of \"{}\" is in a turn, so its build output is kept",
                ticket.title
            );
            self.start_reclaim(&ticket, Why::Asked);
            return Ok(());
        }
        for ticket in self.tickets_on_disk()? {
            if !self.building(&ticket)? {
                self.start_reclaim(&ticket, Why::Asked);
            }
        }
        Ok(())
    }

    /// Starts moving aside each folder a fresh scan found to reclaim, unless an agent of the
    /// ticket started a turn since. A scan older than a few seconds is redone first.
    fn move_reclaimed(
        &mut self,
        id: &str,
        why: Why,
        inspected: Inspected,
        scanned_at: i64,
    ) -> Result<()> {
        let ticket = self.ticket(id)?;
        let folders = match inspected {
            Ok(folders) => folders,
            Err(reason) => {
                eprintln!("Reclaim build output of {id}: kept, {reason}");
                return Ok(());
            }
        };
        if folders.is_empty() {
            return Ok(());
        }
        if now() - scanned_at > FRESH_SCAN_MS {
            self.start_reclaim(&ticket, why);
            return Ok(());
        }
        if self.building(&ticket)? {
            eprintln!("Reclaim build output of {id}: kept, an agent started a turn");
            return Ok(());
        }
        self.build_output.inspecting.insert(id.to_owned());
        self.move_aside(&ticket, Purpose::Reclaim(why), folders, now())
    }
    /// Records and holds the delete of each folder, then renames them aside off the host
    /// thread; see `run_move`. Build output in the worktree waits beside the worktree, never in
    /// it; any other folder waits beside itself. If a delete cannot be recorded, nothing moves
    /// and the ticket is no longer being reclaimed or cleaned up.
    fn move_aside(
        &mut self,
        ticket: &Ticket,
        purpose: Purpose,
        folders: Vec<(PathBuf, u64)>,
        at: i64,
    ) -> Result<()> {
        let worktree = PathBuf::from(&ticket.worktree);
        let mut planned = vec![];
        for (folder, size) in folders {
            let beside = match folder.starts_with(&worktree) {
                true => worktree.parent(),
                false => folder.parent(),
            };
            let Some(beside) = beside else { continue };
            match self.hold_deletion(beside, &ticket.id) {
                Ok(aside) => planned.push((folder, aside, size)),
                Err(error) => {
                    for (_, aside, _) in &planned {
                        if let Err(error) = self.forget_worktree_deletion(aside) {
                            eprintln!("Forget the delete of {}: {error:#}", aside.display());
                        }
                    }
                    self.stop_moving(&ticket.id, purpose, &format!("{error:#}"), at);
                    return Err(error);
                }
            }
        }
        let job = Move {
            ticket: ticket.id.clone(),
            purpose,
            worktree,
            folders: planned,
            home: self.home.clone(),
            moving: self.build_output.moving.clone(),
        };
        self.build_output.spawn(move || {
            let moved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_move(&job)))
                .unwrap_or_else(|_| Moved::Kept("the move panicked".into()));
            Finished::Moved(job, moved)
        });
        Ok(())
    }
    /// Ends a ticket's move before it ran: it is no longer being reclaimed, and a removed
    /// ticket's cleanup counts the attempt as failed for `error`.
    fn stop_moving(&mut self, id: &str, purpose: Purpose, error: &str, at: i64) {
        match purpose {
            Purpose::Reclaim(_) => {
                self.build_output.inspecting.remove(id);
            }
            Purpose::Outside => {
                self.build_output.cleaning.remove(id);
                if let Err(error) = self.outside_cleanup_failed(id, error, at) {
                    eprintln!("Record the cleanup of {id}: {error:#}");
                }
            }
        }
    }
    /// Lets the deletes of what moved start and drops those of what stayed. A reclaim that moved
    /// something is recorded and told to the ticket's project. A removed ticket's cleanup is done
    /// once every folder moved; otherwise it stays recorded and is retried, as a delete is.
    fn finish_move_aside(&mut self, job: Move, moved: Moved, at: i64) -> Result<()> {
        let id = &job.ticket;
        match job.purpose {
            Purpose::Reclaim(_) => self.build_output.inspecting.remove(id),
            Purpose::Outside => self.build_output.cleaning.remove(id),
        };
        let (moved, stuck, incomplete) = match moved {
            Moved::Done(moved, failed) => {
                for failure in &failed {
                    eprintln!("Move build output of {id} aside: could not move {failure}");
                }
                let incomplete = (!failed.is_empty()).then(|| failed.join("; "));
                (moved, vec![], incomplete)
            }
            Moved::Kept(reason) => {
                eprintln!("Move build output of {id} aside: kept, {reason}");
                (vec![], vec![], Some(reason))
            }
            Moved::PutBack(reason, stuck) => {
                eprintln!("Move build output of {id} aside: moved back, {reason}");
                (vec![], stuck, Some(reason))
            }
        };
        for (_, aside, _) in &job.folders {
            let waits = moved.iter().any(|(path, _)| path == aside) || stuck.contains(aside);
            match waits {
                true => self.release_deletion(aside)?,
                false => self.forget_worktree_deletion(aside)?,
            }
        }
        let freed: u64 = moved.iter().map(|(_, size)| size).sum();
        match (job.purpose, incomplete) {
            (Purpose::Outside, None) => self.forget_outside_cleanup(id),
            (Purpose::Outside, Some(error)) => self.outside_cleanup_failed(id, &error, at),
            (Purpose::Reclaim(why), _) if freed > 0 => self.record_reclaim(id, why, freed, at),
            _ => Ok(()),
        }
    }
    fn record_reclaim(&mut self, id: &str, why: Why, freed: u64, at: i64) -> Result<()> {
        let ticket = self.ticket(id)?;
        self.db.execute(
            "INSERT INTO build_output_reclaims VALUES (?1,?2,?3)
             ON CONFLICT(ticket_id) DO UPDATE SET reclaimed_at=excluded.reclaimed_at,bytes=excluded.bytes",
            params![id, at, freed],
        )?;
        if let Some(survey) = self.build_output.survey.as_mut()
            && let Some(measured) = survey.tickets.get_mut(id)
        {
            measured.parts.clear();
        }
        self.notice_reclaim(&ticket, why, freed, at)
    }
    /// Tells the ticket's project, quietly, what was reclaimed and why.
    fn notice_reclaim(&mut self, ticket: &Ticket, why: Why, freed: u64, at: i64) -> Result<()> {
        let disk = self.settings().disk;
        let label = worktree_deletions::size_label;
        let reason = match why {
            Why::Done => format!("the ticket is {}", ticket.state),
            Why::Idle => format!("no agent turn for {} hours", disk.idle_hours),
            Why::Budget(total) => format!(
                "build output was {}, over the {} GB budget",
                label(total),
                disk.budget_gb
            ),
            Why::SoftFloor(free) => format!(
                "free space fell to {}, below the {} GB soft floor",
                label(free),
                disk.soft_floor_gb
            ),
            Why::Asked => "you asked".into(),
        };
        let notice = format!(
            "Reclaimed {} of build output from \"{}\": {reason}. Its source is kept; the next build there makes it again.",
            label(freed),
            ticket.title
        );
        let project = self.session(&ticket.coordinator_id)?.project_id;
        let Some(coordinator) = self.coordinator_of(&project)? else {
            return Ok(());
        };
        if self.session(&coordinator)?.archived {
            return Ok(());
        }
        let id = format!("{RECLAIM_NOTICE_PREFIX}{}:{at}", ticket.id);
        self.send_quietly(&id, None, &coordinator, &notice)?;
        Ok(())
    }

    /// Raises the inbox card saying builds wait while free space is under the hard floor, once
    /// for each time it falls there, and answers it when space returns.
    fn guard_hard_floor(&mut self, under: bool, free: Option<u64>, at: i64) -> Result<()> {
        let open: Vec<String> = self
            .db
            .prepare(
                "SELECT id FROM attention WHERE operation_id LIKE ?1 || '%'
                 AND json_extract(data,'$.answer') IS NULL",
            )?
            .query_map([BUILDS_HELD_OPERATION], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if !under {
            self.build_output.held = None;
            for id in open {
                self.resolve_attention(&id, "Disk space returned")?;
                self.worktree_deletions.inbox_changed = true;
            }
            return Ok(());
        }
        // A card left open by an earlier run of the host is the one for this time.
        let told = self.build_output.held.unwrap_or(false) || !open.is_empty();
        self.build_output.held = Some(told);
        if told {
            return Ok(());
        }
        let disk = self.settings().disk;
        let label = worktree_deletions::size_label;
        let notice = format!(
            "Builds are waiting: only {} is free, under the {} GB hard floor. New builds start once space returns. Wiffletree reclaims idle build output first; Settings › Disk shows where the space is, and reclaiming all but running builds frees more.",
            label(free.unwrap_or(0)),
            disk.hard_floor_gb
        );
        let told = self.tell_inbox(None, &format!("{BUILDS_HELD_OPERATION}{at}"), &notice)?;
        self.build_output.held = Some(told);
        Ok(())
    }

    /// Where the disk space goes, from the latest measurement and the store as it is now.
    pub fn disk_use(&self) -> Result<DiskUse> {
        let (free, total) = self.workspaces_space().unwrap_or_default();
        let disk = self.settings().disk;
        let survey = self.build_output.survey.clone().unwrap_or_default();
        let deletes = self.pending_deletes()?;
        let (turns, reclaims) = (self.last_turns()?, self.last_reclaims()?);
        let mut tickets = vec![];
        for ticket in self.tickets()? {
            let Some(measured) = survey.tickets.get(&ticket.id) else {
                continue;
            };
            let last_turn_at = turns.get(&ticket.id).copied();
            let reclaim = reclaims.get(&ticket.id).copied();
            let deleting = (deletes.iter()).filter(|d| d.ticket_id.as_deref() == Some(&ticket.id));
            let failing = deleting.clone().any(|d| d.failing);
            // A turn comes first: an older delete still waiting never makes the ticket idle.
            let state = if self.building(&ticket)? {
                TicketDiskState::Building
            } else if failing {
                TicketDiskState::FailingToDelete
            } else if deleting.count() > 0 || self.build_output.inspecting.contains(&ticket.id) {
                TicketDiskState::Reclaiming
            } else if reclaim.is_some_and(|(r, _)| last_turn_at.is_none_or(|t| r >= t)) {
                TicketDiskState::Reclaimed
            } else {
                TicketDiskState::Idle
            };
            tickets.push(TicketDiskUse {
                project_id: self.session(&ticket.coordinator_id)?.project_id,
                ticket_id: ticket.id,
                repository_id: ticket.repository_id,
                title: ticket.title,
                source: measured.source,
                build_output: measured.build_output(),
                parts: measured.parts.clone(),
                left_alone: measured.left_alone.clone(),
                last_turn_at,
                state,
                reclaimed_at: reclaim.map(|(at, _)| at),
                reclaimed: reclaim.map_or(0, |(_, bytes)| bytes),
            });
        }
        Ok(DiskUse {
            free,
            total,
            build_output: tickets.iter().map(|t| t.build_output).sum(),
            source: tickets.iter().map(|t| t.source).sum(),
            measured_at: self.build_output.survey.as_ref().map(|s| s.at),
            builds_held: free < disk.hard_floor() && total > 0,
            hard_floor: disk.hard_floor(),
            tickets,
            deletes,
        })
    }

    /// Records that the ticket's build output outside its worktree, Xcode's DerivedData folders
    /// for its path and its scratch folder, is to be deleted as its worktree is removed. Recorded
    /// before the worktree moves, so a stop at any point leaves the cleanup to resume.
    pub(crate) fn record_outside_cleanup(&mut self, ticket: &Ticket) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO outside_cleanups(ticket_id,worktree) VALUES (?1,?2)",
            params![ticket.id, ticket.worktree],
        )?;
        Ok(())
    }
    /// Drops the cleanup of a removal that left the worktree in place.
    pub(crate) fn forget_outside_cleanup(&self, ticket: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM outside_cleanups WHERE ticket_id=?1", [ticket])?;
        Ok(())
    }
    /// Has the next settle start the cleanups recorded for removed worktrees.
    pub(crate) fn outside_cleanups_due(&mut self) {
        self.build_output.cleanups_due = true;
    }
    /// Has the cleanups looked at again by `due`.
    fn retry_outside_cleanups_by(&mut self, due: i64) {
        let retry_at = &mut self.build_output.cleanup_retry_at;
        *retry_at = Some(retry_at.map_or(due, |at| at.min(due)));
    }
    /// Records a cleanup's failed attempt and when it is retried, and tells the human's inbox
    /// once it has failed as often as a delete does before it is told.
    fn outside_cleanup_failed(&mut self, id: &str, error: &str, at: i64) -> Result<()> {
        let Some((worktree, attempts, reported)) = self
            .db
            .query_row(
                "SELECT worktree,attempts+1,reported_at IS NOT NULL FROM outside_cleanups WHERE ticket_id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?)),
            )
            .optional()?
        else {
            return Ok(());
        };
        let next = at + worktree_deletions::backoff(attempts);
        self.db.execute(
            "UPDATE outside_cleanups SET attempts=?2,last_error=?3,next_attempt_at=?4 WHERE ticket_id=?1",
            params![id, attempts, error, next],
        )?;
        self.retry_outside_cleanups_by(next);
        if attempts < worktree_deletions::REPORT_AFTER || reported {
            return Ok(());
        }
        let ticket = self.ticket(id)?;
        let notice = format!(
            "Wiffletree could not delete the build output of the removed ticket \"{}\" outside its worktree after {attempts} attempts: {error}. It keeps retrying, and at every start. Deleting the folder yourself frees the space.",
            ticket.title
        );
        let project = self.session(&ticket.coordinator_id)?.project_id;
        let operation = delete_failed_operation(at, &worktree);
        if !self.tell_inbox(Some(&project), &operation, &notice)? {
            eprintln!("{notice}");
            return Ok(());
        }
        self.db.execute(
            "UPDATE outside_cleanups SET reported_at=?2 WHERE ticket_id=?1",
            params![id, at],
        )?;
        Ok(())
    }
    /// Finds the folders of each recorded cleanup due at `at` off the host thread, as `scan`
    /// would find them.
    fn start_outside_cleanups(&mut self, at: i64) -> Result<()> {
        let recorded: Vec<(String, String, i64)> = self
            .db
            .prepare("SELECT ticket_id,worktree,next_attempt_at FROM outside_cleanups")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        self.build_output.cleanup_retry_at = None;
        for (ticket, worktree, next) in recorded {
            if self.build_output.cleaning.contains(&ticket) {
                continue;
            }
            if next > at {
                self.retry_outside_cleanups_by(next);
                continue;
            }
            if !self.outside_cleanup_applies(&ticket, at)? {
                continue;
            }
            self.build_output.cleaning.insert(ticket.clone());
            let (scratch, root) = (
                self.scratch_folder(&ticket),
                self.build_output.derived_data.clone(),
            );
            self.build_output.spawn(move || {
                let found =
                    root.map(|root| build_output::derived_data(Path::new(&worktree), &root));
                let mut folders: Vec<(PathBuf, u64)> =
                    (found.into_iter().flat_map(|output| output.found))
                        .map(|f| (f.path, f.size))
                        .collect();
                if is_real_folder(&scratch) {
                    folders.push((scratch, 0));
                }
                Finished::Outside(ticket, folders)
            });
        }
        Ok(())
    }
    /// Whether a recorded cleanup is still for a removed worktree. One whose worktree is back
    /// at its path, as a restore puts it, is dropped: its build output is the ticket's again.
    /// One with an agent of the ticket in a turn waits a retry's first wait, without counting
    /// as a failed attempt.
    fn outside_cleanup_applies(&mut self, id: &str, at: i64) -> Result<bool> {
        let ticket = self.ticket(id)?;
        if fs::symlink_metadata(&ticket.worktree).is_ok() {
            self.forget_outside_cleanup(id)?;
            return Ok(false);
        }
        if !self.building(&ticket)? {
            return Ok(true);
        }
        let next = at + worktree_deletions::backoff(1);
        self.db.execute(
            "UPDATE outside_cleanups SET next_attempt_at=?2 WHERE ticket_id=?1",
            params![id, next],
        )?;
        self.retry_outside_cleanups_by(next);
        Ok(false)
    }
    /// Moves aside a removed ticket's folders outside its worktree, each beside itself, once the
    /// cleanup is looked at again on the host thread.
    fn move_outside(&mut self, id: &str, folders: Vec<(PathBuf, u64)>, at: i64) -> Result<()> {
        let applies = self.outside_cleanup_applies(id, at);
        if !matches!(applies, Ok(true)) {
            self.build_output.cleaning.remove(id);
            return applies.map(drop);
        }
        let ticket = self.ticket(id)?;
        self.move_aside(&ticket, Purpose::Outside, folders, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;
    use std::sync::atomic::{AtomicU64, Ordering};

    const HOUR: i64 = 60 * 60_000;

    /// A host whose disk use stays in a temporary folder: its workspaces, Xcode's DerivedData
    /// and the scratch folders, and a free space the test sets.
    struct Fixture {
        folder: tempfile::TempDir,
        host: Host,
        coordinator: String,
        repository: String,
        free: Arc<AtomicU64>,
    }
    impl Fixture {
        fn new() -> Self {
            let folder = tempfile::tempdir().unwrap();
            let main = folder.path().join("web");
            fs::create_dir_all(main.join("src")).unwrap();
            git(&main, &["init", "-q", "-b", "main"]);
            fs::write(main.join(".gitignore"), "target/\nnode_modules/\n").unwrap();
            fs::write(main.join("src/main.rs"), "fn main() {}\n").unwrap();
            fs::create_dir_all(main.join("dist")).unwrap();
            fs::write(main.join("dist/kept.js"), "tracked\n").unwrap();
            git(&main, &["add", "-A"]);
            git(&main, &["commit", "-q", "-m", "start"]);
            let mut host = Host::open_set_up(folder.path().join("home")).unwrap();
            let workspaces = folder.path().join("workspaces");
            host.set_workspaces_dir(workspaces.to_str().unwrap())
                .unwrap();
            let project = host.create_project("Disk").unwrap();
            let coordinator = host.sessions().unwrap().remove(0).id;
            let repository = host
                .attach_repository(&project.id, main.to_str().unwrap(), "HEAD")
                .unwrap()
                .id;
            let mut f = Self {
                folder,
                host,
                coordinator,
                repository,
                free: Arc::new(AtomicU64::new(400 * GB)),
            };
            f.watch();
            f
        }
        /// Starts the watch on the test's own folders and free space, as the service starts it.
        fn watch(&mut self) {
            let reading = self.free.clone();
            let roots = DiskRoots {
                derived_data: Some(self.folder.path().join("DerivedData")),
                scratch_root: self.folder.path().join("scratch"),
                space: Arc::new(move |_| Some((reading.load(Ordering::SeqCst), 500 * GB))),
            };
            self.host.watch_build_output(roots, || {});
        }
        /// The host as a new start opens it.
        fn reopen(&mut self) {
            let home = self.host.home.clone();
            // The store lock is released before the next open takes it.
            self.host = Host::open_set_up(self.folder.path().join("unused")).unwrap();
            self.host = Host::open_set_up(home).unwrap();
            self.watch();
        }
        /// A DerivedData folder whose project is `project`.
        fn derived_data(&self, name: &str, project: &Path) -> PathBuf {
            let folder = self.folder.path().join("DerivedData").join(name);
            fs::create_dir_all(folder.join("Build")).unwrap();
            let plist = format!(
                "<key>WorkspacePath</key>\n<string>{}</string>",
                project.display()
            );
            fs::write(folder.join("info.plist"), plist).unwrap();
            folder
        }
        /// A ticket whose worktree has `size` bytes in `target/` and a file in its scratch folder.
        fn ticket(&mut self, title: &str, size: usize) -> Ticket {
            let ticket = (self.host)
                .create_ticket(&self.coordinator, &self.repository, title, "Do")
                .unwrap();
            let target = Path::new(&ticket.worktree).join("target");
            fs::create_dir_all(target.join("debug")).unwrap();
            fs::write(target.join("debug/app"), vec![1; size]).unwrap();
            let scratch = self.host.scratch_folder(&ticket.id);
            fs::create_dir_all(&scratch).unwrap();
            fs::write(scratch.join("cc-1.o"), "object").unwrap();
            ticket
        }
        /// Records a finished turn of an agent of `ticket` at `at`, or one still running.
        fn turn(&mut self, ticket: &Ticket, at: i64, running: bool) {
            let agent = self
                .host
                .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
                .unwrap();
            let finished = (!running).then_some(at);
            self.host
                .db
                .execute(
                    "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,detail) VALUES (?1,?2,'[]',?3,?4,'{}')",
                    params![new_id(), agent.id, at, finished],
                )
                .unwrap();
        }
        /// Settles until nothing runs off the host thread, deleting what was moved aside.
        fn settle(&mut self, at: i64) {
            loop {
                self.host.settle_build_output(at);
                // Deletes are recorded due at the clock's time, which may be after `at`.
                self.host.settle_worktree_deletions(now().max(at));
                let watch = &self.host.build_output;
                let finished = !watch.finished.lock().unwrap().is_empty();
                if !finished
                    && !watch.surveying
                    && watch.inspecting.is_empty()
                    && watch.cleaning.is_empty()
                    && !watch.cleanups_due
                    && !self.host.worktree_deletions_running()
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        /// Settles with a new look at free space and a new measurement.
        fn check(&mut self, at: i64) {
            self.host.build_output.survey = None;
            self.host.build_output.check_now();
            self.settle(at);
        }
        fn has_build_output(&self, ticket: &Ticket) -> bool {
            Path::new(&ticket.worktree).join("target").exists()
        }
        fn notices(&self) -> Vec<String> {
            self.host
                .db
                .prepare("SELECT body FROM messages WHERE id LIKE 'reclaim:%' ORDER BY sequence")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        }
    }

    fn status(worktree: &Path) -> String {
        git(
            worktree,
            &["status", "--porcelain", "--untracked-files=all"],
        )
    }

    #[test]
    fn reclaiming_keeps_tracked_files_uncommitted_changes_and_other_untracked_files() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        let worktree = PathBuf::from(&ticket.worktree);
        fs::write(worktree.join("src/main.rs"), "fn main() { changed() }\n").unwrap();
        fs::write(worktree.join("notes.txt"), "mine\n").unwrap();
        fs::create_dir_all(worktree.join("node_modules/left-pad")).unwrap();
        fs::write(worktree.join("node_modules/left-pad/index.js"), "x").unwrap();
        // A build output name Git does not ignore, and one holding a tracked file.
        fs::create_dir_all(worktree.join("build")).unwrap();
        fs::write(worktree.join("build/report.txt"), "mine\n").unwrap();
        fs::write(worktree.join("dist/new.js"), "mine\n").unwrap();
        // Xcode's folders for this worktree and for another path.
        let derived = f.folder.path().join("DerivedData");
        let plist = |path: &Path| {
            format!(
                "<plist><dict><key>WorkspacePath</key><string>{}</string></dict></plist>",
                path.display()
            )
        };
        for (name, project) in [
            ("App-a1", worktree.join("App.xcodeproj")),
            ("Other-b2", f.folder.path().join("elsewhere/App.xcodeproj")),
        ] {
            fs::create_dir_all(derived.join(name).join("Build")).unwrap();
            fs::write(derived.join(name).join("info.plist"), plist(&project)).unwrap();
        }
        let before = status(&worktree);
        assert!(before.contains("M src/main.rs") && before.contains("?? notes.txt"));

        f.host.reclaim_build_output(Some(&ticket.id)).unwrap();
        f.settle(now());

        assert!(!worktree.join("target").exists());
        assert!(!worktree.join("node_modules").exists());
        assert!(!f.host.scratch_folder(&ticket.id).exists());
        assert!(!derived.join("App-a1").exists());
        assert!(derived.join("Other-b2").exists());
        assert_eq!(status(&worktree), before);
        assert_eq!(
            fs::read_to_string(worktree.join("src/main.rs")).unwrap(),
            "fn main() { changed() }\n"
        );
        for kept in [
            "notes.txt",
            "build/report.txt",
            "dist/kept.js",
            "dist/new.js",
        ] {
            assert!(worktree.join(kept).exists(), "{kept}");
        }
        // Everything moved aside was deleted, and nothing waits beside the worktree.
        assert_eq!(f.host.pending_deletes().unwrap(), []);
        let parent = worktree.parent().unwrap();
        let beside: Vec<_> = fs::read_dir(parent).unwrap().flatten().collect();
        assert_eq!(beside.len(), 1, "{beside:?}");
        assert_eq!(f.notices().len(), 1);
        assert!(f.notices()[0].contains("\"Toolbar\": you asked"));
    }

    #[test]
    fn done_and_idle_tickets_are_reclaimed_and_a_recent_one_is_kept() {
        let mut f = Fixture::new();
        let start = now();
        let idle = f.ticket("Idle", 4096);
        f.turn(&idle, start, false);
        let done = f.ticket("Done", 4096);
        f.turn(&done, start + 30 * HOUR, false);
        f.host
            .db
            .execute(
                "UPDATE tickets SET data=json_set(data,'$.state','accepted') WHERE id=?1",
                [&done.id],
            )
            .unwrap();
        let recent = f.ticket("Recent", 4096);
        f.turn(&recent, start + 30 * HOUR, false);
        // A ticket no agent has had a turn on is idle from its creation.
        let fresh = f.ticket("Fresh", 4096);
        f.check(start + HOUR);
        assert!(f.has_build_output(&fresh) && f.has_build_output(&idle));

        f.check(start + 30 * HOUR);

        assert!(!f.has_build_output(&fresh));
        assert!(!f.has_build_output(&idle));
        assert!(!f.has_build_output(&done));
        assert!(f.has_build_output(&recent));
        let notices = f.notices().join("\n");
        assert!(
            notices.contains("\"Idle\": no agent turn for 24 hours"),
            "{notices}"
        );
        assert!(
            notices.contains("\"Done\": the ticket is accepted"),
            "{notices}"
        );
        let disk = f.host.disk_use().unwrap();
        let state = |ticket: &Ticket| {
            let row = disk
                .tickets
                .iter()
                .find(|t| t.ticket_id == ticket.id)
                .unwrap();
            (row.state, row.build_output > 0)
        };
        assert_eq!(state(&idle), (TicketDiskState::Reclaimed, false));
        assert_eq!(state(&recent), (TicketDiskState::Idle, true));
        assert!(disk.source > 0 && disk.free == 400 * GB);
    }

    #[test]
    fn a_ticket_in_a_turn_or_with_a_process_in_its_build_output_is_kept() {
        let mut f = Fixture::new();
        let start = now();
        let building = f.ticket("Building", 4096);
        f.turn(&building, start, true);
        let held = f.ticket("Held", 4096);
        let target = Path::new(&held.worktree).join("target");
        let mut holder = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&target)
            .spawn()
            .unwrap();

        f.check(start + 30 * HOUR);
        assert!(f.has_build_output(&building));
        assert!(f.has_build_output(&held));
        assert!(f.host.reclaim_build_output(Some(&building.id)).is_err());
        let disk = f.host.disk_use().unwrap();
        let row = disk.tickets.iter().find(|t| t.ticket_id == building.id);
        assert_eq!(row.unwrap().state, TicketDiskState::Building);
        // An earlier reclaim's delete that keeps failing does not hide the turn.
        let worktree = Path::new(&building.worktree);
        let aside = (worktree.parent().unwrap()).join(format!(".removing-{}-1-0", building.id));
        f.host
            .record_worktree_deletion(&aside, Some(&building.id))
            .unwrap();
        (f.host.db)
            .execute("UPDATE worktree_deletions SET reported_at=1", [])
            .unwrap();
        let disk = f.host.disk_use().unwrap();
        let row = disk.tickets.iter().find(|t| t.ticket_id == building.id);
        assert_eq!(row.unwrap().state, TicketDiskState::Building);
        f.host.forget_worktree_deletion(&aside).unwrap();

        holder.kill().unwrap();
        holder.wait().unwrap();
        f.check(start + 30 * HOUR);
        assert!(!f.has_build_output(&held));
        assert!(f.has_build_output(&building));
    }

    #[test]
    fn a_turn_that_starts_while_its_ticket_is_scanned_keeps_its_build_output() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        f.host.reclaim_build_output(Some(&ticket.id)).unwrap();
        while f.host.build_output.finished.lock().unwrap().is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        f.turn(&ticket, now(), true);
        f.settle(now());
        assert!(f.has_build_output(&ticket));
        assert!(f.host.pending_deletes().unwrap().is_empty());
    }

    #[test]
    fn under_the_soft_floor_recently_used_build_output_is_reclaimed_too() {
        let mut f = Fixture::new();
        let start = now();
        let older = f.ticket("Older", 4096);
        f.turn(&older, start, false);
        let newer = f.ticket("Newer", 4096);
        f.turn(&newer, start + HOUR, false);
        f.check(start + 2 * HOUR);
        assert!(f.has_build_output(&older) && f.has_build_output(&newer));

        f.free.store(10 * GB, Ordering::SeqCst);
        f.check(start + 2 * HOUR);

        // Reclaiming a few kilobytes never reaches the floor, so both go.
        assert!(!f.has_build_output(&older) && !f.has_build_output(&newer));
        let notices = f.notices();
        assert_eq!(notices.len(), 2);
        let soft = "free space fell to 10.0 GB, below the 25 GB soft floor";
        assert!(notices.iter().all(|n| n.contains(soft)), "{notices:?}");
    }

    #[test]
    fn over_the_budget_the_least_recently_used_idle_tickets_go_until_it_is_under() {
        let disk = DiskSettings::default();
        let candidate = |ticket: &str, size: u64, last_active: i64, done: bool| Candidate {
            ticket: ticket.into(),
            size,
            last_active,
            done,
        };
        let at = 100 * HOUR;
        let recent = at - HOUR;
        let candidates = vec![
            candidate("newest", 30 * GB, recent, false),
            candidate("oldest", 20 * GB, recent - 3 * HOUR, false),
            candidate("middle", 25 * GB, recent - 2 * HOUR, false),
            candidate("done", 10 * GB, recent, true),
            candidate("idle", 5 * GB, at - 25 * HOUR, false),
        ];
        // 150 GB in all, with 60 GB in tickets in a turn: done and idle go first, then the
        // least recently used until 100 GB or less is left.
        let chosen = choose(candidates, 150 * GB, Some(400 * GB), &disk, at);
        assert_eq!(
            chosen,
            [
                ("idle".into(), Why::Idle),
                ("done".into(), Why::Done),
                ("oldest".into(), Why::Budget(150 * GB)),
                ("middle".into(), Why::Budget(150 * GB)),
            ]
        );
        // Within the budget and above the soft floor, only done and idle tickets go.
        let calm = || {
            vec![
                candidate("newest", 3 * GB, recent, false),
                candidate("oldest", 2 * GB, recent - HOUR, false),
            ]
        };
        assert!(choose(calm(), 90 * GB, Some(400 * GB), &disk, at).is_empty());
        // Under the soft floor, the least recently used go until it would be reached.
        assert_eq!(
            choose(calm(), 90 * GB, Some(24 * GB), &disk, at),
            [("oldest".into(), Why::SoftFloor(24 * GB))]
        );
    }

    #[test]
    fn the_inbox_says_builds_wait_once_under_the_hard_floor_and_clears_when_space_returns() {
        let mut f = Fixture::new();
        let open = |host: &Host| -> Vec<String> {
            let project = host.sessions().unwrap()[0].project_id.clone();
            (host.open_attention(&project).unwrap().into_iter())
                .filter(|a| a.operation_id.starts_with(BUILDS_HELD_OPERATION))
                .map(|a| a.prompt)
                .collect()
        };
        f.free.store(4 * GB, Ordering::SeqCst);
        f.check(now());
        f.check(now());
        let cards = open(&f.host);
        assert_eq!(cards.len(), 1);
        assert!(cards[0].starts_with("Builds are waiting: only 4.0 GB is free, under the 5 GB"));
        assert!(f.host.disk_use().unwrap().builds_held);
        f.free.store(30 * GB, Ordering::SeqCst);
        f.check(now());
        assert!(open(&f.host).is_empty());
        assert!(!f.host.disk_use().unwrap().builds_held);
    }

    #[test]
    fn removing_a_ticket_deletes_its_derived_data_and_scratch_folder() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        fs::remove_dir_all(Path::new(&ticket.worktree).join("target")).unwrap();
        let worktree = Path::new(&ticket.worktree);
        let derived = f.derived_data("App-a1", &worktree.join("App.xcworkspace"));
        // DerivedData holding a Git repository is ambiguous, as `scan` says, and is kept.
        let cloned = f.derived_data("Cloned-b2", &worktree.join("Cloned.xcodeproj"));
        git(&cloned, &["init", "-q", "clone"]);
        fs::write(cloned.join("clone/unsaved.txt"), "mine").unwrap();
        let scratch = f.host.scratch_folder(&ticket.id);

        f.host
            .with_worktrees_removed(std::slice::from_ref(&ticket), false, |_| Ok(()))
            .unwrap();
        f.settle(now());

        assert!(!worktree.exists());
        assert!(!derived.exists() && !scratch.exists());
        assert!(cloned.join("clone/unsaved.txt").exists());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
        // Nothing else in DerivedData or beside the scratch folder.
        assert_eq!(fs::read_dir(derived.parent().unwrap()).unwrap().count(), 1);
        assert_eq!(fs::read_dir(scratch.parent().unwrap()).unwrap().count(), 0);
    }

    #[test]
    fn a_removal_cut_off_after_its_worktree_moved_still_deletes_what_is_outside() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 0);
        let worktree = Path::new(&ticket.worktree);
        let derived = f.derived_data("App-a1", &worktree.join("App.xcworkspace"));
        let scratch = f.host.scratch_folder(&ticket.id);
        // What a removal records before it moves the worktree, then the move, then a stop.
        f.host.record_outside_cleanup(&ticket).unwrap();
        fs::rename(worktree, worktree.with_file_name(".removing-cut-off")).unwrap();

        f.reopen();
        f.settle(now());

        assert!(!derived.exists() && !scratch.exists());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
    }

    #[test]
    fn build_output_a_process_starts_using_after_its_scan_stays() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        let target = Path::new(&ticket.worktree).join("target");
        f.host.reclaim_build_output(Some(&ticket.id)).unwrap();
        // The scan has found nothing using it; then a build starts there.
        while f.host.build_output.finished.lock().unwrap().is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut build = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&target)
            .spawn()
            .unwrap();
        f.settle(now());
        build.kill().unwrap();
        build.wait().unwrap();

        assert!(target.join("debug/app").exists());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
        assert!(f.notices().is_empty());
    }

    #[test]
    fn a_build_the_gate_admitted_in_the_worktree_keeps_its_build_output() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        // A Cargo build working in the worktree, not in `target/`, holding a gate slot.
        let mut build = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&ticket.worktree)
            .spawn()
            .unwrap();
        let held = f.host.home.join("build-gate/held");
        fs::create_dir_all(&held).unwrap();
        fs::write(held.join(build.id().to_string()), "cargo test").unwrap();

        f.host.reclaim_build_output(Some(&ticket.id)).unwrap();
        f.settle(now());
        build.kill().unwrap();
        build.wait().unwrap();

        assert!(f.has_build_output(&ticket));
        assert!(f.host.scratch_folder(&ticket.id).exists());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
        assert!(f.notices().is_empty());
    }

    #[test]
    fn a_build_the_gate_admits_while_a_move_waits_for_its_lock_keeps_its_build_output() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        let gate = f.host.home.join("build-gate");
        fs::create_dir_all(gate.join("held")).unwrap();
        // The gate is admitting a build: it holds its lock.
        let lock = fs::File::create(gate.join("gate.lock")).unwrap();
        lock.lock().unwrap();
        f.host.reclaim_build_output(Some(&ticket.id)).unwrap();
        while f.host.pending_deletes().unwrap().is_empty() {
            f.host.settle_build_output(now());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // The move waits for the lock; the build is admitted, then the lock released.
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(f.has_build_output(&ticket));
        let mut build = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&ticket.worktree)
            .spawn()
            .unwrap();
        fs::write(gate.join("held").join(build.id().to_string()), "cargo").unwrap();
        drop(lock);
        f.settle(now());
        build.kill().unwrap();
        build.wait().unwrap();

        assert!(f.has_build_output(&ticket));
        assert_eq!(f.host.pending_deletes().unwrap(), []);
    }

    #[test]
    fn a_cleanup_whose_worktree_is_restored_while_it_waits_to_move_moves_nothing() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 0);
        let worktree = Path::new(&ticket.worktree);
        let derived = f.derived_data("App-a1", &worktree.join("App.xcworkspace"));
        let scratch = f.host.scratch_folder(&ticket.id);
        let away = worktree.with_file_name("away");
        f.host.record_outside_cleanup(&ticket).unwrap();
        fs::rename(worktree, &away).unwrap();
        // A restore is under way: the move waits for it.
        let moving = f.host.build_output.moving.clone();
        let restoring = moving.lock().unwrap();
        f.host.outside_cleanups_due();
        while f.host.pending_deletes().unwrap().is_empty() {
            f.host.settle_build_output(now());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        fs::rename(&away, worktree).unwrap();
        drop(restoring);
        f.settle(now());

        assert!(derived.exists() && scratch.exists());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
    }

    #[test]
    fn a_cleanup_with_a_folder_that_cannot_move_stays_recorded_for_it() {
        use std::os::unix::fs::PermissionsExt;
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 0);
        fs::remove_dir_all(Path::new(&ticket.worktree).join("target")).unwrap();
        let worktree = Path::new(&ticket.worktree);
        let derived = f.derived_data("App-a1", &worktree.join("App.xcworkspace"));
        let scratch = f.host.scratch_folder(&ticket.id);
        let root = derived.parent().unwrap().to_path_buf();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();

        f.host
            .with_worktrees_removed(std::slice::from_ref(&ticket), false, |_| Ok(()))
            .unwrap();
        f.settle(now());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(derived.exists() && !scratch.exists());
        let recorded: i64 = (f.host.db)
            .query_row("SELECT COUNT(*) FROM outside_cleanups", [], |r| r.get(0))
            .unwrap();
        assert_eq!(recorded, 1);
        // The next start finds what is left and moves it.
        f.reopen();
        f.settle(now());
        assert!(!derived.exists());
    }

    #[test]
    fn a_cleanup_whose_worktree_is_back_is_dropped_and_moves_nothing() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 0);
        let worktree = Path::new(&ticket.worktree);
        let derived = f.derived_data("App-a1", &worktree.join("App.xcworkspace"));
        let scratch = f.host.scratch_folder(&ticket.id);
        // A removal recorded its cleanup and was cut off, then a restore re-created the
        // worktree, as if the worktree had been back at its path all along.
        f.host.record_outside_cleanup(&ticket).unwrap();

        f.reopen();
        f.settle(now());

        assert!(derived.exists() && scratch.exists());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
        let recorded: i64 = (f.host.db)
            .query_row("SELECT COUNT(*) FROM outside_cleanups", [], |r| r.get(0))
            .unwrap();
        assert_eq!(recorded, 0);
    }

    #[test]
    fn a_cleanup_that_cannot_move_is_retried_while_the_host_runs_and_told_after_three_attempts() {
        use std::os::unix::fs::PermissionsExt;
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 0);
        fs::remove_dir_all(Path::new(&ticket.worktree).join("target")).unwrap();
        let worktree = Path::new(&ticket.worktree);
        let derived = f.derived_data("App-a1", &worktree.join("App.xcworkspace"));
        let root = derived.parent().unwrap().to_path_buf();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
        let recorded = |host: &Host| -> Option<(i64, i64)> {
            (host.db)
                .query_row(
                    "SELECT attempts,next_attempt_at FROM outside_cleanups",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .unwrap()
        };
        let cards = |host: &Host| -> Vec<String> {
            let project = host.sessions().unwrap()[0].project_id.clone();
            (host.open_attention(&project).unwrap().into_iter())
                .filter(|a| a.operation_id.starts_with(DELETE_FAILED_OPERATION))
                .map(|a| a.prompt)
                .collect()
        };

        f.host
            .with_worktrees_removed(std::slice::from_ref(&ticket), false, |_| Ok(()))
            .unwrap();
        let start = now();
        f.settle(start);
        let (attempts, next) = recorded(&f.host).unwrap();
        assert_eq!(attempts, 1);
        assert!(next > start);
        // Not before its wait, then again at it, until the third failure is told.
        f.settle(next - 1);
        assert_eq!(recorded(&f.host).unwrap().0, 1);
        f.settle(next);
        assert!(cards(&f.host).is_empty());
        f.settle(recorded(&f.host).unwrap().1);
        assert_eq!(recorded(&f.host).unwrap().0, 3);
        let told = cards(&f.host);
        assert_eq!(told.len(), 1);
        assert!(
            told[0].contains("\"Toolbar\" outside its worktree after 3 attempts"),
            "{told:?}"
        );
        // DerivedData becomes writable; the next retry moves it, with no restart.
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        f.settle(recorded(&f.host).unwrap().1);

        assert!(!derived.exists());
        assert_eq!(recorded(&f.host), None);
        assert_eq!(f.host.pending_deletes().unwrap(), []);
    }

    #[test]
    fn a_move_whose_deletes_cannot_all_be_recorded_leaves_nothing_reclaiming() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        // The store refuses the second folder's delete, after the first was recorded and held.
        (f.host.db)
            .execute_batch(
                "CREATE TEMP TRIGGER refuse BEFORE INSERT ON worktree_deletions
                 WHEN (SELECT COUNT(*) FROM worktree_deletions) > 0
                 BEGIN SELECT RAISE(ABORT, 'the store refused it'); END",
            )
            .unwrap();
        f.host.reclaim_build_output(Some(&ticket.id)).unwrap();
        while f.host.build_output.finished.lock().unwrap().is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        f.host.settle_build_output(now());

        assert!(f.host.build_output.inspecting.is_empty());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
        assert!(f.has_build_output(&ticket));
        // Nothing waits for a restart: the next reclaim goes through.
        f.host.db.execute_batch("DROP TRIGGER refuse").unwrap();
        f.host.reclaim_build_output(Some(&ticket.id)).unwrap();
        f.settle(now());
        assert!(!f.has_build_output(&ticket));
    }

    #[test]
    fn a_turn_that_starts_while_a_move_waits_for_its_last_look_keeps_its_build_output() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        // A restore is under way, so the move waits before its last look.
        let moving = f.host.build_output.moving.clone();
        let restoring = moving.lock().unwrap();
        f.host.reclaim_build_output(Some(&ticket.id)).unwrap();
        while f.host.pending_deletes().unwrap().is_empty() {
            f.host.settle_build_output(now());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Only the move's own read of the store can see this turn.
        f.turn(&ticket, now(), true);
        drop(restoring);
        f.settle(now());

        assert!(f.has_build_output(&ticket));
        assert!(f.host.scratch_folder(&ticket.id).exists());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
        assert!(f.notices().is_empty());
    }

    #[test]
    fn build_output_a_process_uses_once_it_moved_is_moved_back() {
        let mut f = Fixture::new();
        let ticket = f.ticket("Toolbar", 4096);
        let worktree = PathBuf::from(&ticket.worktree);
        let target = worktree.join("target");
        let aside = (f.host)
            .hold_deletion(worktree.parent().unwrap(), &ticket.id)
            .unwrap();
        fs::rename(&target, &aside).unwrap();
        assert!(put_back_if_used(&[(&target, &aside, 4096)]).is_none());
        // A build starts in the build output just after it moved.
        let mut build = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&aside)
            .spawn()
            .unwrap();
        let moved = put_back_if_used(&[(&target, &aside, 4096)]);
        build.kill().unwrap();
        build.wait().unwrap();
        let moved = moved.unwrap();
        assert!(matches!(&moved, Moved::PutBack(_, stuck) if stuck.is_empty()));
        let job = Move {
            ticket: ticket.id.clone(),
            purpose: Purpose::Reclaim(Why::Asked),
            worktree,
            folders: vec![(target.clone(), aside.clone(), 4096)],
            home: f.host.home.clone(),
            moving: f.host.build_output.moving.clone(),
        };
        f.host.finish_move_aside(job, moved, now()).unwrap();

        assert!(target.join("debug/app").exists() && !aside.exists());
        assert_eq!(f.host.pending_deletes().unwrap(), []);
        assert!(f.notices().is_empty());
    }

    #[test]
    fn the_disk_settings_are_saved_only_when_valid() {
        let mut f = Fixture::new();
        let valid = DiskSettings {
            budget_gb: 50,
            idle_hours: 12,
            soft_floor_gb: 20,
            hard_floor_gb: 2,
        };
        assert_eq!(f.host.set_disk_settings(valid).unwrap().disk, valid);
        for invalid in [
            DiskSettings {
                budget_gb: 0,
                ..valid
            },
            DiskSettings {
                idle_hours: 0,
                ..valid
            },
            DiskSettings {
                idle_hours: MAX_IDLE_HOURS + 1,
                ..valid
            },
            DiskSettings {
                hard_floor_gb: 0,
                ..valid
            },
            DiskSettings {
                soft_floor_gb: 2,
                ..valid
            },
        ] {
            assert!(f.host.set_disk_settings(invalid).is_err(), "{invalid:?}");
        }
        assert_eq!(f.host.settings().disk, valid);
        // The build gate reads the hard floor from the same file.
        let saved: Value =
            serde_json::from_slice(&fs::read(f.host.home.join(settings::FILE)).unwrap()).unwrap();
        assert_eq!(saved["disk"]["hard_floor_gb"], 2);
    }
}
