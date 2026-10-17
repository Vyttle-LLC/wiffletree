//! The progress board as the host last described it. The host signals when a figure changed
//! and the client asks only for the entries changed since its cursor, never for a snapshot.
use super::*;
use activity::paced_reply;
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Default)]
pub(super) struct ProgressBoard {
    revision: u64,
    agents: HashMap<String, AgentProgress>,
    tickets: HashMap<String, TicketProgress>,
    projects: HashMap<String, ProjectProgress>,
}
impl ProgressBoard {
    /// Applies a page of changes. A complete page replaces the board, which is how entries
    /// the host no longer derives, such as a closed ticket, leave it.
    pub fn merge(&mut self, page: ProgressPage) {
        if page.complete {
            *self = Self::default();
        }
        self.revision = page.revision;
        let keyed = |agent: AgentProgress| (agent.session_id.clone(), agent);
        self.agents.extend(page.agents.into_iter().map(keyed));
        let keyed = |ticket: TicketProgress| (ticket.ticket_id.clone(), ticket);
        self.tickets.extend(page.tickets.into_iter().map(keyed));
        let keyed = |project: ProjectProgress| (project.project_id.clone(), project);
        self.projects.extend(page.projects.into_iter().map(keyed));
    }
    pub fn agent(&self, session_id: &str) -> Option<&AgentProgress> {
        self.agents.get(session_id)
    }
    pub fn ticket(&self, ticket_id: &str) -> Option<&TicketProgress> {
        self.tickets.get(ticket_id)
    }
    pub fn project(&self, project_id: &str) -> Option<&ProjectProgress> {
        self.projects.get(project_id)
    }
    /// The project's counted tickets, in the order its bar draws them.
    pub fn tickets_of(&self, project: &ProjectProgress) -> Vec<&TicketProgress> {
        let listed = project.tickets.iter();
        listed.filter_map(|id| self.ticket(id)).collect()
    }
    /// The agents the crew panel lists for a project, each ticket's in turn.
    pub fn crew(&self, project: &ProjectProgress) -> Vec<&AgentProgress> {
        let ids = self.tickets_of(project).into_iter().flat_map(|t| &t.agents);
        ids.filter_map(|id| self.agent(id)).collect()
    }
}

impl Workspace {
    /// Reads the board once, then again whenever the host says a figure changed.
    pub(super) fn watch_progress(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(changes) = self.bridge.progress_changes() else {
            return;
        };
        self.fetch_progress(window, cx);
        cx.spawn_in(window, async move |this, cx| {
            while changes.recv().await.is_ok() {
                let fetched = cx.update(|window, cx| {
                    this.update(cx, |view, cx| view.fetch_progress(window, cx))
                });
                if !matches!(fetched, Ok(Ok(()))) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Asks for the entries changed since the last page; one request at a time.
    fn fetch_progress(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Plans change a few times a turn, so progress needs no pacing.
        let Some(wait) = self.progress_fetch.begin(Duration::ZERO) else {
            return;
        };
        let after = self.shared.progress.revision;
        let page = paced_reply(
            self.bridge.clone(),
            ask::progress(after),
            wait,
            cx.background_executor().clone(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let result = page.await;
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| view.accept_progress(result, window, cx))
            });
        })
        .detach();
    }

    fn accept_progress(
        &mut self,
        result: Result<ProgressPage, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owed = self.progress_fetch.finish();
        match result {
            Ok(page) => {
                Rc::make_mut(&mut self.shared.progress).merge(page);
                self.share(window, cx);
                cx.notify();
            }
            Err(error) => eprintln!("Progress: {error}"),
        }
        if owed {
            self.fetch_progress(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::ProgressBoard;
    use workspace_core::*;

    fn agent(id: &str, done: u32) -> AgentProgress {
        AgentProgress {
            session_id: id.into(),
            state: BarState::InProgress,
            plan: Some(PlanCounts {
                done,
                total: 6,
                current: None,
            }),
            word: None,
            working: true,
            run_started_at: None,
            run_finished_at: None,
        }
    }
    fn ticket(id: &str, agents: &[&str]) -> TicketProgress {
        TicketProgress {
            ticket_id: id.into(),
            stage: Stage::Implementing,
            state: BarState::InProgress,
            position: 3_000,
            agents: agents.iter().map(|id| id.to_string()).collect(),
        }
    }
    fn project(tickets: &[&str]) -> ProjectProgress {
        ProjectProgress {
            project_id: "project".into(),
            phase: Phase::Implementing,
            state: BarState::InProgress,
            position: Some(3_000),
            accepted: 0,
            tickets: tickets.iter().map(|id| id.to_string()).collect(),
            coordinator_id: "coordinator".into(),
        }
    }
    fn done(board: &ProgressBoard, id: &str) -> Option<u32> {
        Some(board.agent(id)?.plan.as_ref()?.done)
    }

    #[test]
    fn a_page_of_changes_updates_its_entries_and_keeps_the_rest() {
        let mut board = ProgressBoard::default();
        board.merge(ProgressPage {
            revision: 3,
            complete: true,
            agents: vec![agent("a", 1), agent("b", 2)],
            tickets: vec![ticket("one", &["a"]), ticket("two", &["b"])],
            projects: vec![project(&["one", "two"])],
        });
        board.merge(ProgressPage {
            revision: 4,
            complete: false,
            agents: vec![agent("a", 5)],
            tickets: vec![],
            projects: vec![],
        });
        assert_eq!(board.revision, 4);
        assert_eq!((done(&board, "a"), done(&board, "b")), (Some(5), Some(2)));
        let project = board.project("project").unwrap();
        assert_eq!(board.tickets_of(project).len(), 2);
        let crew: Vec<_> = board.crew(project).iter().map(|a| &a.session_id).collect();
        assert_eq!(crew, ["a", "b"]);
    }

    #[test]
    fn a_complete_page_drops_what_the_host_no_longer_lists() {
        let mut board = ProgressBoard::default();
        board.merge(ProgressPage {
            revision: 3,
            complete: true,
            agents: vec![agent("a", 1), agent("b", 2)],
            tickets: vec![ticket("one", &["a"]), ticket("two", &["b"])],
            projects: vec![project(&["one", "two"])],
        });
        // Ticket two was closed and its agent archived.
        board.merge(ProgressPage {
            revision: 5,
            complete: true,
            agents: vec![agent("a", 1)],
            tickets: vec![ticket("one", &["a"])],
            projects: vec![project(&["one"])],
        });
        assert!(board.agent("b").is_none() && board.ticket("two").is_none());
        let project = board.project("project").unwrap();
        assert_eq!(board.crew(project).len(), 1);
    }
}
