//! A removed worktree waits, renamed to `.removing-<ticket id>` beside where it was, until its
//! folder is deleted. Each such folder is recorded before it is renamed, so a host that stops
//! partway through a delete resumes it at its next start. A failed delete is retried with
//! backoff while the host runs and again at every start, and after repeated failures the ticket's
//! owner, or the human's inbox when no ticket names the folder, is told once which folder failed,
//! why and how big it is. A record is cleared only once its folder is proven gone. Only folders
//! named with the `.removing-` prefix are ever deleted. A folder still used by a process, as its
//! working directory or through an open file, is not deleted, since its open files would keep
//! their space: the ticket's recorded processes are stopped, and any other holder is named in the
//! failure and waited out, never signalled.
use crate::*;
use std::{
    collections::HashSet,
    os::unix::fs::MetadataExt,
    sync::{Arc, Mutex},
};

/// The name a removed worktree's folder takes, followed by its ticket's id.
pub(crate) const REMOVING_PREFIX: &str = ".removing-";
/// Failed attempts after which the ticket's owner is told.
pub(crate) const REPORT_AFTER: i64 = 3;
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
            reported_at INTEGER)",
    )?;
    Ok(())
}

/// The wait before the next attempt after `attempts` failed ones.
fn backoff(attempts: i64) -> i64 {
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

/// Whether `path` is `folder` or inside it; both canonical.
fn within(path: &Path, folder: &Path) -> bool {
    path.starts_with(folder)
}

/// This user's processes whose working directory or an open file is inside `folder`, each as
/// "pid (command)". Deleting under them would leave their open files' space allocated.
#[cfg(target_os = "macos")]
fn holders(folder: &Path) -> Result<Vec<String>, String> {
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
            Some(("n", name)) if within(Path::new(name), folder) => {
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
fn holders(folder: &Path) -> Result<Vec<String>, String> {
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
            .any(|path| within(&path, folder))
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
    let holders = holders(&canonical)?;
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
fn allocated_size(path: &Path) -> u64 {
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

fn size_label(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    match bytes as f64 {
        b if b >= GB => format!("{:.1} GB", b / GB),
        b => format!("{:.1} MB", b / MB),
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
                // `<ticket id>`, or `<ticket id>-<time>` beside an earlier removal's folder.
                let name = entry.file_name().to_string_lossy()[REMOVING_PREFIX.len()..].to_owned();
                let untimed = name.rsplit_once('-').map(|(id, _)| id.to_owned());
                let ticket = std::iter::once(name)
                    .chain(untimed)
                    .find(|id| self.ticket(id).is_ok());
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
            return self.forget_worktree_deletion(folder);
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
            "UPDATE worktree_deletions SET attempts=?2,last_error=?3,next_attempt_at=?4 WHERE path=?1",
            params![path, attempts, error, at + backoff(attempts)],
        )?;
        eprintln!(
            "Delete removed worktree {} (attempt {attempts}): {error}",
            folder.display()
        );
        if attempts < REPORT_AFTER || reported {
            return Ok(());
        }
        let notice = format!(
            "Wiffletree could not delete the removed worktree folder {} ({}) after {attempts} attempts: {error}. It keeps retrying, and at every start. Deleting the folder yourself frees the space.",
            folder.display(),
            size_label(size)
        );
        match ticket.and_then(|id| self.ticket(&id).ok()) {
            Some(ticket) => {
                let id = format!("worktree-delete-failed:{}:{recorded_at}", ticket.id);
                self.notify_ticket_owner(&ticket, &id, &notice)?;
            }
            None => {
                // No ticket names the folder, so the human's inbox, under the newest project's
                // orchestrator, is told instead.
                let orchestrator =
                    (self.sessions()?.into_iter()).rfind(|s| s.parent_id.is_none() && !s.archived);
                let Some(orchestrator) = orchestrator else {
                    eprintln!("{notice}");
                    return Ok(());
                };
                let name = folder.file_name().unwrap_or_default().to_string_lossy();
                let name = &name[..steps::floor_boundary(&name, name.len().min(80))];
                let id = format!("worktree-delete-failed:{recorded_at}:{name}");
                let notice = &notice[..steps::floor_boundary(&notice, notice.len().min(4096))];
                self.request_attention(&orchestrator.id, "local", &id, notice, &[])?;
            }
        }
        self.db.execute(
            "UPDATE worktree_deletions SET reported_at=?2 WHERE path=?1",
            params![path, at],
        )?;
        Ok(())
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
            if self.worktree_deletions.folders.contains(&folder) {
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
        let in_turn: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_runs r JOIN runtimes t ON t.session_id=r.session_id
             WHERE json_extract(t.data,'$.ticket_id')=?1 AND r.finished_at IS NULL)",
            [ticket_id],
            |r| r.get(0),
        )?;
        if in_turn {
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
