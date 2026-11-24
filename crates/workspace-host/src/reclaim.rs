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
/// The inbox operation of the card saying builds wait for space, followed by when it began.
const BUILDS_HELD: &str = "builds-held:";

/// A disk's free and total bytes, or `None` when it cannot be read.
type Space = Arc<dyn Fn(&Path) -> Option<(u64, u64)> + Send + Sync>;

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
    Inspected(String, Why, Inspected),
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
    /// Free space is under the hard floor; whether the human's inbox says so.
    held: Option<bool>,
    finished: Arc<Mutex<Vec<Finished>>>,
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Default for Watch {
    fn default() -> Self {
        // Tests never reach the user's real DerivedData or temporary folder.
        let (derived_data, scratch_root) = if cfg!(test) {
            (
                None,
                std::env::temp_dir().join("wiffletree-test-scratch-unused"),
            )
        } else {
            (build_output::xcode_derived_data(), std::env::temp_dir())
        };
        Self {
            derived_data,
            scratch_root,
            space: Arc::new(disk::space),
            started: false,
            check_at: 0,
            survey: None,
            surveying: false,
            inspecting: HashSet::new(),
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
            bytes INTEGER NOT NULL)",
    )?;
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

/// A ticket whose build output may be reclaimed now: no agent of it is in a turn.
struct Candidate {
    ticket: String,
    size: u64,
    last_turn: Option<i64>,
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
    candidates.sort_by_key(|c| c.last_turn.unwrap_or(0));
    let (mut chosen, mut rest) = (vec![], vec![]);
    let mut left = total;
    for candidate in candidates {
        let idle = at - candidate.last_turn.unwrap_or(0) >= disk.idle_ms();
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
    /// Starts watching disk use, telling `notify` as each measurement or scan finishes.
    pub(crate) fn watch_build_output(&mut self, notify: impl Fn() + Send + Sync + 'static) {
        let watch = &mut self.build_output;
        watch.notify = Some(Arc::new(notify));
        watch.started = true;
        watch.check_at = 0;
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
                Finished::Inspected(ticket, why, inspected) => {
                    self.build_output.inspecting.remove(&ticket);
                    self.finish_reclaim(&ticket, why, inspected, at)
                }
            };
            if let Err(error) = settled {
                eprintln!("Reclaim build output: {error:#}");
            }
        }
        if at >= self.build_output.check_at {
            self.build_output.check_at = at + CHECK_MS;
            if let Err(error) = self.check_disk(at) {
                eprintln!("Check disk space: {error:#}");
            }
        }
        Some(self.build_output.check_at)
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
                last_turn: turns.get(&ticket.id).copied(),
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
            Finished::Inspected(folders.ticket, why, inspected)
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

    /// Moves aside and records the delete of each folder a scan found to reclaim, unless an
    /// agent of the ticket started a turn meanwhile, and tells the ticket's project.
    fn finish_reclaim(&mut self, id: &str, why: Why, inspected: Inspected, at: i64) -> Result<()> {
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
        if self.building(&ticket)? {
            eprintln!("Reclaim build output of {id}: kept, an agent started a turn");
            return Ok(());
        }
        let worktree = Path::new(&ticket.worktree);
        let mut freed = 0;
        for (folder, size) in folders {
            // Build output in the worktree waits beside the worktree, never in it.
            let beside = match folder.starts_with(worktree) {
                true => worktree.parent(),
                false => folder.parent(),
            };
            let Some(beside) = beside else { continue };
            match self.delete_durably(&folder, beside, id) {
                Ok(()) => freed += size,
                Err(error) => eprintln!("Reclaim build output of {id}: {error:#}"),
            }
        }
        if freed == 0 {
            return Ok(());
        }
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
        let id = format!("reclaim:{}:{at}", ticket.id);
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
            .query_map([BUILDS_HELD], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if !under {
            self.build_output.held = None;
            for id in open {
                self.resolve_attention(&id, "Disk space returned")?;
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
        let told = self.tell_inbox(None, &format!("{BUILDS_HELD}{at}"), &notice)?;
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
            let state = if failing {
                TicketDiskState::FailingToDelete
            } else if deleting.count() > 0 || self.build_output.inspecting.contains(&ticket.id) {
                TicketDiskState::Reclaiming
            } else if self.building(&ticket)? {
                TicketDiskState::Building
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
            tickets,
            deletes,
        })
    }

    /// Deletes the ticket's build output outside its worktree, as its worktree is removed:
    /// Xcode's DerivedData folders for its path, listed by `derived_data`, and its scratch
    /// folder. Each folder waits beside itself. Failures are logged; the removal goes on.
    pub(crate) fn delete_outside_build_output(
        &mut self,
        ticket: &Ticket,
        derived_data: Vec<PathBuf>,
    ) {
        let scratch = self.scratch_folder(&ticket.id);
        for folder in derived_data.into_iter().chain([scratch]) {
            if !is_real_folder(&folder) {
                continue;
            }
            let Some(beside) = folder.parent() else {
                continue;
            };
            if let Err(error) = self.delete_durably(&folder, beside, &ticket.id) {
                eprintln!("Remove {} of {}: {error:#}", folder.display(), ticket.id);
            }
        }
    }
    /// Xcode's DerivedData folders for the ticket's worktree, read before the worktree moves.
    pub(crate) fn derived_data_of(&self, ticket: &Ticket) -> Vec<PathBuf> {
        (self.build_output.derived_data.as_deref())
            .map(|root| build_output::derived_data_for(Path::new(&ticket.worktree), root))
            .unwrap_or_default()
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
            let free = Arc::new(AtomicU64::new(400 * GB));
            let reading = free.clone();
            let watch = &mut host.build_output;
            watch.derived_data = Some(folder.path().join("DerivedData"));
            watch.scratch_root = folder.path().join("scratch");
            watch.space = Arc::new(move |_| Some((reading.load(Ordering::SeqCst), 500 * GB)));
            watch.started = true;
            Self {
                folder,
                host,
                coordinator,
                repository,
                free,
            }
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

        f.check(start + 30 * HOUR);

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
        let candidate = |ticket: &str, size: u64, last_turn: i64, done: bool| Candidate {
            ticket: ticket.into(),
            size,
            last_turn: Some(last_turn),
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
                .filter(|a| a.operation_id.starts_with(BUILDS_HELD))
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
        let derived = f.folder.path().join("DerivedData/App-a1");
        fs::create_dir_all(derived.join("Build")).unwrap();
        let project = Path::new(&ticket.worktree).join("App.xcworkspace");
        fs::write(
            derived.join("info.plist"),
            format!(
                "<key>WorkspacePath</key>\n<string>{}</string>",
                project.display()
            ),
        )
        .unwrap();
        let scratch = f.host.scratch_folder(&ticket.id);

        f.host
            .with_worktrees_removed(std::slice::from_ref(&ticket), |_| Ok(()))
            .unwrap();
        f.settle(now());

        assert!(!Path::new(&ticket.worktree).exists());
        assert!(!derived.exists() && !scratch.exists());
        assert!(f.host.pending_deletes().unwrap().is_empty());
        // Nothing else in DerivedData or beside the scratch folder.
        assert_eq!(fs::read_dir(derived.parent().unwrap()).unwrap().count(), 0);
        assert_eq!(fs::read_dir(scratch.parent().unwrap()).unwrap().count(), 0);
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
