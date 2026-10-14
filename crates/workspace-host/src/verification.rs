//! Ticket verification. One call starts every verifier of the review level the coordinator names
//! on the ticket's current commit; the scheduler runs a round's verifiers together. Verifiers are read-only. Failures go
//! back to the implementer, only failed verifiers re-run, rounds are capped, and the project
//! coordinator wakes once, when the cycle ends. Verifiers are archived once the cycle no longer
//! needs them, so every cycle starts fresh sessions.
use crate::*;
use serde::Deserialize;
use std::collections::BTreeSet;

const REPORT_KINDS: [&str; 6] = [
    "progress",
    "blocked",
    "ready_for_testing",
    "passed",
    "failed",
    "completed",
];
const WORKTREE_CHANGED: &str = "worktree changed during verification";
const UNCOMMITTED: &str = "The worktree has uncommitted or untracked files; ask the implementer to commit or discard them";
/// How many verification cycles may hold a slot at once across the host, so concurrent
/// verifiers do not overload the machine. A further cycle queues; see `start_queued_cycles`.
pub(crate) const VERIFICATION_SLOTS: usize = 2;
/// Bounds on a finding's fields; they keep a ticket's ledger small in `workspace_context`.
const FINDING_LINE_BYTES: usize = 256;
const FINDING_TEXT_BYTES: usize = 2048;

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

/// How many full reviews the ticket has started, its latest cycle included. Re-checks are not
/// counted against the cycle cap.
fn full_reviews(ticket: &Ticket) -> usize {
    ticket
        .previous_cycles
        .iter()
        .chain(&ticket.verification)
        .filter(|v| v.is_full_review())
        .count()
}

/// The rules that keep a lighter level from lowering the bar for accepting or publishing: a
/// re-check starts only on a ticket whose latest cycle passed, no level leaves out a configured
/// verifier that has an open finding, and full reviews stop at the cycle cap. With these, every
/// passed cycle rests on a passed full review.
fn level_may_start(
    ticket: &Ticket,
    settings: &VerificationSettings,
    level: &ReviewLevel,
) -> Result<()> {
    if level.kind == LevelKind::Recheck {
        let full_reviews: Vec<&str> = settings
            .levels
            .iter()
            .filter(|l| l.kind == LevelKind::FullReview)
            .map(|l| l.name.as_str())
            .collect();
        let full_reviews = or_list(&full_reviews);
        match ticket.verification.as_ref().map(|v| v.outcome) {
            Some(VerificationOutcome::Passed) => {}
            None => bail!(
                "{} is a re-check, and this ticket has not passed a review yet. Start with a full review: {full_reviews}.",
                level.name
            ),
            Some(_) => bail!(
                "{} is a re-check, and this ticket's last review ended blocked. Run a full review ({full_reviews}), accept with a waiver, close the ticket or ask the human.",
                level.name
            ),
        }
    }
    for outside in settings.verifiers.iter().filter(|v| !level.runs(&v.focus)) {
        let open: Vec<&str> = ticket
            .ledger
            .iter()
            .filter(|e| e.status == EntryStatus::Open && e.focus == outside.focus)
            .map(|e| e.id.as_str())
            .collect();
        let (findings, them) = match open.as_slice() {
            [] => continue,
            [one] => ("finding", *one),
            _ => ("findings", "them"),
        };
        bail!(
            "{focus} has the open {findings} {}, and {} does not run {focus}. Use a level that runs it, or settle {them} with triage_findings.",
            open.join(", "),
            level.name,
            focus = outside.focus
        );
    }
    ensure!(
        level.kind == LevelKind::Recheck || full_reviews(ticket) < settings.max_cycles as usize,
        "Ticket \"{}\" has used its {} full reviews; accept it with a waiver, close it or ask the human. Only the human can raise the limit, in Models → Review.",
        ticket.title,
        settings.max_cycles
    );
    Ok(())
}

fn verifier_label(run: &VerifierRun) -> String {
    run.role.agent_label(Some(&run.focus))
}

/// Refuses a finding that is malformed or exceeds the bounds, before anything is stored.
fn check_findings(findings: &[ReportedFinding]) -> Result<()> {
    for reported in findings {
        let finding = &reported.finding;
        text(&finding.location, FINDING_LINE_BYTES).context("Finding location")?;
        text(&finding.summary, FINDING_LINE_BYTES).context("Finding summary")?;
        ensure!(
            !finding.summary.contains('\n'),
            "A finding's summary is one line"
        );
        ensure!(
            finding.trigger.len() <= FINDING_TEXT_BYTES
                && finding.evidence.len() <= FINDING_TEXT_BYTES,
            "A finding's trigger and evidence are at most {FINDING_TEXT_BYTES} bytes each"
        );
    }
    Ok(())
}

/// Each reported id must name an open, follow-up or won't-fix entry of the verifier's focus.
fn check_ids(ticket: &Ticket, focus: &str, findings: &[ReportedFinding]) -> Result<()> {
    for id in findings.iter().filter_map(|f| f.id.as_deref()) {
        ensure!(
            ticket.ledger.iter().any(|e| e.id == id
                && e.focus == focus
                && (e.status == EntryStatus::Open || e.status.is_settled())),
            "{id} is not an open, follow-up or won't-fix finding of {focus}; report a new finding without an id"
        );
    }
    Ok(())
}

/// A finding that fails its verifier's check: evidenced, blocking and not one the coordinator
/// already settled.
fn is_failing(ledger: &[LedgerEntry], reported: &ReportedFinding) -> bool {
    let settled = reported
        .id
        .as_deref()
        .is_some_and(|id| ledger.iter().any(|e| e.id == id && e.status.is_settled()));
    reported.finding.is_evidenced_blocking() && !settled
}

/// Applies a completed check to the ledger: the focus's open entries it did not report again are
/// fixed, an open entry it reports again stays open with the new report, and new findings are
/// added. A settled entry reported again is left alone.
fn record_findings(
    ledger: &mut Vec<LedgerEntry>,
    focus: &str,
    (cycle, round): (u32, u32),
    findings: &[ReportedFinding],
) {
    let status = |finding: &Finding| {
        if finding.is_evidenced_blocking() {
            EntryStatus::Open
        } else {
            EntryStatus::Untriaged
        }
    };
    for entry in ledger
        .iter_mut()
        .filter(|e| e.focus == focus && e.status == EntryStatus::Open)
    {
        if !findings.iter().any(|f| f.id.as_ref() == Some(&entry.id)) {
            entry.status = EntryStatus::Fixed;
        }
    }
    for reported in findings {
        let existing = reported
            .id
            .as_ref()
            .and_then(|id| ledger.iter_mut().find(|e| &e.id == id));
        match existing {
            Some(entry) if entry.status.is_settled() => {}
            Some(entry) => {
                entry.finding = reported.finding.clone();
                (entry.cycle, entry.round) = (cycle, round);
            }
            None => {
                let id = format!("F{}", ledger.len() + 1);
                ledger.push(LedgerEntry {
                    id,
                    finding: reported.finding.clone(),
                    focus: focus.into(),
                    cycle,
                    round,
                    status: status(&reported.finding),
                    reason: None,
                });
            }
        }
    }
}

/// When a cycle ends, an open entry that none of its verifiers could re-check awaits triage.
fn untriage_orphans(verification: &Verification, ledger: &mut [LedgerEntry]) {
    let focuses: Vec<&str> = verification
        .rounds
        .iter()
        .flat_map(|r| &r.verifiers)
        .map(|run| run.focus.as_str())
        .collect();
    for entry in ledger
        .iter_mut()
        .filter(|e| e.status == EntryStatus::Open && !focuses.contains(&e.focus.as_str()))
    {
        entry.status = EntryStatus::Untriaged;
    }
}

fn entry_line(entry: &LedgerEntry) -> String {
    format!(
        "{} · {} · {}",
        entry.id, entry.finding.location, entry.finding.summary
    )
}

/// An entry with its trigger and evidence, as the implementer must read it.
fn entry_text(entry: &LedgerEntry) -> String {
    format!(
        "{}\n  Trigger: {}\n  Evidence: {}",
        entry_line(entry),
        entry.finding.trigger,
        entry.finding.evidence
    )
}

/// The open and untriaged entries and the ids already decided, each as a paragraph.
fn ledger_summary(ledger: &[LedgerEntry]) -> Vec<String> {
    let lines = |status: EntryStatus| {
        ledger
            .iter()
            .filter(|e| e.status == status)
            .map(|e| format!("- {}", entry_line(e)))
            .collect::<Vec<_>>()
    };
    let decided: Vec<&str> = ledger
        .iter()
        .filter(|e| {
            matches!(
                e.status,
                EntryStatus::FixNow | EntryStatus::FollowUp | EntryStatus::WontFix
            )
        })
        .map(|e| e.id.as_str())
        .collect();
    let mut paragraphs = vec![];
    for (title, lines) in [
        ("Open findings", lines(EntryStatus::Open)),
        ("Untriaged findings", lines(EntryStatus::Untriaged)),
    ] {
        if !lines.is_empty() {
            paragraphs.push(format!("{title}:\n{}", lines.join("\n")));
        }
    }
    if !decided.is_empty() {
        paragraphs.push(format!("Already decided: {}", decided.join(", ")));
    }
    paragraphs
}

fn paragraphs(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// How a ticket's cycle stands for verification slots.
enum Standing {
    /// No running cycle, or one that cannot make progress for now: its ticket is closed, or
    /// its project is paused or archived.
    Idle,
    /// Started and holding a slot, between rounds included.
    Holding,
    Queued,
    /// It needs a verifier that was archived, so it can never end by itself; why.
    Stalled(String),
}

/// How full the verification slots are.
pub(crate) struct Slots {
    pub taken: usize,
    /// Queued cycles, oldest first.
    pub queued: Vec<Ticket>,
    pub stalled: Vec<(Ticket, String)>,
}

/// A coordinator's decision on one ledger entry.
#[derive(Deserialize)]
pub(crate) struct Triage {
    id: String,
    decision: EntryStatus,
    reason: String,
}

impl Host {
    /// Starts a cycle at the review level named `level`: pins HEAD and sends every verifier of
    /// that level one message naming it. When every verification slot is taken, or another
    /// cycle already waits for one, the cycle queues instead: its verifiers are chosen now and
    /// sent nothing until `start_queued_cycles` starts it. A re-check starts only on a ticket whose latest cycle
    /// passed, no level may leave out a configured verifier that has an open finding, and only
    /// full reviews count against the cycle cap. `choices` gives each verifier of the level, by
    /// focus, the coordinator's exact model and reason; a new verifier is pinned to it. An
    /// existing one is reused only at the same profile; otherwise it is archived and a fresh one
    /// takes the choice, so no pinned model ever changes.
    pub fn verify_ticket(
        &mut self,
        ticket_id: &str,
        level: &str,
        choices: &[VerifierChoice],
    ) -> Result<Ticket> {
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
        ensure!(is_clean(&ticket)?, "{UNCOMMITTED}");
        let settings = self.settings().verification;
        let (level, level_verifiers) = settings.level(level)?;
        level_may_start(&ticket, &settings, level)?;
        let cycle = ticket.verification.as_ref().map_or(1, |v| v.cycle + 1);
        let coordinator = self.session(&ticket.coordinator_id)?;
        let chosen = self.verifier_choices(&coordinator, level, &level_verifiers, choices)?;
        let commit = head(&ticket)?;
        // A cycle queues behind every slot holder and every cycle queued before it.
        let slots = self.verification_slots()?;
        let queued_at = (slots.taken >= VERIFICATION_SLOTS || !slots.queued.is_empty()).then(now);
        let mut replaced = vec![];
        let started = self.atomically(|host| {
            let mut verifiers = vec![];
            for (config, (profile, reason)) in level_verifiers.into_iter().zip(chosen) {
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
                        let archived =
                            host.archive_in_change(&retired.id, Some("verifier_replaced"))?;
                        replaced.extend(archived);
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
            ticket.previous_cycles.extend(ticket.verification.take());
            ticket.verification = Some(Verification {
                cycle,
                level: Some(CycleLevel {
                    name: level.name.clone(),
                    kind: level.kind,
                }),
                max_rounds: settings.max_rounds,
                outcome: VerificationOutcome::Running,
                rounds: vec![VerificationRound {
                    round: 1,
                    commit,
                    verifiers,
                }],
                queued_at,
            });
            let milestone = if queued_at.is_some() {
                ticket.state = "verifying".into();
                host.save_ticket(&ticket)?;
                "verification_queued"
            } else {
                host.start_round(&mut ticket)?;
                "verification_started"
            };
            Self::event(
                &host.db,
                &coordinator.project_id,
                Some(&ticket.coordinator_id),
                milestone,
                &format!("{}; cycle {cycle}", ticket.id),
            )?;
            Ok(ticket)
        })?;
        // Only now: a call that failed to start or queue the cycle leaves a replaced verifier as
        // it was.
        self.stop_archived(&replaced);
        Ok(started)
    }
    /// How a ticket's cycle stands. Only a cycle that can make progress holds or takes a slot:
    /// its ticket is open, its project live with its coordinator not archived, and none of the
    /// verifiers it still needs, pending or due to re-check a fix, archived.
    fn standing(&self, ticket: &Ticket, live: &[String]) -> Result<Standing> {
        let Some(cycle) = ticket.running_cycle().filter(|_| ticket.is_open()) else {
            return Ok(Standing::Idle);
        };
        let coordinator = self.session(&ticket.coordinator_id)?;
        if coordinator.archived || !live.contains(&coordinator.project_id) {
            return Ok(Standing::Idle);
        }
        for run in cycle.latest_results() {
            if run.result != VerifierResult::Passed && self.session(&run.session_id)?.archived {
                return Ok(Standing::Stalled(format!(
                    "{} was archived",
                    verifier_label(run)
                )));
            }
        }
        Ok(match cycle.queued_at {
            Some(_) => Standing::Queued,
            None => Standing::Holding,
        })
    }
    pub(crate) fn verification_slots(&self) -> Result<Slots> {
        let live = self.live_projects()?;
        let mut slots = Slots {
            taken: 0,
            queued: vec![],
            stalled: vec![],
        };
        for ticket in self.tickets()? {
            match self.standing(&ticket, &live)? {
                Standing::Idle => {}
                Standing::Holding => slots.taken += 1,
                Standing::Queued => slots.queued.push(ticket),
                Standing::Stalled(why) => slots.stalled.push((ticket, why)),
            }
        }
        slots.queued.sort_by_key(Ticket::queued_since);
        Ok(slots)
    }
    /// Ends stalled cycles, then starts the oldest queued cycles while slots are free, so a
    /// queued cycle starts once a running one passes, is blocked or its ticket closes. The
    /// scheduler calls it on every pass, and the queue is stored with the tickets, so it
    /// survives a restart. A starting cycle is pinned to the worktree's HEAD then. One that
    /// cannot start, such as one whose worktree has uncommitted files, ends blocked and tells
    /// the coordinator instead of stopping the pass. Returns whether any cycle changed.
    pub fn start_queued_cycles(&mut self) -> Result<bool> {
        let slots = self.verification_slots()?;
        let mut changed = !slots.stalled.is_empty();
        for (ticket, why) in slots.stalled {
            self.end_unstartable(&ticket.id, &why)?;
        }
        let mut free = VERIFICATION_SLOTS.saturating_sub(slots.taken);
        for mut ticket in slots.queued {
            if free == 0 {
                break;
            }
            changed = true;
            let started = self.atomically(|host| {
                ensure!(is_clean(&ticket)?, UNCOMMITTED);
                let commit = head(&ticket)?;
                let verification = ticket.verification.as_mut().context("No verification")?;
                verification.queued_at = None;
                verification.rounds[0].commit = commit;
                let cycle = verification.cycle;
                host.start_round(&mut ticket)?;
                let project = host.session(&ticket.coordinator_id)?.project_id;
                Self::event(
                    &host.db,
                    &project,
                    Some(&ticket.coordinator_id),
                    "verification_started",
                    &format!("{}; cycle {cycle}", ticket.id),
                )
            });
            match started {
                Ok(()) => free -= 1,
                Err(error) => self.end_unstartable(&ticket.id, &format!("{error:#}"))?,
            }
        }
        Ok(changed)
    }
    /// Ends a cycle that cannot start or go on as blocked, and tells its coordinator why.
    fn end_unstartable(&mut self, ticket_id: &str, why: &str) -> Result<()> {
        let mut ticket = self.ticket(ticket_id)?;
        self.atomically(|host| {
            let verification = ticket.verification.as_mut().context("No verification")?;
            verification.outcome = VerificationOutcome::Blocked;
            untriage_orphans(verification, &mut ticket.ledger);
            let id = format!("verification:{}:{}:unstartable", ticket.id, verification.cycle);
            let body = format!(
                "Wiffletree verification cycle {} of ticket \"{}\" ({}) cannot go on: {why}. The cycle is blocked; fix the cause and call verify_ticket again, close the ticket or ask the human.",
                verification.cycle, ticket.title, ticket.id
            );
            ticket.state = "blocked".into();
            ticket.end_unreviewed();
            host.save_ticket(&ticket)?;
            host.send(&id, None, &ticket.coordinator_id, &body)?;
            host.retire(&cycle_verifiers(&ticket))
        })
    }
    /// Records the coordinator's decision on each named entry, all or none, and wakes nobody. An
    /// untriaged entry takes any decision; an open one, already routed for a fix, only follow-up
    /// or won't-fix, which overrules it. A decision never changes.
    pub(crate) fn triage_findings(
        &mut self,
        ticket_id: &str,
        decisions: &[Triage],
    ) -> Result<Ticket> {
        let mut ticket = self.ticket(ticket_id)?;
        let mut named = BTreeSet::new();
        for triage in decisions {
            let id = &triage.id;
            ensure!(named.insert(id), "{id} is named twice");
            let entry = ticket
                .ledger
                .iter_mut()
                .find(|e| &e.id == id)
                .with_context(|| format!("Ticket \"{}\" has no finding {id}", ticket.title))?;
            ensure!(
                matches!(
                    triage.decision,
                    EntryStatus::FixNow | EntryStatus::FollowUp | EntryStatus::WontFix
                ),
                "Decide {id} with fix_now, follow_up or wont_fix"
            );
            match entry.status {
                EntryStatus::Untriaged => {}
                EntryStatus::Open => ensure!(
                    triage.decision != EntryStatus::FixNow,
                    "{id} is open and already routed to the implementer; overrule it only with follow_up or wont_fix"
                ),
                status => bail!("{id} is already {}", status.label()),
            }
            entry.status = triage.decision;
            entry.reason = Some(one_line_reason(&triage.reason)?.to_owned());
        }
        let coordinator = self.session(&ticket.coordinator_id)?;
        self.atomically(|host| {
            host.save_ticket(&ticket)?;
            let decided = decisions
                .iter()
                .map(|t| format!("{} {}", t.id, t.decision.label()))
                .collect::<Vec<_>>()
                .join(", ");
            Self::event(
                &host.db,
                &coordinator.project_id,
                Some(&coordinator.id),
                "findings_triaged",
                &format!("{}; {decided}", ticket.id),
            )?;
            Ok(ticket)
        })
    }
    /// Pairs every verifier of `level` with the coordinator's choice for it, checking each
    /// against the machine's model selection and the verifier's provider. Nothing is substituted:
    /// one refusal starts no verifier.
    fn verifier_choices(
        &self,
        coordinator: &Session,
        level: &ReviewLevel,
        verifiers: &[&VerifierConfig],
        choices: &[VerifierChoice],
    ) -> Result<Vec<(ModelProfile, String)>> {
        let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
        if let Some(outside) = choices
            .iter()
            .find(|c| !verifiers.iter().any(|v| same(&v.focus, &c.focus)))
        {
            let focuses: Vec<&str> = verifiers.iter().map(|v| v.focus.as_str()).collect();
            bail!(
                "{} is not part of {}. Choose a model only for {}.",
                outside.focus.trim(),
                level.name,
                focuses.join(", ")
            );
        }
        let selection = self.model_selection()?;
        let mut chosen = vec![];
        for verifier in verifiers {
            let label = verifier.role.agent_label(Some(&verifier.focus));
            let mut matching = choices.iter().filter(|c| same(&c.focus, &verifier.focus));
            let choice = matching.next().with_context(|| {
                format!(
                    "Choose a profile and reason for {label}, which {} runs",
                    level.name
                )
            })?;
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
    /// Sends the current round's messages and marks the ticket `verifying`. Each message carries
    /// the ticket's review memory: the diff under review, the entries already decided and the
    /// verifier's own open entries. From round 2 on, it limits the check to those entries and
    /// the new diff.
    fn start_round(&mut self, ticket: &mut Ticket) -> Result<()> {
        ticket.state = "verifying".into();
        self.save_ticket(ticket)?;
        let settings = self.settings().verification;
        let verification = ticket.verification.as_ref().context("No verification")?;
        let round = verification.current_round().context("No round")?;
        let mut memory = vec![self.diff_lines(ticket, &round.commit)];
        if let [.., previous, _] = verification.rounds.as_slice() {
            let stat =
                worktrees::shortstat(Path::new(&ticket.worktree), &previous.commit, &round.commit)
                    .unwrap_or_else(|e| format!("unavailable: {e:#}"));
            memory.push(format!(
                "New since round {}: {}..{}, {stat}",
                previous.round, previous.commit, round.commit
            ));
        }
        let decided: Vec<String> = ticket
            .ledger
            .iter()
            .filter(|e| e.status.is_settled())
            .map(|e| format!("- {} ({})", entry_line(e), e.status.label()))
            .collect();
        if !decided.is_empty() {
            memory.push(format!(
                "Already decided; do not re-raise unless the cited code changed:\n{}",
                decided.join("\n")
            ));
        }
        let memory = memory.join("\n");
        // The cycle's first tester owns its one run of the full suite; any other tester is told so.
        let suite_owner = verification.rounds[0]
            .verifiers
            .iter()
            .find(|v| v.role == Role::Tester);
        for run in &round.verifiers {
            let suite = match suite_owner {
                Some(owner) if run.role == Role::Tester && owner.session_id != run.session_id => {
                    format!(
                        "Suite owner: {}. It runs this cycle's full build, lint and test suite; you do not. Check as a reviewer does, running one narrowly targeted check at a time.\n",
                        verifier_label(owner)
                    )
                }
                _ => String::new(),
            };
            let instruction = settings
                .verifiers
                .iter()
                .find(|v| v.focus == run.focus)
                .and_then(|v| v.instruction.as_deref())
                .map_or(String::new(), |i| format!("Instruction: {i}\n"));
            let again = if round.round > 1 {
                "Check only whether your open blocking findings are fixed and whether the new diff introduces a blocking defect; do not raise new findings on code you already reviewed.\n\n"
            } else {
                ""
            };
            let open: Vec<String> = ticket
                .ledger
                .iter()
                .filter(|e| e.status == EntryStatus::Open && e.focus == run.focus)
                .map(|e| format!("- {}", entry_line(e)))
                .collect();
            let open = if open.is_empty() {
                String::new()
            } else {
                format!(
                    "\nYour open findings; report one again by its id only if it is still present:\n{}",
                    open.join("\n")
                )
            };
            self.send(
                &run.message_id,
                Some(&ticket.coordinator_id),
                &run.session_id,
                &format!(
                    "{again}Verify ticket \"{}\" at commit {}: round {} of at most {}.\n\nTicket brief:\n{}\n\nFocus: {}\n{instruction}{suite}Worktree: {}\nBranch: {}\n\n{memory}{open}\n\nYou are read-only: do not edit, stage, commit or switch branches in the worktree. Check commit {}, then report passed, failed or blocked, with every finding in findings.",
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
    /// The base commit and the diff from it to `commit`, or why they are unavailable.
    fn diff_lines(&self, ticket: &Ticket, commit: &str) -> String {
        let worktree = Path::new(&ticket.worktree);
        let lines = || -> Result<String> {
            let base = self.ticket_repository(ticket)?.base;
            let merge_base = worktrees::merge_base(worktree, &base, commit)?;
            let stat = worktrees::shortstat(worktree, &merge_base, commit)?;
            Ok(format!(
                "Base: {merge_base} ({base})\nDiff: {merge_base}..{commit}, {stat}"
            ))
        };
        lines().unwrap_or_else(|e| format!("Base unavailable: {e:#}"))
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
        findings: &[ReportedFinding],
    ) -> Result<Value> {
        ensure!(REPORT_KINDS.contains(&kind), "Unknown report kind");
        check_findings(findings)?;
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
        if let (Routing::Verdict, Some(ticket)) = (&routing, &ticket) {
            let focus = ticket
                .verification
                .as_ref()
                .and_then(Verification::current_round)
                .and_then(|r| r.verifiers.iter().find(|v| v.session_id == session.id))
                .map(|run| run.focus.as_str())
                .context("Not a verifier of this round")?;
            check_ids(ticket, focus, findings)?;
        }
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
                        let prior = host.shown_ticket(ticket.clone()).state;
                        ticket.state = kind.into();
                        host.save_ticket(&ticket)?;
                        if host.in_turn.contains(&session.id) {
                            host.mid_turn_reports.insert(
                                ticket.id,
                                MidTurnReport {
                                    reporter: session.id.clone(),
                                    prior,
                                    reported: kind.into(),
                                },
                            );
                        }
                    }
                    Routing::Plain | Routing::Unrouted | Routing::Late => {}
                    Routing::Verdict => {
                        host.record_verdict(ticket, &session.id, kind, &report_id, findings)?
                    }
                    Routing::Ready => host.next_round(ticket)?,
                    Routing::Ends => {
                        if let Some(verification) = &mut ticket.verification {
                            verification.outcome = VerificationOutcome::Blocked;
                            untriage_orphans(verification, &mut ticket.ledger);
                        }
                        ticket.state = "blocked".into();
                        ticket.end_unreviewed();
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
    /// Records a verifier's result on its round: failed when it changed the worktree or reported
    /// an evidenced blocking finding, else passed, unless it could not verify. While the cycle
    /// runs, a completed check updates the ledger. Ends the round once nobody is pending, unless
    /// the cycle already ended.
    fn record_verdict(
        &mut self,
        mut ticket: Ticket,
        session: &str,
        kind: &str,
        report_id: &str,
        findings: &[ReportedFinding],
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
        let failing = findings.iter().any(|f| is_failing(&ticket.ledger, f));
        run.result = match kind {
            "blocked" => VerifierResult::Blocked,
            _ if !untouched || failing => VerifierResult::Failed,
            _ => VerifierResult::Passed,
        };
        run.reason = match (kind, run.result) {
            _ if !untouched => Some(WORKTREE_CHANGED),
            ("failed", VerifierResult::Passed) => {
                Some("reported failed without an evidenced blocking finding")
            }
            ("passed", VerifierResult::Failed) => {
                Some("reported passed with an evidenced blocking finding")
            }
            _ => None,
        }
        .map(Into::into);
        let focus = run.focus.clone();
        let ended = round
            .verifiers
            .iter()
            .all(|v| v.result != VerifierResult::Pending);
        let source = (verification.cycle, round.round);
        let running = verification.outcome == VerificationOutcome::Running;
        // Only a completed check moves the ledger, and not once the coordinator has the cycle's
        // outcome.
        if running && untouched && kind != "blocked" {
            record_findings(&mut ticket.ledger, &focus, source, findings);
        }
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
            let open = ticket
                .ledger
                .iter()
                .filter(|e| {
                    e.status == EntryStatus::Open && (e.cycle, e.round) == (cycle, round.round)
                })
                .map(entry_text);
            let changed = failed
                .iter()
                .filter(|run| run.reason.as_deref() == Some(WORKTREE_CHANGED))
                .map(|run| format!("{}: {WORKTREE_CHANGED}", verifier_label(run)));
            let body = format!(
                "Verification round {} of ticket \"{}\" failed at commit {} ({} of {} rounds used).\n\n{}\n\nFix each finding with the smallest change, commit it, then report ready_for_testing; only the failed verifiers check again. If a finding is wrong or out of scope, tell the coordinator in one sentence with evidence instead of implementing it. Do not message the verifiers.",
                round.round,
                ticket.title,
                round.commit,
                round.round,
                verification.max_rounds,
                open.chain(changed).collect::<Vec<_>>().join("\n\n")
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
            self.send(&id, None, &implementer, &body)?;
            return self.retire(&passed);
        }
        let max_cycles = self.settings().verification.max_cycles;
        let level = match &verification.level {
            Some(level) if level.kind == LevelKind::Recheck => format!("a {} re-check", level.name),
            level => format!(
                "full review {} of {max_cycles}{}",
                full_reviews(&ticket),
                level
                    .as_ref()
                    .map_or(String::new(), |level| format!(" ({})", level.name))
            ),
        };
        let rounds = format!(
            "cycle {cycle}, {level}, {} of {} rounds",
            round.round, verification.max_rounds
        );
        // The cycle ends here, so its message already lists orphaned entries as untriaged.
        untriage_orphans(verification, &mut ticket.ledger);
        let ledger = ledger_summary(&ticket.ledger);
        let (outcome, summary) = if blocked || !failed.is_empty() {
            let why = if blocked {
                "a verifier could not verify".to_owned()
            } else if below_cap {
                "the ticket has no implementer to fix it".to_owned()
            } else {
                format!("the round cap of {} was reached", verification.max_rounds)
            };
            let blocked_runs: Vec<&VerifierRun> = round
                .verifiers
                .iter()
                .filter(|v| v.result == VerifierResult::Blocked)
                .collect();
            let mut actions = vec![];
            if full_reviews(&ticket) < max_cycles as usize {
                actions.push(
                    "send the implementer fixes and call verify_ticket again with a full-review level, not a re-check",
                );
            }
            if !blocked {
                actions.push(
                    "once no finding is untriaged, accept with accept_ticket's waived reason naming the open findings",
                );
            }
            actions.extend(["close the ticket", "or ask the human"]);
            let mut parts = vec![format!(
                "Wiffletree verification of ticket \"{}\" ({}) is blocked after {rounds}: {why}.",
                ticket.title, ticket.id
            )];
            parts.extend(ledger);
            parts.push(self.findings(&blocked_runs)?);
            parts.push(format!("Decide: {}.", actions.join("; ")));
            (VerificationOutcome::Blocked, paragraphs(parts))
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
            let next = if ticket
                .ledger
                .iter()
                .any(|e| e.status == EntryStatus::Untriaged)
            {
                "Triage the untriaged findings with triage_findings, then accept the ticket."
            } else {
                "Accept the ticket."
            };
            let mut parts = vec![format!(
                "Wiffletree verification of ticket \"{}\" ({}) passed in {rounds}; verified commit {}.\n{passes}",
                ticket.title, ticket.id, round.commit
            )];
            parts.extend(ledger);
            parts.push(next.into());
            (VerificationOutcome::Passed, paragraphs(parts))
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
        ticket.end_unreviewed();
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
        self.start_round(&mut ticket)
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
    /// The reports of verifiers that could not verify, for the coordinator.
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
