//! Ticket worktrees exist while their ticket is open and its project is active. Archiving the
//! project, closing or accepting a ticket removes the worktree; its branch and history always
//! stay. While an agent on the ticket is still in its turn, removal is marked pending and happens
//! when the turn ends. Restoring re-creates open tickets' worktrees from their branches and
//! cancels their pending removals. Closed and accepted tickets take no further work, so their
//! worktrees are never re-created. Worktrees left by tickets finished before removal existed
//! are listed for the human, who alone can choose to remove them.
use crate::*;

impl Host {
    pub(crate) fn ticket_repository(&self, ticket: &Ticket) -> Result<Repository> {
        ensure!(
            !ticket.repository_id.is_empty(),
            "Ticket \"{}\" ({}) records no repository",
            ticket.title,
            ticket.id
        );
        self.repository(&ticket.repository_id)
    }
    /// Ensures an open ticket's worktree exists before an agent runs in it. A finished ticket's
    /// removed worktree is reported rather than re-created.
    pub(crate) fn prepare_ticket_worktree(&self, ticket_id: &str) -> Result<()> {
        let ticket = self.ticket(ticket_id)?;
        if ticket.is_open() {
            return self.restore_worktrees(&[ticket]);
        }
        ensure!(
            Path::new(&ticket.worktree).exists(),
            "Ticket \"{}\" ({}) is {}; its worktree was removed and branch {} keeps the work",
            ticket.title,
            ticket.id,
            ticket.state,
            ticket.branch
        );
        Ok(())
    }
    /// Re-creates any missing worktrees from their saved branches. Checks every branch first, so
    /// a missing one refuses the whole restore, naming its ticket.
    pub(crate) fn restore_worktrees(&self, tickets: &[Ticket]) -> Result<()> {
        let mut missing = vec![];
        for ticket in tickets {
            let repository = self.ticket_repository(ticket)?;
            let branch = format!("refs/heads/{}", ticket.branch);
            if runtime::git_output(
                Path::new(&repository.path),
                &["rev-parse", "--verify", "--quiet", &branch],
            )
            .is_err()
            {
                missing.push(format!(
                    "ticket \"{}\" ({}) has no branch {}",
                    ticket.title, ticket.id, ticket.branch
                ));
            }
        }
        ensure!(
            missing.is_empty(),
            "Cannot restore worktrees: {}",
            missing.join("; ")
        );
        for ticket in tickets {
            let repository = self.ticket_repository(ticket)?;
            ensure_worktree(
                Path::new(&repository.path),
                Path::new(&ticket.worktree),
                &ticket.branch,
                None,
            )
            .with_context(|| format!("Restore \"{}\" ({})", ticket.title, ticket.id))?;
        }
        Ok(())
    }
    /// Removes the tickets' worktrees, keeping their branches, then applies `change`. Refuses
    /// without removing anything while a worktree is locked or holds uncommitted or untracked
    /// work. Ignored files such as build output do not count, and missing worktrees are already
    /// done. A worktree whose agent is still in its turn is marked for removal when the turn
    /// ends instead. `change` and the marks run in one savepoint; if a removal or `change` fails,
    /// its writes roll back and the removed worktrees, proven clean, are re-created.
    pub(crate) fn with_worktrees_removed<T>(
        &mut self,
        tickets: &[Ticket],
        change: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.check_removable(tickets)?;
        let mut removed = vec![];
        let mut deferred = vec![];
        let mut result = Ok(());
        for ticket in tickets {
            match self.agent_in_turn(ticket) {
                Ok(true) => {
                    deferred.push(ticket);
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
            match self.remove_worktree(ticket) {
                Ok(true) => removed.push(ticket),
                Ok(false) => {}
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
        }
        let result = result.and_then(|()| {
            self.db.execute_batch("SAVEPOINT worktrees")?;
            let changed = deferred
                .iter()
                .try_for_each(|ticket| {
                    self.db.execute(
                        "INSERT OR IGNORE INTO pending_worktree_removals VALUES (?1,?2)",
                        params![ticket.id, now()],
                    )?;
                    Ok(())
                })
                .and_then(|()| change(self))
                .and_then(|value| {
                    // Releasing the outermost savepoint commits, which can still fail.
                    self.db.execute_batch("RELEASE worktrees")?;
                    Ok(value)
                });
            changed.map_err(|error| match self.roll_back_change() {
                Ok(()) => error,
                Err(rollback) => error.context(format!("Could not roll back: {rollback:#}")),
            })
        });
        let Err(error) = result else {
            return result;
        };
        let unrestored: Vec<String> = removed
            .into_iter()
            .filter_map(|ticket| {
                let repository = self.ticket_repository(ticket).ok()?;
                ensure_worktree(
                    Path::new(&repository.path),
                    Path::new(&ticket.worktree),
                    &ticket.branch,
                    None,
                )
                .err()
                .map(|e| format!("\"{}\" ({}): {e:#}", ticket.title, ticket.id))
            })
            .collect();
        if unrestored.is_empty() {
            Err(error)
        } else {
            Err(error.context(format!(
                "Could not re-create removed worktrees; their branches keep the work: {}",
                unrestored.join("; ")
            )))
        }
    }
    /// Undoes the `worktrees` savepoint, or the whole transaction if the savepoint is gone.
    fn roll_back_change(&self) -> Result<()> {
        let undone = self
            .db
            .execute_batch("ROLLBACK TO worktrees; RELEASE worktrees");
        if undone.is_err() && !self.db.is_autocommit() {
            self.db.execute_batch("ROLLBACK")?;
        }
        Ok(undone?)
    }
    fn check_removable(&self, tickets: &[Ticket]) -> Result<()> {
        let mut blockers = vec![];
        for ticket in tickets {
            let path = Path::new(&ticket.worktree);
            if !path.exists() {
                continue;
            }
            let repository = self.ticket_repository(ticket)?;
            let canonical = fs::canonicalize(path)?;
            if registered_worktrees(Path::new(&repository.path))?
                .iter()
                .any(|w| w.path == canonical && w.locked)
            {
                blockers.push(format!(
                    "ticket \"{}\" ({}) has a locked worktree in {}",
                    ticket.title, ticket.id, ticket.worktree
                ));
            }
            if !is_clean(path)
                .with_context(|| format!("Check \"{}\" ({})", ticket.title, ticket.id))?
            {
                blockers.push(format!(
                    "ticket \"{}\" ({}) has uncommitted or untracked changes in {}",
                    ticket.title, ticket.id, ticket.worktree
                ));
            }
        }
        ensure!(
            blockers.is_empty(),
            "Cannot remove ticket worktrees: {}",
            blockers.join("; ")
        );
        Ok(())
    }
    /// Whether a worktree was removed; a missing one only has its stale registration pruned.
    fn remove_worktree(&self, ticket: &Ticket) -> Result<bool> {
        let repository = self.ticket_repository(ticket)?;
        let repository = Path::new(&repository.path);
        if !Path::new(&ticket.worktree).exists() {
            runtime::git_output(repository, &["worktree", "prune"])?;
            return Ok(false);
        }
        runtime::git_output(repository, &["worktree", "remove", &ticket.worktree])
            .with_context(|| format!("Remove \"{}\" ({})", ticket.title, ticket.id))?;
        Ok(true)
    }
    /// Whether an agent on the ticket is in its turn: a provider run is open from turn start
    /// until it finishes, and an agent that has reported is Done but still in its turn. A finished
    /// run also counts while its process group exists, whatever its outcome: a stopped or crashed
    /// host does not wait for the process. A normal finish signals the group and reaps its leader,
    /// so only a descendant that ignored the signal or a recycled group id keeps the worktree
    /// longer. Runs recorded before groups were kept do not count.
    fn agent_in_turn(&self, ticket: &Ticket) -> Result<bool> {
        let runs = self
            .db
            .prepare(
                "SELECT r.finished_at IS NULL,r.process_group FROM provider_runs r
                 JOIN runtimes t ON t.session_id=r.session_id
                 WHERE json_extract(t.data,'$.ticket_id')=?1
                 AND (r.finished_at IS NULL OR r.process_group IS NOT NULL)",
            )?
            .query_map([&ticket.id], |r| {
                Ok((r.get::<_, bool>(0)?, r.get::<_, Option<i64>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(runs
            .into_iter()
            .any(|(open, group)| open || group.is_some_and(process_group_exists)))
    }
    /// Worktrees left by finished tickets, and Wiffletree-made worktrees no ticket records, each
    /// with the one class that says whether it may be removed. Changes nothing.
    pub fn leftover_worktrees(&self) -> Result<Vec<LeftoverWorktree>> {
        let pending = self.pending_worktree_removals()?;
        let mut leftovers = vec![];
        let mut recorded = std::collections::HashSet::new();
        for ticket in self.tickets()? {
            if let Ok(path) = fs::canonicalize(&ticket.worktree) {
                recorded.insert(path);
            }
            if self.is_leftover(&ticket, &pending)? {
                leftovers.push(self.leftover(&ticket)?);
            }
        }
        let Ok(tasks) = fs::canonicalize(Path::new(&self.settings().workspaces_dir).join("tasks"))
        else {
            return Ok(leftovers);
        };
        for repository in self.repositories()? {
            let Ok(registered) = registered_worktrees(Path::new(&repository.path)) else {
                continue;
            };
            for worktree in registered {
                let branch = worktree.branch.as_deref().unwrap_or_default();
                if !branch.starts_with("refs/heads/wiffletree/")
                    || !worktree.path.starts_with(&tasks)
                    || recorded.contains(&worktree.path)
                {
                    continue;
                }
                leftovers.push(LeftoverWorktree {
                    project_id: None,
                    project: String::new(),
                    repository: repository.name.clone(),
                    ticket_id: None,
                    title: String::new(),
                    state: String::new(),
                    worktree: worktree.path.to_string_lossy().into_owned(),
                    branch: branch.trim_start_matches("refs/heads/").into(),
                    head: worktree.head,
                    class: LeftoverClass::UntrackedByWiffletree,
                });
            }
        }
        Ok(leftovers)
    }
    /// Removes the selected tickets' leftover worktrees without force, re-checking each first and
    /// keeping any that is no longer clean or unpushed. Branches, tickets, sessions and messages
    /// are never deleted. Each outcome is recorded in its project's activity.
    pub fn remove_leftover_worktrees(
        &mut self,
        ticket_ids: &[String],
    ) -> Result<Vec<LeftoverRemoval>> {
        let pending = self.pending_worktree_removals()?;
        let mut outcomes = vec![];
        for id in ticket_ids {
            let ticket = self.ticket(id)?;
            let removed = (|| -> Result<()> {
                ensure!(
                    self.is_leftover(&ticket, &pending)?,
                    "no longer a leftover worktree"
                );
                let class = self.classify(&ticket)?;
                ensure!(class.is_removable(), "{}", class.label());
                self.remove_worktree(&ticket)?;
                Ok(())
            })();
            let coordinator = self.session(&ticket.coordinator_id)?;
            let reason = removed.err().map(|e| format!("{e:#}"));
            Self::event(
                &self.db,
                &coordinator.project_id,
                Some(&coordinator.id),
                if reason.is_none() {
                    "worktree_removed"
                } else {
                    "worktree_kept"
                },
                &match &reason {
                    Some(reason) => format!("{}: {reason}", ticket.id),
                    None => ticket.id.clone(),
                },
            )?;
            outcomes.push(LeftoverRemoval {
                ticket_id: ticket.id,
                removed: reason.is_none(),
                reason,
            });
        }
        Ok(outcomes)
    }
    /// Finished, or every agent archived, with its worktree still present and no removal pending.
    fn is_leftover(&self, ticket: &Ticket, pending: &[String]) -> Result<bool> {
        if pending.contains(&ticket.id) || !Path::new(&ticket.worktree).exists() {
            return Ok(false);
        }
        if !ticket.is_open() {
            return Ok(true);
        }
        let agents = self.ticket_agents(&ticket.id)?;
        Ok(!agents.is_empty() && agents.iter().all(|a| a.archived))
    }
    fn leftover(&self, ticket: &Ticket) -> Result<LeftoverWorktree> {
        let project = self.project(&self.session(&ticket.coordinator_id)?.project_id)?;
        let repository = self.ticket_repository(ticket)?;
        Ok(LeftoverWorktree {
            project_id: Some(project.id),
            project: project.name,
            repository: repository.name,
            ticket_id: Some(ticket.id.clone()),
            title: ticket.title.clone(),
            state: ticket.state.clone(),
            worktree: ticket.worktree.clone(),
            branch: ticket.branch.clone(),
            head: head(Path::new(&ticket.worktree)).unwrap_or_default(),
            class: self.classify(ticket)?,
        })
    }
    /// The first class that applies, in order of precedence.
    fn classify(&self, ticket: &Ticket) -> Result<LeftoverClass> {
        let repository = self.ticket_repository(ticket)?;
        let repository = Path::new(&repository.path);
        let worktree = Path::new(&ticket.worktree);
        let canonical = fs::canonicalize(worktree)?;
        let branch = format!("refs/heads/{}", ticket.branch);
        let off_branch = || {
            runtime::git_output(worktree, &["symbolic-ref", "-q", "HEAD"])
                .map_or(true, |h| h.trim() != branch)
                && runtime::git_output(worktree, &["merge-base", "--is-ancestor", "HEAD", &branch])
                    .is_err()
        };
        Ok(if self.agent_in_turn(ticket)? {
            LeftoverClass::InTurn
        } else if registered_worktrees(repository)?
            .iter()
            .any(|w| w.path == canonical && w.locked)
        {
            LeftoverClass::Locked
        } else if !is_clean(worktree)? {
            LeftoverClass::Dirty
        } else if off_branch() {
            LeftoverClass::Detached
        } else if runtime::git_output(
            repository,
            &["rev-list", "--count", &branch, "--not", "--remotes"],
        )?
        .trim()
            != "0"
        {
            LeftoverClass::Unpushed
        } else {
            LeftoverClass::Clean
        })
    }
    /// Tickets whose worktree removal waits for an agent's turn to end.
    pub fn pending_worktree_removals(&self) -> Result<Vec<String>> {
        Ok(self
            .db
            .prepare("SELECT ticket_id FROM pending_worktree_removals ORDER BY rowid")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Performs pending removals whose tickets no longer have an agent in its turn. The service
    /// calls this when a provider process ends and at startup. A worktree that gained unsaved
    /// work, or that Git cannot remove, is kept and someone told; the mark is cleared either way.
    /// Failures are logged per ticket and retried at the next settlement, never propagated.
    pub fn settle_pending_worktrees(&mut self) {
        let pending: Result<Vec<(String, i64)>> = self
            .db
            .prepare("SELECT ticket_id,marked_at FROM pending_worktree_removals ORDER BY rowid")
            .and_then(|mut query| {
                query
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .map_err(Into::into);
        match pending {
            Ok(pending) => {
                for (id, marked_at) in pending {
                    if let Err(error) = self.settle_pending_worktree(&id, marked_at) {
                        eprintln!("Pending worktree removal for ticket {id}: {error:#}");
                    }
                }
            }
            Err(error) => eprintln!("Pending worktree removals: {error:#}"),
        }
    }
    fn settle_pending_worktree(&mut self, id: &str, marked_at: i64) -> Result<()> {
        let ticket = self.ticket(id)?;
        if self.agent_in_turn(&ticket)? {
            return Ok(());
        }
        let outcome = match self
            .check_removable(std::slice::from_ref(&ticket))
            .and_then(|()| self.remove_worktree(&ticket))
        {
            Ok(_) => "worktree_removed",
            Err(reason) => {
                self.report_kept_worktree(&ticket, marked_at, &reason)?;
                "worktree_kept"
            }
        };
        let coordinator = self.session(&ticket.coordinator_id)?;
        let tx = self.db.transaction()?;
        Self::event(
            &tx,
            &coordinator.project_id,
            Some(&coordinator.id),
            outcome,
            &ticket.id,
        )?;
        tx.execute(
            "DELETE FROM pending_worktree_removals WHERE ticket_id=?1",
            [&ticket.id],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Tells the nearest unarchived owner of the ticket, from its coordinator up, once per mark.
    /// When the whole tree is archived or the message is refused, the notice goes to the human's
    /// attention inbox instead.
    fn report_kept_worktree(
        &mut self,
        ticket: &Ticket,
        marked_at: i64,
        reason: &anyhow::Error,
    ) -> Result<()> {
        let id = format!("worktree-kept:{}:{marked_at}", ticket.id);
        let reported: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE id=?1)
             OR EXISTS(SELECT 1 FROM attention WHERE operation_id=?1)",
            [&id],
            |r| r.get(0),
        )?;
        if reported {
            return Ok(());
        }
        let notice = format!(
            "Wiffletree kept the worktree of ticket \"{}\" ({}) at {} instead of removing it: {reason:#}. Its branch {} is kept. Commit or discard that work, then remove the worktree yourself.",
            ticket.title, ticket.id, ticket.worktree, ticket.branch
        );
        // Attention prompts allow 4096 bytes; cut on a character boundary within that.
        let notice = &notice[..steps::floor_boundary(&notice, notice.len().min(4096))];
        let mut owner = Some(self.session(&ticket.coordinator_id)?);
        while let Some(session) = owner.take_if(|s| s.archived) {
            owner = session
                .parent_id
                .as_deref()
                .map(|parent| self.session(parent))
                .transpose()?;
        }
        if let Some(owner) = owner
            && self.send(&id, None, &owner.id, notice).is_ok()
        {
            return Ok(());
        }
        self.request_attention(&ticket.coordinator_id, "local", &id, notice, &[])?;
        Ok(())
    }
}

/// Adds provider runs' process group, recorded when the provider starts.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let present: i64 = db.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('provider_runs') WHERE name='process_group'",
        [],
        |r| r.get(0),
    )?;
    if present == 0 {
        db.execute_batch("ALTER TABLE provider_runs ADD COLUMN process_group INTEGER")?;
    }
    Ok(())
}

/// The checked-out commit.
pub(crate) fn head(worktree: &Path) -> Result<String> {
    Ok(runtime::git_output(worktree, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned())
}

/// No uncommitted or untracked files; ignored files such as build output do not count.
pub(crate) fn is_clean(worktree: &Path) -> Result<bool> {
    Ok(runtime::git_output(worktree, &["status", "--porcelain"])?.is_empty())
}

/// Probes with signal 0, which delivers nothing. EPERM still means the group exists.
fn process_group_exists(group: i64) -> bool {
    let Ok(group) = i32::try_from(group) else {
        return false;
    };
    if group <= 1 {
        return false;
    }
    let probe = unsafe { libc::kill(-group, 0) };
    probe == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

struct RegisteredWorktree {
    path: PathBuf,
    locked: bool,
    /// `refs/heads/...`, or `None` when detached.
    branch: Option<String>,
    head: String,
}

/// The repository's worktrees from `git worktree list`, with canonical paths where they exist.
/// Stale registrations whose folder is gone are left out.
fn registered_worktrees(repository: &Path) -> Result<Vec<RegisteredWorktree>> {
    Ok(
        runtime::git_output(repository, &["worktree", "list", "--porcelain"])?
            .split("\n\n")
            .filter_map(|entry| {
                let mut lines = entry.lines();
                let path = lines.next()?.strip_prefix("worktree ")?;
                let mut locked = false;
                let mut branch = None;
                let mut head = String::new();
                for line in lines {
                    if line == "prunable" || line.starts_with("prunable ") {
                        return None;
                    }
                    locked |= line == "locked" || line.starts_with("locked ");
                    if let Some(name) = line.strip_prefix("branch ") {
                        branch = Some(name.to_owned());
                    }
                    if let Some(commit) = line.strip_prefix("HEAD ") {
                        head = commit.to_owned();
                    }
                }
                Some(RegisteredWorktree {
                    path: fs::canonicalize(path).ok()?,
                    locked,
                    branch,
                    head,
                })
            })
            .collect(),
    )
}

/// Whether `path` is the top of a checkout registered as one of the repository's worktrees.
fn is_worktree(repository: &Path, path: &Path) -> Result<bool> {
    let path = fs::canonicalize(path)?;
    let top = runtime::git_output(&path, &["rev-parse", "--show-toplevel"])
        .ok()
        .and_then(|top| fs::canonicalize(top.trim()).ok());
    Ok(top.as_ref() == Some(&path)
        && registered_worktrees(repository)?
            .iter()
            .any(|w| w.path == path))
}

/// Adds the worktree unless it is already registered: from an existing branch, or as a new
/// branch from `base` when the ticket is created. An empty folder left in its place is replaced;
/// any other folder that is not this worktree is refused.
pub(crate) fn ensure_worktree(
    repository: &Path,
    path: &Path,
    branch: &str,
    base: Option<&str>,
) -> Result<()> {
    if path.exists() {
        if is_worktree(repository, path)? {
            return Ok(());
        }
        ensure!(
            fs::read_dir(path)?.next().is_none(),
            "{} exists but is not a worktree of {}",
            path.display(),
            repository.display()
        );
        fs::remove_dir(path)?;
    }
    runtime::git_output(repository, &["worktree", "prune"])?;
    fs::create_dir_all(path.parent().context("Invalid worktree path")?)?;
    let path = path.to_str().context("Invalid worktree path")?;
    let mut args = vec!["worktree", "add"];
    match base {
        Some(base) => args.extend(["-b", branch, path, base]),
        None => args.extend([path, branch]),
    }
    runtime::git_output(repository, &args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(path: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(path)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    fn starting_an_agent_re_creates_an_open_ticket_worktree_but_not_a_finished_one() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("web");
        fs::create_dir_all(&repository).unwrap();
        git(&repository, &["init", "-q", "-b", "main"]);
        git(
            &repository,
            &["commit", "-q", "--allow-empty", "-m", "Fixture"],
        );
        let mut host = Host::open(directory.path().join("home")).unwrap();
        host.set_workspaces_dir(directory.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Theme").unwrap();
        let root = host.sessions().unwrap().remove(0);
        let attached = host
            .attach_repository(&project.id, repository.to_str().unwrap(), "HEAD")
            .unwrap();
        let coordinator = root;
        let open = host
            .create_ticket(&coordinator.id, &attached.id, "Open", "Do")
            .unwrap();
        let closed = host
            .create_ticket(&coordinator.id, &attached.id, "Closed", "Do")
            .unwrap();
        host.close_ticket(&closed.id).unwrap();
        fs::remove_dir_all(&open.worktree).unwrap();

        host.prepare_ticket_worktree(&open.id).unwrap();
        let refused = host.prepare_ticket_worktree(&closed.id).unwrap_err();

        assert!(Path::new(&open.worktree).exists());
        assert!(refused.to_string().contains(&closed.branch), "{refused}");
        assert!(!Path::new(&closed.worktree).exists());
    }
}
