//! A removed worktree waits, renamed to `.removing-<ticket id>` beside where it was, until its
//! folder is deleted. Each such folder is recorded before it is renamed, so a host that stops
//! partway through a delete resumes it at its next start. A failed delete is retried with
//! backoff while the host runs and again at every start, and after repeated failures the ticket's
//! owner is told once which folder failed, why and how big it is. A record is cleared only once
//! its folder is gone. Only folders named with the `.removing-` prefix are ever deleted.
use crate::*;
use std::{collections::HashMap, os::unix::fs::MetadataExt, sync::Arc, thread::JoinHandle};

/// The name a removed worktree's folder takes, followed by its ticket's id.
pub(crate) const REMOVING_PREFIX: &str = ".removing-";
/// Failed attempts after which the ticket's owner is told.
pub(crate) const REPORT_AFTER: i64 = 3;
/// The wait after the first failed attempt; it doubles with each further one.
const FIRST_RETRY_MS: i64 = 15_000;
const LONGEST_RETRY_MS: i64 = 10 * 60_000;

/// A failed attempt: why, and how much the folder still holds on disk.
type Failure = (String, u64);

/// The deletes running off the host thread.
#[derive(Default)]
pub(crate) struct Running {
    deletes: HashMap<PathBuf, JoinHandle<Result<(), Failure>>>,
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

/// A real folder, not a symbolic link to one.
fn is_folder(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
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

fn delete(
    folder: PathBuf,
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
) -> JoinHandle<Result<(), Failure>> {
    std::thread::spawn(move || {
        let deleted = fs::remove_dir_all(&folder);
        let deleted = match is_folder(&folder) {
            false => Ok(()),
            true => Err((
                deleted.map_or_else(
                    |error| error.to_string(),
                    |()| "the folder was there again afterwards".into(),
                ),
                allocated_size(&folder),
            )),
        };
        if let Some(notify) = notify {
            notify();
        }
        deleted
    })
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
        !self.worktree_deletions.deletes.is_empty()
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
                if !is_removal_folder(&path) || !is_folder(&path) {
                    continue;
                }
                let name = entry.file_name();
                let id = name.to_string_lossy()[REMOVING_PREFIX.len()..].to_owned();
                let ticket = self.ticket(&id).is_ok().then_some(id);
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
        let finished: Vec<PathBuf> = (self.worktree_deletions.deletes.iter())
            .filter(|(_, delete)| delete.is_finished())
            .map(|(folder, _)| folder.clone())
            .collect();
        let waiting = self.worktree_deletions.due_at;
        if finished.is_empty() && at < waiting {
            return (waiting < i64::MAX).then_some(waiting);
        }
        for folder in finished {
            let Some(delete) = self.worktree_deletions.deletes.remove(&folder) else {
                continue;
            };
            let outcome = delete
                .join()
                .unwrap_or_else(|_| Err(("the delete panicked".into(), 0)));
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
        let Some(ticket) = ticket.and_then(|id| self.ticket(&id).ok()) else {
            eprintln!("{notice}");
            return Ok(());
        };
        let id = format!("worktree-delete-failed:{}:{recorded_at}", ticket.id);
        self.notify_ticket_owner(&ticket, &id, &notice)?;
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
            if self.worktree_deletions.deletes.contains_key(&folder) {
                continue;
            }
            if !is_folder(&folder) {
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
            let notify = self.worktree_deletions.notify.clone();
            let delete = delete(folder.clone(), notify);
            self.worktree_deletions.deletes.insert(folder, delete);
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
}
