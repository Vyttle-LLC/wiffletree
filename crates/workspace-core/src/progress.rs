//! How far along each agent, ticket and project is, derived from records the host already
//! keeps: plan steps, session status, ticket state and the verification record. Nothing here
//! is stored or reported by an agent.
use crate::{
    PlanCounts, Role, Session, SessionRuntime, Status, Ticket, VerificationOutcome, VerifierResult,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A full bar. Positions are hundredths of a percent, so averages need no floats.
pub const FULL_POSITION: u16 = 10_000;
/// An implemented ticket that verification has not moved yet.
const IMPLEMENTED: u16 = 6_000;
/// A ticket whose every verifier passed, waiting for acceptance.
const VERIFIED: u16 = 9_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarState {
    /// Also an idle bar: a position, or an empty track, in the focus color.
    InProgress,
    /// Working with nothing to count: stripes and no percentage.
    Indeterminate,
    Blocked,
    Finished,
}

/// A ticket's stage, least advanced first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Implementing,
    Verifying,
    AwaitingAcceptance,
    Accepted,
}
impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Implementing => "Implementing",
            Self::Verifying => "Verifying",
            Self::AwaitingAcceptance => "Awaiting acceptance",
            Self::Accepted => "Accepted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Planning,
    Implementing,
    Verifying,
    AwaitingAcceptance,
    Blocked,
    Done,
}
impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Planning => "Planning",
            Self::Implementing => "Implementing",
            Self::Verifying => "Verifying",
            Self::AwaitingAcceptance => "Awaiting acceptance",
            Self::Blocked => "Blocked",
            Self::Done => "Done",
        }
    }
}
impl From<Stage> for Phase {
    fn from(stage: Stage) -> Self {
        match stage {
            Stage::Implementing => Self::Implementing,
            Stage::Verifying => Self::Verifying,
            Stage::AwaitingAcceptance => Self::AwaitingAcceptance,
            Stage::Accepted => Self::Done,
        }
    }
}

/// What an agent's figure says when it is not a checklist count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Word {
    Working,
    Done,
    Passed,
    Failed,
    Blocked,
}
impl Word {
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Done => "done",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProgress {
    pub session_id: String,
    pub state: BarState,
    /// The checklist of the agent's latest run; `None` when that run wrote no plan.
    pub plan: Option<PlanCounts>,
    pub word: Option<Word>,
    /// A turn is running now, which tells a working agent's checklist from an idle one's.
    pub working: bool,
    pub run_started_at: Option<i64>,
    pub run_finished_at: Option<i64>,
}
impl AgentProgress {
    /// Where the bar is filled to; `None` when the rules define no percentage.
    pub fn position(&self) -> Option<u16> {
        if self.state == BarState::Finished {
            return Some(FULL_POSITION);
        }
        self.plan
            .as_ref()
            .map(|plan| share(FULL_POSITION, plan.done, plan.total))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketProgress {
    pub ticket_id: String,
    pub stage: Stage,
    pub state: BarState,
    pub position: u16,
    /// The agents the crew panel lists, in order; none once the ticket is accepted.
    pub agents: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectProgress {
    pub project_id: String,
    pub phase: Phase,
    pub state: BarState,
    /// `None` while the rules define no percentage: planning with no checklist to count.
    pub position: Option<u16>,
    pub accepted: u32,
    /// Counted tickets in creation order; one bar segment each.
    pub tickets: Vec<String>,
    /// The project coordinator, whose checklist is the bar while `tickets` is empty.
    pub coordinator_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub agents: Vec<AgentProgress>,
    pub tickets: Vec<TicketProgress>,
    pub projects: Vec<ProjectProgress>,
}

/// The board entries changed after a cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressPage {
    /// The board's latest revision; pass it back as `after` to receive only newer changes.
    pub revision: u64,
    /// The page is the whole board, because the cursor was zero or an entry has been removed
    /// since it; the client replaces what it holds.
    #[serde(default)]
    pub complete: bool,
    pub agents: Vec<AgentProgress>,
    pub tickets: Vec<TicketProgress>,
    pub projects: Vec<ProjectProgress>,
}

/// A position as the whole percentage shown beside a bar.
pub fn percent(position: u16) -> u16 {
    (position + 50) / 100
}

/// `span × part ÷ whole`, rounded; zero when there is nothing to divide by.
fn share(span: u16, part: u32, whole: u32) -> u16 {
    if whole == 0 {
        return 0;
    }
    let (span, part, whole) = (
        u64::from(span),
        u64::from(part.min(whole)),
        u64::from(whole),
    );
    ((span * part + whole / 2) / whole) as u16
}

/// The whole board. `plans` holds the latest plan of each session's latest run. `tickets` are
/// in creation order.
pub fn derive(
    sessions: &[Session],
    runtimes: &[SessionRuntime],
    tickets: &[Ticket],
    plans: &HashMap<String, PlanCounts>,
) -> Board {
    let records = Records {
        sessions,
        runtimes: runtimes
            .iter()
            .map(|r| (r.session_id.as_str(), r))
            .collect(),
        tickets,
        plans,
    };
    let tickets: Vec<_> = tickets
        .iter()
        .filter(|ticket| ticket.state != "closed")
        .map(|ticket| records.ticket(ticket))
        .collect();
    let crew: HashSet<&str> = tickets
        .iter()
        .flat_map(|ticket| ticket.agents.iter().map(String::as_str))
        .collect();
    let agents = sessions
        .iter()
        .filter(|session| !session.archived || crew.contains(session.id.as_str()))
        .map(|session| records.agent(session))
        .collect();
    let projects = sessions
        .iter()
        .filter(|session| session.role == Role::ProjectOrchestrator && !session.archived)
        .map(|coordinator| records.project(coordinator, &tickets))
        .collect();
    Board {
        agents,
        tickets,
        projects,
    }
}

struct Records<'a> {
    sessions: &'a [Session],
    runtimes: HashMap<&'a str, &'a SessionRuntime>,
    tickets: &'a [Ticket],
    plans: &'a HashMap<String, PlanCounts>,
}
impl Records<'_> {
    fn agent(&self, session: &Session) -> AgentProgress {
        let runtime = self.runtimes.get(session.id.as_str());
        let result = self.verifier_result(session);
        let working = session.status == Status::Working;
        let plan = self.plans.get(&session.id).filter(|plan| plan.total > 0);
        let (state, word) = if result == Some(VerifierResult::Passed) {
            (BarState::Finished, Some(Word::Passed))
        } else if session.status == Status::Done {
            (BarState::Finished, Some(Word::Done))
        } else if session.status == Status::Blocked {
            let failed = result == Some(VerifierResult::Failed);
            let word = if failed { Word::Failed } else { Word::Blocked };
            (BarState::Blocked, Some(word))
        } else if working && plan.is_none() {
            (BarState::Indeterminate, Some(Word::Working))
        } else {
            (BarState::InProgress, None)
        };
        AgentProgress {
            session_id: session.id.clone(),
            state,
            plan: plan.cloned(),
            word,
            working,
            run_started_at: runtime.and_then(|r| r.last_started_at),
            run_finished_at: runtime.and_then(|r| r.last_finished_at),
        }
    }

    fn ticket_id(&self, session: &Session) -> Option<&str> {
        self.runtimes.get(session.id.as_str())?.ticket_id.as_deref()
    }

    /// The session's latest result in its ticket's latest cycle, when it verifies there.
    fn verifier_result(&self, session: &Session) -> Option<VerifierResult> {
        let ticket_id = self.ticket_id(session)?;
        let ticket = self.tickets.iter().find(|t| t.id == ticket_id)?;
        let results = ticket.verification.as_ref()?.latest_results();
        let run = results.iter().find(|run| run.session_id == session.id)?;
        Some(run.result)
    }

    fn ticket(&self, ticket: &Ticket) -> TicketProgress {
        use BarState::{Finished, InProgress};
        let (stage, state, position) = match (ticket.state.as_str(), &ticket.verification) {
            ("accepted", _) => (Stage::Accepted, Finished, FULL_POSITION),
            ("ready_for_testing", _) => (Stage::Implementing, InProgress, IMPLEMENTED),
            ("assigned", _) | (_, None) => {
                let (state, position) = self.implementing(ticket);
                (Stage::Implementing, state, position)
            }
            (_, Some(cycle)) if cycle.outcome == VerificationOutcome::Passed => {
                (Stage::AwaitingAcceptance, InProgress, VERIFIED)
            }
            (_, Some(cycle)) => {
                let results = cycle.latest_results();
                let passed = results
                    .iter()
                    .filter(|run| run.result == VerifierResult::Passed)
                    .count();
                let earned = share(VERIFIED - IMPLEMENTED, passed as u32, results.len() as u32);
                (Stage::Verifying, InProgress, IMPLEMENTED + earned)
            }
        };
        // Blocked wins over stripes: a bar is in exactly one state.
        let state = if ticket.state == "blocked" {
            BarState::Blocked
        } else {
            state
        };
        TicketProgress {
            ticket_id: ticket.id.clone(),
            stage,
            state,
            position,
            agents: self.crew(ticket, stage),
        }
    }

    /// The Implementing band: 0–60% by the implementer's checklist, or stripes while it works
    /// with nothing to count.
    fn implementing(&self, ticket: &Ticket) -> (BarState, u16) {
        let implementer = self.sessions.iter().find(|session| {
            session.role == Role::Implementer
                && !session.archived
                && self.ticket_id(session) == Some(&ticket.id)
        });
        let Some(agent) = implementer.map(|session| self.agent(session)) else {
            return (BarState::InProgress, 0);
        };
        if agent.state == BarState::Indeterminate {
            return (BarState::Indeterminate, 0);
        }
        let fraction = agent.position().unwrap_or(0);
        let position = share(IMPLEMENTED, fraction.into(), FULL_POSITION.into());
        (BarState::InProgress, position)
    }

    /// An open ticket's unarchived agents and its latest cycle's verifiers, each once.
    fn crew(&self, ticket: &Ticket, stage: Stage) -> Vec<String> {
        if stage == Stage::Accepted {
            return vec![];
        }
        let verifiers: HashSet<&str> = ticket
            .verification
            .iter()
            .flat_map(|cycle| cycle.rounds.iter().flat_map(|round| &round.verifiers))
            .map(|run| run.session_id.as_str())
            .collect();
        self.sessions
            .iter()
            .filter(|session| {
                verifiers.contains(session.id.as_str())
                    || (!session.archived && self.ticket_id(session) == Some(&ticket.id))
            })
            .map(|session| session.id.clone())
            .collect()
    }

    /// `counted` are the tickets already derived, which leaves out closed ones.
    fn project(&self, coordinator: &Session, counted: &[TicketProgress]) -> ProjectProgress {
        let tickets: Vec<&TicketProgress> = counted
            .iter()
            .filter(|ticket| self.project_of(&ticket.ticket_id) == Some(&coordinator.project_id))
            .collect();
        let open: Vec<_> = tickets
            .iter()
            .filter(|ticket| ticket.stage != Stage::Accepted)
            .collect();
        let moving = open
            .iter()
            .filter(|ticket| ticket.state != BarState::Blocked)
            .map(|ticket| ticket.stage)
            .min();
        let (phase, state, position) = if tickets.is_empty() {
            let own = self.agent(coordinator);
            (Phase::Planning, own.state, own.position())
        } else if open.is_empty() {
            (Phase::Done, BarState::Finished, Some(FULL_POSITION))
        } else if let Some(stage) = moving {
            (stage.into(), BarState::InProgress, Some(mean(&tickets)))
        } else {
            (Phase::Blocked, BarState::Blocked, Some(mean(&tickets)))
        };
        ProjectProgress {
            project_id: coordinator.project_id.clone(),
            phase,
            state,
            position,
            accepted: (tickets.len() - open.len()) as u32,
            tickets: tickets.iter().map(|t| t.ticket_id.clone()).collect(),
            coordinator_id: coordinator.id.clone(),
        }
    }

    /// The project of the coordinator that owns the ticket.
    fn project_of(&self, ticket_id: &str) -> Option<&String> {
        let ticket = self.tickets.iter().find(|t| t.id == ticket_id)?;
        let owner = self
            .sessions
            .iter()
            .find(|s| s.id == ticket.coordinator_id)?;
        Some(&owner.project_id)
    }
}

/// The equal-weight average of the tickets' exact positions; the caller passes at least one.
fn mean(tickets: &[&TicketProgress]) -> u16 {
    let total: u32 = tickets
        .iter()
        .map(|ticket| u32::from(ticket.position))
        .sum();
    let count = tickets.len() as u32;
    ((total + count / 2) / count) as u16
}
