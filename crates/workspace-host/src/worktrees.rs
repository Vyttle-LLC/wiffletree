//! Ticket worktrees exist while their ticket is open and its team is active. Archiving a team,
//! closing or accepting a ticket removes the worktree; its branch and history always stay.
//! While an agent on the ticket is still in its turn, removal is marked pending and happens when
//! the turn ends. Restoring a team re-creates its open tickets' worktrees from their branches and
//! cancels their pending removals. Closed and accepted tickets take no further work, so their
//! worktrees are never re-created.
use crate::*;

impl Host {
    pub(crate) fn ticket_repository(&self, ticket: &Ticket) -> Result<Repository> {
        self.repository(
            self.session(&ticket.coordinator_id)?
                .repository_id
                .as_deref()
                .context("Missing repository")?,
        )
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
            if !runtime::git_output(path, &["status", "--porcelain"])
                .with_context(|| format!("Check \"{}\" ({})", ticket.title, ticket.id))?
                .is_empty()
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
    /// until it finishes, and service startup settles runs a stopped host left open. An agent
    /// that has reported is Done but still in its turn.
    fn agent_in_turn(&self, ticket: &Ticket) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_runs r JOIN runtimes t ON t.session_id=r.session_id
             WHERE r.finished_at IS NULL AND json_extract(t.data,'$.ticket_id')=?1)",
            [&ticket.id],
            |r| r.get(0),
        )?)
    }
    pub(crate) fn cancel_pending_removals(&self, tickets: &[Ticket]) -> Result<()> {
        for ticket in tickets {
            self.db.execute(
                "DELETE FROM pending_worktree_removals WHERE ticket_id=?1",
                [&ticket.id],
            )?;
        }
        Ok(())
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
    /// calls this when a turn ends and at startup. A worktree that gained unsaved work, or that
    /// Git cannot remove, is kept and its coordinator told; the mark is cleared either way.
    pub fn settle_pending_worktrees(&mut self) -> Result<()> {
        let pending: Vec<(String, i64)> = self
            .db
            .prepare("SELECT ticket_id,marked_at FROM pending_worktree_removals ORDER BY rowid")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, marked_at) in pending {
            let ticket = self.ticket(&id)?;
            if self.agent_in_turn(&ticket)? {
                continue;
            }
            let removed = self
                .check_removable(std::slice::from_ref(&ticket))
                .and_then(|()| self.remove_worktree(&ticket));
            let coordinator = self.session(&ticket.coordinator_id)?;
            match removed {
                Ok(_) => Self::event(
                    &self.db,
                    &coordinator.project_id,
                    Some(&coordinator.id),
                    "worktree_removed",
                    &ticket.id,
                )?,
                Err(reason) => self.report_kept_worktree(&ticket, marked_at, &reason)?,
            }
            self.cancel_pending_removals(&[ticket])?;
        }
        Ok(())
    }
    /// Tells the nearest unarchived owner of the ticket, from its coordinator up, once per mark.
    /// When the whole tree is archived, the notice waits in the human's attention inbox instead.
    fn report_kept_worktree(
        &mut self,
        ticket: &Ticket,
        marked_at: i64,
        reason: &anyhow::Error,
    ) -> Result<()> {
        let notice = format!(
            "Wiffletree kept the worktree of ticket \"{}\" ({}) at {} instead of removing it: {reason:#}. Its branch {} is kept. Commit or discard that work, then remove the worktree yourself.",
            ticket.title, ticket.id, ticket.worktree, ticket.branch
        );
        // Attention prompts allow 4096 bytes; cut on a character boundary within that.
        let mut end = notice.len().min(4096);
        while !notice.is_char_boundary(end) {
            end -= 1;
        }
        let notice = &notice[..end];
        let id = format!("worktree-kept:{}:{marked_at}", ticket.id);
        let mut owner = Some(self.session(&ticket.coordinator_id)?);
        while let Some(session) = owner.take_if(|s| s.archived) {
            owner = session
                .parent_id
                .as_deref()
                .map(|parent| self.session(parent))
                .transpose()?;
        }
        match owner {
            Some(owner) => {
                if self.message(&id).is_err() {
                    self.send(&id, None, &owner.id, notice)?;
                }
            }
            None => {
                self.request_attention(&ticket.coordinator_id, "local", &id, notice, &[])?;
            }
        }
        let coordinator = self.session(&ticket.coordinator_id)?;
        Self::event(
            &self.db,
            &coordinator.project_id,
            Some(&coordinator.id),
            "worktree_kept",
            &ticket.id,
        )
    }
}

struct RegisteredWorktree {
    path: PathBuf,
    locked: bool,
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
                for line in lines {
                    if line == "prunable" || line.starts_with("prunable ") {
                        return None;
                    }
                    locked |= line == "locked" || line.starts_with("locked ");
                }
                Some(RegisteredWorktree {
                    path: fs::canonicalize(path).ok()?,
                    locked,
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
        let coordinator = host
            .create_session(
                &project.id,
                &root.id,
                Some(&attached.id),
                "Web",
                Role::TaskOrchestrator,
                Provider::Codex,
            )
            .unwrap();
        let open = host.create_ticket(&coordinator.id, "Open", "Do").unwrap();
        let closed = host.create_ticket(&coordinator.id, "Closed", "Do").unwrap();
        host.close_ticket(&closed.id).unwrap();
        fs::remove_dir_all(&open.worktree).unwrap();

        host.prepare_ticket_worktree(&open.id).unwrap();
        let refused = host.prepare_ticket_worktree(&closed.id).unwrap_err();

        assert!(Path::new(&open.worktree).exists());
        assert!(refused.to_string().contains(&closed.branch), "{refused}");
        assert!(!Path::new(&closed.worktree).exists());
    }
}
