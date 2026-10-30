//! Durable ticket workspaces under each project's coordinator, and scoped agent operations.
use crate::*;

/// Reports per child that `workspace_context` lists, newest first.
const CONTEXT_REPORTS_PER_CHILD: usize = 5;
/// Characters of a report body shown in `workspace_context`.
const CONTEXT_REPORT_SUMMARY_CHARS: usize = 200;

impl Host {
    pub fn tickets(&self) -> Result<Vec<Ticket>> {
        self.list_data("SELECT data FROM tickets ORDER BY rowid", [])
    }
    pub(crate) fn running_verifications(&self) -> Result<Vec<Ticket>> {
        self.list_data("SELECT data FROM tickets WHERE json_extract(data,'$.verification.outcome')='running' ORDER BY rowid", [])
    }
    pub fn runtimes(&self) -> Result<Vec<SessionRuntime>> {
        self.list_data("SELECT data FROM runtimes ORDER BY rowid", [])
    }
    pub fn live_projects(&self) -> Result<Vec<String>> {
        Ok(self
            .db
            .prepare("SELECT project_id FROM live_projects WHERE enabled=1")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn set_live(&mut self, project: &str, enabled: bool) -> Result<()> {
        self.project(project)?;
        self.db.execute("INSERT INTO live_projects VALUES (?1,?2) ON CONFLICT(project_id) DO UPDATE SET enabled=excluded.enabled", params![project,enabled])?;
        Self::event(
            &self.db,
            project,
            None,
            "live_mode",
            if enabled { "Enabled" } else { "Stopped" },
        )
    }
    pub fn session_runtime(&self, id: &str) -> Result<SessionRuntime> {
        self.session(id)?;
        Ok(self
            .db
            .query_row("SELECT data FROM runtimes WHERE session_id=?1", [id], |r| {
                decode(r, 0)
            })
            .optional()?
            .unwrap_or(SessionRuntime {
                session_id: id.into(),
                ..Default::default()
            }))
    }
    pub fn save_runtime(&self, runtime: &SessionRuntime) -> Result<()> {
        self.db.execute("INSERT INTO runtimes VALUES (?1,?2) ON CONFLICT(session_id) DO UPDATE SET data=excluded.data", params![runtime.session_id,encode(runtime)?])?;
        Ok(())
    }
    pub fn configure_session(&mut self, id: &str, profile: ModelProfile) -> Result<()> {
        text(&profile.model, 128)?;
        text(&profile.effort, 32)?;
        let mut session = self.session(id)?;
        ensure!(
            session.role != Role::Keeper,
            "A PR keeper agent takes its model from the PR keeper's row in Models → Who runs what"
        );
        ensure!(
            session.status != Status::Working,
            "Stop the current turn before changing its runtime"
        );
        let mut runtime = self.session_runtime(id)?;
        ensure!(
            runtime.provider_session_id.is_none() || session.provider == profile.provider,
            "A conversation cannot change provider after starting; create a replacement session"
        );
        session.provider = profile.provider;
        runtime.profile = Some(profile);
        // The human's choice wins: it never waits for a reset.
        runtime.waiting = false;
        runtime.selection = Some(Selection {
            chosen_by: Chooser::Human,
            reason: "Chosen by you".into(),
            revision: self.model_selection()?.revision,
            at: now(),
        });
        self.db.execute(
            "UPDATE sessions SET data=?2 WHERE id=?1",
            params![id, encode(&session)?],
        )?;
        self.save_runtime(&runtime)
    }
    /// Releases a paused or interrupted session: its held input is queued again or dropped.
    /// Both the human's Retry and Skip and a coordinator's resume_agent come through here, as
    /// does the automatic retry after a usage limit resets, which either of them cancels.
    pub fn reconcile_session(&mut self, id: &str, retry: bool) -> Result<()> {
        let session = self.session(id)?;
        ensure!(
            session.status != Status::Working,
            "Stop the active turn first"
        );
        self.db.execute(
            "UPDATE messages SET receipt=?2 WHERE recipient=?1 AND receipt='held'",
            params![id, if retry { "queued" } else { "cancelled" }],
        )?;
        // A held turn retains its input as queued; skipping drops it as it would held input,
        // even if the hold's cause was fixed meanwhile.
        let mut runtime = self.session_runtime(id)?;
        if !retry && runtime.held {
            self.db.execute(
                "UPDATE messages SET receipt='cancelled' WHERE recipient=?1 AND receipt='queued'",
                [id],
            )?;
            runtime.held = false;
        }
        runtime.last_error = None;
        runtime.stopped_by = None;
        runtime.limit_retry = None;
        self.save_runtime(&runtime)?;
        self.set_status(id, Status::Ready)
    }
    /// Records who stopped a session, and whether this is news. A parent's stop never replaces
    /// the human's emergency stop.
    pub fn record_stop(&mut self, id: &str, by: Stopper) -> Result<bool> {
        let mut runtime = self.session_runtime(id)?;
        if runtime.stopped_by == Some(by) || runtime.stopped_by == Some(Stopper::Human) {
            return Ok(false);
        }
        runtime.stopped_by = Some(by);
        self.save_runtime(&runtime)?;
        Ok(true)
    }
    /// Forgets who stopped a session once it runs again.
    pub(crate) fn clear_stop(&mut self, id: &str) -> Result<()> {
        let mut runtime = self.session_runtime(id)?;
        if runtime.stopped_by.take().is_some() {
            self.save_runtime(&runtime)?;
        }
        Ok(())
    }
    /// Input an interrupted turn left held for Retry or Skip.
    pub fn held_input(&self, id: &str) -> Result<i64> {
        Ok(self.db.query_row(
            "SELECT COUNT(*) FROM messages WHERE recipient=?1 AND receipt='held'",
            [id],
            |r| r.get(0),
        )?)
    }
    /// Input waiting to start the session's next turn; quiet messages never start one.
    pub fn queued_input(&self, id: &str) -> Result<i64> {
        Ok(self.db.query_row(
            "SELECT COUNT(*) FROM messages WHERE recipient=?1 AND receipt='queued' AND quiet=0",
            [id],
            |r| r.get(0),
        )?)
    }
    /// A ticket is a workspace: one repository the project uses, one worktree and branch. Repeating
    /// the same coordinator, repository and title returns the existing ticket.
    pub fn create_ticket(
        &mut self,
        owner: &str,
        repository_id: &str,
        title: &str,
        brief: &str,
    ) -> Result<Ticket> {
        self.create_stacked_ticket(owner, repository_id, title, brief, None)
    }
    /// `create_ticket`, with the branch started from the `stack_on` ticket's branch when given.
    pub fn create_stacked_ticket(
        &mut self,
        owner: &str,
        repository_id: &str,
        title: &str,
        brief: &str,
        stack_on: Option<&str>,
    ) -> Result<Ticket> {
        let git = publish::Tools::default().git;
        self.create_ticket_with(owner, repository_id, title, brief, stack_on, None, &git)
    }
    /// `create_stacked_ticket`, or with the branch started from the `base` branch on origin when
    /// given, running Git's network commands with `git`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_ticket_with(
        &mut self,
        owner: &str,
        repository_id: &str,
        title: &str,
        brief: &str,
        stack_on: Option<&str>,
        base: Option<&str>,
        git: &publish::GitTool,
    ) -> Result<Ticket> {
        let prepared =
            checkout::Creation::prepare(self, owner, repository_id, title, brief, stack_on, base)?;
        match prepared {
            checkout::Prepared::Existing(ticket) => Ok(*ticket),
            checkout::Prepared::Create(plan) => {
                let branches = plan.inspect(git)?;
                let creation = plan.reserve(self, &branches)?;
                creation.run(git)?;
                creation.apply(self)
            }
        }
    }
    pub fn ticket(&self, id: &str) -> Result<Ticket> {
        self.db
            .query_row("SELECT data FROM tickets WHERE id=?1", [id], |r| {
                decode(r, 0)
            })
            .context("Ticket not found")
    }
    pub(crate) fn save_ticket(&self, ticket: &Ticket) -> Result<()> {
        self.db.execute(
            "UPDATE tickets SET data=?2 WHERE id=?1",
            params![ticket.id, encode(ticket)?],
        )?;
        Ok(())
    }
    pub fn assign_ticket(
        &mut self,
        ticket_id: &str,
        role: Role,
        provider: Provider,
        instruction: &str,
        focus: Option<&str>,
    ) -> Result<Session> {
        text(instruction, MAX_TEXT_BYTES)?;
        ensure!(
            role != Role::Keeper,
            "Only Wiffletree starts PR keeper agents"
        );
        let mut ticket = self.ticket(ticket_id)?;
        let (session, created) = self.ticket_agent(&ticket, role, provider, focus)?;
        if !created {
            return Ok(session);
        }
        self.send(
            &format!("assignment:{}", session.id),
            Some(&ticket.coordinator_id),
            &session.id,
            &format!(
                "Ticket: {}\n{}\n\nAssignment: {}\n\nWorktree: {}\nBranch: {}",
                ticket.title, ticket.brief, instruction, ticket.worktree, ticket.branch
            ),
        )?;
        // A verification cycle alone moves the ticket while it runs.
        if ticket.running_cycle().is_none() {
            ticket.state = TicketState::Assigned;
            self.save_ticket(&ticket)?;
        }
        Ok(session)
    }
    /// The ticket's agent with this role and focus, created as the coordinator's child working
    /// in the ticket's worktree when there is none yet. A role session is never recycled across
    /// tickets; a different focus adds another agent in that role, such as a second reviewer.
    pub(crate) fn ticket_agent(
        &mut self,
        ticket: &Ticket,
        role: Role,
        provider: Provider,
        focus: Option<&str>,
    ) -> Result<(Session, bool)> {
        if let Some(focus) = focus {
            text(focus, 60)?;
        }
        ensure!(
            role.is_worker(),
            "Assign an implementer, tester or reviewer"
        );
        ensure!(ticket.is_open(), "Ticket is already {}", ticket.state);
        let owner = self.session(&ticket.coordinator_id)?;
        if let Some(session) = self.existing_assignment(&ticket.id, role, focus)? {
            ensure!(
                session.provider == provider,
                "Existing ticket agent uses another provider"
            );
            return Ok((session, false));
        }
        let session = self.create_session(
            &owner.project_id,
            &owner.id,
            Some(&ticket.repository_id),
            &format!(
                "{} · {}",
                role.agent_label(focus),
                ticket.title.chars().take(80).collect::<String>()
            ),
            role,
            provider,
        )?;
        self.save_runtime(&SessionRuntime {
            session_id: session.id.clone(),
            ticket_id: Some(ticket.id.clone()),
            focus: focus.map(Into::into),
            workdir: Some(ticket.worktree.clone()),
            ..Default::default()
        })?;
        Ok((session, true))
    }
    /// The ticket's agents, archived or not.
    pub(crate) fn ticket_agents(&self, ticket_id: &str) -> Result<Vec<Session>> {
        self.runtimes()?
            .into_iter()
            .filter(|r| r.ticket_id.as_deref() == Some(ticket_id))
            .map(|r| self.session(&r.session_id))
            .collect()
    }
    /// A role session is never recycled across tickets. Retrying the same assignment is
    /// idempotent; a different focus adds another agent in that role, such as a second reviewer.
    /// Archived agents, such as a retired verifier, are not reused.
    pub(crate) fn existing_assignment(
        &self,
        ticket_id: &str,
        role: Role,
        focus: Option<&str>,
    ) -> Result<Option<Session>> {
        self.runtimes()?
            .into_iter()
            .find(|r| {
                r.ticket_id.as_deref() == Some(ticket_id)
                    && r.focus.as_deref() == focus
                    && self
                        .session(&r.session_id)
                        .is_ok_and(|s| s.role == role && !s.archived)
            })
            .map(|r| self.session(&r.session_id))
            .transpose()
    }
    /// Retires one ticket agent the coordinator no longer needs, keeping it restorable. Refuses
    /// an agent in its turn, an open ticket's implementer, and a verifier whose result the
    /// running cycle still needs: pending, or failed and due to re-check the fix.
    pub fn archive_agent(&mut self, session_id: &str) -> Result<Session> {
        let agent = self.session(session_id)?;
        if agent.archived {
            return Ok(agent);
        }
        let ticket = self
            .session_runtime(&agent.id)?
            .ticket_id
            .map(|id| self.ticket(&id))
            .transpose()?
            .with_context(|| format!("{} is not a ticket agent", agent.name))?;
        ensure!(
            agent.status != Status::Working,
            "{} is mid-turn; wait for its report or stop it with stop_agents, then archive it",
            agent.name
        );
        ensure!(
            agent.role != Role::Implementer || !ticket.is_open(),
            "{} is the implementer of open ticket \"{}\"; it stays until the ticket is accepted or closed",
            agent.name,
            ticket.title
        );
        if let Some(cycle) = ticket.running_cycle() {
            ensure!(
                !cycle
                    .latest_results()
                    .iter()
                    .any(|run| run.session_id == agent.id && run.result != VerifierResult::Passed),
                "{} still owes verification cycle {} a result; it is archived when the cycle ends",
                agent.name,
                cycle.cycle
            );
        }
        self.set_archived(&agent.id, true)?;
        self.session(&agent.id)
    }
    /// Archives a finished ticket's agents and removes its worktree. Their conversations and the
    /// branch are kept. Refuses while an agent is working or the worktree holds unsaved work.
    pub fn close_ticket(&mut self, ticket_id: &str) -> Result<Ticket> {
        let ticket = self.ticket(ticket_id)?;
        if ticket.state == TicketState::Closed {
            return Ok(ticket);
        }
        if let Some(busy) = self
            .ticket_agents(ticket_id)?
            .into_iter()
            .find(|s| s.status == Status::Working)
        {
            bail!(
                "{} is still working; close the ticket after it reports",
                busy.name
            );
        }
        self.finish_ticket(ticket, TicketState::Closed, "ticket_closed")
    }
    /// Accepts a ticket on its latest verification cycle: one that passed, or, with a waiver
    /// reason, one blocked only by findings it checked. No cycle may be running or finding
    /// untriaged, and HEAD must be the verified commit or patch-equivalent to it, or, at the
    /// full-review cap, past it only by commits the waiver names. The ticket's
    /// state is not the gate, so a report after the cycle does not undo it. Like closing, it
    /// archives the agents and removes the worktree, keeping the branch.
    pub fn accept_ticket(&mut self, ticket_id: &str, waived: Option<&str>) -> Result<Ticket> {
        let mut ticket = self.ticket(ticket_id)?;
        if ticket.state == TicketState::Accepted {
            return Ok(ticket);
        }
        ensure!(ticket.is_open(), "Ticket is already {}", ticket.state);
        if let Some(cycle) = ticket.running_cycle() {
            bail!(
                "Verification cycle {} is still running; accept the ticket once it ends",
                cycle.cycle
            );
        }
        let verification = ticket.verification.as_ref().context(
            "Start verification with verify_ticket; acceptance needs the commit it verified",
        )?;
        let waived = waived.map(one_line_reason).transpose()?;
        let unreviewed_waiver = self.unreviewed_waiver(&ticket, waived);
        match (verification.outcome, waived) {
            (VerificationOutcome::Passed, None) | (VerificationOutcome::Blocked, Some(_)) => {}
            (VerificationOutcome::Passed, Some(_)) if unreviewed_waiver.is_some() => {}
            (VerificationOutcome::Passed, Some(_)) => bail!(
                "Verification cycle {} passed; there is nothing to waive",
                verification.cycle
            ),
            _ => bail!(
                "Verification cycle {} is blocked; fix and verify again, or accept with a waived reason",
                verification.cycle
            ),
        }
        ensure_findings_settled(&ticket, verification, "accepting")?;
        let verified = verification.verified_commit().context("No round")?;
        let waived_commits = self.check_verified_head(&ticket, verified, unreviewed_waiver)?;
        ensure!(
            verification.outcome == VerificationOutcome::Blocked
                || waived.is_none()
                || !waived_commits.is_empty(),
            "Verification cycle {} passed at the commit checked out; there is nothing to waive",
            verification.cycle
        );
        // Commits a publish's waiver covered are accepted only under a waiver naming them too.
        ensure!(
            waived.is_some() || waived_commits.is_empty(),
            "The branch has commits no review checked; accept with waived naming each by its short SHA"
        );
        ticket.waiver = waived.map(Into::into);
        ticket.waived_commits = waived_commits;
        self.finish_ticket(ticket, TicketState::Accepted, "ticket_accepted")
    }
    /// HEAD is the verified commit, or has the same patch id against the ticket's base, as
    /// after a squash or a clean rebase, or, with a waiver `unreviewed_waiver` allows at the
    /// full-review cap, is past it only by commits the waiver names, which are returned. Either
    /// way the keeper adopted no commit no review has checked.
    pub(crate) fn check_verified_head(
        &self,
        ticket: &Ticket,
        verified: &str,
        waived: Option<&str>,
    ) -> Result<Vec<String>> {
        verified_head(ticket, verified, || self.ticket_base(ticket), waived)
    }
    /// The waiver, when it may cover commits no review checked: once the ticket has started
    /// every full review the cycle cap allows. Below the cap they need a review.
    pub(crate) fn unreviewed_waiver<'a>(
        &self,
        ticket: &Ticket,
        waived: Option<&'a str>,
    ) -> Option<&'a str> {
        let cap = self.settings().verification.max_cycles as usize;
        waived.filter(|_| verification::full_reviews(ticket) >= cap)
    }
    fn finish_ticket(
        &mut self,
        mut ticket: Ticket,
        state: TicketState,
        milestone: &str,
    ) -> Result<Ticket> {
        // The agents' processes are stopped before the worktree goes, and not at all when the
        // worktree cannot be removed.
        self.check_removable(std::slice::from_ref(&ticket))?;
        let agents = self.ticket_agents(&ticket.id)?;
        let stopped = self.stop_started_by(agents.iter().map(|agent| &agent.id))?;
        let finished = self.with_worktrees_removed(&[ticket.clone()], |host| {
            // Before its caller is archived, so the caller still gets the request's one message.
            host.withdraw_request(&mut ticket, &format!("the ticket was {state}"))?;
            for agent in host.ticket_agents(&ticket.id)? {
                host.set_archived(&agent.id, true)?;
            }
            ticket.state = state;
            host.save_ticket(&ticket)?;
            let coordinator = host.session(&ticket.coordinator_id)?;
            let body = match &ticket.waiver {
                Some(waiver) => {
                    let open: Vec<&str> = ticket
                        .ledger
                        .iter()
                        .filter(|e| e.status == EntryStatus::Open)
                        .map(|e| e.id.as_str())
                        .collect();
                    let mut body =
                        format!("{}; waived: {waiver}; open: {}", ticket.id, open.join(", "));
                    if !ticket.waived_commits.is_empty() {
                        let commits: Vec<&str> =
                            ticket.waived_commits.iter().map(|c| short_sha(c)).collect();
                        body.push_str(&format!("; unreviewed: {}", commits.join(", ")));
                    }
                    body
                }
                None => ticket.id.clone(),
            };
            Self::event(
                &host.db,
                &coordinator.project_id,
                Some(&coordinator.id),
                milestone,
                &body,
            )
        });
        // The stop cannot be undone, so what it says depends on whether the ticket was finished.
        self.archive_settled(&stopped, finished.is_ok());
        finished?;
        Ok(ticket)
    }
    pub fn agent_context(&self, id: &str) -> Result<Value> {
        let session = self.session(id)?;
        let waiting = self.questions_waiting()?;
        let mut team = Vec::new();
        for member in self.sessions()? {
            if member.project_id == session.project_id {
                let count = waiting
                    .iter()
                    .find(|q| q.session_id == member.id)
                    .map_or(0, |q| q.count);
                if member.archived {
                    team.push(json!({"id":member.id,"parent_id":member.parent_id,"name":member.name,"role":member.role,"status":member.status,"archived":true}));
                    continue;
                }
                let mut entry = serde_json::to_value(self.shown(member))?;
                entry["questions_waiting"] = json!(count);
                team.push(entry);
            }
        }
        // Each repository says whether a ticket there publishes without asking the human, and
        // whether the PR keeper keeps its tickets' PRs.
        let settings = self.settings();
        let mut repositories = vec![];
        for repository in self.project_repositories(&session.project_id)? {
            let mut entry = serde_json::to_value(&repository)?;
            let allowed = is_allowed(&repository, &settings.allowed_owners);
            entry["publishes_without_asking"] = json!(allowed);
            entry["kept"] = json!(allowed && settings.keeper.is_on());
            repositories.push(entry);
        }
        let mut context = json!({"self":self.shown(session.clone()),"project":self.project(&session.project_id)?,
            "repositories":repositories,
            "team":team,
            "tickets":self.ticket_overview(&session.project_id)?,
            "runtime":self.session_runtime(id)?,
            "memory":self.logs(&session.project_id,None,10)?,
            "open_questions":self.open_questions(&session)?});
        if session.role == Role::ProjectOrchestrator {
            context["model_selection"] = self.model_context()?;
            let created = self.created_projects(id)?;
            if !created.is_empty() {
                context["created_projects"] = Value::Array(created);
            }
        }
        let child_reports = self.child_reports(id)?;
        if !child_reports.is_empty() {
            context["child_reports"] = Value::Array(child_reports);
        }
        Ok(context)
    }
    /// The project's open tickets in full, each with its repository name, agents, verification
    /// record and any warnings about its branch; accepted and closed tickets as one short entry,
    /// so the reply stays bounded as the project ages. `ticket_record` has the full record.
    fn ticket_overview(&self, project: &str) -> Result<Vec<Value>> {
        let repositories = self.repositories()?;
        let runtimes = self.runtimes()?;
        let warnings = self.open_branch_warnings()?;
        let mut overview = vec![];
        for ticket in self.shown_tickets()? {
            if !self
                .session(&ticket.coordinator_id)
                .is_ok_and(|s| s.project_id == project)
            {
                continue;
            }
            overview.push(if ticket.is_open() {
                self.full_ticket(&ticket, &repositories, &runtimes, &warnings)?
            } else {
                Self::short_ticket(&ticket, &repositories)
            });
        }
        Ok(overview)
    }
    fn short_ticket(ticket: &Ticket, repositories: &[Repository]) -> Value {
        let mut short = json!({"id":ticket.id,"title":ticket.title,"state":ticket.state,
            "repository":Self::repository_name(repositories, ticket),"branch":ticket.branch});
        if let Some(parent) = &ticket.stacked_on {
            short["stacked_on"] = json!(parent);
        }
        if let Some(base) = &ticket.base {
            short["base"] = json!(base);
        }
        if let Some(pr) = &ticket.pull_request {
            short["pull_request"] = json!({"number":pr.number,"state":pr.state,"url":pr.url});
        }
        short
    }
    /// The projects this coordinator created with `create_project`, each with its tickets as
    /// short entries, so it can follow that work. Read only: it cannot act on those projects.
    fn created_projects(&self, coordinator: &str) -> Result<Vec<Value>> {
        let repositories = self.repositories()?;
        let tickets = self.shown_tickets()?;
        let mut created = vec![];
        for project in self.projects()? {
            if project
                .created_by
                .as_ref()
                .is_none_or(|by| by.session_id != coordinator)
            {
                continue;
            }
            let created_at: Option<i64> = self
                .db
                .query_row(
                    "SELECT created_at FROM activity WHERE project_id=?1 AND kind='project_created'",
                    [&project.id],
                    |r| r.get(0),
                )
                .optional()?;
            let tickets: Vec<Value> = tickets
                .iter()
                .filter(|t| {
                    self.session(&t.coordinator_id)
                        .is_ok_and(|s| s.project_id == project.id)
                })
                .map(|t| Self::short_ticket(t, &repositories))
                .collect();
            created.push(json!({"id":project.id,"name":project.name,"created_at":created_at,"tickets":tickets}));
        }
        Ok(created)
    }
    /// One ticket of the caller's project in full, whatever its state.
    fn ticket_record(&self, caller: &Session, ticket_id: &str) -> Result<Value> {
        let ticket = self.shown_ticket(self.ticket(ticket_id)?);
        ensure!(
            self.session(&ticket.coordinator_id)?.project_id == caller.project_id,
            "Ticket {ticket_id} is not in your project"
        );
        self.full_ticket(
            &ticket,
            &self.repositories()?,
            &self.runtimes()?,
            &self.open_branch_warnings()?,
        )
    }
    fn repository_name<'a>(repositories: &'a [Repository], ticket: &Ticket) -> Option<&'a str> {
        repositories
            .iter()
            .find(|r| r.id == ticket.repository_id)
            .map(|r| r.name.as_str())
    }
    fn full_ticket(
        &self,
        ticket: &Ticket,
        repositories: &[Repository],
        runtimes: &[SessionRuntime],
        warnings: &[BranchWarnings],
    ) -> Result<Value> {
        let mut agents = vec![];
        for runtime in runtimes
            .iter()
            .filter(|r| r.ticket_id.as_ref() == Some(&ticket.id))
        {
            let s = self.shown(self.session(&runtime.session_id)?);
            agents.push(json!({"id":s.id,"name":s.name,"role":s.role,"focus":runtime.focus,"status":s.status,"archived":s.archived}));
        }
        let mut entry = serde_json::to_value(ticket)?;
        entry["repository"] = json!(Self::repository_name(repositories, ticket));
        entry["agents"] = json!(agents);
        if let Some(warnings) = warnings.iter().find(|w| w.ticket_id == ticket.id) {
            entry["warnings"] = json!(warnings.lines());
        }
        Ok(entry)
    }
    /// Recent reports each direct child sent this session, newest first. Children
    /// that never reported, and archived ones, are omitted, so a session without children
    /// gets nothing.
    fn child_reports(&self, id: &str) -> Result<Vec<Value>> {
        let mut children = Vec::new();
        for child in self
            .sessions()?
            .into_iter()
            .filter(|s| s.parent_id.as_deref() == Some(id) && !s.archived)
        {
            let reports = self
                .db
                .prepare("SELECT * FROM messages WHERE recipient=?1 AND sender=?2 AND id LIKE 'report:%' ORDER BY sequence DESC LIMIT ?3")?
                .query_map(params![id, child.id, CONTEXT_REPORTS_PER_CHILD], Self::message_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if reports.is_empty() {
                continue;
            }
            let reports = reports.iter().map(|m| {
                // Body format is "[kind] name\nbody"; see the `report` tool.
                let (head, body) = m.body.split_once('\n').unwrap_or((&m.body, ""));
                let kind = head.strip_prefix('[').and_then(|h| h.split_once(']')).map_or("", |(k, _)| k);
                let summary: String = body.chars().take(CONTEXT_REPORT_SUMMARY_CHARS).collect();
                let ellipsis = if body.chars().count() > CONTEXT_REPORT_SUMMARY_CHARS { "…" } else { "" };
                json!({"at":m.created_at,"kind":kind,"summary":format!("{summary}{ellipsis}"),
                    "read":matches!(m.receipt, Receipt::Delivered | Receipt::Acknowledged | Receipt::Completed)})
            }).collect::<Vec<_>>();
            children.push(json!({"child_id":child.id,"child":child.name,"reports":reports}));
        }
        Ok(children)
    }
    /// Questions this session placed in the inbox that the human has not yet settled.
    pub fn open_questions(&self, session: &Session) -> Result<Vec<Attention>> {
        Ok(self
            .open_attention(&session.project_id)?
            .into_iter()
            .filter(|a| {
                a.session_id == session.id && !a.is_permission() && !is_check_in(&a.operation_id)
            })
            .collect())
    }
    /// The Who runs what table as the coordinator reads it: each row's lines and what each
    /// would start on now and why, the threshold, the review settings and the guide as notes.
    fn model_context(&self) -> Result<Value> {
        let selection = self.model_selection()?;
        let verification = self.settings().verification;
        let (quotas, unusable, switch_at) = self.limits()?;
        let limits = Limits {
            quotas: &quotas,
            unusable: &unusable,
            switch_at,
            now: now(),
        };
        let line = |line: &Line| json!({"first": line.first, "then": line.then});
        let right_now = |row: &JobRow, small: bool| match resolve(
            row,
            small,
            true,
            &limits,
            &policies::reset_time,
        ) {
            Ok(pick) if pick.skip.is_some() => json!({"skips": true, "reason": pick.reason}),
            Ok(pick) => {
                json!({"profile": pick.profile, "waits": pick.waits, "reason": pick.reason})
            }
            Err(error) => json!({"cannot_start": format!("{error:#}")}),
        };
        let jobs: Vec<Value> = (selection.jobs.iter())
            .map(|row| {
                let mut entry = json!({"job": row.job, "name": row.job.name(),
                    "main": line(&row.main), "right_now": right_now(row, false)});
                if let Some(small) = &row.small {
                    entry["small"] = line(small);
                    entry["right_now_small"] = right_now(row, true);
                }
                entry
            })
            .collect();
        Ok(
            json!({"revision": selection.revision, "switch_at": switch_at, "jobs": jobs,
            "verifiers": verification.verifiers, "review_levels": verification.levels,
            "max_rounds": verification.max_rounds, "max_cycles": verification.max_cycles,
            "notes": selection.guide}),
        )
    }
    /// The job an assignment resolves: the role's row, a configured verifier's when the focus
    /// names one, and for an ad-hoc reviewer the row of the verifier `like` names, else the
    /// first reviewer's.
    fn assignment_job(&self, role: Role, focus: Option<&str>, like: Option<&str>) -> Result<Job> {
        let verification = self.settings().verification;
        let configured = |focus: &str| {
            verification
                .verifiers
                .iter()
                .find(|v| same_name(&v.focus, focus))
        };
        if let Some(like) = like {
            ensure!(
                role == Role::Reviewer && focus.is_none_or(|f| configured(f).is_none()),
                "`like` is only for a reviewer whose focus is not a configured verifier"
            );
            let verifier = configured(like).with_context(|| {
                let focuses: Vec<&str> =
                    verification.verifiers.iter().map(|v| v.focus.as_str()).collect();
                format!(
                    "No verifier is named \"{}\". Name one of {} as `like`, or leave it out to use the first reviewer's row.",
                    like.trim(),
                    or_list(&focuses)
                )
            })?;
            return Ok(Job::Verifier {
                focus: verifier.focus.clone(),
            });
        }
        Job::for_role(role, focus, &verification)
            .with_context(|| format!("{} has no row in Models → Who runs what", role.label()))
    }
    /// A tool result: the session plus its pinned profile and how it was chosen.
    fn assignment_result(&self, session: &Session) -> Result<Value> {
        let runtime = self.session_runtime(&session.id)?;
        let mut value = serde_json::to_value(session)?;
        value["profile"] = json!(runtime.profile);
        value["selection"] = json!(runtime.selection);
        Ok(value)
    }
    pub fn agent_tool(&mut self, id: &str, name: &str, args: Value) -> Result<Value> {
        let session = self.session(id)?;
        let string = |key: &str| -> Result<&str> {
            args[key].as_str().with_context(|| format!("Missing {key}"))
        };
        match name {
            "workspace_context" => match args.get("ticket_id").filter(|id| !id.is_null()) {
                Some(_) => self.ticket_record(&session, string("ticket_id")?),
                None => self.agent_context(id),
            },
            "schedule" | "unschedule" | "list_schedules" => {
                self.schedule_tool(&session, name, &args, now())
            }
            "send_message" => {
                let recipient = self.session(string("recipient")?)?;
                ensure!(
                    session.parent_id.as_deref() == Some(&recipient.id)
                        || recipient.parent_id.as_deref() == Some(id),
                    "Messages must follow coordinator ownership"
                );
                ensure!(
                    recipient.role != Role::Keeper,
                    "{} is a PR keeper agent; only Wiffletree messages it",
                    recipient.name
                );
                let message = self.send(
                    &format!("agent:{id}:{}", string("message_id")?),
                    Some(id),
                    &recipient.id,
                    string("body")?,
                )?;
                let mut receipt = serde_json::to_value(&message)?;
                // Tool result only: the stored receipt stays `queued` until the next turn starts.
                let recipient = self.session(&recipient.id)?;
                let held = matches!(recipient.status, Status::Paused | Status::Disconnected);
                // A turn still runs after its session reported, whatever status that stored.
                let in_turn =
                    recipient.status == Status::Working || self.in_turn.contains(&recipient.id);
                if message.receipt == Receipt::Queued && !held && in_turn {
                    receipt["receipt"] = json!("queued_behind_turn");
                    receipt["recipient_turn_started_at"] =
                        json!(self.session_runtime(&recipient.id)?.last_started_at);
                }
                if message.receipt == Receipt::Queued && held {
                    receipt["receipt"] = json!("queued_while_held");
                    let runtime = self.session_runtime(&recipient.id)?;
                    let waits_for_limit =
                        recipient.status == Status::Disconnected && runtime.limit_retry.is_some();
                    receipt["hint"] = json!(match runtime.stopped_by {
                        _ if waits_for_limit => format!(
                            "{} hit its provider's usage limit and resumes by itself when it resets; this runs then. Nothing is needed; resume_agent with held: retry runs it sooner.",
                            recipient.name
                        ),
                        Some(Stopper::Human) => format!(
                            "{} was stopped by the human, so this waits until it is resumed. If it is your child, call resume_agent with session_id {} only when the human says so.",
                            recipient.name, recipient.id
                        ),
                        _ => format!(
                            "{} is stopped, so this waits until it is resumed. If it is your child, call resume_agent with session_id {}.",
                            recipient.name, recipient.id
                        ),
                    });
                }
                Ok(receipt)
            }
            "create_ticket" => {
                ensure!(
                    session.role == Role::ProjectOrchestrator,
                    "Only the project coordinator creates tickets"
                );
                Ok(serde_json::to_value(self.create_ticket_with(
                    id,
                    string("repository_id")?,
                    string("title")?,
                    string("brief")?,
                    args["stack_on"].as_str(),
                    args["base"].as_str(),
                    &publish::Tools::default().git,
                )?)?)
            }
            "create_project" => {
                service::only_arguments(
                    "create_project",
                    &args,
                    &["name", "brief", "repository_ids"],
                )?;
                let repositories = match args.get("repository_ids").filter(|ids| !ids.is_null()) {
                    Some(ids) => Some(serde_json::from_value(ids.clone())?),
                    None => None,
                };
                Ok(serde_json::to_value(self.create_project_from(
                    id,
                    string("name")?,
                    string("brief")?,
                    repositories,
                )?)?)
            }
            "assign_ticket" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                ensure!(ticket.is_open(), "Ticket is already {}", ticket.state);
                let role: Role = serde_json::from_value(args["role"].clone())?;
                ensure!(
                    role != Role::Keeper,
                    "Only Wiffletree starts PR keeper agents"
                );
                refuse_model_arguments(
                    &args,
                    &["profile", "reason", "size", "complexity", "provider"],
                )?;
                let small = small_work(&args)?;
                let focus = args["focus"].as_str();
                if let Some(existing) = self.existing_assignment(&ticket.id, role, focus)? {
                    return self.assignment_result(&existing);
                }
                let job = self.assignment_job(role, focus, args["like"].as_str())?;
                let pick = self.resolve_job(&job, small, false)?;
                let instruction = string("instruction")?;
                // The agent, its instruction and the ticket update exist only with the agent's
                // model and reason, or not at all.
                let worker = self.atomically(|host| {
                    let worker = host.assign_ticket(
                        &ticket.id,
                        role,
                        pick.profile.provider,
                        instruction,
                        focus,
                    )?;
                    host.pin_pick(&worker, &pick)?;
                    Ok(worker)
                })?;
                self.assignment_result(&worker)
            }
            "report" => {
                let findings: Vec<ReportedFinding> = match args.get("findings") {
                    Some(findings) => serde_json::from_value(findings.clone())
                        .context("Each finding needs a severity of blocking, non_blocking or pre_existing, a location and a summary")?,
                    None => vec![],
                };
                self.report(
                    &session,
                    string("kind")?,
                    string("body")?,
                    string("message_id")?,
                    &findings,
                )
            }
            "verify_ticket" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                refuse_model_arguments(&args, &["verifiers", "profile", "reason"])?;
                let small = small_work(&args)?;
                let level = args["level"]
                    .as_str()
                    .with_context(|| self.settings().verification.name_a_level())?;
                Ok(serde_json::to_value(
                    self.verify_ticket(&ticket.id, level, small)?,
                )?)
            }
            "triage_findings" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                let decisions: Vec<verification::Triage> =
                    serde_json::from_value(args.get("decisions").cloned().context(
                        "Missing decisions: give each finding's id, decision and reason",
                    )?)?;
                Ok(serde_json::to_value(
                    self.triage_findings(&ticket.id, &decisions)?,
                )?)
            }
            "accept_ticket" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                Ok(serde_json::to_value(
                    self.accept_ticket(&ticket.id, args["waived"].as_str())?,
                )?)
            }
            "close_ticket" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                Ok(serde_json::to_value(self.close_ticket(&ticket.id)?)?)
            }
            "archive_agent" => {
                let agent = self.session(string("session_id")?)?;
                let owned = self
                    .session_runtime(&agent.id)?
                    .ticket_id
                    .is_some_and(|ticket| self.owned_ticket(id, &ticket).is_ok());
                ensure!(owned, "{} is not an agent of a ticket you own", agent.name);
                Ok(serde_json::to_value(self.archive_agent(&agent.id)?)?)
            }
            "ask_user" => {
                ensure!(
                    session.role == Role::ProjectOrchestrator,
                    "Escalate to your coordinator first"
                );
                ensure_unreserved(string("request_id")?)?;
                let options = match args.get("options") {
                    Some(options) => serde_json::from_value(options.clone())
                        .context("options must be a list of strings")?,
                    None => vec![],
                };
                Ok(serde_json::to_value(self.request_attention(
                    id,
                    "local",
                    string("request_id")?,
                    string("question")?,
                    &options,
                )?)?)
            }
            "close_question" => {
                let request = string("request_id")?;
                let attention = self
                    .db
                    .query_row(
                        "SELECT data FROM attention WHERE session_id=?1 AND operation_id=?2",
                        params![id, request],
                        |r| decode::<Attention>(r, 0),
                    )
                    .optional()?
                    .context("No question with that request_id")?;
                ensure!(
                    !attention.is_permission(),
                    "Permission requests close when the human decides"
                );
                ensure!(
                    !is_check_in(&attention.operation_id),
                    "Check-ins close when the human steps in"
                );
                if attention.answer.is_some() {
                    return Ok(serde_json::to_value(attention)?);
                }
                Ok(serde_json::to_value(self.resolve_attention(
                    &attention.id,
                    &format!("Closed by coordinator: {}", string("resolution")?),
                )?)?)
            }
            _ => bail!("Unknown agent tool"),
        }
    }
    /// The ticket, if the caller is the project coordinator that owns it.
    fn owned_ticket(&self, caller: &str, ticket_id: &str) -> Result<Ticket> {
        let ticket = self.ticket(ticket_id)?;
        ensure!(
            ticket.coordinator_id == caller
                && self.session(caller)?.role == Role::ProjectOrchestrator,
            "Only the ticket's project coordinator manages it"
        );
        Ok(ticket)
    }
    pub(crate) fn append_output(&mut self, session: &Session, run: &str, body: &str) -> Result<()> {
        self.append_reply(session, run, body, None)
    }
    /// `append_output` for a turn's reply, with how it compares with the last full reply.
    pub(crate) fn append_reply(
        &mut self,
        session: &Session,
        run: &str,
        body: &str,
        update: Option<&ReplyUpdate>,
    ) -> Result<()> {
        if body.trim().is_empty() {
            return Ok(());
        }
        let body = body.chars().take(MAX_TEXT_BYTES / 4).collect::<String>();
        let update = update.map(encode).transpose()?;
        self.db.execute("INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at,reply_update) VALUES (?1,?2,?3,?3,?4,'completed',?5,?6) ON CONFLICT(id) DO UPDATE SET body=excluded.body,reply_update=excluded.reply_update",params![reply_id(run),session.project_id,session.id,body,now(),update])?;
        Ok(())
    }
}

/// Refuses arguments that chose a model before the Who runs what table.
fn refuse_model_arguments(args: &Value, removed: &[&str]) -> Result<()> {
    for removed in removed {
        ensure!(
            args.get(removed).is_none(),
            "`{removed}` was removed: Wiffletree picks the model from Models → Who runs what. Pass small: true for small work."
        );
    }
    Ok(())
}
/// The optional `small` flag: whether the coordinator asked for small work.
fn small_work(args: &Value) -> Result<bool> {
    match args.get("small") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(small)) => Ok(*small),
        Some(_) => bail!("small must be true or false"),
    }
}

/// Refuses while a finding is untriaged, and while a verifier of a blocked cycle could not
/// verify or never reported: a waiver covers only findings that were checked and triaged.
pub(crate) fn ensure_findings_settled(
    ticket: &Ticket,
    verification: &Verification,
    before: &str,
) -> Result<()> {
    ensure!(
        verification.outcome != VerificationOutcome::Blocked
            || verification.latest_results().iter().all(|run| !matches!(
                run.result,
                VerifierResult::Blocked | VerifierResult::Pending
            )),
        "A verifier of cycle {} could not verify or never reported; a waiver covers only checked findings",
        verification.cycle
    );
    let untriaged: Vec<&str> = (ticket.ledger.iter())
        .filter(|e| e.status == EntryStatus::Untriaged)
        .map(|e| e.id.as_str())
        .collect();
    ensure!(
        untriaged.is_empty(),
        "Triage {} with triage_findings before {before}",
        untriaged.join(", ")
    );
    Ok(())
}

/// `Host::check_verified_head` for a caller that has the ticket's base without the store,
/// as a publish step has on its own thread. With `waived`, which `Host::unreviewed_waiver`
/// allows only at the full-review cap, HEAD may be past the verified commit when the waiver
/// names every commit since; those commits are returned. So are the commits a waiver recorded
/// on the ticket, when HEAD has their changes, as after the PR keeper's rebase.
pub(crate) fn verified_head(
    ticket: &Ticket,
    verified: &str,
    base: impl Fn() -> Result<stacks::Base>,
    waived: Option<&str>,
) -> Result<Vec<String>> {
    // Whatever their patch id: `git patch-id` ignores whitespace, which can change behaviour.
    if let Some(unreviewed) = ticket.unreviewed() {
        bail!(
            "The branch has commits no review has checked: the PR keeper took GitHub's branch at {} into it. Start a review with verify_ticket; nothing is pushed or accepted until one ends.",
            unreviewed.head
        );
    }
    let head = verification::head(ticket)?;
    if head == verified {
        return Ok(vec![]);
    }
    let worktree = Path::new(&ticket.worktree);
    let same_patch = |commit: &str| base()?.same_patch(worktree, commit, &head);
    if let Some(waived) = ticket.waived_commits.last()
        && same_patch(waived).is_ok_and(|same| same)
    {
        return Ok(ticket.waived_commits.clone());
    }
    let differs = match same_patch(verified) {
        Ok(true) => return Ok(vec![]),
        Ok(false) => format!(
            "The worktree's changes differ from the verified commit: verified {verified}, current {head}"
        ),
        Err(error) => format!(
            "The worktree moved past the verified commit: verified {verified}, current {head}, and their patches could not be compared ({error:#})"
        ),
    };
    let unreviewed = match waived {
        Some(_) => unreviewed_commits(worktree, verified, &head)?,
        None => vec![],
    };
    let (Some(waiver), false) = (waived, unreviewed.is_empty()) else {
        bail!("{differs}; verify again");
    };
    let named: Vec<String> = (waiver.split(|c: char| !c.is_ascii_hexdigit()))
        .filter(|word| word.len() >= 7)
        .map(str::to_ascii_lowercase)
        .collect();
    let missing: Vec<&str> = (unreviewed.iter())
        .filter(|commit| !named.iter().any(|name| commit.starts_with(name.as_str())))
        .map(|commit| short_sha(commit))
        .collect();
    ensure!(
        missing.is_empty(),
        "The waiver does not name every commit no review checked. Read {}, then name each by its short SHA of at least 7 characters",
        missing.join(", ")
    );
    Ok(unreviewed)
}

/// The commits on HEAD past the verified one, oldest first.
fn unreviewed_commits(worktree: &Path, verified: &str, head: &str) -> Result<Vec<String>> {
    let range = format!("{verified}..{head}");
    let listed = runtime::git_answer(worktree, &["rev-list", "--reverse", &range])?;
    ensure!(
        listed.status.success(),
        "The commits past the verified one could not be listed: {}",
        listed.output.trim()
    );
    Ok(listed.output.lines().map(Into::into).collect())
}
