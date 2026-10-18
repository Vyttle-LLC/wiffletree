//! Prepared checkout operations. Git owns no store writes.
use crate::*;

pub(crate) struct History {
    repository: Repository,
    skip: usize,
    limit: usize,
}
impl History {
    pub(crate) fn prepare(
        host: &Host,
        repository: &str,
        skip: usize,
        limit: usize,
    ) -> Result<Self> {
        page_limit(limit)?;
        ensure!(skip <= 100_000, "History cursor too large");
        Ok(Self {
            repository: host.repository(repository)?,
            skip,
            limit,
        })
    }
    pub(crate) fn run(&self, git: &publish::GitTool) -> Result<Value> {
        let Self {
            repository,
            skip,
            limit,
        } = self;
        let answer = git(
            Path::new(&repository.path),
            &[
                "log",
                "--date-order",
                &format!("--max-count={limit}"),
                &format!("--skip={skip}"),
                "--format=%H%x1f%P%x1f%an%x1f%s",
                "HEAD",
                "--",
            ],
        )?;
        ensure!(answer.status.success(), "git log failed{}", answer.reason());
        let commits: Vec<_> = answer.output.lines().filter_map(|line| {
            let parts: Vec<_> = line.splitn(4, '\u{1f}').collect();
            (parts.len() == 4).then(|| json!({"sha":parts[0],"parents":parts[1].split_whitespace().collect::<Vec<_>>(),"author":parts[2],"subject":parts[3]}))
        }).collect();
        Ok(
            json!({"repository":repository,"commits":commits,"next_skip":skip+commits.len(),"observed_at":now(),"source":"local Git · read only"}),
        )
    }
}

/// The actor validates ownership before any Git starts, then again before reserving a path.
pub(crate) enum Prepared {
    Existing(Box<Ticket>),
    Create(Box<Creation>),
}
pub(crate) struct Creation {
    owner: String,
    repository: Repository,
    title: String,
    brief: String,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReservedCreation {
    pub ticket: Ticket,
    repository: Repository,
    project_id: String,
}
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS checkout_creations(id TEXT PRIMARY KEY, data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS checkout_moves(id TEXT PRIMARY KEY, data TEXT NOT NULL);",
    )?;
    Ok(())
}
pub(crate) fn pending_creations(host: &Host) -> Result<Vec<ReservedCreation>> {
    host.list_data("SELECT data FROM checkout_creations ORDER BY rowid", [])
}
impl Creation {
    pub(crate) fn prepare(
        host: &Host,
        owner: &str,
        repository_id: &str,
        title: &str,
        brief: &str,
    ) -> Result<Prepared> {
        text(title, 128)?;
        text(brief, MAX_TEXT_BYTES)?;
        let coordinator = host.session(owner)?;
        ensure!(
            coordinator.role == Role::ProjectOrchestrator,
            "Tickets belong to the project coordinator"
        );
        ensure!(
            !coordinator.archived,
            "{} is archived; restore it before adding tickets",
            coordinator.name
        );
        let repo = host.repository(repository_id)?;
        let project = host.project(&coordinator.project_id)?;
        ensure!(
            project.uses(&repo.id),
            "Project {} does not use repository {}",
            project.name,
            repo.name
        );
        if let Some(ticket) = host
            .tickets()?
            .into_iter()
            .find(|t| t.coordinator_id == owner && t.repository_id == repo.id && t.title == title)
        {
            ensure!(
                ticket.brief == brief,
                "Ticket title already exists with a different brief"
            );
            return Ok(Prepared::Existing(Box::new(ticket)));
        }

        Ok(Prepared::Create(Box::new(Self {
            owner: owner.into(),
            repository: repo,
            title: title.into(),
            brief: brief.into(),
        })))
    }
    pub(crate) fn existing(&self, host: &Host) -> Result<Option<Ticket>> {
        Ok(
            match Self::prepare(
                host,
                &self.owner,
                &self.repository.id,
                &self.title,
                &self.brief,
            )? {
                Prepared::Existing(ticket) => Some(*ticket),
                Prepared::Create(_) => None,
            },
        )
    }
    pub(crate) fn inspect(&self, git: &publish::GitTool) -> Result<Vec<String>> {
        let output = git_output(
            git,
            Path::new(&self.repository.path),
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/wiffletree",
            ],
        )?;
        let branches: Vec<String> = output.lines().map(str::to_owned).collect();
        ensure!(
            !branches.iter().any(|r| r == "refs/heads/wiffletree"),
            "A branch named \"wiffletree\" blocks ticket branches; rename it in {}",
            self.repository.path
        );
        Ok(branches)
    }
    pub(crate) fn reserve(self, host: &Host, branches: &[String]) -> Result<ReservedCreation> {
        let prepared = Self::prepare(
            host,
            &self.owner,
            &self.repository.id,
            &self.title,
            &self.brief,
        )?;
        if let Prepared::Existing(_) = prepared {
            bail!("Ticket was created while its checkout was being prepared; repeat the request");
        }
        ensure!(
            same_repository(&host.repository(&self.repository.id)?, &self.repository),
            "Repository changed while preparing its checkout; repeat the request"
        );
        if let Some(pending) = pending_creations(host)?.into_iter().find(|c| {
            c.ticket.coordinator_id == self.owner
                && c.ticket.repository_id == self.repository.id
                && c.ticket.title == self.title
        }) {
            ensure!(
                pending.ticket.brief == self.brief,
                "Ticket title already exists with a different brief"
            );
            if same_repository(&pending.repository, &self.repository) {
                return Ok(pending);
            }
            // A failed attempt keeps its exact placement for retry. If its repository
            // changed meanwhile, its artifacts become ordinary leftovers and a fresh
            // placement is chosen; the stale reservation must not wedge the title.
            pending.release(host)?;
        }
        let coordinator = host.session(&self.owner)?;
        let project = host.project(&coordinator.project_id)?;
        let (path, branch) =
            host.new_ticket_place_observed(&project, &self.repository, &self.title, branches)?;
        let ticket = Ticket {
            id: new_id(),
            coordinator_id: self.owner.clone(),
            repository_id: self.repository.id.clone(),
            title: self.title.clone(),
            brief: self.brief.clone(),
            worktree: path.to_string_lossy().into(),
            branch,
            state: TicketState::Planned,
            verification: None,
            previous_cycles: vec![],
            ledger: vec![],
            waiver: None,
            pull_request: None,
            publishing: None,
            pushed: None,
            keeper: None,
        };

        let reserved = ReservedCreation {
            ticket,
            repository: self.repository,
            project_id: coordinator.project_id,
        };
        host.db.execute(
            "INSERT INTO checkout_creations VALUES (?1,?2)",
            params![reserved.ticket.id, encode(&reserved)?],
        )?;
        Ok(reserved)
    }
}
impl ReservedCreation {
    pub(crate) fn run(&self, git: &publish::GitTool) -> Result<()> {
        worktrees::ensure_worktree_with(
            Path::new(&self.repository.path),
            Path::new(&self.ticket.worktree),
            &self.ticket.branch,
            Some(&self.repository.base),
            git,
        )
    }
    pub(crate) fn apply(self, host: &mut Host) -> Result<Ticket> {
        let validated = (|| {
            let prepared = Creation::prepare(
                host,
                &self.ticket.coordinator_id,
                &self.repository.id,
                &self.ticket.title,
                &self.ticket.brief,
            )?;
            if matches!(prepared, Prepared::Create(_)) {
                ensure!(
                    same_repository(&host.repository(&self.repository.id)?, &self.repository),
                    "Repository changed while preparing its checkout; repeat the request"
                );
            }
            Ok(prepared)
        })();
        match validated {
            Ok(Prepared::Existing(ticket)) => {
                self.release(host)?;
                return Ok(*ticket);
            }
            Ok(Prepared::Create(_)) => {}
            Err(error) => {
                self.release(host)?;
                return Err(error);
            }
        }
        host.atomically(|host| {
            host.db.execute(
                "INSERT INTO tickets VALUES (?1,?2,?3)",
                params![
                    self.ticket.id,
                    self.ticket.coordinator_id,
                    encode(&self.ticket)?
                ],
            )?;
            Host::event(
                &host.db,
                &self.project_id,
                Some(&self.ticket.coordinator_id),
                "ticket_created",
                &self.ticket.id,
            )?;
            self.release(host)
        })?;
        Ok(self.ticket)
    }
    pub(crate) fn release(&self, host: &Host) -> Result<()> {
        host.db.execute(
            "DELETE FROM checkout_creations WHERE id=?1",
            [&self.ticket.id],
        )?;
        Ok(())
    }
}
fn same_repository(left: &Repository, right: &Repository) -> bool {
    (left.id.as_str(), left.path.as_str(), left.base.as_str())
        == (right.id.as_str(), right.path.as_str(), right.base.as_str())
}
pub(crate) fn git_output(git: &publish::GitTool, path: &Path, args: &[&str]) -> Result<String> {
    let answer = git(path, args)?;
    ensure!(
        answer.status.success(),
        "Git query failed{}",
        answer.reason()
    );
    Ok(answer.output)
}

/// A ticket's preparation is valid only for the ticket and repository that requested it.
#[derive(Clone)]
pub(crate) struct Worktree {
    ticket: Ticket,
    repository: Repository,
}
impl Worktree {
    pub(crate) fn prepare(host: &Host, id: &str) -> Result<Self> {
        let ticket = host.ticket(id)?;
        let repository = host.ticket_repository(&ticket)?;
        Ok(Self { ticket, repository })
    }
    pub(crate) fn id(&self) -> &str {
        &self.ticket.id
    }
    pub(crate) fn current(&self, host: &Host) -> bool {
        host.ticket(&self.ticket.id).is_ok_and(|ticket| {
            ticket.worktree == self.ticket.worktree
                && ticket.branch == self.ticket.branch
                && ticket.is_open() == self.ticket.is_open()
        }) && host
            .repository(&self.repository.id)
            .is_ok_and(|repository| repository == self.repository)
    }
    pub(crate) fn run(&self, git: &publish::GitTool) -> Result<()> {
        let ticket = &self.ticket;
        if ticket.is_open() {
            let branch = format!("refs/heads/{}", ticket.branch);
            git_output(
                git,
                Path::new(&self.repository.path),
                &["rev-parse", "--verify", "--quiet", &branch],
            )
            .with_context(|| {
                format!(
                    "Cannot restore worktrees: ticket \"{}\" ({}) has no branch {}",
                    ticket.title, ticket.id, ticket.branch
                )
            })?;
            worktrees::ensure_worktree_with(
                Path::new(&self.repository.path),
                Path::new(&ticket.worktree),
                &ticket.branch,
                None,
                git,
            )?;
        } else {
            ensure!(
                Path::new(&ticket.worktree).exists(),
                "Ticket \"{}\" ({}) is {}; its worktree was removed and branch {} keeps the work",
                ticket.title,
                ticket.id,
                ticket.state,
                ticket.branch
            );
        }
        Ok(())
    }
}

/// A local publish inspection carries the exact store generation it observed. The actor
/// applies it only after revalidation; the network job still rechecks the live store and Git.
pub(crate) struct Publication {
    ticket: Ticket,
    repository: Repository,
}
pub(crate) struct PublicationResult {
    ticket: Ticket,
    repository: Repository,
    checked: std::result::Result<publish::Checked, String>,
}
impl Publication {
    pub(crate) fn prepare(host: &Host, ticket: &str) -> Result<Self> {
        let ticket = host.ticket(ticket)?;
        let repository = host.ticket_repository(&ticket)?;
        Ok(Self { ticket, repository })
    }
    pub(crate) fn run(&self) -> PublicationResult {
        PublicationResult {
            ticket: self.ticket.clone(),
            repository: self.repository.clone(),
            checked: publish::check_with_size(&self.ticket, self.repository.clone())
                .map_err(|error| format!("{error:#}")),
        }
    }
    pub(crate) fn failed(&self) -> PublicationResult {
        PublicationResult {
            ticket: self.ticket.clone(),
            repository: self.repository.clone(),
            checked: Err("The publish inspection stopped unexpectedly".into()),
        }
    }
}
impl PublicationResult {
    pub(crate) fn ticket_id(&self) -> &str {
        &self.ticket.id
    }
    pub(crate) fn get(&self, ticket: &Ticket, repository: &Repository) -> Result<publish::Checked> {
        // Publishing advances these fields when a push reports back. They are not inputs
        // to the verification/branch check, which is shared by both publish steps.
        let inputs = |ticket: &Ticket| {
            json!({"id":ticket.id,"state":ticket.state,"verification":ticket.verification,
            "worktree":ticket.worktree,"branch":ticket.branch,"unreviewed":ticket.unreviewed()})
        };
        ensure!(
            inputs(ticket) == inputs(&self.ticket) && repository == &self.repository,
            "The ticket or repository changed during its publish inspection; call publish_ticket again for what is current"
        );
        self.checked.clone().map_err(anyhow::Error::msg)
    }
    pub(crate) fn apply<T>(
        self,
        host: &mut Host,
        apply: impl FnOnce(&mut Host) -> Result<T>,
    ) -> Result<T> {
        host.checked_publish = Some(self);
        let result = apply(host);
        host.checked_publish = None;
        result
    }
}

/// The actor reserves a ticket across a returned job. Conflicting lifecycle commands wait
/// in arrival order; provider updates and unrelated ticket operations continue normally.
#[derive(Default)]
pub(crate) struct Reservations {
    held: std::collections::HashSet<String>,
}
impl Reservations {
    pub(crate) fn acquire(&mut self, ticket: &str) -> bool {
        self.held.insert(ticket.into())
    }
    pub(crate) fn release(&mut self, ticket: &str) {
        self.held.remove(ticket);
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
    pub(crate) fn contains(&self, ticket: &str) -> bool {
        self.held.contains(ticket)
    }
    pub(crate) fn blocks_command(&self, host: &Host, command: &Command) -> bool {
        if self.held.is_empty() || command.is_read() {
            return false;
        }
        match command {
            Command::AssignTicket { ticket_id, .. } => self.contains(ticket_id),
            Command::ResolveAttention { id, .. } => host.tickets().is_ok_and(|tickets| {
                tickets.iter().any(|ticket| {
                    self.contains(&ticket.id)
                        && ticket
                            .publishing
                            .as_ref()
                            .is_some_and(|request| request.waiting.as_deref() == Some(id))
                })
            }),
            Command::RemoveLeftoverWorktrees { ticket_ids } => {
                ticket_ids.iter().any(|id| self.contains(id))
            }
            Command::SetArchived { session_id, .. } => host
                .session_tree(session_id)
                .is_ok_and(|sessions| sessions.iter().any(|s| self.blocks_session(host, &s.id))),
            Command::SetProjectRepositories { project_id, .. }
            | Command::SetLive { project_id, .. } => self.blocks_project(host, project_id),
            Command::RemoveRepository { repository_id }
            | Command::SetRepositoryChecks { repository_id, .. } => self.held.iter().any(|id| {
                host.ticket(id)
                    .is_ok_and(|t| t.repository_id == *repository_id)
            }),
            Command::AttachRepository { .. }
            | Command::SetKeeper { .. }
            | Command::SetOwnerAllowed { .. }
            | Command::SetVerification { .. }
            | Command::SetModelSelection { .. } => true,
            _ => false,
        }
    }
    pub(crate) fn blocks_repository(&self, host: &Host, repository: &str) -> bool {
        self.held.iter().any(|id| {
            host.ticket(id)
                .is_ok_and(|ticket| ticket.repository_id == repository)
        })
    }
    pub(crate) fn blocks_session(&self, host: &Host, id: &str) -> bool {
        host.session_runtime(id)
            .ok()
            .and_then(|runtime| runtime.ticket_id)
            .is_some_and(|ticket| self.contains(&ticket))
    }
    fn blocks_project(&self, host: &Host, project: &str) -> bool {
        self.held.iter().any(|id| {
            host.ticket(id)
                .ok()
                .and_then(|t| host.session(&t.coordinator_id).ok())
                .is_some_and(|s| s.project_id == project)
        })
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Movement {
    ticket: Ticket,
    repository: Repository,
    from: String,
    to: String,
    fast_forward: bool,
}
impl Movement {
    fn validate(&self, git: &publish::GitTool) -> Result<()> {
        validate_movement(&self.ticket, &self.from, git)
    }
    fn run(&self, git: &publish::GitTool) -> Result<()> {
        let path = Path::new(&self.ticket.worktree);
        let args = if self.fast_forward {
            vec![
                "-c",
                "core.hooksPath=/dev/null",
                "merge",
                "--quiet",
                "--ff-only",
                "--no-autostash",
                &self.to,
            ]
        } else {
            vec!["reset", "--quiet", "--hard", &self.to]
        };
        let answer = git(path, &args)?;
        ensure!(
            answer.status.success(),
            "{}{}",
            if self.fast_forward {
                "git merge failed"
            } else {
                "Git query failed"
            },
            answer.reason()
        );
        Ok(())
    }
}
pub(crate) fn validate_movement(ticket: &Ticket, from: &str, git: &publish::GitTool) -> Result<()> {
    ensure!(
        ticket.running_cycle().is_none(),
        "a verification cycle is running"
    );
    ensure!(
        ticket.publishing.is_none(),
        "a publish request is running or waiting"
    );
    let path = Path::new(&ticket.worktree);
    let on = git(path, &["symbolic-ref", "--quiet", "HEAD"])?;
    ensure!(
        on.status.success() && on.output.trim() == format!("refs/heads/{}", ticket.branch),
        "the worktree is not on the ticket's branch"
    );
    ensure!(
        git_output(git, path, &["rev-parse", "HEAD"])?.trim() == from,
        "the worktree's HEAD moved"
    );
    ensure!(
        git_output(
            git,
            path,
            &["status", "--porcelain", "--untracked-files=normal"]
        )?
        .trim()
        .is_empty(),
        "the worktree has uncommitted or untracked files"
    );
    Ok(())
}
pub(crate) fn move_now(
    host: &Host,
    ticket: &Ticket,
    from: &str,
    to: &str,
    how: keeper::Move<'_>,
) -> Result<()> {
    let real = publish::Tools::default().git;
    validate_movement(ticket, from, &real)?;
    let (fast_forward, git) = match how {
        keeper::Move::Reset => (false, &real),
        keeper::Move::FastForward(git) => (true, git),
    };
    Movement {
        ticket: ticket.clone(),
        repository: host.ticket_repository(ticket)?,
        from: from.into(),
        to: to.into(),
        fast_forward,
    }
    .run(git)
}
pub(crate) enum KeeperAction {
    Waiting,
    ReviewDone,
    Fetched(keeper::Fetched, bool),
    PutBack(Box<KeeperReview>, String),
}
pub(crate) struct KeeperCheckout {
    pub(crate) ticket: Ticket,
    repository: Repository,
    pub(crate) action: KeeperAction,
    movement: Option<Movement>,
    allowed: std::result::Result<(), String>,
}
pub(crate) struct KeeperResult {
    ticket_id: String,
    from: Option<String>,
    to: Option<String>,
    moved: std::result::Result<(), String>,
    movable: std::result::Result<(), String>,
    cleanup: std::result::Result<(), String>,
    pub(crate) head: std::result::Result<String, String>,
    pub(crate) clean: std::result::Result<bool, String>,
    pub(crate) commits: std::result::Result<u32, String>,
    may_change: bool,
    uncertain: bool,
    publication: PublicationResult,
}
impl KeeperCheckout {
    pub(crate) fn prepare(host: &mut Host, id: &str, action: KeeperAction) -> Result<Self> {
        let ticket = host.ticket(id)?;
        let repository = host.ticket_repository(&ticket)?;
        let mut allowed = Ok(());
        let movement = match &action {
            KeeperAction::Waiting => {
                let record = ticket.keeper.clone().unwrap_or_default();
                let from = ticket
                    .running_try()
                    .map(|t| t.head.clone())
                    .or_else(|| record.rebased.map(|r| r.head));
                match (from, record.waiting) {
                    (Some(from), Some(to)) => {
                        allowed = host
                            .keeper_checkout_allowed(&ticket, &repository, true)
                            .map_err(|e| format!("{e:#}"));
                        Some(Movement {
                            ticket: ticket.clone(),
                            repository: repository.clone(),
                            from,
                            to,
                            fast_forward: false,
                        })
                    }
                    _ => None,
                }
            }
            KeeperAction::ReviewDone => ticket
                .keeper_reviews()
                .last()
                .filter(|r| r.withdrawn.as_ref().is_some_and(|w| w.putting_back))
                .map(|r| Movement {
                    ticket: ticket.clone(),
                    repository: repository.clone(),
                    from: r.after.clone(),
                    to: r.before.clone(),
                    fast_forward: false,
                }),
            KeeperAction::Fetched(fetched, in_turn) => match &fetched.found {
                keeper::Found::Ahead { head, .. } if !in_turn => {
                    allowed = host
                        .keeper_checkout_allowed(&ticket, &repository, false)
                        .map_err(|e| format!("{e:#}"));
                    Some(Movement {
                        ticket: ticket.clone(),
                        repository: repository.clone(),
                        from: fetched.from.clone(),
                        to: head.clone(),
                        fast_forward: true,
                    })
                }
                _ => None,
            },
            KeeperAction::PutBack(review, _) => Some(Movement {
                ticket: ticket.clone(),
                repository: repository.clone(),
                from: review.after.clone(),
                to: review.before.clone(),
                fast_forward: false,
            }),
        };
        if let Some(movement) = &movement {
            // This durable intent is a hold before the branch moves. Startup reconciles an
            // interrupted move before any ticket can be scheduled, verified or published.
            host.db.execute(
                "INSERT OR REPLACE INTO checkout_moves VALUES (?1,?2)",
                params![id, encode(movement)?],
            )?;
        }
        host.checkout_reservations.acquire(id);
        Ok(Self {
            ticket,
            repository,
            action,
            movement,
            allowed,
        })
    }
    pub(crate) fn run(&self, git: &publish::GitTool) -> KeeperResult {
        let may_change = self
            .ticket
            .verification
            .as_ref()
            .filter(|v| v.outcome == VerificationOutcome::Passed)
            .and_then(Verification::verified_commit)
            .zip(self.ticket.open_pull_request())
            .is_some_and(|(verified, pr)| {
                self.ticket.publishing.is_none()
                    && verification::head(&self.ticket).is_ok_and(|head| head == pr.head)
                    && live::verified_head(&self.ticket, verified, || {
                        Ok(self.repository.base.clone())
                    })
                    .is_ok()
            });
        let allowed = self
            .allowed
            .clone()
            .map_err(anyhow::Error::msg)
            .and_then(|()| {
                if matches!(self.action, KeeperAction::Waiting) {
                    ensure!(may_change, "the keeper may no longer change the branch");
                }
                Ok(())
            });
        let movable = allowed
            .and_then(|()| match &self.movement {
                Some(movement) => movement.validate(git),
                None => Ok(()),
            })
            .map_err(|e| format!("{e:#}"));
        let moved = movable
            .clone()
            .map_err(anyhow::Error::msg)
            .and_then(|()| match &self.movement {
                Some(movement) => movement.run(git),
                None => Ok(()),
            })
            .map_err(|e| format!("{e:#}"));
        let cleanup = if matches!(
            self.action,
            KeeperAction::Waiting | KeeperAction::Fetched(..)
        ) {
            git_output(
                git,
                Path::new(&self.repository.path),
                &[
                    "update-ref",
                    "-d",
                    &format!("refs/wiffletree/keeper/{}", self.ticket.id),
                ],
            )
            .map(drop)
            .map_err(|e| format!("{e:#}"))
        } else {
            Ok(())
        };
        let head = verification::head(&self.ticket).map_err(|e| format!("{e:#}"));
        let clean =
            worktrees::is_clean(Path::new(&self.ticket.worktree)).map_err(|e| format!("{e:#}"));
        let commits = self.ticket.keeper_reviews().last().map_or(Ok(0), |review| {
            git_output(
                git,
                Path::new(&self.ticket.worktree),
                &[
                    "rev-list",
                    "--count",
                    &format!("{}..{}", review.base, review.after),
                ],
            )
            .and_then(|count| Ok(count.trim().parse()?))
            .map_err(|e| format!("{e:#}"))
        });
        let publication = Publication {
            ticket: self.ticket.clone(),
            repository: self.repository.clone(),
        }
        .run();
        KeeperResult {
            ticket_id: self.ticket.id.clone(),
            from: self.movement.as_ref().map(|m| m.from.clone()),
            to: self.movement.as_ref().map(|m| m.to.clone()),
            moved,
            movable,
            cleanup,
            head,
            clean,
            commits,
            may_change,
            uncertain: false,
            publication,
        }
    }
    pub(crate) fn failed(&self) -> KeeperResult {
        KeeperResult {
            ticket_id: self.ticket.id.clone(),
            from: self.movement.as_ref().map(|m| m.from.clone()),
            to: self.movement.as_ref().map(|m| m.to.clone()),
            moved: Err("The checkout move stopped unexpectedly; inspect the worktree".into()),
            movable: Err("The checkout move stopped unexpectedly; inspect the worktree".into()),
            cleanup: Ok(()),
            head: Err("Checkout move stopped".into()),
            clean: Err("Checkout move stopped".into()),
            commits: Ok(0),
            may_change: false,
            uncertain: true,
            publication: Publication {
                ticket: self.ticket.clone(),
                repository: self.repository.clone(),
            }
            .failed(),
        }
    }
    fn reconcile(&self, host: &mut Host, head: Option<&str>) -> Result<()> {
        if let Some(movement) = &self.movement {
            let head = head.unwrap_or(&movement.to);
            if head != movement.from {
                hold_movement(host, &self.ticket.id, head)?;
            }
        }
        host.db
            .execute("DELETE FROM checkout_moves WHERE id=?1", [&self.ticket.id])?;
        Ok(())
    }
    pub(crate) fn apply(
        self,
        host: &mut Host,
        result: KeeperResult,
        git: &publish::GitTool,
    ) -> Result<Option<publish::Job>> {
        let current = encode(&host.ticket(&self.ticket.id)?)? == encode(&self.ticket)?;
        if result.uncertain || !current {
            let reconciled = self.reconcile(host, result.head.as_deref().ok());
            if reconciled.is_ok() {
                host.keeper_checkout_stopped(&self.ticket.id);
                host.checkout_reservations.release(&self.ticket.id);
            }
            reconciled?;
            bail!(
                "The checkout move did not finish reliably; inspect its worktree before continuing"
            );
        }
        let resulting_head = result.head.clone().ok();
        let publication = result.publication;
        host.completed_move = Some(KeeperResult {
            publication: Publication {
                ticket: self.ticket.clone(),
                repository: self.repository.clone(),
            }
            .failed(),
            ..result
        });
        let ticket_id = self.ticket.id.clone();
        let movement = self.movement.clone();
        let applied = publication.apply(host, |host| match self.action {
            KeeperAction::Waiting => host.keeper_move(&ticket_id),
            KeeperAction::ReviewDone => host.keeper_review_done(&ticket_id),
            KeeperAction::Fetched(fetched, in_turn) => {
                host.keeper_fetched(fetched, in_turn, git).map(|()| None)
            }
            KeeperAction::PutBack(review, why) => {
                let mut ticket = host.ticket(&ticket_id)?;
                host.put_back(&mut ticket, &review, &why).map(|()| None)
            }
        });
        host.completed_move = None;
        let followup = host
            .checkout_rollbacks
            .iter()
            .any(|(id, _, _)| id == &ticket_id);
        if applied.is_ok() && !followup {
            host.db
                .execute("DELETE FROM checkout_moves WHERE id=?1", [&ticket_id])?;
        } else if applied.is_err() {
            if let Some(movement) = movement {
                let head = resulting_head.as_deref().unwrap_or(&movement.to);
                if head != movement.from {
                    hold_movement(host, &ticket_id, head)?;
                }
            }
            host.db
                .execute("DELETE FROM checkout_moves WHERE id=?1", [&ticket_id])?;
            host.keeper_checkout_stopped(&ticket_id);
        }
        if !followup {
            host.checkout_reservations.release(&ticket_id);
        }
        applied
    }
}
impl KeeperResult {
    pub(crate) fn for_id(&self, id: &str) -> bool {
        self.ticket_id == id
    }
    pub(crate) fn for_ticket(&self, ticket: &Ticket) -> bool {
        self.for_id(&ticket.id)
    }
    pub(crate) fn matches(&self, ticket: &Ticket, from: &str, to: &str) -> bool {
        self.for_ticket(ticket)
            && self.from.as_deref() == Some(from)
            && self.to.as_deref() == Some(to)
    }
    pub(crate) fn movable(&self) -> Result<()> {
        self.movable.clone().map_err(anyhow::Error::msg)
    }
    pub(crate) fn cleanup(&self) -> Result<()> {
        self.cleanup.clone().map_err(anyhow::Error::msg)
    }
    pub(crate) fn movement(&self, ticket: &Ticket, from: &str, to: &str) -> Result<()> {
        ensure!(
            self.matches(ticket, from, to),
            "A different checkout move must run as its own job"
        );
        self.moved.clone().map_err(anyhow::Error::msg)
    }
    pub(crate) fn may_change(&self, ticket: &Ticket) -> Option<bool> {
        self.for_ticket(ticket).then_some(self.may_change)
    }
}
/// An interrupted movement never silently publishes. Its durable intent holds any head that
/// differs from the starting commit as unreviewed before the actor admits further work.
pub(crate) fn recover_moves(host: &mut Host) -> Result<()> {
    let moves: Vec<Movement> =
        host.list_data("SELECT data FROM checkout_moves ORDER BY rowid", [])?;
    for movement in moves {
        let ticket = host.ticket(&movement.ticket.id)?;
        let head = verification::head(&ticket).unwrap_or(movement.to);
        host.atomically(|host| {
            if head != movement.from {
                hold_movement(host, &ticket.id, &head)?;
            }
            host.db
                .execute("DELETE FROM checkout_moves WHERE id=?1", [&ticket.id])?;
            Ok(())
        })?;
    }
    Ok(())
}

fn hold_movement(host: &mut Host, ticket_id: &str, head: &str) -> Result<()> {
    let mut ticket = host.ticket(ticket_id)?;
    ticket.keeper.get_or_insert_default().unreviewed = Some(Unreviewed {
        head: head.into(),
        commits: 1,
        cycle: ticket.verification.as_ref().map_or(0, |v| v.cycle),
    });
    host.save_ticket(&ticket)?;
    host.send(&format!("{KEEPER_NOTICE_PREFIX}{}:checkout-recovery:{head:.20}", ticket.id), None, &ticket.coordinator_id,
        "Wiffletree stopped during a ticket checkout move. Inspect its worktree and start a review before publishing; the resulting head is held as unreviewed.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn creation_reserves_colliding_names_and_resumes_the_same_worktree_after_restart() {
        let mut f = publish::tests::fixture();
        let repository = f.ticket().repository_id;
        let prepare = |host: &Host, title: &str| match Creation::prepare(
            host,
            &f.coordinator.id,
            &repository,
            title,
            "Do",
        )
        .unwrap()
        {
            Prepared::Create(plan) => plan,
            _ => panic!("new ticket"),
        };
        let git = publish::Tools::default().git;
        let first = prepare(&f.host, "A/B");
        let branches = first.inspect(&git).unwrap();
        let first = first.reserve(&f.host, &branches).unwrap();
        let second = prepare(&f.host, "A B").reserve(&f.host, &branches).unwrap();
        assert_ne!(first.ticket.worktree, second.ticket.worktree);
        assert_ne!(first.ticket.branch, second.ticket.branch);
        first.run(&git).unwrap();
        let id = first.ticket.id.clone();
        let home = f.host.home.clone();
        drop(f.host);
        f.host = Host::open(home).unwrap();
        let recovered = pending_creations(&f.host)
            .unwrap()
            .into_iter()
            .find(|p| p.ticket.id == id)
            .unwrap();
        recovered.run(&git).unwrap();
        let created = recovered.apply(&mut f.host).unwrap();
        assert_eq!(created.id, id);
        assert!(
            pending_creations(&f.host)
                .unwrap()
                .iter()
                .all(|p| p.ticket.id != id)
        );
        assert_eq!(
            f.host
                .tickets()
                .unwrap()
                .iter()
                .filter(|t| t.id == id)
                .count(),
            1
        );
    }

    #[test]
    fn a_creation_observed_before_the_coordinator_was_archived_cannot_apply() {
        let mut f = publish::tests::fixture();
        let repository = f.ticket().repository_id;
        let Prepared::Create(plan) =
            Creation::prepare(&f.host, &f.coordinator.id, &repository, "Later", "Do").unwrap()
        else {
            panic!()
        };
        let git = publish::Tools::default().git;
        let branches = plan.inspect(&git).unwrap();
        let reserved = plan.reserve(&f.host, &branches).unwrap();
        reserved.run(&git).unwrap();
        let mut coordinator = f.coordinator.clone();
        coordinator.archived = true;
        f.host
            .db
            .execute(
                "UPDATE sessions SET data=?2 WHERE id=?1",
                params![coordinator.id, encode(&coordinator).unwrap()],
            )
            .unwrap();
        let id = reserved.ticket.id.clone();
        assert!(reserved.apply(&mut f.host).is_err());
        assert!(f.host.ticket(&id).is_err());
    }

    #[test]
    fn dirty_worktrees_are_refused_even_when_git_hides_untracked_files() {
        let (mut f, remote, _) = keeper::tests::waiting();
        let before = f.head();
        runtime::git_output(&f.worktree, &["config", "status.showUntrackedFiles", "no"]).unwrap();
        fs::write(f.worktree.join("untracked"), "keep me").unwrap();
        let job = KeeperCheckout::prepare(&mut f.host, &f.ticket, KeeperAction::Waiting).unwrap();
        let done = job.run(&remote.tools.git);
        assert!(
            done.moved
                .as_ref()
                .unwrap_err()
                .contains("uncommitted or untracked")
        );
        assert_eq!(f.head(), before);
        job.apply(&mut f.host, done, &remote.tools.git).unwrap();
        assert_eq!(
            fs::read_to_string(f.worktree.join("untracked")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn a_reserved_move_reports_back_before_store_state_applies_and_releases_its_reservation() {
        let (mut f, remote, target) = keeper::tests::waiting();
        let id = f.ticket.clone();
        let job = KeeperCheckout::prepare(&mut f.host, &id, KeeperAction::Waiting).unwrap();
        assert!(f.host.checkout_reservations.contains(&id));
        assert!(f.host.checkout_reservations.blocks_command(
            &f.host,
            &Command::SetArchived {
                session_id: f.coordinator.id.clone(),
                archived: true
            }
        ));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let real = remote.tools.git.clone();
        let git: publish::GitTool = Arc::new(move |path, args| {
            seen.lock()
                .unwrap()
                .push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            real(path, args)
        });
        let done = job.run(&git);
        assert_eq!(f.head(), target);
        assert!(f.ticket().keeper.unwrap().waiting.is_some());
        let calls = calls.lock().unwrap();
        let check = calls
            .iter()
            .position(|a| a.first().is_some_and(|a| a == "status"))
            .unwrap();
        let reset = calls
            .iter()
            .position(|a| a.first().is_some_and(|a| a == "reset"))
            .unwrap();
        assert!(check < reset);
        drop(calls);
        let _network_step = job.apply(&mut f.host, done, &git).unwrap();
        assert!(!f.host.checkout_reservations.contains(&id));
        assert!(f.ticket().keeper.unwrap().waiting.is_none());
        assert_eq!(
            f.host
                .db
                .query_row("SELECT count(*) FROM checkout_moves", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn a_stop_between_git_and_apply_holds_the_result_before_admission() {
        let (mut f, remote, target) = keeper::tests::waiting();
        let id = f.ticket.clone();
        let job = KeeperCheckout::prepare(&mut f.host, &id, KeeperAction::Waiting).unwrap();
        let _unapplied = job.run(&remote.tools.git);
        assert_eq!(f.head(), target);
        let home = f.host.home.clone();
        drop(f.host);
        f.host = Host::open(home).unwrap();
        recover_moves(&mut f.host).unwrap();
        assert_eq!(f.ticket().unreviewed().unwrap().head, target);
        assert_eq!(
            f.host
                .db
                .query_row("SELECT count(*) FROM checkout_moves", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn a_panicking_git_adapter_after_a_move_keeps_the_result_held() {
        let (mut f, remote, target) = keeper::tests::waiting();
        let job = KeeperCheckout::prepare(&mut f.host, &f.ticket, KeeperAction::Waiting).unwrap();
        let real = remote.tools.git.clone();
        let git: publish::GitTool = Arc::new(move |path, args| {
            let result = real(path, args);
            if args.first() == Some(&"reset") {
                panic!("after Git changed HEAD");
            }
            result
        });
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run(&git))).is_err());
        let fallback = job.failed();
        assert!(job.apply(&mut f.host, fallback, &git).is_err());
        assert_eq!(f.head(), target);
        assert_eq!(f.ticket().unreviewed().unwrap().head, target);
    }

    #[test]
    fn publication_revalidates_the_review_and_repository_before_its_result_can_apply() {
        let f = publish::tests::fixture();
        let job = Publication::prepare(&f.host, &f.ticket).unwrap();
        let observed = job.run();
        let mut ticket = f.ticket();
        ticket.state = TicketState::Closed;
        f.host.save_ticket(&ticket).unwrap();
        assert!(
            observed
                .get(&ticket, &f.host.ticket_repository(&ticket).unwrap())
                .is_err()
        );
    }
    #[test]
    fn an_invalidated_creation_releases_its_title_and_a_retry_uses_the_current_base() {
        let mut f = publish::tests::fixture();
        let mut repository = f.host.ticket_repository(&f.ticket()).unwrap();
        let git = publish::Tools::default().git;
        let Prepared::Create(plan) =
            Creation::prepare(&f.host, &f.coordinator.id, &repository.id, "Retry", "Do").unwrap()
        else {
            panic!()
        };
        let branches = plan.inspect(&git).unwrap();
        let old = plan.reserve(&f.host, &branches).unwrap();
        let old_path = old.ticket.worktree.clone();
        old.run(&git).unwrap();
        repository.base = "HEAD".into();
        f.host
            .db
            .execute(
                "UPDATE repositories SET data=?2 WHERE id=?1",
                params![repository.id, encode(&repository).unwrap()],
            )
            .unwrap();
        assert!(old.apply(&mut f.host).is_err());
        assert!(pending_creations(&f.host).unwrap().is_empty());
        let retried = f
            .host
            .create_ticket(&f.coordinator.id, &repository.id, "Retry", "Do")
            .unwrap();
        assert_ne!(retried.worktree, old_path);
        assert!(
            Path::new(&old_path).exists(),
            "an invalidation never deletes its checkout artifacts"
        );
    }
}
