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
        self.db.execute(
            "UPDATE sessions SET data=?2 WHERE id=?1",
            params![id, encode(&session)?],
        )?;
        self.save_runtime(&runtime)
    }
    pub fn reconcile_session(&mut self, id: &str, retry: bool) -> Result<()> {
        ensure!(
            self.session(id)?.status != Status::Working,
            "Stop the active turn first"
        );
        self.db.execute(
            "UPDATE messages SET receipt=?2 WHERE recipient=?1 AND receipt='held'",
            params![id, if retry { "queued" } else { "cancelled" }],
        )?;
        let mut runtime = self.session_runtime(id)?;
        runtime.last_error = None;
        self.save_runtime(&runtime)?;
        self.set_status(id, Status::Ready)
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
        text(title, 128)?;
        text(brief, MAX_TEXT_BYTES)?;
        let coordinator = self.session(owner)?;
        ensure!(
            coordinator.role == Role::ProjectOrchestrator,
            "Tickets belong to the project coordinator"
        );
        ensure!(
            !coordinator.archived,
            "{} is archived; restore it before adding tickets",
            coordinator.name
        );
        let repo = self.repository(repository_id)?;
        let project = self.project(&coordinator.project_id)?;
        ensure!(
            project.uses(&repo.id),
            "Project {} does not use repository {}",
            project.name,
            repo.name
        );
        if let Some(ticket) = self
            .tickets()?
            .into_iter()
            .find(|t| t.coordinator_id == owner && t.repository_id == repo.id && t.title == title)
        {
            ensure!(
                ticket.brief == brief,
                "Ticket title already exists with a different brief"
            );
            return Ok(ticket);
        }
        let (path, branch) = self.new_ticket_place(&project, &repo, title)?;
        worktrees::ensure_worktree(Path::new(&repo.path), &path, &branch, Some(&repo.base))?;
        let ticket = Ticket {
            id: new_id(),
            coordinator_id: owner.into(),
            repository_id: repo.id.clone(),
            title: title.into(),
            brief: brief.into(),
            worktree: path.to_string_lossy().into(),
            branch,
            state: "planned".into(),
            verification: None,
        };
        self.db.execute(
            "INSERT INTO tickets VALUES (?1,?2,?3)",
            params![ticket.id, owner, encode(&ticket)?],
        )?;
        Self::event(
            &self.db,
            &coordinator.project_id,
            Some(owner),
            "ticket_created",
            &ticket.id,
        )?;
        Ok(ticket)
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
            ticket.state = "assigned".into();
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
        if let Some(existing) = self.runtimes()?.into_iter().find(|r| {
            r.ticket_id.as_deref() == Some(&ticket.id)
                && r.focus.as_deref() == focus
                && self.session(&r.session_id).is_ok_and(|s| s.role == role)
        }) {
            let session = self.session(&existing.session_id)?;
            ensure!(
                session.provider == provider,
                "Existing ticket agent uses another provider"
            );
            return Ok((session, false));
        }
        let session = self.create_session(
            &owner.project_id,
            &owner.id,
            Some(&ticket.repository_id)
                .filter(|r| !r.is_empty())
                .map(String::as_str),
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
    /// Archives a finished ticket's agents and removes its worktree. Their conversations and the
    /// branch are kept. Refuses while an agent is working or the worktree holds unsaved work.
    pub fn close_ticket(&mut self, ticket_id: &str) -> Result<Ticket> {
        let ticket = self.ticket(ticket_id)?;
        if ticket.state == "closed" {
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
        self.finish_ticket(ticket, "closed", "ticket_closed")
    }
    /// Accepts a ticket whose last verification cycle passed at the commit still checked out.
    /// Like closing, it archives the agents and removes the worktree, keeping the branch.
    pub fn accept_ticket(&mut self, ticket_id: &str) -> Result<Ticket> {
        let ticket = self.ticket(ticket_id)?;
        if ticket.state == "accepted" {
            return Ok(ticket);
        }
        ensure!(
            ticket.state == "passed",
            "Ticket is {}; independent verification must pass before acceptance",
            ticket.state
        );
        let verified = ticket
            .verification
            .as_ref()
            .filter(|v| v.outcome == VerificationOutcome::Passed)
            .and_then(|v| v.verified_commit())
            .context(
                "Start verification with verify_ticket; acceptance needs the commit it verified",
            )?
            .to_owned();
        let head = verification::head(&ticket)?;
        ensure!(
            head == verified,
            "The worktree moved past the verified commit: verified {verified}, current {head}; verify again"
        );
        self.finish_ticket(ticket, "accepted", "ticket_accepted")
    }
    fn finish_ticket(
        &mut self,
        mut ticket: Ticket,
        state: &str,
        milestone: &str,
    ) -> Result<Ticket> {
        self.with_worktrees_removed(&[ticket.clone()], |host| {
            for agent in host.ticket_agents(&ticket.id)? {
                host.set_archived(&agent.id, true)?;
            }
            ticket.state = state.into();
            host.save_ticket(&ticket)?;
            let coordinator = host.session(&ticket.coordinator_id)?;
            Self::event(
                &host.db,
                &coordinator.project_id,
                Some(&coordinator.id),
                milestone,
                &ticket.id,
            )
        })?;
        Ok(ticket)
    }
    pub fn agent_context(&self, id: &str) -> Result<Value> {
        let session = self.session(id)?;
        let mut context = json!({"self":session,"project":self.project(&session.project_id)?,
            "repositories":self.project_repositories(&session.project_id)?,
            "team":self.sessions()?.into_iter().filter(|s|s.project_id==session.project_id).collect::<Vec<_>>(),
            "tickets":self.ticket_overview(&session.project_id)?,
            "runtime":self.session_runtime(id)?, "policies":self.policies()?,
            "memory":self.logs(&session.project_id,None,10)?,
            "open_questions":self.open_questions(&session)?});
        let child_reports = self.child_reports(id)?;
        if !child_reports.is_empty() {
            context["child_reports"] = Value::Array(child_reports);
        }
        Ok(context)
    }
    /// The project's tickets, each with its repository name, agents and verification record.
    fn ticket_overview(&self, project: &str) -> Result<Vec<Value>> {
        let repositories = self.repositories()?;
        let runtimes = self.runtimes()?;
        let mut overview = vec![];
        for ticket in self.tickets()? {
            if !self
                .session(&ticket.coordinator_id)
                .is_ok_and(|s| s.project_id == project)
            {
                continue;
            }
            let mut agents = vec![];
            for runtime in runtimes
                .iter()
                .filter(|r| r.ticket_id.as_ref() == Some(&ticket.id))
            {
                let s = self.session(&runtime.session_id)?;
                agents.push(json!({"id":s.id,"name":s.name,"role":s.role,"focus":runtime.focus,"status":s.status,"archived":s.archived}));
            }
            let mut entry = serde_json::to_value(&ticket)?;
            entry["repository"] = json!(
                repositories
                    .iter()
                    .find(|r| r.id == ticket.repository_id)
                    .map(|r| r.name.as_str())
            );
            entry["agents"] = json!(agents);
            overview.push(entry);
        }
        Ok(overview)
    }
    /// Recent reports each direct child sent this session, newest first. Children
    /// that never reported are omitted, so a session without children gets nothing.
    fn child_reports(&self, id: &str) -> Result<Vec<Value>> {
        let mut children = Vec::new();
        for child in self
            .sessions()?
            .into_iter()
            .filter(|s| s.parent_id.as_deref() == Some(id))
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
            .filter(|a| a.session_id == session.id && !a.is_permission())
            .collect())
    }
    pub(crate) fn assignment_route(&self, role: Role, args: &Value) -> Result<Route> {
        let policy = self.policy(role)?;
        let provider = args
            .get("provider")
            .map(|p| serde_json::from_value(p.clone()))
            .transpose()?
            .unwrap_or(policy.default.provider);
        let complexity = match args.get("size") {
            Some(size) => match size.as_str() {
                Some("small") => Complexity::Small,
                Some("big") => Complexity::Complex,
                _ => bail!("Choose the configured big or small profile"),
            },
            None => args
                .get("complexity")
                .map(|c| serde_json::from_value(c.clone()))
                .transpose()?
                .unwrap_or(Complexity::Standard),
        };
        let proposal = args
            .get("profile")
            .map(|p| serde_json::from_value::<ModelProfile>(p.clone()))
            .transpose()?;
        policy.select(complexity, proposal.as_ref(), Some(provider), None)
    }
    pub fn agent_tool(&mut self, id: &str, name: &str, args: Value) -> Result<Value> {
        let session = self.session(id)?;
        let string = |key: &str| -> Result<&str> {
            args[key].as_str().with_context(|| format!("Missing {key}"))
        };
        match name {
            "workspace_context" => self.agent_context(id),
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
                let message = self.send(
                    &format!("agent:{id}:{}", string("message_id")?),
                    Some(id),
                    &recipient.id,
                    string("body")?,
                )?;
                let mut receipt = serde_json::to_value(&message)?;
                // Tool result only: the stored receipt stays `queued` until the next turn starts.
                let recipient = self.session(&recipient.id)?;
                if message.receipt == Receipt::Queued && recipient.status == Status::Working {
                    receipt["receipt"] = json!("queued_behind_turn");
                    receipt["recipient_turn_started_at"] =
                        json!(self.session_runtime(&recipient.id)?.last_started_at);
                }
                Ok(receipt)
            }
            "create_ticket" => {
                ensure!(
                    session.role == Role::ProjectOrchestrator,
                    "Only the project coordinator creates tickets"
                );
                Ok(serde_json::to_value(self.create_ticket(
                    id,
                    string("repository_id")?,
                    string("title")?,
                    string("brief")?,
                )?)?)
            }
            "assign_ticket" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                let role: Role = serde_json::from_value(args["role"].clone())?;
                let route = self.assignment_route(role, &args)?;
                let provider = route.profile.provider;
                let worker = self.assign_ticket(
                    &ticket.id,
                    role,
                    provider,
                    string("instruction")?,
                    args["focus"].as_str(),
                )?;
                let mut runtime = self.session_runtime(&worker.id)?;
                if runtime.profile.is_none() {
                    runtime.profile = Some(route.profile);
                    self.save_runtime(&runtime)?;
                }
                Self::event(
                    &self.db,
                    &session.project_id,
                    Some(&worker.id),
                    "model_routed",
                    &route.reason,
                )?;
                Ok(serde_json::to_value(worker)?)
            }
            "report" => self.report(
                &session,
                string("kind")?,
                string("body")?,
                string("message_id")?,
            ),
            "verify_ticket" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                Ok(serde_json::to_value(self.verify_ticket(&ticket.id)?)?)
            }
            "accept_ticket" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                Ok(serde_json::to_value(self.accept_ticket(&ticket.id)?)?)
            }
            "close_ticket" => {
                let ticket = self.owned_ticket(id, string("ticket_id")?)?;
                Ok(serde_json::to_value(self.close_ticket(&ticket.id)?)?)
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
        if body.trim().is_empty() {
            return Ok(());
        }
        let body = body.chars().take(MAX_TEXT_BYTES / 4).collect::<String>();
        self.db.execute("INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?3,?4,'completed',?5) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![format!("output:{run}"),session.project_id,session.id,body,now()])?;
        Ok(())
    }
}
