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
    /// Removes the tickets' worktrees, keeping their branches. Refuses without removing anything
    /// while an agent on one of them is working or one holds uncommitted or untracked work.
    /// Ignored files such as build output do not count. Missing worktrees are already done.
    pub(crate) fn remove_worktrees(&self, tickets: &[Ticket]) -> Result<()> {
        let runtimes = self.runtimes()?;
        let mut blockers = vec![];
        for ticket in tickets {
            for runtime in runtimes
                .iter()
                .filter(|r| r.ticket_id.as_deref() == Some(&ticket.id))
            {
                let agent = self.session(&runtime.session_id)?;
                if agent.status == Status::Working {
                    blockers.push(format!(
                        "{} is still working on \"{}\" ({})",
                        agent.name, ticket.title, ticket.id
                    ));
                }
            }
            let path = Path::new(&ticket.worktree);
            if path.exists()
                && !runtime::git_output(path, &["status", "--porcelain"])
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
        for ticket in tickets {
            let repository = Path::new(&self.ticket_repository(ticket)?.path).to_owned();
            if Path::new(&ticket.worktree).exists() {
                runtime::git_output(&repository, &["worktree", "remove", &ticket.worktree])
                    .with_context(|| format!("Remove \"{}\" ({})", ticket.title, ticket.id))?;
            } else {
                runtime::git_output(&repository, &["worktree", "prune"])?;
            }
        }
        Ok(())
    }
}

/// Adds the worktree unless it is already registered: from an existing branch, or as a new
/// branch from `base` when the ticket is created.
pub(crate) fn ensure_worktree(
    repository: &Path,
    path: &Path,
    branch: &str,
    base: Option<&str>,
) -> Result<()> {
    if path.exists() {
        let path = fs::canonicalize(path)?;
        let registered = runtime::git_output(repository, &["worktree", "list", "--porcelain"])?
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .any(|listed| fs::canonicalize(listed).is_ok_and(|listed| listed == path));
        ensure!(
            registered,
            "{} exists but is not a worktree of {}",
            path.display(),
            repository.display()
        );
        return Ok(());
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
