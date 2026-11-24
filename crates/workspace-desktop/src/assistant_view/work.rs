//! The Assistant's work and what waits for the human, as the chat's cards, the rail and the
//! sidebar's assistant row read them (OpenSpec change `assistant-does-the-work`, tasks 3.2–3.4):
//! each work item's stage and pull requests, Needs you, Running and Done recently.
use workspace_core::assistant::conversation::{CardKind, ConversationCard, ConversationState};
use workspace_core::job::{JobState, RunOutcome};
use workspace_core::{
    AssistantWork, Project, PullRequest, Role, Session, Snapshot, Status, Ticket, TicketState,
    WorkState, WorkspaceIndex,
};

/// How long finished or stopped work stays under Done recently.
const DONE_RECENTLY_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// How long the Assistant's turn runs before the chat and its row say so, and how often after.
pub(crate) const LONG_TURN_MINUTES: i64 = 30;

/// Where a work item has got to, as its card's steps show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// No ticket yet: the coordinator is planning.
    Planned,
    /// Building the `at`th of `of` tickets.
    Building {
        at: usize,
        of: usize,
    },
    Review,
    /// Every ticket passed review: its pull requests, open or merged.
    PullRequests,
}
impl Stage {
    /// The card's steps, each with whether it is done and whether it is the stage now.
    pub(crate) fn steps(self) -> [(String, StepMark); 4] {
        let at = match self {
            Self::Planned => 0,
            Self::Building { .. } => 1,
            Self::Review => 2,
            Self::PullRequests => 3,
        };
        let building = match self {
            Self::Building { at, of } => format!("Building {at} of {of}"),
            _ => "Building".into(),
        };
        let mark = |step: usize| match step.cmp(&at) {
            std::cmp::Ordering::Less => StepMark::Done,
            std::cmp::Ordering::Equal => StepMark::Now,
            std::cmp::Ordering::Greater => StepMark::Later,
        };
        [
            ("Planned".into(), mark(0)),
            (building, mark(1)),
            ("Review".into(), mark(2)),
            ("PRs".into(), mark(3)),
        ]
    }
    /// The stage in a few words, for the rail.
    pub(crate) fn label(self) -> String {
        match self {
            Self::Planned => "Planning".into(),
            Self::Building { at, of } => format!("Building {at} of {of}"),
            Self::Review => "In review".into(),
            Self::PullRequests => "PRs open".into(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StepMark {
    Done,
    Now,
    Later,
}

/// The stage of work whose tickets are `tickets`, closed ones left out.
pub(crate) fn stage<'a>(tickets: impl IntoIterator<Item = &'a Ticket>) -> Stage {
    let states: Vec<TicketState> = (tickets.into_iter())
        .map(|ticket| ticket.state)
        .filter(|state| *state != TicketState::Closed)
        .collect();
    let building = |state: &TicketState| {
        matches!(
            state,
            TicketState::Planned
                | TicketState::Assigned
                | TicketState::Failed
                | TicketState::Blocked
        )
    };
    let reviewing = |state: &TicketState| {
        matches!(state, TicketState::ReadyForTesting | TicketState::Verifying)
    };
    let of = states.len();
    if of == 0 {
        return Stage::Planned;
    }
    let built = of - states.iter().filter(|s| building(s)).count();
    if built < of {
        return Stage::Building { at: built + 1, of };
    }
    match states.iter().any(reviewing) {
        true => Stage::Review,
        false => Stage::PullRequests,
    }
}

/// One work item of the Assistant: its project and what its card and the rail show.
pub(crate) struct WorkItem<'a> {
    pub(crate) project: &'a Project,
    pub(crate) work: &'a AssistantWork,
    pub(crate) coordinator: Option<&'a Session>,
    pub(crate) stage: Stage,
    pub(crate) pull_requests: Vec<&'a PullRequest>,
    /// Any of its agents is in a turn.
    pub(crate) working: bool,
}
impl WorkItem<'_> {
    pub(crate) fn running(&self) -> bool {
        matches!(self.work.state, WorkState::Running | WorkState::Paused)
    }
    /// The rail's second line: its stage while it runs, else how it ended.
    pub(crate) fn state_line(&self) -> String {
        match self.work.state {
            WorkState::Running => self.stage.label(),
            WorkState::Paused => "Paused".into(),
            WorkState::Stopped => "Stopped".into(),
            WorkState::Done => match self.pull_requests.len() {
                0 => "Done".into(),
                _ => pull_request_line(&self.pull_requests),
            },
        }
    }
}

/// "PR #88 open", or "PRs #88, #91" for several.
pub(crate) fn pull_request_line(pulls: &[&PullRequest]) -> String {
    use workspace_core::PrState;
    match pulls {
        [] => String::new(),
        [pull] => {
            let state = match pull.state {
                PrState::Open => "open",
                PrState::Merged => "merged",
                PrState::Closed => "closed",
            };
            format!("PR #{} {state}", pull.number)
        }
        pulls => {
            let numbers: Vec<String> = pulls.iter().map(|p| format!("#{}", p.number)).collect();
            format!("PRs {}", numbers.join(", "))
        }
    }
}

/// Every work item of the Assistant, oldest first.
pub(crate) fn work_items(index: &WorkspaceIndex) -> Vec<WorkItem<'_>> {
    (index.snapshot().projects.iter())
        .filter_map(|project| {
            let work = project.assistant_work.as_ref()?;
            let coordinator = (index.roots(&project.id))
                .find(|session| session.role == Role::ProjectOrchestrator);
            let tickets: Vec<&Ticket> = coordinator
                .map(|c| index.tickets_owned_by(&c.id).collect())
                .unwrap_or_default();
            let working = (index.sessions())
                .any(|s| s.project_id == project.id && s.status == Status::Working);
            Some(WorkItem {
                project,
                work,
                coordinator,
                stage: stage(tickets.iter().copied()),
                pull_requests: (tickets.iter())
                    .filter_map(|ticket| ticket.pull_request.as_ref())
                    .collect(),
                working,
            })
        })
        .collect()
}

/// Work finished or stopped within the last 7 days, newest first.
pub(crate) fn done_recently<'a>(items: &'a [WorkItem<'a>], now: i64) -> Vec<&'a WorkItem<'a>> {
    let mut done: Vec<&WorkItem> = (items.iter())
        .filter(|item| matches!(item.work.state, WorkState::Done | WorkState::Stopped))
        .filter(|item| {
            item.work
                .ended_at
                .is_some_and(|at| now - at < DONE_RECENTLY_MS)
        })
        .collect();
    done.sort_by_key(|item| std::cmp::Reverse(item.work.ended_at));
    done
}

/// Something under Needs you.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Need<'a> {
    /// An open decision, a waiting action or a loop change waiting for Apply.
    Card(&'a ConversationCard),
    /// An approval card whose Inbox item is still open.
    Approval(&'a ConversationCard),
    /// A loop with runs that failed, did not start or wait for approval, unseen.
    Loop(&'a JobState),
}

/// What the rail lists under Needs you, in the conversation's order, then the loops.
pub(crate) fn needs_you(snapshot: &Snapshot) -> Vec<Need<'_>> {
    let cards = snapshot.conversation.iter().flat_map(|c| &c.cards);
    let mut needs: Vec<Need> = cards
        .filter_map(|card| match &card.kind {
            CardKind::Approval { attention_id, .. } => (snapshot.attention.iter())
                .any(|a| &a.id == attention_id)
                .then_some(Need::Approval(card)),
            _ => card.needs_human().then_some(Need::Card(card)),
        })
        .collect();
    let loops = snapshot.jobs.iter().filter(|job| job.attention > 0);
    needs.extend(loops.map(Need::Loop));
    needs
}

/// Whether the assistant row shows its working mark: the Assistant, any of its work, an issue
/// thread or a loop run is in a turn.
pub(crate) fn assistant_working(index: &WorkspaceIndex) -> bool {
    let snapshot = index.snapshot();
    let conversation =
        (snapshot.conversation.as_ref()).is_some_and(|c| c.status == Status::Working);
    let threads = super::any_working(&snapshot.threads);
    let runs = (snapshot.jobs.iter())
        .flat_map(|job| &job.runs)
        .any(|run| run.outcome == RunOutcome::Running);
    conversation || threads || runs || work_items(index).iter().any(|item| item.working)
}

/// The minutes the conversation's turn has run, once that is at least 30 and at each 30 after:
/// 30, 60, 90 and so on.
pub(crate) fn long_turn_minutes(conversation: &ConversationState, now: i64) -> Option<i64> {
    let started = conversation.turn_started_at?;
    let minutes = (now - started) / 60_000;
    (minutes >= LONG_TURN_MINUTES).then_some(minutes / LONG_TURN_MINUTES * LONG_TURN_MINUTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_core::assistant::conversation::{ActCall, ActionCard, ActionState, Decision};
    use workspace_core::{SpaceId, Status};

    fn ticket(state: TicketState) -> Ticket {
        Ticket {
            state,
            ..serde_json::from_value(serde_json::json!({
                "id": "t", "project_id": "p", "coordinator_id": "c", "repository_id": "r",
                "title": "T", "brief": "", "worktree": "", "branch": "", "state": "planned",
            }))
            .unwrap()
        }
    }

    #[test]
    fn a_works_stage_follows_its_tickets() {
        use TicketState::*;
        let stage_of = |states: &[TicketState]| {
            let tickets: Vec<Ticket> = states.iter().map(|s| ticket(*s)).collect();
            stage(&tickets)
        };
        assert_eq!(stage_of(&[]), Stage::Planned);
        // The theme is built, nvim is building and Chrome waits: building the second of three.
        assert_eq!(
            stage_of(&[Accepted, Assigned, Planned]),
            Stage::Building { at: 2, of: 3 }
        );
        assert_eq!(stage_of(&[Passed, Verifying]), Stage::Review);
        assert_eq!(stage_of(&[Passed, Accepted, Closed]), Stage::PullRequests);
        assert_eq!(stage_of(&[Closed]), Stage::Planned);
        let steps = Stage::Building { at: 2, of: 3 }.steps();
        assert_eq!(steps[1].0, "Building 2 of 3");
        assert_eq!(
            steps.map(|(_, mark)| mark),
            [
                StepMark::Done,
                StepMark::Now,
                StepMark::Later,
                StepMark::Later
            ]
        );
    }

    fn card(id: &str, kind: CardKind) -> ConversationCard {
        ConversationCard {
            id: id.into(),
            created_at: 0,
            read: vec![],
            kind,
        }
    }

    fn conversation(cards: Vec<ConversationCard>) -> ConversationState {
        ConversationState {
            session_id: "assistant".into(),
            status: Status::Ready,
            turn_started_at: None,
            message_revision: 0,
            profile: None,
            cards,
            routes: vec![],
        }
    }

    #[test]
    fn needs_you_lists_open_decisions_waiting_actions_open_approvals_and_failed_loops() {
        let decision = |answer: Option<&str>| {
            CardKind::Decision(Decision {
                project_id: Some("p".into()),
                question: "Which shade?".into(),
                recommendation: "The lighter one".into(),
                options: vec!["Lighter".into(), "Warmer".into()],
                answer: answer.map(str::to_owned),
            })
        };
        let action = |state| {
            CardKind::Action(ActionCard {
                space_id: SpaceId("personal".into()),
                source: "gmail".into(),
                call: ActCall::Cli {
                    command: "gws gmail send".into(),
                },
                state,
                without_asking: false,
            })
        };
        let approval = |attention: &str| CardKind::Approval {
            project_id: "p".into(),
            attention_id: attention.into(),
        };
        let mut snapshot = Snapshot {
            conversation: Some(conversation(vec![
                card("open", decision(None)),
                card("answered", decision(Some("Lighter"))),
                card("email", action(ActionState::Waiting)),
                card(
                    "sent",
                    action(ActionState::Done {
                        result: "ok".into(),
                    }),
                ),
                card("merge", approval("a1")),
                card("settled", approval("gone")),
            ])),
            ..Default::default()
        };
        snapshot.attention.push(
            serde_json::from_value(serde_json::json!({
                "id": "a1", "project_id": "p", "session_id": "c", "host": "", "operation_id": "o",
                "prompt": "Merge after a blocked review?", "options": ["Approve", "Decline"],
            }))
            .unwrap(),
        );
        let ids: Vec<&str> = needs_you(&snapshot)
            .iter()
            .map(|need| match need {
                Need::Card(card) | Need::Approval(card) => card.id.as_str(),
                Need::Loop(job) => job.recipe.name.as_str(),
            })
            .collect();
        assert_eq!(ids, ["open", "email", "merge"]);
    }

    fn work(state: WorkState, ended_at: Option<i64>) -> AssistantWork {
        AssistantWork {
            small: false,
            state,
            ended_at,
        }
    }

    #[test]
    fn a_work_item_reads_its_coordinators_tickets_and_marks_the_row_working() {
        let (_directory, host, root, _) = crate::test_support::ticket_store();
        let mut snapshot = host.snapshot().unwrap();
        snapshot.projects[0].assistant_work = Some(work(WorkState::Running, None));
        // A project the human started is not the Assistant's work.
        let mut theirs = snapshot.projects[0].clone();
        (theirs.id, theirs.assistant_work) = ("theirs".into(), None);
        snapshot.projects.push(theirs);
        let index = WorkspaceIndex::new(snapshot.clone(), Default::default());
        let items = work_items(&index);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].coordinator.map(|c| &c.id), Some(&root.id));
        assert_eq!(items[0].stage, Stage::Building { at: 1, of: 1 });
        assert!(!items[0].working);
        assert!(!assistant_working(&index));

        let coordinator = (snapshot.sessions.iter_mut()).find(|s| s.id == root.id);
        coordinator.unwrap().status = Status::Working;
        let index = WorkspaceIndex::new(snapshot, Default::default());
        assert!(work_items(&index)[0].working);
        assert!(assistant_working(&index));
    }

    #[test]
    fn the_assistant_row_works_while_its_conversation_or_a_loop_run_does() {
        let idle = Snapshot {
            conversation: Some(conversation(vec![])),
            ..Default::default()
        };
        assert!(!assistant_working(&WorkspaceIndex::new(
            idle.clone(),
            Default::default()
        )));
        let mut turn = idle.clone();
        turn.conversation.as_mut().unwrap().status = Status::Working;
        assert!(assistant_working(&WorkspaceIndex::new(
            turn,
            Default::default()
        )));
        let mut looping = idle;
        let job: JobState = serde_json::from_value(serde_json::json!({
            "recipe": {
                "id": "brief", "name": "Morning brief", "space_id": "personal", "prompt": "",
                "schedule": {"cron": "0 8 * * 1-5", "timezone": "America/Chicago"},
                "enabled": true,
            },
            "schedule": "Weekdays at 08:00 (America/Chicago)",
            "model": "Sonnet",
            "runs": [{"id": "r", "job": "personal/brief", "started_at": 0, "outcome": "running"}],
        }))
        .unwrap();
        looping.jobs.push(job);
        assert!(assistant_working(&WorkspaceIndex::new(
            looping,
            Default::default()
        )));
    }

    #[test]
    fn done_recently_lists_work_ended_in_the_last_7_days_newest_first() {
        let day = 24 * 60 * 60 * 1000;
        let now = 30 * day;
        let mut snapshot = Snapshot::default();
        for (id, state, ended_at) in [
            ("running", WorkState::Running, None),
            ("done-long-ago", WorkState::Done, Some(now - 8 * day)),
            ("stopped", WorkState::Stopped, Some(now - 2 * day)),
            ("done", WorkState::Done, Some(now - day)),
            ("paused", WorkState::Paused, Some(now - day)),
        ] {
            let project: Project = serde_json::from_value(serde_json::json!({
                "id": id, "name": id,
            }))
            .unwrap();
            snapshot.projects.push(Project {
                assistant_work: Some(work(state, ended_at)),
                ..project
            });
        }
        let index = WorkspaceIndex::new(snapshot, Default::default());
        let items = work_items(&index);
        let done: Vec<&str> = (done_recently(&items, now).iter())
            .map(|item| item.project.id.as_str())
            .collect();
        assert_eq!(done, ["done", "stopped"]);
    }

    #[test]
    fn a_long_turn_is_noted_at_30_minutes_and_each_30_after() {
        let minute = 60_000;
        let mut state = conversation(vec![]);
        assert_eq!(long_turn_minutes(&state, 100 * minute), None);
        state.turn_started_at = Some(0);
        assert_eq!(long_turn_minutes(&state, 29 * minute), None);
        assert_eq!(long_turn_minutes(&state, 30 * minute), Some(30));
        assert_eq!(long_turn_minutes(&state, 59 * minute), Some(30));
        assert_eq!(long_turn_minutes(&state, 61 * minute), Some(60));
    }
}
