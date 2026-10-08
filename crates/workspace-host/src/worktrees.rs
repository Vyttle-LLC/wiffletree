//! Ticket worktrees exist while their ticket is open and its team is active. Archiving a team,
//! closing or accepting a ticket removes the worktree; its branch and history always stay.
//! Restoring a team re-creates its open tickets' worktrees from their branches. Closed and
//! accepted tickets take no further work, so their worktrees are never re-created.
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
    /// without removing anything while an agent on one of them is working or in its turn, a
    /// worktree is locked or holds uncommitted or untracked work. Ignored files such as build
    /// output do not count, and missing worktrees are already done. `change` runs in one
    /// savepoint; if a removal or `change` fails, its writes roll back and the removed worktrees,
    /// proven clean, are re-created from their branches.
    pub(crate) fn with_worktrees_removed<T>(
        &mut self,
        tickets: &[Ticket],
        change: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.check_removable(tickets)?;
        let mut removed = vec![];
        let mut result = Ok(());
        for ticket in tickets {
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
            let changed = change(self);
            let _ = self.db.execute_batch(if changed.is_ok() {
                "RELEASE worktrees"
            } else {
                "ROLLBACK TO worktrees; RELEASE worktrees"
            });
            changed
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
    fn check_removable(&self, tickets: &[Ticket]) -> Result<()> {
        let runtimes = self.runtimes()?;
        let mut blockers = vec![];
        for ticket in tickets {
            for runtime in runtimes
                .iter()
                .filter(|r| r.ticket_id.as_deref() == Some(&ticket.id))
            {
                let agent = self.session(&runtime.session_id)?;
                if agent.status == Status::Working || self.in_turn(&agent.id)? {
                    blockers.push(format!(
                        "{} is still working on \"{}\" ({})",
                        agent.name, ticket.title, ticket.id
                    ));
                }
            }
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
    /// A provider run is open from turn start until it finishes; service startup settles runs a
    /// stopped host left open.
    fn in_turn(&self, session_id: &str) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_runs WHERE session_id=?1 AND finished_at IS NULL)",
            [session_id],
            |r| r.get(0),
        )?)
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
