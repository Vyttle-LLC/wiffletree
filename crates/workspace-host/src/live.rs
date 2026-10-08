//! Durable project teams, ticket workspaces and scoped agent operations.
use crate::*;

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
    pub(crate) fn save_runtime(&self, runtime: &SessionRuntime) -> Result<()> {
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
    pub fn create_ticket(&mut self, owner: &str, title: &str, brief: &str) -> Result<Ticket> {
        text(title, 128)?;
        text(brief, MAX_TEXT_BYTES)?;
        let coordinator = self.session(owner)?;
        ensure!(
            coordinator.role == Role::TaskOrchestrator,
            "Tickets belong to a repository coordinator"
        );
        ensure!(
            !coordinator.archived,
            "{} is archived; restore it before adding tickets",
            coordinator.name
        );
        if let Some(ticket) = self
            .tickets()?
            .into_iter()
            .find(|t| t.coordinator_id == owner && t.title == title)
        {
            ensure!(
                ticket.brief == brief,
                "Ticket title already exists with a different brief"
            );
            return Ok(ticket);
        }
        let repo = self.repository(
            coordinator
                .repository_id
                .as_deref()
                .context("Missing repository")?,
        )?;
        let project = self.project(&coordinator.project_id)?;
        let (path, branch) = self.new_ticket_place(&project, &repo, title)?;
        worktrees::ensure_worktree(Path::new(&repo.path), &path, &branch, Some(&repo.base))?;
        let ticket = Ticket {
            id: new_id(),
            coordinator_id: owner.into(),
            title: title.into(),
            brief: brief.into(),
            worktree: path.to_string_lossy().into(),
            branch,
            state: "planned".into(),
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
        if let Some(focus) = focus {
            text(focus, 60)?;
        }
        ensure!(
            role.is_worker(),
            "Assign an implementer, tester or reviewer"
        );
        let mut ticket = self.ticket(ticket_id)?;
        ensure!(ticket.is_open(), "Ticket is already {}", ticket.state);
        let owner = self.session(&ticket.coordinator_id)?;
        // A role session is never recycled across tickets. Retrying the same assignment is
        // idempotent; a different focus adds another agent in that role, such as a second reviewer.
        if let Some(existing) = self.runtimes()?.into_iter().find(|r| {
            r.ticket_id.as_deref() == Some(ticket_id)
                && r.focus.as_deref() == focus
                && self.session(&r.session_id).is_ok_and(|s| s.role == role)
        }) {
            let session = self.session(&existing.session_id)?;
            ensure!(
                session.provider == provider,
                "Existing ticket agent uses another provider"
            );
            return Ok(session);
        }
        let session = self.create_session(
            &owner.project_id,
            &owner.id,
            None,
            &format!(
                "{} · {}",
                role.agent_label(focus),
                ticket.title.chars().take(80).collect::<String>()
            ),
            role,
            provider,
        )?;
        let runtime = SessionRuntime {
            session_id: session.id.clone(),
            ticket_id: Some(ticket.id.clone()),
            focus: focus.map(Into::into),
            workdir: Some(ticket.worktree.clone()),
            ..Default::default()
        };
        self.save_runtime(&runtime)?;
        self.send(
            &format!("assignment:{}", session.id),
            Some(&owner.id),
            &session.id,
            &format!(
                "Ticket: {}\n{}\n\nAssignment: {}\n\nWorktree: {}\nBranch: {}",
                ticket.title, ticket.brief, instruction, ticket.worktree, ticket.branch
            ),
        )?;
        ticket.state = "assigned".into();
        self.save_ticket(&ticket)?;
        Ok(session)
    }
    /// Archives a finished ticket's agents and removes its worktree. Their conversations and the
    /// branch are kept. Refuses while an agent is working or the worktree holds unsaved work.
    pub fn close_ticket(&mut self, ticket_id: &str) -> Result<Ticket> {
        let mut ticket = self.ticket(ticket_id)?;
        if ticket.state == "closed" {
            return Ok(ticket);
        }
        self.remove_worktrees(std::slice::from_ref(&ticket))?;
        let agents: Vec<Session> = self
            .runtimes()?
            .into_iter()
            .filter(|r| r.ticket_id.as_deref() == Some(ticket_id))
            .map(|r| self.session(&r.session_id))
            .collect::<Result<_>>()?;
        for agent in &agents {
            self.set_archived(&agent.id, true)?;
        }
        ticket.state = "closed".into();
        self.save_ticket(&ticket)?;
        let coordinator = self.session(&ticket.coordinator_id)?;
        Self::event(
            &self.db,
            &coordinator.project_id,
            Some(&coordinator.id),
            "ticket_closed",
            &ticket.id,
        )?;
        Ok(ticket)
    }
    /// Archives a main coordinator's finished repository team: its ticket worktrees are removed,
    /// its accepted tickets are closed and the coordinator's whole tree is archived. Retrying an
    /// archived team is harmless. Refuses while any team member is working, any ticket is still
    /// open or any worktree holds unsaved work, naming every blocker.
    pub fn archive_team(&mut self, main: &str, coordinator_id: &str) -> Result<Vec<Session>> {
        ensure!(
            self.session(main)?.role == Role::ProjectOrchestrator,
            "Only the main coordinator archives repository teams"
        );
        let coordinator = self.session(coordinator_id)?;
        ensure!(
            coordinator.role == Role::TaskOrchestrator
                && coordinator.parent_id.as_deref() == Some(main),
            "Archive only your own repository coordinators"
        );
        let tickets: Vec<Ticket> = self
            .tickets()?
            .into_iter()
            .filter(|t| t.coordinator_id == coordinator.id)
            .collect();
        let blockers: Vec<String> = self
            .session_tree(&coordinator.id)?
            .iter()
            .filter(|s| s.status == Status::Working)
            .map(|s| format!("{} is working", s.name))
            .chain(
                tickets
                    .iter()
                    .filter(|t| t.is_open())
                    .map(|t| format!("ticket \"{}\" ({}) is {}", t.title, t.id, t.state)),
            )
            .collect();
        ensure!(
            blockers.is_empty(),
            "{} still has outstanding work: {}",
            coordinator.name,
            blockers.join("; ")
        );
        self.remove_worktrees(&tickets)?;
        for ticket in tickets.iter().filter(|t| t.state == "accepted") {
            self.close_ticket(&ticket.id)?;
        }
        self.change_archived(&coordinator.id, true, Some("team_archived"))
    }
    pub fn agent_context(&self, id: &str) -> Result<Value> {
        let session = self.session(id)?;
        let mut context = json!({"self":session,"project":self.project(&session.project_id)?,
            "repositories":self.project_repositories(&session.project_id)?,
            "team":self.sessions()?.into_iter().filter(|s|s.project_id==session.project_id).collect::<Vec<_>>(),
            "tickets":self.tickets()?.into_iter().filter(|t|self.session(&t.coordinator_id).is_ok_and(|s|s.project_id==session.project_id)).collect::<Vec<_>>(),
            "runtime":self.session_runtime(id)?, "policies":self.policies()?,
            "memory":self.logs(&session.project_id,None,10)?,
            "open_questions":self.open_questions(&session)?});
        if session.role == Role::ProjectOrchestrator {
            context["teams"] = json!(self.teams(id)?);
        }
        Ok(context)
    }
    /// The main coordinator's view of each repository team and the state of its tickets.
    fn teams(&self, main: &str) -> Result<Vec<Value>> {
        let tickets = self.tickets()?;
        Ok(self
            .sessions()?
            .into_iter()
            .filter(|s| s.role == Role::TaskOrchestrator && s.parent_id.as_deref() == Some(main))
            .map(|s| {
                json!({"id":s.id,"name":s.name,"status":s.status,"archived":s.archived,
                "tickets":tickets.iter().filter(|t|t.coordinator_id==s.id)
                    .map(|t|json!({"id":t.id,"title":t.title,"state":t.state})).collect::<Vec<_>>()})
            })
            .collect())
    }
    /// Questions this session placed in the inbox that the human has not yet settled.
    pub fn open_questions(&self, session: &Session) -> Result<Vec<Attention>> {
        Ok(self
            .open_attention(&session.project_id)?
            .into_iter()
            .filter(|a| a.session_id == session.id && !a.is_permission())
            .collect())
    }
    fn assignment_route(&self, role: Role, args: &Value) -> Result<Route> {
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
            "send_message" => {
                let recipient = self.session(string("recipient")?)?;
                ensure!(
                    session.parent_id.as_deref() == Some(&recipient.id)
                        || recipient.parent_id.as_deref() == Some(id),
                    "Messages must follow coordinator ownership"
                );
                Ok(serde_json::to_value(self.send(
                    &format!("agent:{id}:{}", string("message_id")?),
                    Some(id),
                    &recipient.id,
                    string("body")?,
                )?)?)
            }
            "create_repo_coordinator" => {
                ensure!(
                    session.role == Role::ProjectOrchestrator,
                    "Only the main coordinator creates repository teams"
                );
                let repo = self.repository(string("repository_id")?)?;
                ensure!(
                    self.project(&session.project_id)?.uses(&repo.id),
                    "This project does not use that repository"
                );
                if let Some(existing) = self.sessions()?.into_iter().find(|s| {
                    s.parent_id.as_deref() == Some(id)
                        && s.repository_id.as_deref() == Some(&repo.id)
                        && s.role == Role::TaskOrchestrator
                }) {
                    return Ok(serde_json::to_value(existing)?);
                }
                let route = self.assignment_route(Role::TaskOrchestrator, &args)?;
                let coordinator = self.create_session(
                    &session.project_id,
                    id,
                    Some(&repo.id),
                    &repo.name,
                    Role::TaskOrchestrator,
                    route.profile.provider,
                )?;
                let mut runtime = self.session_runtime(&coordinator.id)?;
                runtime.profile = Some(route.profile);
                self.save_runtime(&runtime)?;
                Ok(serde_json::to_value(coordinator)?)
            }
            "create_ticket" => {
                ensure!(
                    session.role == Role::TaskOrchestrator,
                    "Only repository coordinators create tickets"
                );
                Ok(serde_json::to_value(self.create_ticket(
                    id,
                    string("title")?,
                    string("brief")?,
                )?)?)
            }
            "assign_ticket" => {
                let ticket = self.ticket(string("ticket_id")?)?;
                ensure!(
                    ticket.coordinator_id == id,
                    "Ticket belongs to another coordinator"
                );
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
            "report" => {
                let kind = string("kind")?;
                ensure!(
                    [
                        "progress",
                        "blocked",
                        "ready_for_testing",
                        "passed",
                        "failed",
                        "completed"
                    ]
                    .contains(&kind),
                    "Unknown report kind"
                );
                ensure!(
                    kind != "passed" || matches!(session.role, Role::Tester | Role::Reviewer),
                    "Only verification agents report passed"
                );
                let body = string("body")?;
                if kind == "progress" {
                    Self::event(
                        &self.db,
                        &session.project_id,
                        Some(id),
                        "agent_progress",
                        body,
                    )?;
                    return Ok(json!({"recorded":true,"woke_coordinator":false}));
                }
                let parent = session
                    .parent_id
                    .as_deref()
                    .context("Main coordinator communicates with the human directly")?;
                let report_id = format!("report:{id}:{}", string("message_id")?);
                if let Ok(existing) = self.message(&report_id) {
                    ensure!(
                        existing.body == format!("[{kind}] {}\n{body}", session.name),
                        "Report ID reused with different content"
                    );
                    return Ok(serde_json::to_value(existing)?);
                }
                let message = self.send(
                    &report_id,
                    Some(id),
                    parent,
                    &format!("[{kind}] {}\n{body}", session.name),
                )?;
                if let Some(ticket_id) = self.session_runtime(id)?.ticket_id {
                    let mut ticket = self.ticket(&ticket_id)?;
                    if kind != "progress" && ticket.is_open() {
                        ticket.state = kind.into();
                        self.save_ticket(&ticket)?;
                    }
                }
                self.set_status(
                    id,
                    match kind {
                        "blocked" | "failed" => Status::Blocked,
                        "progress" => Status::Working,
                        _ => Status::Done,
                    },
                )?;
                Ok(serde_json::to_value(message)?)
            }
            "accept_ticket" => {
                let mut ticket = self.ticket(string("ticket_id")?)?;
                ensure!(
                    ticket.coordinator_id == id,
                    "Ticket belongs to another coordinator"
                );
                if ticket.state == "accepted" {
                    return Ok(serde_json::to_value(ticket)?);
                }
                ensure!(
                    ticket.state == "passed",
                    "Independent verification must pass before acceptance"
                );
                self.remove_worktrees(std::slice::from_ref(&ticket))?;
                ticket.state = "accepted".into();
                self.save_ticket(&ticket)?;
                Ok(serde_json::to_value(ticket)?)
            }
            "close_ticket" => {
                let ticket = self.ticket(string("ticket_id")?)?;
                ensure!(
                    ticket.coordinator_id == id,
                    "Ticket belongs to another coordinator"
                );
                Ok(serde_json::to_value(self.close_ticket(&ticket.id)?)?)
            }
            "archive_team" => Ok(serde_json::to_value(
                self.archive_team(id, string("session_id")?)?,
            )?),
            "ask_user" => {
                ensure!(
                    session.role == Role::ProjectOrchestrator,
                    "Escalate to your coordinator first"
                );
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
    pub(crate) fn append_output(&mut self, session: &Session, run: &str, body: &str) -> Result<()> {
        if body.trim().is_empty() {
            return Ok(());
        }
        let body = body.chars().take(MAX_TEXT_BYTES / 4).collect::<String>();
        self.db.execute("INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?3,?4,'completed',?5) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![format!("output:{run}"),session.project_id,session.id,body,now()])?;
        Ok(())
    }
}
