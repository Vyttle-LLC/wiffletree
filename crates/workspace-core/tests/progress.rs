//! The work-progress spec's agent, ticket and project scenarios.
use serde_json::json;
use std::collections::HashMap;
use workspace_core::*;

const PROJECT: &str = "project";
const COORDINATOR: &str = "coordinator";
const TICKET: &str = "ticket";

/// The records `derive` reads, built up one agent and ticket at a time.
#[derive(Default)]
struct World {
    sessions: Vec<Session>,
    runtimes: Vec<SessionRuntime>,
    tickets: Vec<Ticket>,
    plans: HashMap<String, PlanCounts>,
}
impl World {
    /// A project with its coordinator, ready.
    fn new() -> Self {
        let mut world = Self::default();
        world.session(COORDINATOR, Role::ProjectOrchestrator, Status::Ready, None);
        world
    }
    fn session(&mut self, id: &str, role: Role, status: Status, ticket: Option<&str>) {
        self.sessions.push(Session {
            id: id.into(),
            project_id: PROJECT.into(),
            parent_id: (id != COORDINATOR).then(|| COORDINATOR.into()),
            repository_id: None,
            name: id.into(),
            role,
            provider: Provider::Claude,
            status,
            archived: false,
        });
        self.runtimes.push(SessionRuntime {
            session_id: id.into(),
            ticket_id: ticket.map(str::to_owned),
            ..Default::default()
        });
    }
    fn agent(mut self, id: &str, role: Role, status: Status) -> Self {
        self.session(id, role, status, Some(TICKET));
        self
    }
    fn implementer(self, status: Status) -> Self {
        self.agent("implementer", Role::Implementer, status)
    }
    fn plan(mut self, id: &str, done: u32, total: u32) -> Self {
        self.plans.insert(id.into(), counts(done, total));
        self
    }
    fn status(mut self, id: &str, status: Status) -> Self {
        self.sessions
            .iter_mut()
            .find(|s| s.id == id)
            .unwrap()
            .status = status;
        self
    }
    fn archived(mut self, id: &str) -> Self {
        self.sessions
            .iter_mut()
            .find(|s| s.id == id)
            .unwrap()
            .archived = true;
        self
    }
    fn ticket(self, state: &str) -> Self {
        self.ticket_named(TICKET, state, None)
    }
    fn verified(self, state: &str, cycle: Verification) -> Self {
        self.ticket_named(TICKET, state, Some(cycle))
    }
    fn ticket_named(mut self, id: &str, state: &str, cycle: Option<Verification>) -> Self {
        let mut ticket: Ticket = serde_json::from_value(json!({
            "id": id, "coordinator_id": COORDINATOR, "title": id, "brief": "",
            "worktree": "/w", "branch": "b", "state": state
        }))
        .unwrap();
        ticket.verification = cycle;
        self.tickets.push(ticket);
        self
    }
    fn board(&self) -> Board {
        derive(&self.sessions, &self.runtimes, &self.tickets, &self.plans)
    }
    fn agent_progress(&self, id: &str) -> AgentProgress {
        let board = self.board();
        board
            .agents
            .into_iter()
            .find(|a| a.session_id == id)
            .unwrap()
    }
    /// The ticket's stage, bar state and whole percentage.
    fn ticket_progress(&self) -> (Stage, BarState, u16) {
        let ticket = self.board().tickets.remove(0);
        (ticket.stage, ticket.state, percent(ticket.position))
    }
    fn project(&self) -> ProjectProgress {
        self.board().projects.remove(0)
    }
}

fn counts(done: u32, total: u32) -> PlanCounts {
    PlanCounts {
        done,
        total,
        current: (done < total).then(|| format!("Item {}", done + 1)),
    }
}

fn run(session: &str, result: VerifierResult) -> VerifierRun {
    VerifierRun {
        session_id: session.into(),
        role: Role::Reviewer,
        focus: session.into(),
        message_id: format!("verify:{session}"),
        result,
        report_id: None,
        reason: None,
    }
}

/// A cycle whose rounds each list their verifiers' results.
fn cycle(outcome: VerificationOutcome, rounds: &[&[(&str, VerifierResult)]]) -> Verification {
    let rounds = rounds
        .iter()
        .zip(1..)
        .map(|(verifiers, round)| VerificationRound {
            round,
            commit: "abc".into(),
            verifiers: verifiers
                .iter()
                .map(|(id, result)| run(id, *result))
                .collect(),
        });
    Verification {
        cycle: 1,
        level: None,
        max_rounds: 2,
        queued_at: None,
        small: false,
        keeper: false,
        skipped: vec![],
        outcome,
        rounds: rounds.collect(),
    }
}

use BarState::{Blocked, Finished, InProgress, Indeterminate};
use VerificationOutcome as Outcome;
use VerifierResult::{Failed, Passed, Pending};

// Agent progress

#[test]
fn checklist_count() {
    let mut world = World::new().ticket("assigned").implementer(Status::Working);
    world.plans.insert(
        "implementer".into(),
        PlanCounts {
            done: 3,
            total: 6,
            current: Some("Update the design README".into()),
        },
    );
    let agent = world.agent_progress("implementer");
    assert_eq!(
        (agent.state, agent.word, agent.working),
        (InProgress, None, true)
    );
    assert_eq!(agent.position().map(percent), Some(50));
    let plan = agent.plan.unwrap();
    assert_eq!((plan.done, plan.total), (3, 6));
    assert_eq!(plan.current.as_deref(), Some("Update the design README"));
}

#[test]
fn working_with_no_plan_is_indeterminate() {
    let world = World::new().ticket("assigned").implementer(Status::Working);
    let agent = world.agent_progress("implementer");
    assert_eq!(
        (agent.state, agent.word),
        (Indeterminate, Some(Word::Working))
    );
    assert_eq!(agent.position(), None);
}

#[test]
fn an_empty_plan_counts_as_no_plan() {
    let world = World::new()
        .ticket("assigned")
        .implementer(Status::Working)
        .plan("implementer", 0, 0);
    let agent = world.agent_progress("implementer");
    assert_eq!((agent.state, agent.plan), (Indeterminate, None));
}

#[test]
fn a_rewritten_plan_may_lower_the_figure() {
    let world = World::new().ticket("assigned").implementer(Status::Working);
    let before = world.plan("implementer", 2, 3);
    assert_eq!(
        before.agent_progress("implementer").position().map(percent),
        Some(67)
    );
    let after = before.plan("implementer", 2, 5);
    assert_eq!(
        after.agent_progress("implementer").position().map(percent),
        Some(40)
    );
}

#[test]
fn between_turns_the_checklist_stays_without_the_word_working() {
    let world = World::new()
        .ticket("assigned")
        .implementer(Status::Ready)
        .plan("implementer", 4, 6);
    let agent = world.agent_progress("implementer");
    assert_eq!(
        (agent.state, agent.word, agent.working),
        (InProgress, None, false)
    );
    assert_eq!(agent.position().map(percent), Some(67));
}

#[test]
fn an_idle_agent_with_no_plan_has_nothing_to_show() {
    for status in [Status::Ready, Status::Paused, Status::Disconnected] {
        let agent = World::new()
            .ticket("assigned")
            .implementer(status)
            .agent_progress("implementer");
        assert_eq!(
            (agent.state, agent.word, agent.position()),
            (InProgress, None, None)
        );
    }
}

#[test]
fn a_passed_verifier_is_finished() {
    let world = World::new()
        .verified(
            "verifying",
            cycle(Outcome::Running, &[&[("tester", Passed)]]),
        )
        .agent("tester", Role::Tester, Status::Done)
        .plan("tester", 3, 5);
    let agent = world.agent_progress("tester");
    assert_eq!((agent.state, agent.word), (Finished, Some(Word::Passed)));
    assert_eq!(agent.position().map(percent), Some(100));
}

#[test]
fn an_implementer_that_reported_ready_is_done() {
    let world = World::new()
        .ticket("ready_for_testing")
        .implementer(Status::Done)
        .plan("implementer", 5, 6);
    let agent = world.agent_progress("implementer");
    assert_eq!((agent.state, agent.word), (Finished, Some(Word::Done)));
    assert_eq!(agent.position().map(percent), Some(100));
}

#[test]
fn a_blocked_agent_keeps_its_position() {
    let world = World::new()
        .ticket("blocked")
        .implementer(Status::Blocked)
        .plan("implementer", 3, 7);
    let agent = world.agent_progress("implementer");
    assert_eq!((agent.state, agent.word), (Blocked, Some(Word::Blocked)));
    assert_eq!(agent.position().map(percent), Some(43));
}

#[test]
fn a_failed_verifier_is_blocked_with_the_word_failed() {
    let world = World::new()
        .verified(
            "failed",
            cycle(Outcome::Running, &[&[("reviewer", Failed)]]),
        )
        .agent("reviewer", Role::Reviewer, Status::Blocked)
        .plan("reviewer", 4, 5);
    let agent = world.agent_progress("reviewer");
    assert_eq!((agent.state, agent.word), (Blocked, Some(Word::Failed)));
    assert_eq!(agent.position().map(percent), Some(80));
}

#[test]
fn a_failed_verifier_starting_the_next_round_is_indeterminate() {
    let rounds: &[&[_]] = &[&[("reviewer", Failed)], &[("reviewer", Pending)]];
    let world = World::new()
        .verified("verifying", cycle(Outcome::Running, rounds))
        .agent("reviewer", Role::Reviewer, Status::Working);
    let agent = world.agent_progress("reviewer");
    assert_eq!(
        (agent.state, agent.word),
        (Indeterminate, Some(Word::Working))
    );
}

// Ticket progress

#[test]
fn ticket_progress_follows_state_status_and_plan() {
    for (state, status, plan, bar, position) in [
        ("assigned", Status::Working, Some((3, 6)), InProgress, 30),
        ("assigned", Status::Working, None, Indeterminate, 0),
        ("ready_for_testing", Status::Done, None, InProgress, 60),
        ("completed", Status::Done, None, InProgress, 60),
        ("blocked", Status::Blocked, Some((3, 7)), Blocked, 26),
    ] {
        let mut world = World::new().ticket(state).implementer(status);
        if let Some((done, total)) = plan {
            world = world.plan("implementer", done, total);
        }
        assert_eq!(
            world.ticket_progress(),
            (Stage::Implementing, bar, position),
            "{state}, {status:?}, plan {plan:?}"
        );
    }
}

#[test]
fn a_ready_ticket_stays_at_sixty_when_its_implementer_starts_another_run() {
    let world = World::new()
        .ticket("ready_for_testing")
        .implementer(Status::Working);
    assert_eq!(
        world.ticket_progress(),
        (Stage::Implementing, InProgress, 60)
    );
    assert_eq!(world.agent_progress("implementer").state, Indeterminate);
}

#[test]
fn verifying_counts_the_verifiers_that_passed() {
    let round: &[_] = &[("a", Passed), ("b", Pending), ("c", Pending)];
    let world = World::new()
        .verified("verifying", cycle(Outcome::Running, &[round]))
        .implementer(Status::Done);
    assert_eq!(world.ticket_progress(), (Stage::Verifying, InProgress, 70));
}

#[test]
fn a_failed_round_keeps_the_passes() {
    let first: &[_] = &[("a", Passed), ("b", Passed), ("c", Failed)];
    let fixing = World::new()
        .verified("failed", cycle(Outcome::Running, &[first]))
        .implementer(Status::Working);
    assert_eq!(fixing.ticket_progress(), (Stage::Verifying, InProgress, 80));
    // Round 2 runs only the verifier that failed.
    let second = World::new().verified(
        "verifying",
        cycle(Outcome::Running, &[first, &[("c", Pending)]]),
    );
    assert_eq!(second.ticket_progress(), (Stage::Verifying, InProgress, 80));
}

#[test]
fn a_passed_cycle_awaits_acceptance_at_ninety() {
    let round: &[_] = &[("a", Passed), ("b", Passed)];
    let world = World::new().verified("passed", cycle(Outcome::Passed, &[round]));
    assert_eq!(
        world.ticket_progress(),
        (Stage::AwaitingAcceptance, InProgress, 90)
    );
}

#[test]
fn another_report_kind_after_a_pass_still_awaits_acceptance() {
    let world = World::new()
        .verified("completed", cycle(Outcome::Passed, &[&[("a", Passed)]]))
        .implementer(Status::Done);
    assert_eq!(
        world.ticket_progress(),
        (Stage::AwaitingAcceptance, InProgress, 90)
    );
}

#[test]
fn a_passed_ticket_reassigned_for_a_fix_follows_the_fix_run() {
    let world = World::new()
        .verified("assigned", cycle(Outcome::Passed, &[&[("a", Passed)]]))
        .implementer(Status::Working);
    assert_eq!(
        world.ticket_progress(),
        (Stage::Implementing, Indeterminate, 0)
    );
    let planned = world.plan("implementer", 1, 2);
    assert_eq!(
        planned.ticket_progress(),
        (Stage::Implementing, InProgress, 30)
    );
}

#[test]
fn an_accepted_ticket_is_finished_and_lists_no_agents() {
    let world = World::new()
        .verified("accepted", cycle(Outcome::Passed, &[&[("a", Passed)]]))
        .implementer(Status::Done);
    assert_eq!(world.ticket_progress(), (Stage::Accepted, Finished, 100));
    assert!(world.board().tickets[0].agents.is_empty());
}

#[test]
fn blocked_wins_over_indeterminate() {
    // The coordinator answered with a message, so the state is still `blocked` while the
    // implementer's new run has no plan yet.
    let world = World::new().ticket("blocked").implementer(Status::Working);
    assert_eq!(world.ticket_progress(), (Stage::Implementing, Blocked, 0));
    let verified = World::new()
        .verified(
            "blocked",
            cycle(
                Outcome::Blocked,
                &[&[("a", Passed), ("b", Passed), ("c", Failed)]],
            ),
        )
        .implementer(Status::Working);
    assert_eq!(verified.ticket_progress(), (Stage::Verifying, Blocked, 80));
}

#[test]
fn blocked_at_the_round_cap_keeps_the_passes() {
    let round: &[_] = &[("a", Passed), ("b", Passed), ("c", Failed)];
    let world = World::new().verified("blocked", cycle(Outcome::Blocked, &[round]));
    assert_eq!(world.ticket_progress(), (Stage::Verifying, Blocked, 80));
}

#[test]
fn a_second_cycle_starts_again_at_sixty_and_never_exceeds_ninety() {
    let first: &[_] = &[("a", Passed), ("b", Passed), ("c", Failed)];
    let ready = World::new().verified("ready_for_testing", cycle(Outcome::Blocked, &[first]));
    assert_eq!(
        ready.ticket_progress(),
        (Stage::Implementing, InProgress, 60)
    );
    let pending: &[_] = &[("d", Pending), ("e", Pending), ("f", Pending)];
    let started = World::new().verified("verifying", cycle(Outcome::Running, &[pending]));
    assert_eq!(
        started.ticket_progress(),
        (Stage::Verifying, InProgress, 60)
    );
    let passed: &[_] = &[("d", Passed), ("e", Passed), ("f", Passed)];
    let done = World::new().verified("passed", cycle(Outcome::Passed, &[passed]));
    assert_eq!(
        done.ticket_progress(),
        (Stage::AwaitingAcceptance, InProgress, 90)
    );
}

#[test]
fn fixes_after_a_pass_return_to_sixty_when_ready() {
    let world = World::new()
        .verified(
            "ready_for_testing",
            cycle(Outcome::Passed, &[&[("a", Passed)]]),
        )
        .implementer(Status::Done);
    assert_eq!(
        world.ticket_progress(),
        (Stage::Implementing, InProgress, 60)
    );
}

// The agents a ticket lists

#[test]
fn an_archived_passed_verifier_stays_listed() {
    let rounds: &[&[_]] = &[
        &[("tester", Passed), ("reviewer", Failed)],
        &[("reviewer", Pending)],
    ];
    let world = World::new()
        .verified("verifying", cycle(Outcome::Running, rounds))
        .implementer(Status::Done)
        .agent("tester", Role::Tester, Status::Done)
        .agent("reviewer", Role::Reviewer, Status::Working)
        .archived("tester");
    let board = world.board();
    assert_eq!(
        board.tickets[0].agents,
        ["implementer", "tester", "reviewer"]
    );
    let tester = world.agent_progress("tester");
    assert_eq!((tester.state, tester.word), (Finished, Some(Word::Passed)));
    assert_eq!(world.agent_progress("reviewer").state, Indeterminate);
}

#[test]
fn an_earlier_cycles_archived_verifiers_are_not_listed() {
    let mut world = World::new()
        .verified("verifying", cycle(Outcome::Running, &[&[("new", Pending)]]))
        .agent("old", Role::Reviewer, Status::Done)
        .agent("new", Role::Reviewer, Status::Working)
        .archived("old");
    world.tickets[0].previous_cycles = vec![cycle(Outcome::Blocked, &[&[("old", Failed)]])];
    let board = world.board();
    assert_eq!(board.tickets[0].agents, ["new"]);
    assert!(board.agents.iter().all(|agent| agent.session_id != "old"));
}

// Project progress

/// Tickets at 100%, 70%, 30% and blocked at 24%.
fn four_tickets() -> World {
    let verifying: &[_] = &[("a", Passed), ("b", Pending), ("c", Pending)];
    let mut world = World::new()
        .ticket_named("one", "accepted", None)
        .ticket_named(
            "two",
            "verifying",
            Some(cycle(Outcome::Running, &[verifying])),
        )
        .ticket_named("three", "assigned", None)
        .ticket_named("four", "blocked", None);
    world.session(
        "three-impl",
        Role::Implementer,
        Status::Working,
        Some("three"),
    );
    world.session(
        "four-impl",
        Role::Implementer,
        Status::Blocked,
        Some("four"),
    );
    world.plan("three-impl", 1, 2).plan("four-impl", 2, 5)
}

#[test]
fn a_project_averages_its_tickets() {
    let world = four_tickets();
    let project = world.project();
    assert_eq!(
        (project.phase, project.state),
        (Phase::Implementing, InProgress)
    );
    assert_eq!(
        (project.position.map(percent), project.accepted),
        (Some(56), 1)
    );
    assert_eq!(project.tickets, ["one", "two", "three", "four"]);
    let states: Vec<_> = world.board().tickets.iter().map(|t| t.state).collect();
    assert_eq!(states, [Finished, InProgress, InProgress, Blocked]);
}

#[test]
fn a_project_without_tickets_is_planning_on_its_coordinators_checklist() {
    let world = World::new()
        .status(COORDINATOR, Status::Working)
        .plan(COORDINATOR, 1, 4);
    let project = world.project();
    assert_eq!(
        (project.phase, project.state),
        (Phase::Planning, InProgress)
    );
    assert_eq!(
        (
            project.position.map(percent),
            project.coordinator_id.as_str()
        ),
        (Some(25), COORDINATOR)
    );
}

#[test]
fn a_project_without_tickets_or_a_plan_is_indeterminate() {
    let project = World::new().status(COORDINATOR, Status::Working).project();
    assert_eq!(
        (project.phase, project.state, project.position),
        (Phase::Planning, Indeterminate, None)
    );
    // An idle coordinator with no checklist: an empty track, and still no percentage.
    let idle = World::new().project();
    assert_eq!((idle.state, idle.position), (InProgress, None));
}

#[test]
fn a_closed_ticket_is_left_out() {
    let mut world = World::new()
        .ticket_named("closed", "closed", None)
        .ticket_named("done", "accepted", None)
        .ticket_named("half", "assigned", None);
    world.session(
        "half-impl",
        Role::Implementer,
        Status::Working,
        Some("half"),
    );
    let project = world.plan("half-impl", 5, 6).project();
    assert_eq!(
        (project.position.map(percent), project.accepted),
        (Some(75), 1)
    );
    assert_eq!(project.tickets, ["done", "half"]);
}

#[test]
fn a_project_whose_every_ticket_is_closed_is_planning_again() {
    let project = World::new().ticket("closed").project();
    assert_eq!(project.phase, Phase::Planning);
}

#[test]
fn a_project_is_blocked_when_everything_left_is_blocked() {
    let project = World::new()
        .ticket_named("one", "accepted", None)
        .ticket_named("two", "blocked", None)
        .ticket_named("three", "blocked", None)
        .project();
    assert_eq!((project.phase, project.state), (Phase::Blocked, Blocked));
}

#[test]
fn a_project_with_every_ticket_accepted_is_done() {
    let project = World::new()
        .ticket_named("one", "accepted", None)
        .ticket_named("two", "accepted", None)
        .project();
    assert_eq!((project.phase, project.state), (Phase::Done, Finished));
    assert_eq!(
        (project.position.map(percent), project.accepted),
        (Some(100), 2)
    );
}

#[test]
fn the_phase_is_the_least_advanced_stage_still_moving() {
    let passed = cycle(Outcome::Passed, &[&[("a", Passed)]]);
    let running = cycle(Outcome::Running, &[&[("b", Pending)]]);
    let project = World::new()
        .ticket_named("one", "passed", Some(passed))
        .ticket_named("two", "verifying", Some(running))
        .ticket_named("three", "blocked", None)
        .project();
    assert_eq!(project.phase, Phase::Verifying);
}

#[test]
fn a_ticket_counts_only_toward_its_own_project() {
    let mut world = World::new().ticket("accepted");
    world.session("other", Role::ProjectOrchestrator, Status::Ready, None);
    let other = world.sessions.last_mut().unwrap();
    (other.project_id, other.parent_id) = ("elsewhere".into(), None);
    let board = world.board();
    assert_eq!(board.projects[0].tickets, [TICKET]);
    assert_eq!(board.projects[1].phase, Phase::Planning);
    assert!(board.projects[1].tickets.is_empty());
}

// Wire format

#[test]
fn the_board_round_trips_through_json() {
    let board = four_tickets().board();
    let value = serde_json::to_value(&board).unwrap();
    assert_eq!(value["tickets"][3]["state"], "blocked");
    assert_eq!(value["projects"][0]["phase"], "implementing");
    assert_eq!(serde_json::from_value::<Board>(value).unwrap(), board);
    let page = ProgressPage {
        revision: 7,
        complete: true,
        agents: board.agents,
        tickets: board.tickets,
        projects: board.projects,
    };
    let decoded: ProgressPage =
        serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
    assert_eq!(decoded, page);
}

#[test]
fn a_step_stored_without_plan_counts_still_decodes() {
    let stored = json!({
        "run_id": "run", "id": "plan", "kind": "plan", "state": "succeeded",
        "title": "Plan · 1 of 3 done", "seq": 0, "revision": 1,
        "started_at": 0, "finished_at": null
    });
    let mut step: Step = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(step.plan, None);
    // A step without counts is written back unchanged.
    assert!(serde_json::to_value(&step).unwrap().get("plan").is_none());
    step.plan = Some(counts(1, 3));
    let value = serde_json::to_value(&step).unwrap();
    assert_eq!(
        value["plan"],
        json!({"done": 1, "total": 3, "current": "Item 2"})
    );
    assert_eq!(serde_json::from_value::<Step>(value).unwrap(), step);
}

#[test]
fn an_edit_stored_before_diffs_still_decodes_and_round_trips() {
    let stored = json!({
        "run_id": "run", "id": "edit", "kind": "file_edit", "state": "succeeded",
        "title": "src/a.rs", "note": "+1 −1", "paths": ["src/a.rs"], "seq": 0, "revision": 1,
        "started_at": 0, "finished_at": null
    });
    let mut step: Step = serde_json::from_value(stored).unwrap();
    assert!(step.diffs.is_empty());
    assert!(serde_json::to_value(&step).unwrap().get("diffs").is_none());
    step.diffs = vec![FileDiff {
        path: "src/a.rs".into(),
        created: false,
        added: 1,
        removed: 1,
        hunks: vec![DiffHunk {
            old_start: 4,
            new_start: 4,
            lines: vec!["-a".into(), "+b".into()],
        }],
        omitted_lines: 0,
    }];
    let value = serde_json::to_value(&step).unwrap();
    assert_eq!(serde_json::from_value::<Step>(value).unwrap(), step);
}
