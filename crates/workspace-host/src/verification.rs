//! Ticket verification. One call starts every configured verifier on the ticket's current
//! commit; the scheduler runs a round's verifiers together. Verifiers are read-only. Failures go
//! back to the implementer, only failed verifiers re-run, rounds are capped, and the project
//! coordinator wakes once, when the cycle ends. Verifiers are archived once the cycle no longer
//! needs them, so every cycle starts fresh sessions.
use crate::service::{HOST_WORKER_TURNS, worker_turns};
use crate::*;

const REPORT_KINDS: [&str; 6] = [
    "progress",
    "blocked",
    "ready_for_testing",
    "passed",
    "failed",
    "completed",
];
const WORKTREE_CHANGED: &str = "worktree changed during verification";

/// How a report relates to the ticket's verification cycle.
enum Routing {
    /// No cycle is running: a non-progress report sets the ticket's state to its kind.
    Plain,
    /// A cycle is running but does not route this report; it wakes the coordinator as usual.
    Unrouted,
    /// A verifier's result for the round input it last took, recorded even after the cycle ended.
    Verdict,
    /// Any other verdict from one of the cycle's verifiers, such as one answering an earlier
    /// cycle's input: stored quietly, changing neither the ticket nor the cycle.
    Late,
    /// The implementer is ready again after a failed round.
    Ready,
    /// Any other implementer report ends the cycle as blocked.
    Ends,
}

/// `answered` is the round input the reporter last took: its latest consumed `verify:` message.
fn routing(ticket: &Ticket, session: &Session, kind: &str, answered: Option<&str>) -> Routing {
    let Some(verification) = &ticket.verification else {
        return Routing::Plain;
    };
    // A session that ever took round input is one of the ticket's verifiers, even after the
    // settings dropped it and a later cycle's record no longer lists it.
    let verifier = answered.is_some()
        || verification
            .rounds
            .iter()
            .flat_map(|r| &r.verifiers)
            .any(|v| v.session_id == session.id);
    if verifier && matches!(kind, "passed" | "failed" | "blocked") {
        // Sessions are reused across rounds, and a restored archived verifier can reappear in a
        // later cycle, so a verdict counts only for the input it answers.
        let current = verification.current_round().is_some_and(|round| {
            round.verifiers.iter().any(|v| {
                v.session_id == session.id
                    && v.result == VerifierResult::Pending
                    && Some(v.message_id.as_str()) == answered
            })
        });
        return if current {
            Routing::Verdict
        } else {
            Routing::Late
        };
    }
    if verification.outcome != VerificationOutcome::Running {
        return Routing::Plain;
    }
    match (session.role, kind) {
        (Role::Implementer, "ready_for_testing") => Routing::Ready,
        (Role::Implementer, _) => Routing::Ends,
        _ => Routing::Unrouted,
    }
}

/// The ticket worktree's checked-out commit.
pub(crate) fn head(ticket: &Ticket) -> Result<String> {
    worktrees::head(Path::new(&ticket.worktree))
        .with_context(|| format!("Read HEAD of \"{}\" ({})", ticket.title, ticket.id))
}

fn is_clean(ticket: &Ticket) -> Result<bool> {
    worktrees::is_clean(Path::new(&ticket.worktree))
        .with_context(|| format!("Check \"{}\" ({})", ticket.title, ticket.id))
}

/// Every verifier session of the ticket's latest cycle.
fn cycle_verifiers(ticket: &Ticket) -> Vec<String> {
    ticket.verification.as_ref().map_or(vec![], |v| {
        v.latest_results()
            .into_iter()
            .map(|run| run.session_id.clone())
            .collect()
    })
}

fn verifier_label(run: &VerifierRun) -> String {
    run.role.agent_label(Some(&run.focus))
}

impl Host {
    /// Starts a cycle: pins HEAD and sends every configured verifier one message naming it.
    /// `choices` gives each verifier, by focus, the coordinator's exact model and reason; a new
    /// verifier is pinned to it. An existing one is reused only at the same profile; otherwise it is archived and a fresh one takes the choice, so no pinned model ever changes.
    pub fn verify_ticket(&mut self, ticket_id: &str, choices: &[VerifierChoice]) -> Result<Ticket> {
        let mut ticket = self.ticket(ticket_id)?;
        if let Some(cycle) = ticket.running_cycle() {
            bail!("Verification cycle {} is already running", cycle.cycle);
        }
        // A finished ticket that was never verified, for example a migrated one, has no cycle to
        // accept against yet.
        let finished_unverified =
            ticket.verification.is_none() && matches!(ticket.state.as_str(), "passed" | "failed");
        ensure!(
            ticket.state == "ready_for_testing" || finished_unverified,
            "Ticket is {}; verify it once its implementer reports ready_for_testing",
            ticket.state
        );
        self.prepare_ticket_worktree(&ticket.id)?;
        ensure!(
            is_clean(&ticket)?,
            "The worktree has uncommitted or untracked files; ask the implementer to commit or discard them"
        );
        let settings = self.settings().verification;
        let coordinator = self.session(&ticket.coordinator_id)?;
        let project = self.project(&coordinator.project_id)?;
        let count = settings.verifiers.len();
        let worker_turns = worker_turns(&project);
        ensure!(
            count <= worker_turns,
            "{count} verifiers cannot run at once: the project's turn limit of {} leaves {worker_turns} worker turns",
            project.turn_limit
        );
        ensure!(
            count <= HOST_WORKER_TURNS,
            "{count} verifiers cannot run at once: the host-wide limit is {HOST_WORKER_TURNS} worker turns"
        );
        let chosen = self.verifier_choices(&coordinator, &settings.verifiers, choices)?;
        let commit = head(&ticket)?;
        let cycle = ticket.verification.as_ref().map_or(1, |v| v.cycle + 1);
        self.atomically(|host| {
            let mut verifiers = vec![];
            for (config, (profile, reason)) in settings.verifiers.iter().zip(chosen) {
                let existing =
                    host.existing_assignment(&ticket.id, config.role, Some(&config.focus))?;
                // A verifier keeps its model: one pinned to another profile is retired, and a
                // fresh session takes the coordinator's choice, as in a first cycle.
                let reused = match existing {
                    Some(agent)
                        if host.session_runtime(&agent.id)?.profile.as_ref() == Some(&profile) =>
                    {
                        Some(agent)
                    }
                    Some(retired) => {
                        host.change_archived(&retired.id, true, Some("verifier_replaced"))?;
                        None
                    }
                    None => None,
                };
                let agent = match reused {
                    Some(agent) => {
                        // Never start a round that a verifier could only run on a substitute.
                        if let Some(reason) = host.hold_reason(&agent)? {
                            bail!("{}: {reason}", agent.name);
                        }
                        agent
                    }
                    None => {
                        let (agent, _) = host.ticket_agent(
                            &ticket,
                            config.role,
                            profile.provider,
                            Some(&config.focus),
                        )?;
                        host.record_choice(&coordinator, &agent, profile, &reason)?;
                        agent
                    }
                };
                verifiers.push(VerifierRun {
                    session_id: agent.id.clone(),
                    role: config.role,
                    focus: config.focus.clone(),
                    message_id: format!("verify:{}:{cycle}:1:{}", ticket.id, agent.id),
                    result: VerifierResult::Pending,
                    report_id: None,
                    reason: None,
                });
            }
            ticket.verification = Some(Verification {
                cycle,
                max_rounds: settings.max_rounds,
                outcome: VerificationOutcome::Running,
                rounds: vec![VerificationRound {
                    round: 1,
                    commit,
                    verifiers,
                }],
            });
            let instructions = settings
                .verifiers
                .iter()
                .map(|v| (v.focus.as_str(), v.instruction.as_deref()))
                .collect();
            host.start_round(&mut ticket, &instructions)?;
            Self::event(
                &host.db,
                &project.id,
                Some(&ticket.coordinator_id),
                "verification_started",
                &format!("{}; cycle {cycle}", ticket.id),
            )?;
            Ok(ticket)
        })
    }
    /// Pairs every configured verifier with the coordinator's choice for it, checking each
    /// against the machine's model selection and the verifier's provider. Nothing is substituted:
    /// one refusal starts no verifier.
    fn verifier_choices(
        &self,
        coordinator: &Session,
        verifiers: &[VerifierConfig],
        choices: &[VerifierChoice],
    ) -> Result<Vec<(ModelProfile, String)>> {
        let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
        if let Some(unknown) = choices
            .iter()
            .find(|c| !verifiers.iter().any(|v| same(&v.focus, &c.focus)))
        {
            bail!(
                "No verifier has the focus \"{}\"; choose one model for each verifier in model_selection",
                unknown.focus
            );
        }
        let selection = self.model_selection()?;
        let mut chosen = vec![];
        for verifier in verifiers {
            let label = verifier.role.agent_label(Some(&verifier.focus));
            let mut matching = choices.iter().filter(|c| same(&c.focus, &verifier.focus));
            let choice = matching
                .next()
                .with_context(|| format!("Choose a profile and reason for {label}"))?;
            ensure!(matching.next().is_none(), "Choose {label}'s model once");
            let reason = Selection::check_reason(&choice.reason)?.to_owned();
            if let Err(rejection) = selection.permits_verifier(verifier, &choice.profile) {
                Self::event(
                    &self.db,
                    &coordinator.project_id,
                    Some(&coordinator.id),
                    "model_rejected",
                    &format!(
                        "{} asked for {} ({label}). {rejection} No verifier was started.",
                        coordinator.name, choice.profile
                    ),
                )?;
                bail!("{rejection} No verifier was started.");
            }
            chosen.push((choice.profile.clone(), reason));
        }
        Ok(chosen)
    }
    /// Sends the current round's messages and marks the ticket `verifying`. `instructions` maps
    /// a focus to its configured instruction; later rounds have none.
    fn start_round(
        &mut self,
        ticket: &mut Ticket,
        instructions: &std::collections::HashMap<&str, Option<&str>>,
    ) -> Result<()> {
        ticket.state = "verifying".into();
        self.save_ticket(ticket)?;
        let verification = ticket.verification.as_ref().context("No verification")?;
        let round = verification.current_round().context("No round")?;
        for run in &round.verifiers {
            let instruction = instructions
                .get(run.focus.as_str())
                .copied()
                .flatten()
                .map_or(String::new(), |i| format!("Instruction: {i}\n"));
            let again = if round.round > 1 {
                "The implementer committed fixes for your earlier failure; check again.\n\n"
            } else {
                ""
            };
            self.send(
                &run.message_id,
                Some(&ticket.coordinator_id),
                &run.session_id,
                &format!(
                    "{again}Verify ticket \"{}\" at commit {}: round {} of at most {}.\n\nTicket brief:\n{}\n\nFocus: {}\n{instruction}Worktree: {}\nBranch: {}\n\nYou are read-only: do not edit, stage, commit or switch branches in the worktree. Check commit {}, then report passed, failed or blocked with evidence.",
                    ticket.title,
                    round.commit,
                    round.round,
                    verification.max_rounds,
                    ticket.brief,
                    run.focus,
                    ticket.worktree,
                    ticket.branch,
                    round.commit
                ),
            )?;
        }
        Ok(())
    }
    /// Stores a report to the reporter's parent and applies it to the ticket. During a running
    /// verification cycle only the cycle moves the ticket, and verdicts and the implementer's
    /// readiness ride along with the coordinator's next turn instead of waking it.
    pub(crate) fn report(
        &mut self,
        session: &Session,
        kind: &str,
        body: &str,
        message_id: &str,
    ) -> Result<Value> {
        ensure!(REPORT_KINDS.contains(&kind), "Unknown report kind");
        ensure!(
            kind != "passed" || matches!(session.role, Role::Tester | Role::Reviewer),
            "Only verification agents report passed"
        );
        let parent = session
            .parent_id
            .clone()
            .context("You have no parent to report to; answer the human in this chat instead")?;
        let report_id = format!("report:{}:{message_id}", session.id);
        let content = format!("[{kind}] {}\n{body}", session.name);
        if let Ok(existing) = self.message(&report_id) {
            ensure!(
                existing.body == content,
                "Report ID reused with different content"
            );
            return Ok(serde_json::to_value(existing)?);
        }
        let ticket = self
            .session_runtime(&session.id)?
            .ticket_id
            .map(|id| self.ticket(&id))
            .transpose()?;
        let routing = match &ticket {
            Some(ticket) if kind != "progress" => {
                let answered = self.answered_round(ticket, &session.id)?;
                routing(ticket, session, kind, answered.as_deref())
            }
            _ => Routing::Plain,
        };
        if let (Routing::Ready, Some(ticket)) = (&routing, &ticket) {
            ensure!(
                ticket.state == "failed",
                "Verification round is still running; wait for its result before reporting ready again"
            );
            ensure!(
                is_clean(ticket)?,
                "The worktree has uncommitted or untracked files; commit or discard them, then report ready_for_testing again"
            );
        }
        self.atomically(|host| {
            let message = match routing {
                Routing::Verdict | Routing::Late | Routing::Ready => {
                    host.send_quietly(&report_id, Some(&session.id), &parent, &content)?
                }
                _ => host.send(&report_id, Some(&session.id), &parent, &content)?,
            };
            // Progress is delivered in batches (see the scheduler) and never moves the ticket
            // or the reporter's status.
            if kind == "progress" {
                Self::event(
                    &host.db,
                    &session.project_id,
                    Some(&session.id),
                    "agent_progress",
                    body,
                )?;
                return Ok(serde_json::to_value(message)?);
            }
            if let Some(mut ticket) = ticket {
                match routing {
                    Routing::Plain if ticket.is_open() => {
                        ticket.state = kind.into();
                        host.save_ticket(&ticket)?;
                    }
                    Routing::Plain | Routing::Unrouted | Routing::Late => {}
                    Routing::Verdict => {
                        host.record_verdict(ticket, &session.id, kind, &report_id)?
                    }
                    Routing::Ready => host.next_round(ticket)?,
                    Routing::Ends => {
                        if let Some(verification) = &mut ticket.verification {
                            verification.outcome = VerificationOutcome::Blocked;
                        }
                        ticket.state = "blocked".into();
                        host.save_ticket(&ticket)?;
                        host.retire(&cycle_verifiers(&ticket))?;
                    }
                }
            }
            // A report from a turn being stopped leaves the session stopped until it is resumed.
            if !matches!(
                host.session(&session.id)?.status,
                Status::Paused | Status::Disconnected
            ) {
                host.set_status(
                    &session.id,
                    match kind {
                        "blocked" | "failed" => Status::Blocked,
                        _ => Status::Done,
                    },
                )?;
            }
            Ok(serde_json::to_value(message)?)
        })
    }
    /// The verifier's latest `verify:` message on this ticket that a turn has taken.
    fn answered_round(&self, ticket: &Ticket, session: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT id FROM messages WHERE recipient=?1 AND substr(id,1,length(?2))=?2 AND receipt IN ('delivered','acknowledged','completed','held') ORDER BY sequence DESC LIMIT 1",
                params![session, format!("verify:{}:", ticket.id)],
                |r| r.get(0),
            )
            .optional()?)
    }
    /// Records a verifier's result on its round, failing a pass or failure that changed the
    /// worktree. Ends the round once nobody is pending, unless the cycle already ended.
    fn record_verdict(
        &mut self,
        mut ticket: Ticket,
        session: &str,
        kind: &str,
        report_id: &str,
    ) -> Result<()> {
        let commit = ticket
            .verification
            .as_ref()
            .and_then(Verification::current_round)
            .context("No round")?
            .commit
            .clone();
        let untouched = kind == "blocked" || (head(&ticket)? == commit && is_clean(&ticket)?);
        let verification = ticket.verification.as_mut().context("No verification")?;
        let round = verification.rounds.last_mut().context("No round")?;
        let run = round
            .verifiers
            .iter_mut()
            .find(|v| v.session_id == session)
            .context("Not a verifier of this round")?;
        run.report_id = Some(report_id.into());
        run.result = match kind {
            "passed" if untouched => VerifierResult::Passed,
            "blocked" => VerifierResult::Blocked,
            _ => VerifierResult::Failed,
        };
        if !untouched {
            run.reason = Some(WORKTREE_CHANGED.into());
        }
        let ended = round
            .verifiers
            .iter()
            .all(|v| v.result != VerifierResult::Pending);
        let running = verification.outcome == VerificationOutcome::Running;
        self.save_ticket(&ticket)?;
        if ended && running {
            self.end_round(ticket)?;
        }
        Ok(())
    }
    /// Decides what a finished round means: everyone passed, a blocked verifier or the cap ends
    /// the cycle and wakes the coordinator once; otherwise the failures go to the implementer.
    fn end_round(&mut self, mut ticket: Ticket) -> Result<()> {
        let verification = ticket.verification.as_ref().context("No verification")?;
        let round = verification.current_round().context("No round")?;
        let cycle = verification.cycle;
        let failed: Vec<&VerifierRun> = round
            .verifiers
            .iter()
            .filter(|v| v.result == VerifierResult::Failed)
            .collect();
        let blocked = round
            .verifiers
            .iter()
            .any(|v| v.result == VerifierResult::Blocked);
        let below_cap = round.round < verification.max_rounds;
        let implementer = self.ready_implementer(&ticket)?;
        if !blocked
            && !failed.is_empty()
            && below_cap
            && let Some(implementer) = implementer
        {
            let body = format!(
                "Verification round {} of ticket \"{}\" failed at commit {} ({} of {} rounds used).\n\n{}\n\nCommit fixes in the worktree, then report ready_for_testing; only the failed verifiers check again. Do not message the verifiers.",
                round.round,
                ticket.title,
                round.commit,
                round.round,
                verification.max_rounds,
                self.findings(&failed)?
            );
            let id = format!("verification:{}:{cycle}:{}", ticket.id, round.round);
            let passed: Vec<String> = round
                .verifiers
                .iter()
                .filter(|v| v.result == VerifierResult::Passed)
                .map(|v| v.session_id.clone())
                .collect();
            ticket.state = "failed".into();
            self.save_ticket(&ticket)?;
            self.send(&id, Some(&ticket.coordinator_id), &implementer, &body)?;
            return self.retire(&passed);
        }
        let (outcome, summary) = if blocked || !failed.is_empty() {
            let unresolved: Vec<&VerifierRun> = round
                .verifiers
                .iter()
                .filter(|v| v.result != VerifierResult::Passed)
                .collect();
            let why = if blocked {
                "a verifier could not verify".to_owned()
            } else if below_cap {
                "the ticket has no implementer to fix it".to_owned()
            } else {
                format!("the round cap of {} was reached", verification.max_rounds)
            };
            (
                VerificationOutcome::Blocked,
                format!(
                    "Wiffletree verification of ticket \"{}\" ({}) is blocked after {} of {} rounds: {why}. Decide whether to fix further, verify again, close the ticket or ask the human.\n\n{}",
                    ticket.title,
                    ticket.id,
                    round.round,
                    verification.max_rounds,
                    self.findings(&unresolved)?
                ),
            )
        } else {
            let passes = verification
                .latest_results()
                .into_iter()
                .map(|run| {
                    let commit = verification
                        .rounds
                        .iter()
                        .rev()
                        .find(|r| r.verifiers.contains(run))
                        .map_or("", |r| r.commit.as_str());
                    format!("- {}: passed at {commit}", verifier_label(run))
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                VerificationOutcome::Passed,
                format!(
                    "Wiffletree verification of ticket \"{}\" ({}) passed in {} of {} rounds; verified commit {}.\n{passes}\n\nAccept the ticket or start a fresh cycle.",
                    ticket.title, ticket.id, round.round, verification.max_rounds, round.commit
                ),
            )
        };
        let id = format!("verification:{}:{cycle}:outcome", ticket.id);
        let coordinator = ticket.coordinator_id.clone();
        ticket.state = match outcome {
            VerificationOutcome::Passed => "passed",
            _ => "blocked",
        }
        .into();
        if let Some(verification) = &mut ticket.verification {
            verification.outcome = outcome;
        }
        self.save_ticket(&ticket)?;
        self.send(&id, None, &coordinator, &summary)?;
        self.retire(&cycle_verifiers(&ticket))
    }
    /// Archives verifiers the cycle no longer needs, keeping their conversations. Implementers
    /// are never retired here; they live until the ticket is accepted or closed.
    fn retire(&mut self, sessions: &[String]) -> Result<()> {
        for id in sessions {
            let session = self.session(id)?;
            if !session.archived && matches!(session.role, Role::Tester | Role::Reviewer) {
                self.set_archived(id, true)?;
            }
        }
        Ok(())
    }
    /// Starts the next round with only the verifiers whose latest result failed, pinned to the
    /// implementer's new commit.
    fn next_round(&mut self, mut ticket: Ticket) -> Result<()> {
        let commit = head(&ticket)?;
        let verification = ticket.verification.as_mut().context("No verification")?;
        let number = verification.current_round().map_or(1, |r| r.round + 1);
        let cycle = verification.cycle;
        let verifiers = verification
            .latest_results()
            .into_iter()
            .filter(|run| run.result == VerifierResult::Failed)
            .map(|run| VerifierRun {
                message_id: format!("verify:{}:{cycle}:{number}:{}", ticket.id, run.session_id),
                result: VerifierResult::Pending,
                report_id: None,
                reason: None,
                ..run.clone()
            })
            .collect();
        verification.rounds.push(VerificationRound {
            round: number,
            commit,
            verifiers,
        });
        self.start_round(&mut ticket, &Default::default())
    }
    /// The implementer that last reported ready_for_testing, else the ticket's only one.
    fn ready_implementer(&self, ticket: &Ticket) -> Result<Option<String>> {
        let implementers: Vec<String> = self
            .ticket_agents(&ticket.id)?
            .into_iter()
            .filter(|s| s.role == Role::Implementer && !s.archived)
            .map(|s| s.id)
            .collect();
        let latest: Option<String> = self
            .db
            .query_row(
                "SELECT sender FROM messages WHERE recipient=?1 AND substr(body,1,20)='[ready_for_testing] ' AND sender IN (SELECT value FROM json_each(?2)) ORDER BY sequence DESC LIMIT 1",
                params![ticket.coordinator_id, json!(implementers).to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(latest.or_else(|| implementers.into_iter().next()))
    }
    /// Each verifier's report, or the host's reason for overriding it.
    fn findings(&self, runs: &[&VerifierRun]) -> Result<String> {
        let mut findings = vec![];
        for run in runs {
            let report = match &run.report_id {
                Some(id) => self
                    .message(id)?
                    .body
                    .split_once('\n')
                    .map_or(String::new(), |(_, body)| body.to_owned()),
                None => "no report".into(),
            };
            let reason = run
                .reason
                .as_ref()
                .map_or(String::new(), |r| format!(" ({r})"));
            findings.push(format!(
                "{} [{}{reason}]:\n{report}",
                verifier_label(run),
                run.result.label()
            ));
        }
        let mut findings = findings.join("\n\n");
        findings.truncate(steps::floor_boundary(
            &findings,
            findings.len().min(MAX_TEXT_BYTES / 2),
        ));
        Ok(findings)
    }
}
