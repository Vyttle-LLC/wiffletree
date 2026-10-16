//! The selected session's conversation: header, transcript, notices and composer.
use super::*;
use gpui_component::{
    Disableable, Selectable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    text::TextView,
};
use ui::{
    age, banner, brand_image, effort_label, hint, humanize, icon, pill, progress_bar,
    segmented_bar, session_icon,
};

/// Wide enough for the composer, narrow enough that message lines stay readable.
const COLUMN: f32 = 720.;
/// The progress strip never changes height, so the transcript below it never moves.
const STRIP_HEIGHT: f32 = 36.;
const STRIP_BAR: f32 = 6.;

/// Kinds an agent can pass to the host's `report` tool.
const REPORT_KINDS: [&str; 6] = [
    "progress",
    "blocked",
    "ready_for_testing",
    "passed",
    "failed",
    "completed",
];

/// How a refreshed transcript page lines up with the one on screen: how many leading messages
/// fell off the page and how many after them are unchanged. `None` when the pages don't overlap.
pub(super) fn transcript_overlap(
    shown: &[Message],
    refreshed: &[Message],
) -> Option<(usize, usize)> {
    let first = refreshed.first()?;
    let dropped = shown.iter().position(|m| m.id == first.id)?;
    let unchanged = shown[dropped..]
        .iter()
        .zip(refreshed)
        .take_while(|(old, new)| old == new)
        .count();
    Some((dropped, unchanged))
}

/// Forgets the minimized card of every project that has no open decision, so the next one it
/// brings starts expanded. A decision that merely arrives or changes leaves the card as it is.
pub(super) fn forget_settled(minimized: &mut BTreeSet<String>, open: &[Attention]) {
    minimized.retain(|project| open.iter().any(|item| &item.project_id == project));
}

/// Agent reports arrive as `[kind] sender name\nbody`, and timer fires as `[timer] label\nbody`;
/// returns the kind and body.
fn report(body: &str) -> Option<(&str, &str)> {
    let (kind, rest) = body.strip_prefix('[')?.split_once("] ")?;
    (REPORT_KINDS.contains(&kind) || kind == "timer")
        .then(|| (kind, rest.split_once('\n').map_or("", |(_, text)| text)))
}

/// A peer message is coordination traffic rather than conversation: anything another session
/// sent this one (reports and verdicts, turn results, check-ins, direct messages, a
/// coordinator's assignments) and this session's own timer fires. The human's messages,
/// Wiffletree's notices and the session's own replies, errors and compaction lines are not.
fn peer_message(message: &Message, session_id: &str) -> bool {
    message.sender.as_deref().is_some_and(|sender| {
        (sender != session_id && host_notice(&message.id).is_none())
            || message.id.starts_with("timer:")
    })
}

/// Where a message sits in the transcript. Every message keeps its own list row, so the list's
/// splices stay one row per message; a run of peer messages draws as one row at its head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Placement {
    Alone,
    /// Heads a run of this many consecutive peer messages.
    Heads(usize),
    /// Drawn by the run above it; its own row is empty.
    Folded,
}

pub(super) fn placements(messages: &[Message], session_id: &str) -> Vec<Placement> {
    let mut placements = Vec::with_capacity(messages.len());
    let mut head = None;
    for (row, message) in messages.iter().enumerate() {
        if !peer_message(message, session_id) {
            head = None;
            placements.push(Placement::Alone);
        } else if let Some(head) = head {
            if let Placement::Heads(count) = &mut placements[head] {
                *count += 1;
            }
            placements.push(Placement::Folded);
        } else {
            head = Some(row);
            placements.push(Placement::Heads(1));
        }
    }
    placements
}

/// A peer message as one line of its run: its kind, its text and what went wrong, if anything.
#[derive(Debug, PartialEq, Eq)]
struct PeerLine<'a> {
    kind: Option<&'a str>,
    text: &'a str,
    trouble: Option<&'static str>,
}

fn peer_line(message: &Message) -> PeerLine<'_> {
    let body = message.body.as_str();
    let (kind, text) = if let Some((kind, text)) = report(body) {
        (Some(kind), text)
    } else if let Some(text) = body.strip_prefix("[check-in] ") {
        (Some("check_in"), text)
    } else if message.id.starts_with("turn-result:") {
        (Some("turn_result"), body)
    } else {
        (None, body)
    };
    let trouble = match kind {
        Some("failed") => Some("failed"),
        Some("blocked") => Some("blocked"),
        Some("turn_result") if body.contains(" stopped with an error: ") => Some("error"),
        _ => None,
    };
    PeerLine {
        kind,
        text,
        trouble,
    }
}

/// "1 failed", "2 blocked", "1 error": what went wrong across a run, for its row.
fn run_troubles(run: &[Message]) -> Vec<String> {
    ["failed", "blocked", "error"]
        .into_iter()
        .filter_map(|trouble| {
            let count = run
                .iter()
                .filter(|m| peer_line(m).trouble == Some(trouble))
                .count();
            match (count, trouble) {
                (0, _) => None,
                (1, _) | (_, "failed" | "blocked") => Some(format!("{count} {trouble}")),
                _ => Some(format!("{count} {trouble}s")),
            }
        })
        .collect()
}

/// "3 agent messages · Implementer, Tester", the start of a run's row.
fn run_heading(count: usize, senders: &[String]) -> String {
    let mut names: Vec<&str> = vec![];
    for sender in senders {
        if !names.contains(&sender.as_str()) {
            names.push(sender);
        }
    }
    format!(
        "{count} agent message{} · {}",
        if count == 1 { "" } else { "s" },
        names.join(", ")
    )
}

pub(super) fn first_line(text: &str, limit: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() > limit {
        format!("{}…", line.chars().take(limit).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// Renders a message's Markdown. Relative file references resolve against `bases` in order.
pub(super) fn markdown(id: &str, body: &str, bases: &[PathBuf]) -> TextView {
    let link_bases = bases.to_vec();
    TextView::markdown(
        SharedString::from(format!("message-markdown-{id}")),
        file_links::linkify(body, bases),
    )
    .selectable(true)
    .on_link_click(move |url, event, window, cx| {
        file_links::open_link(url, event, &link_bases, window, cx)
    })
    .code_block_actions(|block, _, _| {
        let code = block.code();
        button("copy-code")
            .ghost()
            .icon(icon("copy"))
            .label("Copy")
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()))
            })
    })
    .text_size(px(14.))
    .line_height(relative(1.5))
}

fn avatar(icon_name: &str, color: Hsla) -> Div {
    div()
        .flex_none()
        .size(px(26.))
        .rounded_lg()
        .bg(color.opacity(0.12))
        .text_color(color)
        .flex()
        .items_center()
        .justify_center()
        .child(icon(icon_name).size(px(15.)))
}

fn meta(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .text_size(px(10.5))
        .text_color(p.subtle)
        .child(text.into())
}

fn receipt_color(receipt: Receipt, p: Palette) -> Hsla {
    match receipt {
        Receipt::Held => p.yellow,
        Receipt::Completed | Receipt::Acknowledged => p.green,
        _ => p.subtle,
    }
}

/// The human's own message: a right-aligned bubble and its files, with its delivery state.
fn human_message(message: &Message, bases: &[PathBuf], p: Palette) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_end()
        .gap_1()
        .when(!message.body.trim().is_empty(), |row| {
            row.child(
                div()
                    .max_w(px(620.))
                    .px_4()
                    .py_2()
                    .rounded_xl()
                    .bg(p.overlay)
                    .child(markdown(&message.id, &message.body, bases)),
            )
        })
        .when(!message.attachments.is_empty(), |row| {
            row.child(attachment_chips::sent(message, p).max_w(px(620.)))
        })
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    meta(format!("{:?}", message.receipt), p)
                        .text_color(receipt_color(message.receipt, p)),
                )
                .child(meta(age(message.created_at), p)),
        )
}

/// Messages without a sender are the human's, apart from Wiffletree's own notices.
fn from_human(message: &Message) -> bool {
    message.sender.is_none() && host_notice(&message.id).is_none()
}

/// A notice from Wiffletree itself, such as the PR watcher's.
fn notice_message(message: &Message, bases: &[PathBuf], p: Palette) -> Div {
    div()
        .w_full()
        .flex()
        .gap_3()
        .child(avatar("server", p.subtle))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .min_h(px(26.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(host_notice(&message.id).unwrap_or("Wiffletree")),
                        )
                        .child(div().flex_1())
                        .child(
                            meta(format!("{:?}", message.receipt), p)
                                .text_color(receipt_color(message.receipt, p)),
                        )
                        .child(meta(age(message.created_at), p)),
                )
                .child(markdown(&message.id, &message.body, bases)),
        )
}

/// The line a compaction left in the conversation, split into its title, the rest of its first
/// line and its "Kept:" line, and whether it failed. `None` for any other message: a reply is
/// written under an `output:` id, never this one.
fn compaction_line(message: &Message) -> Option<(&str, &str, Option<&str>, bool)> {
    if !message.id.starts_with(COMPACTION_LINE_PREFIX) {
        return None;
    }
    let (first, kept) = match message.body.split_once('\n') {
        Some((first, kept)) => (first, Some(kept)),
        None => (message.body.as_str(), None),
    };
    let (title, rest) = match first.find(" · ") {
        Some(at) => first.split_at(at),
        None => (first, ""),
    };
    Some((title, rest, kept, title != "Context compacted"))
}

/// One line where a finished turn's summary would be, never a bubble; red when it failed.
fn compaction_message(line: (&str, &str, Option<&str>, bool), p: Palette) -> Div {
    let (title, rest, kept, failed) = line;
    let (color, strong) = if failed {
        (p.red, p.red)
    } else {
        (p.subtle, p.text)
    };
    div()
        .ml(px(38.))
        .flex()
        .flex_col()
        .gap(px(1.))
        .text_size(px(11.5))
        .text_color(color)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(icon("fold").size(px(13.)))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(strong)
                        .child(title.to_owned()),
                )
                .child(rest.trim_start().to_owned()),
        )
        .when_some(kept, |d, kept| {
            d.child(div().ml(px(18.)).child(kept.to_owned()))
        })
}

/// A message written by an agent: this session's own reply, or another agent's report.
fn agent_message(
    message: &Message,
    sender: Option<&Session>,
    own: bool,
    bases: &[PathBuf],
    p: Palette,
) -> Div {
    let (kind, body) = match report(&message.body) {
        Some((kind, body)) if !own => (Some(kind), body),
        _ => (None, message.body.as_str()),
    };
    let role = sender.map(|s| s.role);
    div()
        .w_full()
        .flex()
        .gap_3()
        .child(avatar(
            role.map_or("agent", session_icon),
            if own { p.accent } else { p.focus },
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .min_h(px(26.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .text_ellipsis()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(sender.map_or("Agent".to_owned(), |s| s.name.clone())),
                        )
                        .when_some(role.filter(|_| !own), |d, role| {
                            d.child(meta(role.label(), p))
                        })
                        .when_some(kind, |d, kind| d.child(pill(humanize(kind), p.focus)))
                        .child(div().flex_1())
                        .when(!own, |d| {
                            d.child(
                                meta(format!("{:?}", message.receipt), p)
                                    .text_color(receipt_color(message.receipt, p)),
                            )
                        })
                        .child(meta(age(message.created_at), p)),
                )
                .when(!body.trim().is_empty(), |d| {
                    d.child(markdown(&message.id, body, bases))
                }),
        )
}

/// The peer-message runs and lines the human opened, by the run's first message id and by
/// message id. Kept in memory only.
#[derive(Clone, Default)]
pub(super) struct OpenPeers {
    runs: BTreeSet<String>,
    lines: BTreeSet<String>,
}

/// Who sent a peer message: the sending agent, or a timer's label.
fn peer_sender(message: &Message, sessions: &[Session]) -> String {
    let timer = message
        .body
        .strip_prefix("[timer] ")
        .filter(|_| message.id.starts_with("timer:"))
        .and_then(|body| body.lines().next());
    match timer {
        Some(label) => label.to_owned(),
        None => sessions
            .iter()
            .find(|s| Some(&s.id) == message.sender.as_ref())
            .map_or("Agent".to_owned(), |s| s.name.clone()),
    }
}

/// Where relative file references in a message resolve: under its sender's directories, or the
/// selected session's for messages from the human and Wiffletree.
struct LinkBases {
    senders: BTreeMap<String, Vec<PathBuf>>,
    current: Vec<PathBuf>,
}
impl LinkBases {
    fn of(&self, message: &Message) -> &[PathBuf] {
        message
            .sender
            .as_ref()
            .and_then(|id| self.senders.get(id))
            .unwrap_or(&self.current)
    }
}

/// A run of peer messages as one line where a turn summary would be. Opened, it lists a line
/// per message, and an opened line shows that message in full.
fn peer_run(
    run: &[Message],
    sessions: &[Session],
    open: &OpenPeers,
    links: &LinkBases,
    p: Palette,
    view: WeakEntity<Workspace>,
) -> Div {
    let head = run[0].id.clone();
    let senders: Vec<String> = run.iter().map(|m| peer_sender(m, sessions)).collect();
    let (first, last) = (age(run[0].created_at), age(run[run.len() - 1].created_at));
    let span = if first == last {
        first
    } else {
        format!("{first} – {last}")
    };
    let opened = open.runs.contains(&head);
    let toggle = {
        let (view, head) = (view.clone(), head.clone());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| view.toggle_peers(&head, None, cx));
        }
    };
    div()
        .ml(px(38.))
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(11.5))
        .text_color(p.subtle)
        .child(
            div()
                .id(SharedString::from(format!("peer-run-{head}")))
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .hover(|style| style.text_color(p.text))
                .child(
                    icon(if opened {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    })
                    .size(px(12.))
                    .flex_none(),
                )
                .child(icon("arrows-left-right").size(px(12.)).flex_none())
                .child(
                    div()
                        .min_w_0()
                        .text_ellipsis()
                        .child(run_heading(run.len(), &senders)),
                )
                // Long sender names give way, never the counts or the time.
                .children(run_troubles(run).into_iter().map(|trouble| {
                    div()
                        .flex_none()
                        .text_color(p.red)
                        .child(format!("· {trouble}"))
                }))
                .child(div().flex_none().child(format!("· {span}")))
                .on_click(toggle),
        )
        .when(opened, |d| {
            d.child(
                div()
                    .ml(px(18.))
                    .pl(px(10.))
                    .border_l_2()
                    .border_color(p.overlay)
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .children(run.iter().zip(senders).map(|(message, sender)| {
                        peer_line_row(
                            message,
                            sender,
                            &head,
                            open,
                            links.of(message),
                            p,
                            view.clone(),
                        )
                    })),
            )
        })
}

/// One message of an opened run: sender, kind, first line and time; opened, its full text.
fn peer_line_row(
    message: &Message,
    sender: String,
    head: &str,
    open: &OpenPeers,
    bases: &[PathBuf],
    p: Palette,
    view: WeakEntity<Workspace>,
) -> Div {
    let line = peer_line(message);
    let toggle = {
        let (head, id) = (head.to_owned(), message.id.clone());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| view.toggle_peers(&head, Some(&id), cx));
        }
    };
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .id(SharedString::from(format!("peer-line-{}", message.id)))
                .flex()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .hover(|style| style.text_color(p.text))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(160.))
                        .text_ellipsis()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text)
                        .child(sender),
                )
                .when_some(line.kind, |d, kind| {
                    let color = if line.trouble.is_some() {
                        p.red
                    } else {
                        p.focus
                    };
                    d.child(pill(humanize(kind), color))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .child(first_line(line.text, 160)),
                )
                .child(div().flex_none().child(age(message.created_at)))
                .on_click(toggle),
        )
        .when(open.lines.contains(&message.id), |d| {
            d.child(
                div()
                    .mt_1()
                    .mb(px(6.))
                    .px(px(10.))
                    .py_2()
                    .rounded_md()
                    .bg(p.overlay.opacity(0.5))
                    .text_color(p.text)
                    .child(markdown(&message.id, line.text, bases).text_size(px(12.5))),
            )
        })
}

impl Workspace {
    /// Opens or closes a peer-message run, or one line within it, and measures its row again.
    pub(super) fn toggle_peers(&mut self, head: &str, line: Option<&str>, cx: &mut Context<Self>) {
        let (open, id) = match line {
            Some(line) => (&mut self.open_peers.lines, line),
            None => (&mut self.open_peers.runs, head),
        };
        if !open.remove(id) {
            open.insert(id.to_owned());
        }
        if let Some(row) = self.messages.iter().position(|m| m.id == head)
            && row < self.list.item_count()
        {
            self.list.remeasure_items(row..row + 1);
        }
        cx.notify();
    }

    fn waiting_messages(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| from_human(m) && m.receipt == Receipt::Queued)
            .count()
    }

    /// Stop is offered only while some agent in the project is mid-turn.
    fn project_working(&self, session: &Session) -> bool {
        self.snapshot
            .iter()
            .flat_map(|s| &s.sessions)
            .any(|s| s.project_id == session.project_id && s.status == Status::Working)
    }

    fn breadcrumb(&self, session: &Session) -> String {
        let mut parts = vec![self.project_name()];
        if session.role.is_worker() {
            if let Some(team) = session.parent_id.as_deref().and_then(|id| self.session(id)) {
                parts.push(team.name.clone());
            }
            if let Some(ticket) = self.ticket_of(&session.id) {
                parts.push(ticket.title.clone());
            }
        }
        parts.push(session.role.label().into());
        parts.join("  ›  ")
    }

    fn conversation_header(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let live = self.live_enabled();
        let paused = session.status == Status::Paused;
        let compacting = self.compacting(&session.id);
        let title = if session.role.is_worker() {
            self.ticket_of(&session.id)
                .map_or(session.name.clone(), |t| t.title.clone())
        } else {
            session.name.clone()
        };
        div()
            .flex_none()
            .h(px(64.))
            .px_6()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(p.edge.opacity(0.3))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.subtle)
                            .text_ellipsis()
                            .child(self.breadcrumb(session)),
                    )
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_ellipsis()
                            .child(title),
                    ),
            )
            .children(self.context_chip(session, p, cx))
            .child(self.session_badge(session, p))
            .when(!session.archived, |d| {
                d.child(
                    button("pause-session")
                        .ghost()
                        .icon(icon(if paused { "play" } else { "pause" }))
                        // A compaction that has started cannot be taken back.
                        .disabled(compacting)
                        .tooltip(if compacting {
                            "Compacting · it cannot be interrupted"
                        } else if paused {
                            "Resume this session"
                        } else {
                            "Pause this session · interrupts an active turn"
                        })
                        .on_click(cx.listener(|v, _, w, c| v.toggle_pause(w, c))),
                )
            })
            .when(
                session.role == Role::ProjectOrchestrator && !session.archived,
                |d| {
                    d.child(
                        button("new-ticket")
                            .ghost()
                            .icon(icon("plus"))
                            .label("New ticket")
                            .on_click(cx.listener(|v, _, w, c| {
                                v.open_creation(Creation::Ticket, None, w, c)
                            })),
                    )
                },
            )
            // A project runs once you message it, so the only control it needs is Stop.
            .children(if session.archived {
                Some(
                    button("restore-session")
                        .outline()
                        .icon(icon("archive"))
                        .label("Restore")
                        .on_click(cx.listener(|v, _, w, c| {
                            if let Some(id) = v.selected.clone() {
                                v.set_archived(id, false, w, c);
                            }
                        })),
                )
            } else if live && self.project_working(session) {
                Some(
                    button("stop-project")
                        .outline()
                        .icon(icon("stop").text_color(p.red))
                        .label("Stop")
                        .tooltip("Interrupt this project’s active turns and hold its queue")
                        .on_click(cx.listener(|v, _, w, c| v.set_live(false, w, c))),
                )
            } else {
                None
            })
    }

    /// Conditions that explain why nothing is happening, most urgent first.
    fn notices(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Vec<Div> {
        if session.archived {
            return vec![
                banner(p.subtle)
                    .child(icon("archive").text_color(p.subtle))
                    .child(div().flex_1().child(
                        "Archived. Its conversation and work are kept, and it receives nothing until restored.",
                    )),
            ];
        }
        let mut notices = vec![];
        if !self.refused_files.is_empty() {
            notices.push(
                banner(p.yellow)
                    .items_start()
                    .child(icon("triangle-alert").text_color(p.yellow).mt(px(2.)))
                    .child(
                        div().flex_1().min_w_0().flex().flex_col().children(
                            self.refused_files
                                .iter()
                                .map(|reason| div().child(reason.clone())),
                        ),
                    )
                    .child(
                        button("dismiss-refused")
                            .ghost()
                            .icon(icon("close"))
                            .tooltip("Dismiss")
                            .on_click(cx.listener(|v, _, _, c| {
                                v.refused_files.clear();
                                c.notify();
                            })),
                    ),
            );
        }
        if let Some(error) = self
            .runtime(&session.id)
            .and_then(|r| r.last_error.as_deref())
        {
            // Pausing is the user's own action, so it does not read as a failure.
            let tone = if session.status == Status::Paused {
                p.yellow
            } else {
                p.red
            };
            notices.push(
                banner(tone)
                    .items_start()
                    .child(icon("triangle-alert").text_color(tone).mt(px(2.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(first_line(error, 180))
                            .child(hint(
                                "The last input is held. Inspect the worktree before retrying: interrupted tools may already have run.",
                                p,
                            )),
                    )
                    .child(
                        button("skip-held")
                            .ghost()
                            .label("Skip")
                            .tooltip("Skip the held input")
                            .on_click(cx.listener(|v, _, w, c| v.reconcile(false, w, c))),
                    )
                    .child(
                        button("retry-held")
                            .label("Retry")
                            .tooltip("Retry the held input")
                            .on_click(cx.listener(|v, _, w, c| v.reconcile(true, w, c))),
                    ),
            );
        }
        let waiting = self.waiting_messages();
        if !self.live_enabled() && waiting > 0 {
            notices.push(
                banner(p.focus)
                    .child(icon("pause").text_color(p.focus))
                    .child(div().flex_1().child(format!(
                        "This project is stopped. {waiting} {} waiting.",
                        if waiting == 1 {
                            "message is"
                        } else {
                            "messages are"
                        }
                    )))
                    .child(
                        button("notice-resume")
                            .primary()
                            .label("Resume")
                            .on_click(cx.listener(|v, _, w, c| v.set_live(true, w, c))),
                    ),
            );
        }
        notices
    }

    /// The project's open decisions, one at a time, so the human can settle them in place.
    /// Hidden while the inbox is open, which already lists them all.
    fn decisions(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        if session.role != Role::ProjectOrchestrator || self.panel == Some(Panel::Attention) {
            return None;
        }
        let items = self.project_attention();
        let position = self
            .answering
            .as_ref()
            .and_then(|id| items.iter().position(|item| &item.id == id))
            .unwrap_or(0);
        let item = items.get(position)?;
        let project = item.project_id.clone();
        if self.minimized_decisions.contains(&project) {
            return Some(self.minimized_decision(item, position, items.len(), p, cx));
        }
        let step = |offset: usize| {
            let id = items[(position + offset) % items.len()].id.clone();
            cx.listener(move |v, _, w, c| v.start_answer(&id, w, c))
        };
        let navigation = div()
            .flex()
            .items_center()
            .gap_1()
            .child(hint(
                if items.len() == 1 {
                    "Your coordinator is waiting on a decision".to_owned()
                } else {
                    format!(
                        "Your coordinator is waiting on {} decisions · {} of {}",
                        items.len(),
                        position + 1,
                        items.len()
                    )
                },
                p,
            ))
            .child(div().flex_1())
            .when(items.len() > 1, |d| {
                d.child(
                    button("previous-decision")
                        .ghost()
                        .small()
                        .icon(icon("chevron-left"))
                        .tooltip("Previous decision")
                        .on_click(step(items.len() - 1)),
                )
                .child(
                    button("next-decision")
                        .ghost()
                        .small()
                        .icon(icon("chevron-right"))
                        .tooltip("Next decision")
                        .on_click(step(1)),
                )
            })
            .child(
                button("open-inbox")
                    .ghost()
                    .small()
                    .label("Open inbox")
                    .on_click(cx.listener(|v, _, w, c| v.show_panel(Panel::Attention, w, c))),
            )
            .child(
                button("minimize-decision")
                    .ghost()
                    .small()
                    .icon(icon("chevron-down"))
                    .tooltip("Minimize")
                    .on_click(cx.listener(move |v, _, _, c| {
                        v.minimized_decisions.insert(project.clone());
                        c.notify();
                    })),
            );
        Some(
            div()
                .w_full()
                .max_w(px(COLUMN))
                .flex()
                .flex_col()
                .gap_1()
                .child(navigation)
                .child(
                    self.attention_card(item, true, p, cx)
                        .border_color(p.yellow.opacity(0.45)),
                ),
        )
    }

    fn minimized_decision(
        &self,
        item: &Attention,
        position: usize,
        total: usize,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let project = item.project_id.clone();
        let expand = move |v: &mut Self, _: &ClickEvent, _: &mut Window, c: &mut Context<Self>| {
            v.minimized_decisions.remove(&project);
            c.notify();
        };
        div().w_full().max_w(px(COLUMN)).child(
            div()
                .id("minimized-decision")
                .flex()
                .items_center()
                .gap_2()
                .pl_3()
                .pr_2()
                .py_1p5()
                .rounded_lg()
                .border_1()
                .border_color(p.yellow.opacity(0.45))
                .bg(p.surface)
                .cursor_pointer()
                .child(div().size(px(7.)).flex_none().rounded_full().bg(p.yellow))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(12.5))
                        .child(first_line(&item.prompt, 200)),
                )
                .when(total > 1, |d| {
                    d.child(hint(format!("{} of {total}", position + 1), p).flex_none())
                })
                .child(
                    button("expand-decision")
                        .ghost()
                        .small()
                        .icon(icon("chevron-up"))
                        .tooltip("Expand"),
                )
                .on_click(cx.listener(expand)),
        )
    }

    fn empty_conversation(&self, role: Role, p: Palette, cx: &mut Context<Self>) -> Div {
        let (title, description, prompts): (_, _, &[(&str, &str)]) = match role {
            Role::ProjectOrchestrator => (
                "What are we building?",
                "Give your coordinator a goal. Shape the work together, then let the team take it forward.",
                &[
                    (
                        "Shape a plan",
                        "Help me turn this goal into a clear implementation plan: ",
                    ),
                    (
                        "Break down the work",
                        "Break this work into tickets with clear ownership and acceptance criteria: ",
                    ),
                    (
                        "Review the approach",
                        "Review this approach and identify the most important risks: ",
                    ),
                ],
            ),
            Role::TaskOrchestrator => (
                "A retired repository coordinator.",
                "Its tickets and agents now belong to the project coordinator. This conversation stays readable.",
                &[
                    (
                        "Propose tickets",
                        "Propose tickets with acceptance criteria for: ",
                    ),
                    (
                        "Check status",
                        "Summarize every ticket’s state and anything blocking it.",
                    ),
                ],
            ),
            _ => (
                "A clear brief starts here.",
                "This agent works on one ticket. Message it directly to steer or question its work.",
                &[
                    (
                        "Check status",
                        "Summarize what you have done and what remains.",
                    ),
                    ("Redirect", "Change of direction: "),
                ],
            ),
        };
        let mut suggestions = div().flex().flex_wrap().gap_2().mt_3();
        for (name, text) in prompts {
            suggestions = suggestions.child(button(*name).outline().label(*name).on_click(
                cx.listener(move |v, _, w, c| {
                    v.input.update(c, |input, c| {
                        input.set_value(*text, w, c);
                        input.focus(w, c);
                    })
                }),
            ));
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .px_8()
            .child(
                div()
                    .w_full()
                    .max_w(px(480.))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .child(if role == Role::ProjectOrchestrator {
                        brand_image(
                            if is_dark(cx) {
                                "brand/symbol-dark.svg"
                            } else {
                                "brand/symbol-light.svg"
                            },
                            72.,
                            36.,
                        )
                        .into_any_element()
                    } else {
                        div()
                            .size(px(44.))
                            .flex_none()
                            .rounded_xl()
                            .bg(p.accent.opacity(0.10))
                            .text_color(p.accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(session_icon(role)).size(px(22.)))
                            .into_any_element()
                    })
                    .child(
                        div()
                            .mt_3()
                            .text_size(px(26.))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(relative(1.2))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(13.5))
                            .line_height(relative(1.6))
                            .text_color(p.subtle)
                            .child(description),
                    )
                    .child(suggestions),
            )
    }

    fn transcript(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let messages = self.messages.clone();
        let card = self.activity_card();
        let summaries = Rc::new(self.run_summaries.clone());
        let minimized = cx.global::<Preferences>().minimize_activity;
        let view = cx.entity().downgrade();
        let sessions = self
            .snapshot
            .as_ref()
            .map(|s| s.sessions.clone())
            .unwrap_or_default();
        let current = session.clone();
        let placements = self.placements.clone();
        let open = Rc::new(self.open_peers.clone());
        let links = LinkBases {
            senders: messages
                .iter()
                .filter_map(|m| m.sender.as_ref())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|id| (id.clone(), self.link_bases(id)))
                .collect(),
            current: self.link_bases(&current.id),
        };
        let transcript = list(self.list.clone(), move |ix, _, _| {
            let placement = placements.get(ix).copied().unwrap_or(Placement::Alone);
            let content = match (placement, messages.get(ix)) {
                // A run draws in its head's row; the rest of its rows stay empty.
                (Placement::Folded, _) => return div().into_any_element(),
                (Placement::Heads(count), _) => {
                    let run = &messages[ix..(ix + count).min(messages.len())];
                    peer_run(run, &sessions, &open, &links, p, view.clone())
                }
                (_, Some(message)) if from_human(message) => {
                    human_message(message, links.of(message), p)
                }
                (_, Some(message)) if message.sender.is_none() => {
                    notice_message(message, links.of(message), p)
                }
                (_, Some(message)) if compaction_line(message).is_some() => {
                    compaction_line(message).map_or_else(div, |line| compaction_message(line, p))
                }
                (_, Some(message)) => {
                    let sender = sessions
                        .iter()
                        .find(|s| Some(&s.id) == message.sender.as_ref());
                    // Timer fires, also sent by the session itself, are drawn in a peer run.
                    let own = message.sender.as_ref() == Some(&current.id);
                    let reply = agent_message(message, sender, own, links.of(message), p);
                    // A reply whose turn did something shows how long and how much, above it.
                    let summary = message
                        .id
                        .strip_prefix("output:")
                        .and_then(|run| summaries.get(run))
                        .filter(|s| own && s.actions > 0);
                    match summary {
                        Some(summary) => div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .child(activity::turn_summary(summary, p, view.clone()))
                            .child(reply),
                        None => reply,
                    }
                }
                // The row after the last message appears only while the agent works.
                (_, None) => div()
                    .w_full()
                    .flex()
                    .gap_3()
                    .child(avatar(session_icon(current.role), p.accent))
                    .child(div().flex_1().min_w_0().child(activity::activity_row(
                        &card,
                        minimized,
                        p,
                        view.clone(),
                    ))),
            };
            div()
                .w_full()
                .px_6()
                .py_3()
                .flex()
                .justify_center()
                .child(div().w_full().max_w(px(COLUMN)).child(content))
                .into_any_element()
        })
        .size_full();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(transcript)
            .vertical_scrollbar(&self.list)
    }

    fn pagination(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let older = self.messages.len() == MAX_PAGE;
        let viewing_history = self.page.is_some();
        (older || viewing_history).then(|| {
            div()
                .flex_none()
                .py_1()
                .flex()
                .justify_center()
                .gap_2()
                .border_b_1()
                .border_color(p.edge.opacity(0.3))
                .when(older, |d| {
                    d.child(
                        button("older")
                            .ghost()
                            .icon(icon("chevron-up"))
                            .label("Earlier messages")
                            .on_click(cx.listener(|v, _, w, c| {
                                v.page = v.messages.first().map(|m| m.sequence);
                                v.load_messages(w, c);
                            })),
                    )
                })
                .when(viewing_history, |d| {
                    d.child(
                        button("latest")
                            .ghost()
                            .icon(icon("chevron-down"))
                            .label("Back to latest")
                            .on_click(cx.listener(|v, _, w, c| {
                                v.page = None;
                                v.load_messages(w, c);
                            })),
                    )
                })
        })
    }

    fn model_controls(&self, session: &Session, cx: &mut Context<Self>) -> Div {
        // An agent without a model is held; its menu asks for one instead of naming a model.
        let pinned = self.selected_profile();
        let unchosen = pinned.is_none();
        let Some(profile) = pinned.or_else(|| {
            session.parent_id.is_some().then(|| ModelProfile {
                provider: session.provider,
                model: String::new(),
                effort: "high".into(),
            })
        }) else {
            return div();
        };
        let started = self
            .runtime(&session.id)
            .is_some_and(|runtime| runtime.provider_session_id.is_some());
        let busy = session.status == Status::Working;
        // A PR keeper agent keeps the model chosen in Models when it was created.
        let fixed = busy || session.role == Role::Keeper;
        let mut models: BTreeMap<(Provider, String), String> = self
            .model_catalog
            .iter()
            .map(|model| ((model.provider, model.model.clone()), model.label.clone()))
            .collect();
        if let Some(snapshot) = &self.snapshot {
            for provider in PROVIDERS {
                for allowed in snapshot.model_selection.allowed(provider) {
                    models
                        .entry((provider, allowed.model.clone()))
                        .or_insert_with(|| allowed.model.clone());
                }
            }
        }
        let label = if unchosen {
            "Choose a model".to_owned()
        } else {
            let model = models
                .entry((profile.provider, profile.model.clone()))
                .or_insert_with(|| profile.model.clone());
            format!("{:?} · {model}", profile.provider)
        };
        let session_id = session.id.clone();
        let current = profile.clone();
        let weak = cx.weak_entity();
        let model = button("chat-model")
            .ghost()
            .disabled(fixed)
            .label(label)
            .dropdown_caret(true)
            .tooltip(if session.role == Role::Keeper {
                "The PR keeper's model is chosen in Models"
            } else if busy {
                "Pause the active turn to change the model"
            } else {
                "Model for this agent only"
            })
            .dropdown_menu(move |mut menu, _, _| {
                for ((provider, model), label) in &models {
                    let next = ModelProfile {
                        provider: *provider,
                        model: model.clone(),
                        effort: if *provider == current.provider {
                            current.effort.clone()
                        } else {
                            "high".into()
                        },
                    };
                    let session_id = session_id.clone();
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("{provider:?} · {label}"))
                            .checked(*provider == current.provider && *model == current.model)
                            .disabled(started && *provider != current.provider)
                            .on_click(move |_, w, c| {
                                let _ = weak.update(c, |v, c| {
                                    v.request(
                                        Command::ConfigureSession {
                                            session_id: session_id.clone(),
                                            profile: next.clone(),
                                        },
                                        w,
                                        c,
                                    )
                                });
                            }),
                    );
                }
                menu.scrollable(true).max_h(px(320.))
            });
        let efforts = models::effort_choices(
            &self.model_catalog,
            profile.provider,
            &profile.model,
            &[&profile.effort],
        );
        let session_id = session.id.clone();
        let weak = cx.weak_entity();
        let effort = button("chat-effort")
            .ghost()
            .disabled(fixed)
            .label(effort_label(&profile.effort))
            .dropdown_caret(true)
            .tooltip("Reasoning effort for this agent only")
            .dropdown_menu(move |mut menu, _, _| {
                for effort in &efforts {
                    let next = ModelProfile {
                        effort: effort.clone(),
                        ..profile.clone()
                    };
                    let session_id = session_id.clone();
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(effort_label(effort))
                            .checked(*effort == profile.effort)
                            .on_click(move |_, w, c| {
                                let _ = weak.update(c, |v, c| {
                                    v.request(
                                        Command::ConfigureSession {
                                            session_id: session_id.clone(),
                                            profile: next.clone(),
                                        },
                                        w,
                                        c,
                                    )
                                });
                            }),
                    );
                }
                menu
            });
        div()
            .flex()
            .items_center()
            .child(model)
            .when(!unchosen, |d| d.child(effort))
    }

    /// Who chose this agent's model and why, above the composer's model pills.
    fn chosen_by_line(&self, session: &Session, p: Palette) -> Option<Div> {
        let selection = self.runtime(&session.id)?.selection.as_ref()?;
        let sessions = &self.snapshot.as_ref()?.sessions;
        Some(
            div()
                .w_full()
                .max_w(px(COLUMN))
                .px_1()
                .text_size(px(12.))
                .text_color(p.subtle)
                .text_ellipsis()
                .child(models::chosen_by(selection, sessions)),
        )
    }
    fn composer(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let sending = self.sending.contains(&session.id);
        let attached = self.attached.get(&session.id).cloned().unwrap_or_default();
        let empty = self.input.read(cx).value().trim().is_empty() && attached.is_empty();
        let unsendable = empty || sending || session.archived;
        div()
            .key_context("ChatComposer")
            .on_action(cx.listener(|v, _: &SendMessage, w, c| {
                if w.has_active_dialog(c)
                    || v.input
                        .update(c, |input, c| input.marked_text_range(w, c).is_some())
                {
                    c.propagate();
                    return;
                }
                v.send(w, c);
            }))
            .flex_none()
            .px_6()
            .pb_4()
            .pt_1()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .children(self.decisions(session, p, cx))
            .children(
                self.notices(session, p, cx)
                    .into_iter()
                    .map(|notice| notice.max_w(px(COLUMN))),
            )
            .children(self.chosen_by_line(session, p))
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN))
                    .flex()
                    .flex_col()
                    .rounded_xl()
                    .border_1()
                    .border_color(p.edge.opacity(0.7))
                    .bg(p.surface)
                    .px_3()
                    .pt_2()
                    .pb_2()
                    .when(!attached.is_empty(), |composer| {
                        composer.child(self.pending_attachments(&attached, p, cx))
                    })
                    .child(
                        Textarea::new(&self.input)
                            .w_full()
                            .appearance(false)
                            .bordered(false),
                    )
                    .child(
                        div()
                            .mt_1()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(self.model_controls(session, cx))
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(p.subtle)
                                            .child("⏎ send · ⇧⏎ newline"),
                                    )
                                    .child(
                                        button("send")
                                            .primary()
                                            .icon(icon("arrow-up"))
                                            .loading(sending)
                                            .disabled(unsendable)
                                            // The stock disabled arrow is too faint on its tint.
                                            .when(unsendable, |b| b.text_color(p.subtle))
                                            .tooltip("Send")
                                            .on_click(cx.listener(|v, _, w, c| v.send(w, c))),
                                    ),
                            ),
                    ),
            )
    }

    /// Dropped files waiting in the composer, each with a button to take it back out.
    fn pending_attachments(
        &self,
        attached: &[Attachment],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .pt_1()
            .pb_2()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(attached.iter().enumerate().map(|(index, attachment)| {
                let path = attachment.path.clone();
                attachment_chips::chip(("pending-attachment", index), attachment, p)
                    // Keeps a file's name clear of the remove button.
                    .when(attachment.image.is_none(), |chip| chip.pr_7())
                    .child(
                        div().absolute().top(px(2.)).right(px(2.)).child(
                            button(("remove-attachment", index))
                                .ghost()
                                .xsmall()
                                .icon(icon("close"))
                                .tooltip(format!("Remove {}", attachment.name()))
                                .on_click(
                                    cx.listener(move |v, _, _, c| v.remove_attachment(&path, c)),
                                ),
                        ),
                    )
            }))
    }

    /// The progress strip pinned under the title: the project for a coordinator, and for any
    /// other agent its own checklist beside its ticket's figure. Its height is fixed and it
    /// is always drawn, so a progress change never moves the transcript.
    fn progress_strip(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let project = self
            .project_progress()
            .filter(|project| project.coordinator_id == session.id);
        let strip = div()
            .flex_none()
            .h(px(STRIP_HEIGHT))
            .px_6()
            .flex()
            .items_center()
            .gap_3()
            .bg(p.surface)
            .border_b_1()
            .border_color(p.edge.opacity(0.3))
            .text_size(px(12.));
        let (label, bar, figure) = match project {
            Some(project) => self.project_strip(project, p),
            None => self.agent_strip(session, p),
        };
        strip
            .child(
                div()
                    .flex_none()
                    .max_w(relative(0.55))
                    .text_ellipsis()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(div().flex_1().min_w(px(80.)).child(bar))
            .child(div().flex_none().text_color(p.subtle).child(figure))
            .children(project.map(|project| {
                let crew = self.progress.crew(project).len();
                button("crew")
                    .outline()
                    .xsmall()
                    .label(tree::crew_label(crew))
                    .selected(self.panel == Some(Panel::Crew))
                    .tooltip("Every ticket and the agents working on it")
                    .on_click(cx.listener(|v, _, w, c| v.toggle_panel(Panel::Crew, w, c)))
            }))
    }

    /// A project's phase, its bar with a segment per ticket, and its figure. Before any
    /// ticket exists the bar is the coordinator's own.
    fn project_strip(&self, project: &ProjectProgress, p: Palette) -> (String, Div, String) {
        let tickets = self.progress.tickets_of(project);
        let bar = if tickets.is_empty() {
            progress_bar(project.position, project.state, STRIP_BAR, p)
        } else {
            let segments: Vec<_> = tickets.iter().map(|t| (t.position, t.state)).collect();
            segmented_bar(&segments, STRIP_BAR, p)
        };
        (
            project.phase.label().into(),
            bar,
            tree::project_summary(project),
        )
    }

    /// An agent's own label and bar, or its ticket's stage and bar when it has nothing of
    /// its own to show.
    fn agent_strip(&self, session: &Session, p: Palette) -> (String, Div, String) {
        let agent = self.progress.agent(&session.id);
        let ticket = self
            .ticket_of(&session.id)
            .and_then(|ticket| self.progress.ticket(&ticket.id));
        let figure = tree::strip_figure(agent, ticket);
        let own = agent.and_then(|agent| Some((tree::strip_label(agent)?, agent)));
        match (own, ticket) {
            (Some((label, agent)), _) => {
                let bar = progress_bar(agent.position(), agent.state, STRIP_BAR, p);
                (label, bar, figure)
            }
            (None, Some(ticket)) => {
                let bar = progress_bar(Some(ticket.position), ticket.state, STRIP_BAR, p);
                (ticket.stage.label().into(), bar, figure)
            }
            (None, None) => {
                let empty = progress_bar(None, BarState::InProgress, STRIP_BAR, p);
                (String::new(), empty, figure)
            }
        }
    }

    pub(super) fn conversation(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(session) = self.selected_session() else {
            return div();
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.base)
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(p.overlay))
            .on_drop(cx.listener(|v, paths: &ExternalPaths, _, c| v.attach_files(paths.paths(), c)))
            .child(self.conversation_header(session, p, cx))
            .child(self.progress_strip(session, p, cx))
            .children(self.pagination(p, cx))
            .child(if self.list.item_count() == 0 {
                self.empty_conversation(session.role, p, cx)
                    .into_any_element()
            } else {
                self.transcript(session, p, cx).into_any_element()
            })
            .child(self.composer(session, p, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Placement::{Alone, Folded, Heads},
        compaction_line, first_line, forget_settled, from_human, peer_line, placements, report,
        run_heading, run_troubles, transcript_overlap,
    };
    use std::collections::BTreeSet;
    use workspace_core::{Attention, Message, Receipt};

    #[test]
    fn reports_split_into_kind_and_body() {
        assert_eq!(
            report("[ready_for_testing] Implementer · Toolbar\nresult.txt written"),
            Some(("ready_for_testing", "result.txt written"))
        );
        assert_eq!(report("[completed] celadon-web"), Some(("completed", "")));
    }

    #[test]
    fn ordinary_messages_are_not_reports() {
        assert_eq!(report("[x] A checklist item"), None);
        assert_eq!(report("[See the docs] for details\nmore"), None);
        assert_eq!(report("Plain text"), None);
    }

    #[test]
    fn long_errors_show_only_a_bounded_first_line() {
        assert_eq!(
            first_line("Provider exited\nTraceback…", 80),
            "Provider exited"
        );
        assert_eq!(first_line("abcdef", 3), "abc…");
    }

    fn message(id: &str, body: &str) -> Message {
        Message {
            sequence: 0,
            id: id.into(),
            project_id: "p".into(),
            sender: None,
            recipient: "s".into(),
            body: body.into(),
            receipt: Receipt::Completed,
            created_at: 0,
            attachments: vec![],
        }
    }

    #[test]
    fn host_messages_are_wiffletree_notices_not_the_humans() {
        assert!(from_human(&message("m1", "Hello")));
        assert!(from_human(&message("answer:a", "Human answer to Q: yes")));
        assert!(!from_human(&message("pr:t:merged:141", "PR #141 merged.")));
        assert!(!from_human(&message("verification:t:1:outcome", "Passed")));
        let mut reply = message("output:r", "Done");
        reply.sender = Some("agent".into());
        assert!(!from_human(&reply));
    }

    #[test]
    fn a_compaction_line_is_its_own_kind_of_entry_never_a_reply() {
        let own = |id: &str, body: &str| {
            let mut own = message(id, body);
            own.sender = Some("s".into());
            own
        };
        let compacted = own(
            "compaction:r",
            "Context compacted · 812k → 5k\nKept: The failing test names",
        );
        assert_eq!(
            compaction_line(&compacted),
            Some((
                "Context compacted",
                " · 812k → 5k",
                Some("Kept: The failing test names"),
                false
            ))
        );
        let failed = own("compaction:r", "Compaction failed · context unchanged");
        assert_eq!(
            compaction_line(&failed),
            Some(("Compaction failed", " · context unchanged", None, true))
        );
        let interrupted = own("compaction:r", "Compaction interrupted");
        assert_eq!(
            compaction_line(&interrupted),
            Some(("Compaction interrupted", "", None, true))
        );
        // A reply that merely says the same is still a reply, with its bubble and turn summary.
        assert_eq!(compaction_line(&own("output:r", "Context compacted")), None);
    }

    #[test]
    fn identical_refreshes_change_nothing() {
        let page = [message("a", "hi"), message("b", "there")];
        assert_eq!(transcript_overlap(&page, &page), Some((0, 2)));
    }

    #[test]
    fn refreshes_keep_the_unchanged_prefix() {
        let shown = [message("a", "hi"), message("b", "there")];
        let appended = [
            message("a", "hi"),
            message("b", "there"),
            message("c", "new"),
        ];
        assert_eq!(transcript_overlap(&shown, &appended), Some((0, 2)));
        let edited = [message("a", "hi"), message("b", "edited")];
        assert_eq!(transcript_overlap(&shown, &edited), Some((0, 1)));
    }

    #[test]
    fn a_full_page_drops_its_oldest_message() {
        let shown = [message("a", "1"), message("b", "2")];
        let refreshed = [message("b", "2"), message("c", "3")];
        assert_eq!(transcript_overlap(&shown, &refreshed), Some((1, 1)));
    }

    #[test]
    fn unrelated_pages_do_not_overlap() {
        let shown = [message("c", "3")];
        assert_eq!(transcript_overlap(&shown, &[message("a", "1")]), None);
        assert_eq!(transcript_overlap(&shown, &[]), None);
    }

    /// A message in session `me`'s transcript from `sender`; `None` is the human or Wiffletree.
    fn from(sender: Option<&str>, id: &str, body: &str) -> Message {
        let mut sent = message(id, body);
        sent.sender = sender.map(Into::into);
        sent
    }

    #[test]
    fn a_run_of_peer_messages_folds_under_its_first() {
        let transcript = [
            from(None, "m1", "Go ahead and build it."),
            from(
                Some("impl"),
                "report:impl:1",
                "[ready_for_testing] Implementer\nCommit 3f2a91c",
            ),
            from(
                Some("tester"),
                "report:tester:1",
                "[passed] Tester\nNo findings",
            ),
            from(Some("impl"), "agent:impl:2", "Which branch?"),
            from(Some("me"), "output:r1", "Accepted."),
            from(None, "m2", "Thanks"),
        ];
        assert_eq!(
            placements(&transcript, "me"),
            [Alone, Heads(3), Folded, Folded, Alone, Alone]
        );
    }

    #[test]
    fn a_single_peer_message_still_folds_into_a_row() {
        let transcript = [
            from(Some("me"), "output:r1", "Assigned."),
            from(Some("coordinator"), "assignment:me", "Ticket: Fix the list"),
        ];
        assert_eq!(placements(&transcript, "me"), [Alone, Heads(1)]);
        assert_eq!(
            run_heading(1, &["Coordinator".into()]),
            "1 agent message · Coordinator"
        );
    }

    #[test]
    fn the_sessions_own_reply_ends_a_run() {
        let transcript = [
            from(
                Some("tester"),
                "report:tester:1",
                "[failed] Tester\nBlocking",
            ),
            from(Some("me"), "output:r1", "Routing the finding."),
            from(
                Some("impl"),
                "report:impl:2",
                "[ready_for_testing] Implementer\nFixed",
            ),
        ];
        assert_eq!(placements(&transcript, "me"), [Heads(1), Alone, Heads(1)]);
    }

    #[test]
    fn timers_and_check_ins_are_peer_messages_but_own_output_is_not() {
        let transcript = [
            from(
                Some("me"),
                "timer:s1:100",
                "[timer] Watch CI\nYour timer s1 fired.",
            ),
            from(
                Some("impl"),
                "check-in:r9:30",
                "[check-in] Implementer has been in one turn for 30 minutes.",
            ),
            from(Some("me"), "error:r2", "The provider stopped."),
            from(Some("me"), "compaction:r3", "Context compacted · 812k → 5k"),
            from(Some("me"), "stopped:r4", "Stopped by your coordinator."),
        ];
        assert_eq!(
            placements(&transcript, "me"),
            [Heads(2), Folded, Alone, Alone, Alone]
        );
        let check_in = peer_line(&transcript[1]);
        assert_eq!(check_in.kind, Some("check_in"));
        assert_eq!(
            check_in.text,
            "Implementer has been in one turn for 30 minutes."
        );
    }

    #[test]
    fn wiffletree_notices_and_the_humans_messages_break_a_run() {
        let transcript = [
            from(
                Some("impl"),
                "report:impl:1",
                "[progress] Implementer\nHalfway",
            ),
            from(None, "pr:t:merged:141", "PR #141 merged."),
            from(
                Some("impl"),
                "report:impl:2",
                "[completed] Implementer\nDone",
            ),
            from(None, "m1", "Nice"),
        ];
        assert_eq!(
            placements(&transcript, "me"),
            [Heads(1), Alone, Heads(1), Alone]
        );
    }

    #[test]
    fn a_run_counts_what_failed_was_blocked_or_errored() {
        let run = [
            from(Some("a"), "report:a:1", "[failed] Reviewer\nBlocking: x"),
            from(Some("b"), "report:b:1", "[failed] Tester\nBlocking: y"),
            from(
                Some("c"),
                "report:c:1",
                "[blocked] Implementer\nWhich base?",
            ),
            from(Some("c"), "report:c:2", "[passed] Implementer\nfine"),
            from(
                Some("c"),
                "turn-result:r1",
                "Implementer stopped with an error: provider exited. Inspect its worktree.",
            ),
            from(Some("c"), "turn-result:r2", "Implementer finished a turn."),
        ];
        assert_eq!(run_troubles(&run), ["2 failed", "1 blocked", "1 error"]);
        assert_eq!(run_troubles(&run[3..4]), Vec::<String>::new());
        assert_eq!(peer_line(&run[4]).trouble, Some("error"));
        assert_eq!(peer_line(&run[5]).trouble, None);
    }

    #[test]
    fn a_runs_heading_names_each_sender_once() {
        let senders = ["Implementer", "Tester", "Implementer"].map(String::from);
        assert_eq!(
            run_heading(3, &senders),
            "3 agent messages · Implementer, Tester"
        );
    }

    fn decision(id: &str, project: &str) -> Attention {
        Attention {
            id: id.to_owned(),
            project_id: project.to_owned(),
            session_id: "s".to_owned(),
            host: String::new(),
            operation_id: String::new(),
            prompt: "Which way?".to_owned(),
            options: Vec::new(),
            answer: None,
        }
    }

    #[test]
    fn a_minimized_card_survives_another_decision_arriving() {
        let mut minimized = BTreeSet::from(["p1".to_owned()]);
        forget_settled(&mut minimized, &[decision("a", "p1")]);
        forget_settled(&mut minimized, &[decision("a", "p1"), decision("b", "p1")]);
        assert!(minimized.contains("p1"));
    }

    #[test]
    fn the_last_decision_settling_expands_the_next() {
        let mut minimized = BTreeSet::from(["p1".to_owned(), "p2".to_owned()]);
        forget_settled(&mut minimized, &[decision("b", "p2")]);
        assert_eq!(minimized, BTreeSet::from(["p2".to_owned()]));
    }
}
