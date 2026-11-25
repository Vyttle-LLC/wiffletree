//! Durable deletes. A folder to delete is first recorded, then renamed aside to
//! `.removing-<ticket id>[-<time>[-<n>]]` on the same filesystem, and deleted off the host thread.
//! A removed worktree waits beside where it was; reclaimed build output waits beside its
//! worktree, never inside it, and Xcode's DerivedData folders and scratch folders wait beside
//! themselves. Because each folder is recorded before it is renamed, a host that stops partway
//! through a delete resumes it at its next start. A failed delete is retried with backoff while
//! the host runs and again at every start, and after repeated failures the human's inbox is told
//! once which folder failed, why and how big it is. A record is cleared only once its folder is
//! proven gone. Only folders named with the `.removing-` prefix are ever deleted. A folder still
//! used by a process, as its working directory or through an open file, is not deleted, since
//! its open files would keep their space: the ticket's recorded processes are stopped once its
//! worktree is removed, and any other holder is named in the failure and waited out, never
//! signalled.
use crate::*;
use std::{
    collections::HashSet,
    os::unix::fs::MetadataExt,
    sync::{Arc, Mutex},
};

/// The name a folder waiting to be deleted takes, followed by its ticket's id.
pub(crate) const REMOVING_PREFIX: &str = ".removing-";
/// Failed attempts after which the human is told.
pub(crate) const REPORT_AFTER: i64 = 3;
/// The next attempt of a held delete; see `hold_deletion`.
const HELD: i64 = i64::MAX;
/// The wait after the first failed attempt; it doubles with each further one.
const FIRST_RETRY_MS: i64 = 15_000;
const LONGEST_RETRY_MS: i64 = 10 * 60_000;

/// A failed attempt: why, and how much the folder still holds on disk.
type Failure = (String, u64);

/// A delete's folder and what its attempt did.
type Finished = (PathBuf, Result<(), Failure>);

/// The deletes running off the host thread.
#[derive(Default)]
pub(crate) struct Running {
    folders: HashSet<PathBuf>,
    /// Each delete's outcome, put here before `notify` is called.
    finished: Arc<Mutex<Vec<Finished>>>,
    /// Called from a delete's thread as it finishes, so the service settles it promptly.
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
    /// When a waiting delete next comes due; nothing is read before then unless one finished.
    due_at: i64,
    /// The disk work raised or answered an inbox item since the service last told clients.
    pub(crate) inbox_changed: bool,
}

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS worktree_deletions (
            path TEXT PRIMARY KEY,
            ticket_id TEXT,
            recorded_at INTEGER NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            next_attempt_at INTEGER NOT NULL,
            reported_at INTEGER,
            size INTEGER)",
    )?;
    let sized: i64 = db.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('worktree_deletions') WHERE name='size'",
        [],
        |r| r.get(0),
    )?;
    if sized == 0 {
        db.execute_batch("ALTER TABLE worktree_deletions ADD COLUMN size INTEGER")?;
    }
    Ok(())
}

/// The wait before the next attempt after `attempts` failed ones.
pub(crate) fn backoff(attempts: i64) -> i64 {
    let doublings = u32::try_from(attempts.saturating_sub(1)).unwrap_or(u32::MAX);
    FIRST_RETRY_MS
        .saturating_mul(2_i64.saturating_pow(doublings))
        .min(LONGEST_RETRY_MS)
}

fn is_removal_folder(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(REMOVING_PREFIX) && name.len() > REMOVING_PREFIX.len())
}

/// Proven absent. A path whose metadata cannot be read, such as behind a folder without search
/// permission, is not.
fn is_gone(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}

/// Whether `path` is one of `folders` or inside one; all canonical.
fn within(path: &Path, folders: &[PathBuf]) -> bool {
    folders.iter().any(|folder| path.starts_with(folder))
}

/// This user's processes whose working directory or an open file is inside one of `folders`,
/// each as "pid (command)". Deleting under them would leave their open files' space allocated,
/// and moving a folder from under a build would break it. The folders must be canonical.
#[cfg(target_os = "macos")]
pub(crate) fn holders(folders: &[PathBuf]) -> Result<Vec<String>, String> {
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() }.to_string();
    let listed = std::process::Command::new("lsof")
        .args(["-nPw", "-u", &uid, "-F", "pcn"])
        .output()
        .map_err(|e| format!("could not list open files with lsof: {e}"))?;
    let mut holders = vec![];
    let (mut pid, mut command) = (String::new(), String::new());
    for line in String::from_utf8_lossy(&listed.stdout).lines() {
        match line.split_at_checked(1) {
            Some(("p", value)) => (pid, command) = (value.to_owned(), String::new()),
            Some(("c", value)) => command = value.to_owned(),
            Some(("n", name)) if within(Path::new(name), folders) => {
                let holder = format!("{pid} ({command})");
                if !holders.contains(&holder) {
                    holders.push(holder);
                }
            }
            _ => {}
        }
    }
    Ok(holders)
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn holders(folders: &[PathBuf]) -> Result<Vec<String>, String> {
    let mut holders = vec![];
    for process in fs::read_dir("/proc").map_err(|e| e.to_string())?.flatten() {
        let pid = process.file_name().to_string_lossy().into_owned();
        if pid.parse::<u32>().is_err() {
            continue;
        }
        let proc = process.path();
        let open = (fs::read_dir(proc.join("fd"))
            .into_iter()
            .flatten()
            .flatten())
        .filter_map(|fd| fs::read_link(fd.path()).ok());
        if fs::read_link(proc.join("cwd"))
            .into_iter()
            .chain(open)
            .any(|path| within(&path, folders))
        {
            let command = fs::read_to_string(proc.join("comm")).unwrap_or_default();
            holders.push(format!("{pid} ({})", command.trim()));
        }
    }
    Ok(holders)
}

/// Deletes the folder unless a process still uses it.
fn attempt(folder: &Path) -> Result<(), String> {
    let canonical = fs::canonicalize(folder).map_err(|e| e.to_string())?;
    let holders = holders(&[canonical])?;
    if !holders.is_empty() {
        return Err(format!(
            "processes still use files in it: {}",
            holders.join(", ")
        ));
    }
    let deleted = fs::remove_dir_all(folder);
    match is_gone(folder) {
        true => Ok(()),
        false => Err(deleted.map_or_else(
            |error| error.to_string(),
            |()| "the folder was there again afterwards".into(),
        )),
    }
}

/// Bytes allocated on disk to everything under `path`, as far as it can be read.
pub(crate) fn allocated_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    let own = metadata.blocks() * 512;
    if !metadata.is_dir() {
        return own;
    }
    let entries = fs::read_dir(path).into_iter().flatten().flatten();
    own + entries.map(|e| allocated_size(&e.path())).sum::<u64>()
}

/// A size as macOS shows it, in decimal gigabytes or megabytes.
pub(crate) fn size_label(bytes: u64) -> String {
    let (gb, mb) = (workspace_core::GB as f64, 1_000_000.0);
    match bytes as f64 {
        b if b >= gb => format!("{:.1} GB", b / gb),
        b => format!("{:.1} MB", b / mb),
    }
}

/// Attempts the delete on a thread of its own, leaving its outcome in `finished` before
/// `notify` is called.
fn delete(
    folder: PathBuf,
    finished: Arc<Mutex<Vec<Finished>>>,
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
) {
    std::thread::spawn(move || {
        let attempted = std::panic::catch_unwind(|| attempt(&folder))
            .unwrap_or_else(|_| Err("the delete panicked".into()));
        let outcome = attempted.map_err(|error| (error, allocated_size(&folder)));
        finished
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((folder, outcome));
        if let Some(notify) = notify {
            notify();
        }
    });
}

impl Host {
    /// Records that `folder`, a removed worktree's, is to be deleted. Called before the folder
    /// is renamed there, so a stop at any point leaves the delete recorded.
    pub(crate) fn record_worktree_deletion(
        &mut self,
        folder: &Path,
        ticket: Option<&str>,
    ) -> Result<()> {
        ensure!(
            is_removal_folder(folder),
            "{} is not a removed worktree's folder",
            folder.display()
        );
        let at = now();
        self.db.execute(
            "INSERT INTO worktree_deletions(path,ticket_id,recorded_at,next_attempt_at) VALUES (?1,?2,?3,?3)
             ON CONFLICT(path) DO UPDATE SET next_attempt_at=excluded.next_attempt_at",
            params![folder.to_string_lossy(), ticket, at],
        )?;
        self.worktree_deletions.due_at = 0;
        Ok(())
    }
    /// Records the delete of a folder of `ticket` that is to be renamed into `beside`, which
    /// must be on its filesystem and outside any worktree it should leave clean, and returns
    /// where it is to wait. The delete is held, so nothing deletes or forgets it while the
    /// rename happens off the host thread, until `release_deletion`, or `forget_worktree_deletion`
    /// if it never moved. A start releases every held delete, and drops one whose folder never
    /// moved as gone.
    pub(crate) fn hold_deletion(&mut self, beside: &Path, ticket: &str) -> Result<PathBuf> {
        // Neither on disk nor held for a folder still to move there.
        let at = now();
        let mut names = (0..).map(|n| beside.join(format!("{REMOVING_PREFIX}{ticket}-{at}-{n}")));
        let aside = loop {
            let name = names.next().expect("an unused name exists");
            let recorded: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM worktree_deletions WHERE path=?1)",
                [name.to_string_lossy()],
                |r| r.get(0),
            )?;
            if !recorded && fs::symlink_metadata(&name).is_err() {
                break name;
            }
        };
        self.record_worktree_deletion(&aside, Some(ticket))?;
        self.db.execute(
            "UPDATE worktree_deletions SET next_attempt_at=?2 WHERE path=?1",
            params![aside.to_string_lossy(), HELD],
        )?;
        Ok(aside)
    }
    /// Lets a held delete start now.
    pub(crate) fn release_deletion(&mut self, folder: &Path) -> Result<()> {
        self.db.execute(
            "UPDATE worktree_deletions SET next_attempt_at=?2 WHERE path=?1",
            params![folder.to_string_lossy(), now()],
        )?;
        self.worktree_deletions.due_at = 0;
        Ok(())
    }
    /// Makes the waiting delete of `folder` due now, as the human asked. A held delete stays
    /// held until its folder has moved.
    pub fn retry_delete(&mut self, folder: &Path) -> Result<()> {
        let changed = self.db.execute(
            "UPDATE worktree_deletions SET next_attempt_at=?2 WHERE path=?1 AND next_attempt_at<?3",
            params![folder.to_string_lossy(), now(), HELD],
        )?;
        ensure!(
            changed == 1,
            "{} is not waiting to be deleted",
            folder.display()
        );
        self.worktree_deletions.due_at = 0;
        Ok(())
    }
    /// The folders waiting to be deleted, oldest first.
    pub(crate) fn pending_deletes(&self) -> Result<Vec<PendingDelete>> {
        Ok(self
            .db
            .prepare(
                "SELECT path,ticket_id,attempts,last_error,next_attempt_at,reported_at IS NOT NULL,size
                 FROM worktree_deletions ORDER BY recorded_at",
            )?
            .query_map([], |r| {
                Ok(PendingDelete {
                    path: r.get(0)?,
                    ticket_id: r.get(1)?,
                    attempts: r.get(2)?,
                    last_error: r.get(3)?,
                    next_attempt_at: r.get(4)?,
                    failing: r.get(5)?,
                    size: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Drops the record of a folder a removal put back or never moved.
    pub(crate) fn forget_worktree_deletion(&self, folder: &Path) -> Result<()> {
        self.db.execute(
            "DELETE FROM worktree_deletions WHERE path=?1",
            [folder.to_string_lossy()],
        )?;
        Ok(())
    }
    /// Has the service hear as each delete finishes.
    pub(crate) fn notify_worktree_deletions(&mut self, notify: impl Fn() + Send + Sync + 'static) {
        self.worktree_deletions.notify = Some(Arc::new(notify));
    }
    /// Whether a delete is running off the host thread.
    pub fn worktree_deletions_running(&self) -> bool {
        !self.worktree_deletions.folders.is_empty()
    }
    /// At startup: records every removed worktree's folder found beside a ticket's worktree or
    /// in a project's repository folder under the workspaces folder, whether or not anything
    /// records it, and makes every recorded delete due now.
    pub(crate) fn sweep_worktree_deletions(&mut self) -> Result<()> {
        let mut folders: Vec<PathBuf> = self
            .tickets()?
            .iter()
            .filter_map(|t| Path::new(&t.worktree).parent().map(Path::to_path_buf))
            .collect();
        let tasks = Path::new(&self.settings().workspaces_dir).join("tasks");
        for project in fs::read_dir(&tasks).into_iter().flatten().flatten() {
            folders.extend(
                (fs::read_dir(project.path()).into_iter().flatten().flatten())
                    .map(|repository| repository.path()),
            );
        }
        folders.sort();
        folders.dedup();
        for folder in folders {
            for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
                let path = entry.path();
                // The listing's own type, which is never a link's target.
                let folder = entry.file_type().is_ok_and(|t| t.is_dir());
                if !is_removal_folder(&path) || !folder {
                    continue;
                }
                // `<ticket id>`, or that followed by `-<time>` or `-<time>-<n>`.
                let name = entry.file_name().to_string_lossy()[REMOVING_PREFIX.len()..].to_owned();
                let shortened = name.match_indices('-').map(|(end, _)| &name[..end]);
                let ticket = std::iter::once(name.as_str())
                    .chain(shortened.rev())
                    .find(|id| self.ticket(id).is_ok())
                    .map(str::to_owned);
                self.record_worktree_deletion(&path, ticket.as_deref())?;
            }
        }
        self.db
            .execute("UPDATE worktree_deletions SET next_attempt_at=?1", [now()])?;
        self.worktree_deletions.due_at = 0;
        Ok(())
    }
    /// Records what each finished delete did and starts those due at `at`. Returns when the
    /// next waiting one comes due. The service calls this after every event; it reads the store
    /// only when a delete has finished or one is due. Failures are logged, never propagated.
    pub fn settle_worktree_deletions(&mut self, at: i64) -> Option<i64> {
        let finished = std::mem::take(
            &mut *(self.worktree_deletions.finished)
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let waiting = self.worktree_deletions.due_at;
        if finished.is_empty() && at < waiting {
            return (waiting < i64::MAX).then_some(waiting);
        }
        for (folder, outcome) in finished {
            self.worktree_deletions.folders.remove(&folder);
            if let Err(error) = self.record_attempt(&folder, outcome, at) {
                eprintln!("Delete removed worktree {}: {error:#}", folder.display());
            }
        }
        match self.start_due_deletions(at) {
            Ok(due) => {
                self.worktree_deletions.due_at = due.unwrap_or(i64::MAX);
                due
            }
            Err(error) => {
                eprintln!("Delete removed worktrees: {error:#}");
                self.worktree_deletions.due_at = at + FIRST_RETRY_MS;
                Some(self.worktree_deletions.due_at)
            }
        }
    }
    fn record_attempt(
        &mut self,
        folder: &Path,
        outcome: Result<(), Failure>,
        at: i64,
    ) -> Result<()> {
        let Err((error, size)) = outcome else {
            // A folder back at the path since the delete is left recorded, and due again.
            if is_gone(folder) {
                self.forget_worktree_deletion(folder)?;
            }
            return Ok(());
        };
        let path = folder.to_string_lossy();
        let (ticket, recorded_at, attempts, reported): (Option<String>, i64, i64, bool) =
            self.db.query_row(
                "SELECT ticket_id,recorded_at,attempts+1,reported_at IS NOT NULL
                 FROM worktree_deletions WHERE path=?1",
                [&path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        self.db.execute(
            "UPDATE worktree_deletions SET attempts=?2,last_error=?3,next_attempt_at=?4,size=?5 WHERE path=?1",
            params![path, attempts, error, at + backoff(attempts), size],
        )?;
        eprintln!("Delete {} (attempt {attempts}): {error}", folder.display());
        if attempts < REPORT_AFTER || reported {
            return Ok(());
        }
        let notice = format!(
            "Wiffletree could not delete the folder {} ({}) after {attempts} attempts: {error}. It keeps retrying, and at every start. Deleting the folder yourself frees the space.",
            folder.display(),
            size_label(size)
        );
        let id = delete_failed_operation(recorded_at, &path);
        let project = ticket
            .and_then(|id| self.ticket(&id).ok())
            .and_then(|ticket| self.session(&ticket.coordinator_id).ok())
            .map(|coordinator| coordinator.project_id);
        if !self.tell_inbox(project.as_deref(), &id, &notice)? {
            // The disk use read still lists the failing delete.
            eprintln!("{notice}");
            return Ok(());
        }
        self.db.execute(
            "UPDATE worktree_deletions SET reported_at=?2 WHERE path=?1",
            params![path, at],
        )?;
        Ok(())
    }
    /// The session whose inbox tells the human about this machine: `project`'s main
    /// coordinator while it is unarchived, else the newest unarchived main coordinator of any
    /// project. `None` when there is no such coordinator.
    pub(crate) fn inbox_session(&self, project: Option<&str>) -> Result<Option<Session>> {
        let coordinators: Vec<Session> = (self.sessions()?.into_iter())
            .filter(|s| {
                s.parent_id.is_none()
                    && s.role == Role::ProjectOrchestrator
                    && !s.archived
                    && self.project(&s.project_id).is_ok()
            })
            .collect();
        let own = coordinators
            .iter()
            .find(|s| Some(s.project_id.as_str()) == project);
        Ok(own.or(coordinators.last()).cloned())
    }
    /// Raises `notice` in the inbox of `inbox_session(project)` under `operation`. Returns
    /// whether there was an inbox to tell.
    pub(crate) fn tell_inbox(
        &mut self,
        project: Option<&str>,
        operation: &str,
        notice: &str,
    ) -> Result<bool> {
        let Some(session) = self.inbox_session(project)? else {
            return Ok(false);
        };
        // Attention prompts allow 4096 bytes; cut on a character boundary within that.
        let notice = &notice[..steps::floor_boundary(notice, notice.len().min(4096))];
        self.request_attention(&session.id, "local", operation, notice, &[])?;
        self.worktree_deletions.inbox_changed = true;
        Ok(true)
    }
    /// Whether the disk work raised or answered an inbox item since this was last asked, so
    /// the service tells clients.
    pub(crate) fn take_inbox_changed(&mut self) -> bool {
        std::mem::take(&mut self.worktree_deletions.inbox_changed)
    }
    /// Starts each recorded delete due at `at` that is not already running, and returns when
    /// the earliest of the rest comes due.
    fn start_due_deletions(&mut self, at: i64) -> Result<Option<i64>> {
        let recorded: Vec<(String, Option<String>, i64)> = self
            .db
            .prepare("SELECT path,ticket_id,next_attempt_at FROM worktree_deletions ORDER BY recorded_at")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut due = None::<i64>;
        for (path, ticket, next) in recorded {
            let folder = PathBuf::from(path);
            if self.worktree_deletions.folders.contains(&folder) || next == HELD {
                continue;
            }
            if is_gone(&folder) {
                self.forget_worktree_deletion(&folder)?;
                continue;
            }
            if next > at {
                due = Some(due.map_or(next, |d| d.min(next)));
                continue;
            }
            if let Some(ticket) = ticket
                && let Err(error) = self.stop_processes_in_removed_worktree(&ticket)
            {
                eprintln!("Stop processes of ticket {ticket}: {error:#}");
            }
            let running = &mut self.worktree_deletions;
            let (finished, notify) = (running.finished.clone(), running.notify.clone());
            delete(folder.clone(), finished, notify);
            running.folders.insert(folder);
        }
        Ok(due)
    }
    /// Stops what the ticket's agents started with their tools and left running, such as a
    /// build that outlived its agent and still writes into the removed folder. Only the groups
    /// Wiffletree recorded for those agents are ever signalled, and only while the ticket has no
    /// worktree of its own again and no agent of it is in a turn, so nothing that could be
    /// working elsewhere for the ticket is touched.
    fn stop_processes_in_removed_worktree(&mut self, ticket_id: &str) -> Result<()> {
        let ticket = self.ticket(ticket_id)?;
        if Path::new(&ticket.worktree).exists() {
            return Ok(());
        }
        if self.ticket_run_open(ticket_id)? {
            return Ok(());
        }
        let agents: Vec<String> = self
            .ticket_agents(ticket_id)?
            .into_iter()
            .map(|a| a.id)
            .collect();
        self.stop_processes(&agents, agent_processes::StopReason::WorktreeRemoved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_back_off_from_fifteen_seconds_to_ten_minutes() {
        let waits: Vec<i64> = (1..=8).map(backoff).collect();
        assert_eq!(
            waits,
            [
                15_000, 30_000, 60_000, 120_000, 240_000, 480_000, 600_000, 600_000
            ]
        );
    }

    #[test]
    fn only_folders_named_as_removals_are_recorded() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        for refused in [
            "/work/web/toolbar",
            "/work/web/.removing-",
            "/work/web/removing-1",
        ] {
            assert!(
                host.record_worktree_deletion(Path::new(refused), None)
                    .is_err()
            );
        }
        host.record_worktree_deletion(Path::new("/work/web/.removing-1"), None)
            .unwrap();
    }

    /// What a removal leaves behind: a ticket whose agent's finished run recorded a process
    /// group, standing in for a build that outlived its turn.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_removed_worktrees_processes_are_stopped_only_with_no_worktree_back_and_no_turn() {
        use crate::test_support::git;
        use std::os::unix::process::CommandExt;
        let folder = tempfile::tempdir().unwrap();
        let main = folder.path().join("web");
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "start"]);
        let mut host = Host::open_set_up(folder.path().join("home")).unwrap();
        host.set_workspaces_dir(folder.path().join("w").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Stops").unwrap();
        let coordinator = host.sessions().unwrap().remove(0).id;
        let repository = (host.attach_repository(&project.id, main.to_str().unwrap(), "HEAD"))
            .unwrap()
            .id;
        let ticket = host
            .create_ticket(&coordinator, &repository, "Toolbar", "Do")
            .unwrap();
        let agent = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
            .unwrap();
        let run = |host: &Host, id: &str, finished: Option<i64>| {
            host.db.execute(
                "INSERT OR REPLACE INTO provider_runs(id,session_id,messages,started_at,finished_at,detail) VALUES (?1,?2,'[]',1,?3,'{}')",
                params![id, agent.id, finished],
            )
            .unwrap();
        };
        run(&host, "earlier", Some(2));
        let mut build = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let seen = processes::read(build.id() as i32).unwrap();
        host.record_sightings(&agent.id, "earlier", &[processes::Sighting::Member(seen)])
            .unwrap();
        let running = |build: &mut std::process::Child| build.try_wait().unwrap().is_none();

        host.stop_processes_in_removed_worktree(&ticket.id).unwrap();
        assert!(running(&mut build), "the worktree is still there");
        fs::rename(&ticket.worktree, folder.path().join("aside")).unwrap();
        run(&host, "current", None);
        host.stop_processes_in_removed_worktree(&ticket.id).unwrap();
        assert!(running(&mut build), "an agent of the ticket is in a turn");
        run(&host, "current", Some(3));
        host.stop_processes_in_removed_worktree(&ticket.id).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while running(&mut build) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let stopped = !running(&mut build);
        let _ = build.kill();
        assert!(stopped);
    }

    #[test]
    fn the_inbox_told_is_an_unarchived_project_coordinators() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let first = host.create_project("First").unwrap().id;
        let second = host.create_project("Second").unwrap().id;
        let coordinator = |host: &Host, project: &str| host.coordinator_of(project).unwrap();
        let told = |host: &Host, project: Option<&str>| {
            (host.inbox_session(project).unwrap()).map(|s| s.project_id)
        };
        assert_eq!(told(&host, Some(&first)), Some(first.clone()));
        assert_eq!(told(&host, None), Some(second.clone()));
        // Raised outside an event's handling, so the service tells clients once.
        assert!(host.tell_inbox(Some(&first), "notice:0", "Told").unwrap());
        assert!(host.take_inbox_changed());
        assert!(!host.take_inbox_changed());
        let archive = |host: &Host, project: &str| {
            host.db
                .execute(
                    "UPDATE sessions SET data=json_set(data,'$.archived',json('true')) WHERE id=?1",
                    [coordinator(host, project)],
                )
                .unwrap();
        };
        archive(&host, &first);
        assert_eq!(told(&host, Some(&first)), Some(second.clone()));
        archive(&host, &second);
        assert_eq!(told(&host, Some(&first)), None);
        assert!(!host.tell_inbox(None, "notice:1", "Nobody to tell").unwrap());
    }

    #[test]
    fn a_retry_makes_a_waiting_delete_due_now_but_leaves_a_held_one_held() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let waiting = home.path().join("web/.removing-1");
        host.record_worktree_deletion(&waiting, None).unwrap();
        let later = now() + LONGEST_RETRY_MS;
        (host.db)
            .execute("UPDATE worktree_deletions SET next_attempt_at=?1", [later])
            .unwrap();
        host.retry_delete(&waiting).unwrap();
        let next = |host: &Host| host.pending_deletes().unwrap()[0].next_attempt_at;
        assert!(next(&host) < later);

        let held = host.hold_deletion(&home.path().join("web"), "t").unwrap();
        assert!(host.retry_delete(&held).is_err());
        assert!(
            host.retry_delete(&home.path().join("web/.removing-9"))
                .is_err()
        );
    }

    #[test]
    fn a_folder_whose_metadata_cannot_be_read_is_not_gone() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("web/.removing-1");
        fs::create_dir_all(&folder).unwrap();
        let parent = directory.path().join("web");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o000)).unwrap();
        let gone = is_gone(&folder);
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!gone);
        assert!(is_gone(&directory.path().join("web/.removing-2")));
    }
}
