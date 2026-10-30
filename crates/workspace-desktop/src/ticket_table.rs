//! The table pinned above a coordinator's transcript: its tickets in progress with their status
//! and PR, from Wiffletree's records, so the coordinator never has to repeat them. It redraws with
//! the conversation when a new snapshot arrives and fetches nothing itself.
use super::*;
use conversation::{COLUMN, Conversation};
use ui::{hint, icon, ticket_state_color};
use wording::humanize;

/// A coordinator's tickets split into the rows the table lists and the ones it counts as done.
/// Planned tickets without an open PR are neither: their work has not started.
pub(super) struct TicketTable<'a> {
    pub in_progress: Vec<&'a Ticket>,
    /// The done tickets held: those still in the snapshot, and the archive's once it is read.
    pub done: Vec<&'a Ticket>,
    /// How many tickets are done, from the snapshot's count of tickets owned, so it is right
    /// before the archive that lists most of them is read.
    pub done_count: usize,
}
impl<'a> TicketTable<'a> {
    /// `owned` is how many tickets the coordinator owns, open or not.
    pub(super) fn of(tickets: impl IntoIterator<Item = &'a Ticket>, owned: usize) -> Self {
        let mut table = Self {
            in_progress: vec![],
            done: vec![],
            done_count: 0,
        };
        let mut open = 0;
        for ticket in tickets {
            let pr_open =
                (ticket.pull_request.as_ref()).is_some_and(|pr| pr.state == PrState::Open);
            if !ticket.is_open() {
                table.done.push(ticket);
                continue;
            }
            open += 1;
            if ticket.state != TicketState::Planned || pr_open {
                table.in_progress.push(ticket);
            }
        }
        table.done_count = owned.saturating_sub(open).max(table.done.len());
        table
    }
    fn is_empty(&self) -> bool {
        self.in_progress.is_empty() && self.done_count == 0
    }
}

/// The Tickets inspector's state wording, with the round while a review runs, such as
/// "Verifying · round 2".
pub(super) fn status(ticket: &Ticket) -> String {
    let state = humanize(ticket.state.name());
    match &ticket.verification {
        Some(cycle)
            if cycle.outcome == VerificationOutcome::Running && !cycle.rounds.is_empty() =>
        {
            format!("{state} · round {}", cycle.rounds.len())
        }
        _ => state,
    }
}

/// "#184 merged", "#12 draft", or "Not published" before the ticket has a PR.
pub(super) fn pr_label(ticket: &Ticket) -> String {
    let Some(pr) = &ticket.pull_request else {
        return "Not published".into();
    };
    let state = match pr.state {
        PrState::Open if pr.draft => "draft",
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    };
    format!("#{} {state}", pr.number)
}

impl Conversation {
    /// Pinned at the top of the selected coordinator's conversation while it has tickets.
    pub(super) fn ticket_table(
        &self,
        session: &Session,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let index = self.shared.index.as_ref()?;
        if session.role != Role::ProjectOrchestrator {
            return None;
        }
        let owned = index.snapshot().tickets_owned.get(&session.id).copied();
        let table = TicketTable::of(
            index.tickets_owned_by(&session.id),
            owned.unwrap_or_default() as usize,
        );
        if table.is_empty() {
            return None;
        }
        let collapsed = cx.global::<Preferences>().collapse_ticket_table;
        let summary = match (table.in_progress.len(), table.done_count) {
            (0, done) => format!("{done} done"),
            (open, 0) => format!("{open} in progress"),
            (open, done) => format!("{open} in progress, {done} done"),
        };
        let head = div()
            .id("ticket-table-toggle")
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .text_color(p.subtle)
            .hover(|d| d.text_color(p.text))
            .child(
                icon(if collapsed {
                    "chevron-right"
                } else {
                    "chevron-down"
                })
                .size(px(12.)),
            )
            .child(div().flex_1().child(format!("Tickets · {summary}")))
            .child(hint("Live from Wiffletree", p).text_size(px(font::CAPTION)))
            .on_click(cx.listener(move |_, _, _, cx| {
                set_collapse_ticket_table(!collapsed, cx);
                cx.notify();
            }));
        let mut body = div()
            .w_full()
            .max_w(px(COLUMN))
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .rounded_lg()
            .border_1()
            .border_color(p.overlay)
            .bg(p.surface)
            .text_size(px(font::MESSAGE))
            .child(head);
        if !collapsed {
            body = body.children(table.in_progress.iter().map(|t| ticket_row(t, p)));
            if table.done_count > 0 {
                let open = self.done_tickets_open;
                body = body.child(
                    div()
                        .id("ticket-table-done")
                        .flex()
                        .items_center()
                        .gap_1()
                        .cursor_pointer()
                        .text_color(p.subtle)
                        .hover(|d| d.text_color(p.text))
                        .child(
                            icon(if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            })
                            .size(px(12.)),
                        )
                        .child(format!("{} done", table.done_count))
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.done_tickets_open = !view.done_tickets_open;
                            // Most done tickets are only in the archive.
                            if view.done_tickets_open {
                                cx.emit(Intent::DoneTicketsShown);
                            }
                            cx.notify();
                        })),
                );
                if open {
                    body = body.children(table.done.iter().map(|t| ticket_row(t, p)));
                }
            }
        }
        Some(
            div()
                .w_full()
                .px_4()
                .pt_2()
                .flex()
                .justify_center()
                .child(body),
        )
    }
}

/// One ticket: its title, its status in its state's color and its PR, linked when it has one.
fn ticket_row(ticket: &Ticket, p: Palette) -> Div {
    let pr = div()
        .w(px(110.))
        .flex_none()
        .text_ellipsis()
        .child(pr_label(ticket));
    let pr = match &ticket.pull_request {
        Some(pull_request) => {
            let url = pull_request.url.clone();
            pr.child(
                div()
                    .id(SharedString::from(format!("ticket-table-pr-{}", ticket.id)))
                    .absolute()
                    .size_full()
                    .cursor_pointer()
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            )
            .relative()
            .text_color(p.focus)
        }
        None => pr.text_color(p.subtle),
    };
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(ticket.title.clone()),
        )
        .child(
            div()
                .w(px(200.))
                .flex_none()
                .text_ellipsis()
                .text_color(ticket_state_color(ticket.state, p))
                .child(status(ticket)),
        )
        .child(pr)
}

#[cfg(test)]
mod tests {
    use super::{TicketTable, pr_label, status};
    use workspace_core::{PrState, PullRequest, Ticket, TicketState};

    fn ticket(id: &str, state: TicketState) -> Ticket {
        let stored = r#"{"id":"","coordinator_id":"c","title":"","brief":"","worktree":"","branch":"","state":"planned"}"#;
        let mut ticket: Ticket = serde_json::from_str(stored).unwrap();
        (ticket.id, ticket.title, ticket.state) = (id.into(), id.into(), state);
        ticket
    }
    fn pull_request(number: u64, state: PrState) -> PullRequest {
        let stored = r#"{"number":0,"url":"u","state":"open","draft":false,"base":"main","head":"h","merge_state":"clean","checks":"none","unresolved_threads":0,"comments":{}}"#;
        let mut pr: PullRequest = serde_json::from_str(stored).unwrap();
        (pr.number, pr.state) = (number, state);
        pr
    }

    #[test]
    fn the_table_lists_work_in_progress_and_counts_done_tickets() {
        let mut planned_with_pr = ticket("planned-pr", TicketState::Planned);
        planned_with_pr.pull_request = Some(pull_request(12, PrState::Open));
        let tickets = [
            ticket("implementing", TicketState::Assigned),
            ticket("planned", TicketState::Planned),
            ticket("blocked", TicketState::Blocked),
            ticket("accepted", TicketState::Accepted),
            ticket("closed", TicketState::Closed),
            ticket("passed", TicketState::Passed),
            planned_with_pr,
        ];
        let table = TicketTable::of(&tickets, tickets.len());
        let ids = |list: &[&Ticket]| list.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(&table.in_progress),
            ["implementing", "blocked", "passed", "planned-pr"]
        );
        assert_eq!(ids(&table.done), ["accepted", "closed"]);
        assert_eq!(table.done_count, 2);
    }

    #[test]
    fn done_tickets_only_the_archive_holds_are_counted_before_it_is_read() {
        // The snapshot lists the open ticket; the owned count also has two settled ones that
        // only the archive holds, such as a planned ticket closed without agents.
        let tickets = [ticket("open", TicketState::Planned)];
        let table = TicketTable::of(&tickets, 3);
        assert!(table.in_progress.is_empty() && table.done.is_empty());
        assert_eq!(table.done_count, 2);
        assert!(!table.is_empty());
    }

    #[test]
    fn status_and_pr_read_as_in_the_tickets_inspector() {
        let mut verifying = ticket("v", TicketState::Verifying);
        assert_eq!(status(&verifying), "Verifying");
        let cycle = r#"{"cycle":1,"max_rounds":3,"outcome":"running","rounds":[{"round":1,"commit":"a","verifiers":[]},{"round":2,"commit":"b","verifiers":[]}]}"#;
        verifying.verification = Some(serde_json::from_str(cycle).unwrap());
        assert_eq!(status(&verifying), "Verifying · round 2");
        assert_eq!(
            status(&ticket("r", TicketState::ReadyForTesting)),
            "Ready for testing"
        );
        let mut accepted = ticket("a", TicketState::Accepted);
        assert_eq!(pr_label(&accepted), "Not published");
        accepted.pull_request = Some(pull_request(184, PrState::Merged));
        assert_eq!(pr_label(&accepted), "#184 merged");
        let mut draft = pull_request(12, PrState::Open);
        draft.draft = true;
        accepted.pull_request = Some(draft);
        assert_eq!(pr_label(&accepted), "#12 draft");
    }
}
