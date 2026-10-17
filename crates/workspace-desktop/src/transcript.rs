//! The selected session's transcript: its messages, prepared as rows when a page arrives, the
//! older pages, and the live activity row while the agent works.
use super::*;
use activity::{Card, RunActivity, STEP_PACE, StepFetch, paced_reply};
use conversation::{COLUMN, first_line};
use file_links::Linked;
use gpui_component::button::ButtonVariants;
use std::{collections::HashMap, sync::Arc, time::Duration};
use ui::{age, brand_image, humanize, icon, pill, session_icon};

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

/// A peer message is coordination traffic rather than conversation: anything another session
/// sent this one (reports and verdicts, turn results, check-ins, direct messages, a
/// coordinator's assignments) and this session's own timer fires. The human's messages,
/// Wiffletree's notices and the session's own replies, errors and compaction lines are not.
fn peer_message(message: &Message, session_id: &str) -> bool {
    message.sender.as_deref().is_some_and(|sender| {
        (sender != session_id && host_notice(&message.id).is_none())
            || message.id.starts_with(TIMER_PREFIX)
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
    let kind = MessageKind::of(&message.id, &message.body);
    let trouble = match kind {
        MessageKind::Report(ReportKind::Failed) => Some("failed"),
        MessageKind::Report(ReportKind::Blocked) => Some("blocked"),
        MessageKind::TurnResult { failed: true } => Some("error"),
        _ => None,
    };
    PeerLine {
        kind: kind.name(),
        text: kind.text(&message.body),
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
fn run_heading(count: usize, senders: &[SharedString]) -> String {
    let mut names: Vec<&str> = vec![];
    for sender in senders {
        if !names.contains(&sender.as_ref()) {
            names.push(sender);
        }
    }
    format!(
        "{count} agent message{} · {}",
        if count == 1 { "" } else { "s" },
        names.join(", ")
    )
}

/// Messages without a sender are the human's, apart from Wiffletree's own notices.
fn from_human(message: &Message) -> bool {
    message.sender.is_none() && host_notice(&message.id).is_none()
}

/// How a row not in a run of peer messages draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Human,
    /// From Wiffletree itself, such as the PR watcher.
    Notice,
    Compaction,
    /// From an agent: this session's own reply, or another agent's report.
    Agent {
        own: bool,
    },
}

/// A message as the transcript draws it. Everything here is worked out when its page arrives,
/// off the UI thread, so drawing a row parses nothing and reads nothing from disk.
pub(super) struct Row {
    placement: Placement,
    shape: Shape,
    /// The sending agent's name, a timer's label, or the notice's sender.
    sender: SharedString,
    role: Option<Role>,
    /// The kind's label, and whether it says something went wrong.
    pill: Option<(SharedString, bool)>,
    receipt: SharedString,
    /// The first line of what it says, for its line in an opened run.
    first_line: SharedString,
    /// What it says, its file references linked; none when blank.
    body: Option<Linked>,
    /// A run's heading and what went wrong across it, on the run's first row.
    run: Option<(String, Vec<String>)>,
    run_id: SharedString,
    line_id: SharedString,
}

/// Who may have sent the messages on a page, and where each one's file references resolve.
struct Senders {
    session_id: String,
    names: HashMap<String, (SharedString, Role)>,
    bases: HashMap<String, Arc<[PathBuf]>>,
    /// For messages from the human and Wiffletree: the selected session's.
    current: Arc<[PathBuf]>,
}
impl Senders {
    fn of(shared: &Shared, session_id: &str, messages: &[Message]) -> Self {
        let mut names = HashMap::new();
        let mut bases = HashMap::new();
        for sender in messages.iter().filter_map(|m| m.sender.as_ref()) {
            if !bases.contains_key(sender) {
                if let Some(session) = shared.session(sender) {
                    names.insert(sender.clone(), (session.name.clone().into(), session.role));
                }
                bases.insert(sender.clone(), shared.link_bases(sender).into());
            }
        }
        Self {
            session_id: session_id.to_owned(),
            names,
            bases,
            current: shared.link_bases(session_id).into(),
        }
    }
    fn bases(&self, message: &Message) -> Arc<[PathBuf]> {
        let sender = message.sender.as_ref().and_then(|id| self.bases.get(id));
        sender.unwrap_or(&self.current).clone()
    }
}

/// The rows for a page of messages, aligned with them.
fn prepare(messages: &[Message], senders: &Senders) -> Vec<Row> {
    let placements = placements(messages, &senders.session_id);
    let named = |message: &Message| {
        let sender = message.sender.as_ref()?;
        senders.names.get(sender)
    };
    let mut rows: Vec<Row> = messages
        .iter()
        .zip(&placements)
        .map(|(message, &placement)| {
            let kind = MessageKind::of(&message.id, &message.body);
            let own = message.sender.as_deref() == Some(senders.session_id.as_str());
            let shape = if from_human(message) {
                Shape::Human
            } else if message.sender.is_none() {
                Shape::Notice
            } else if kind == MessageKind::Compaction {
                Shape::Compaction
            } else {
                Shape::Agent { own }
            };
            let peer = placement != Placement::Alone;
            let line = peer_line(message);
            let tagged = matches!(kind, MessageKind::Report(_) | MessageKind::Timer);
            let (pill, text) = match shape {
                _ if peer => {
                    let pill = line
                        .kind
                        .map(|kind| (humanize(kind).into(), line.trouble.is_some()));
                    (pill, line.text)
                }
                Shape::Agent { own: false } if tagged => {
                    let pill = kind.name().map(|kind| (humanize(kind).into(), false));
                    (pill, kind.text(&message.body))
                }
                _ => (None, message.body.as_str()),
            };
            let shown = match shape {
                _ if peer => true,
                Shape::Notice => true,
                Shape::Compaction => false,
                Shape::Human | Shape::Agent { .. } => !text.trim().is_empty(),
            };
            let sender = match (shape, kind.timer_label(&message.body), named(message)) {
                (Shape::Notice, ..) => host_notice(&message.id).unwrap_or("Wiffletree").into(),
                (_, Some(label), _) => label.to_owned().into(),
                (_, None, Some((name, _))) => name.clone(),
                _ => "Agent".into(),
            };
            Row {
                placement,
                shape,
                sender,
                role: named(message).map(|(_, role)| *role),
                pill,
                receipt: format!("{:?}", message.receipt).into(),
                first_line: first_line(line.text, 160).into(),
                body: shown.then(|| Linked::new(&message.id, text, senders.bases(message))),
                run: None,
                run_id: format!("peer-run-{}", message.id).into(),
                line_id: format!("peer-line-{}", message.id).into(),
            }
        })
        .collect();
    for (head, placement) in placements.iter().enumerate() {
        if let Placement::Heads(count) = *placement {
            let run = head..(head + count).min(messages.len());
            let names: Vec<SharedString> = rows[run.clone()]
                .iter()
                .map(|row| row.sender.clone())
                .collect();
            rows[head].run = Some((run_heading(run.len(), &names), run_troubles(&messages[run])));
        }
    }
    rows
}

/// A page of messages with the rows that draw them.
#[derive(Clone, Default)]
struct Loaded {
    messages: Rc<Vec<Message>>,
    rows: Rc<Vec<Row>>,
}

fn avatar(icon_name: &'static str, color: Hsla) -> Div {
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

fn receipt(message: &Message, row: &Row, p: Palette) -> Div {
    meta(row.receipt.clone(), p).text_color(receipt_color(message.receipt, p))
}

/// The human's own message: a right-aligned bubble and its files, with its delivery state.
fn human_message(message: &Message, row: &Row, p: Palette) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_end()
        .gap_1()
        .when_some(row.body.as_ref(), |d, body| {
            d.child(
                div()
                    .max_w(px(620.))
                    .px_4()
                    .py_2()
                    .rounded_xl()
                    .bg(p.overlay)
                    .child(body.view()),
            )
        })
        .when(!message.attachments.is_empty(), |d| {
            d.child(attachment_chips::sent(message, p).max_w(px(620.)))
        })
        .child(
            div()
                .flex()
                .gap_2()
                .child(receipt(message, row, p))
                .child(meta(age(message.created_at), p)),
        )
}

/// A notice from Wiffletree itself, such as the PR watcher's.
fn notice_message(message: &Message, row: &Row, p: Palette) -> Div {
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
                                .child(row.sender.clone()),
                        )
                        .child(div().flex_1())
                        .child(receipt(message, row, p))
                        .child(meta(age(message.created_at), p)),
                )
                .children(row.body.as_ref().map(Linked::view)),
        )
}

/// One line where a finished turn's summary would be, never a bubble; red when it failed.
fn compaction_message(line: CompactionLine, p: Palette) -> Div {
    let CompactionLine {
        title,
        rest,
        kept,
        failed,
    } = line;
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
fn agent_message(message: &Message, row: &Row, own: bool, p: Palette) -> Div {
    div()
        .w_full()
        .flex()
        .gap_3()
        .child(avatar(
            row.role.map_or("agent", session_icon),
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
                                .child(row.sender.clone()),
                        )
                        .when_some(row.role.filter(|_| !own), |d, role| {
                            d.child(meta(role.label(), p))
                        })
                        .when_some(row.pill.as_ref(), |d, (kind, _)| {
                            d.child(pill(kind.clone(), p.focus))
                        })
                        .child(div().flex_1())
                        .when(!own, |d| d.child(receipt(message, row, p)))
                        .child(meta(age(message.created_at), p)),
                )
                .children(row.body.as_ref().map(Linked::view)),
        )
}

/// The peer-message runs and lines the human opened, by the run's first message id and by
/// message id. Kept in memory only.
#[derive(Clone, Default)]
pub(super) struct OpenPeers {
    runs: BTreeSet<String>,
    lines: BTreeSet<String>,
}

/// A run of peer messages as one line where a turn summary would be. Opened, it lists a line
/// per message, and an opened line shows that message in full.
fn peer_run(
    run: &[Message],
    rows: &[Row],
    open: &OpenPeers,
    p: Palette,
    view: WeakEntity<Transcript>,
) -> Div {
    let head = run[0].id.clone();
    let (heading, troubles) = rows[0].run.clone().unwrap_or_default();
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
                .id(rows[0].run_id.clone())
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
                .child(div().min_w_0().text_ellipsis().child(heading))
                // Long sender names give way, never the counts or the time.
                .children(troubles.into_iter().map(|trouble| {
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
                    .children(run.iter().zip(rows).map(|(message, row)| {
                        peer_line_row(message, row, &head, open, p, view.clone())
                    })),
            )
        })
}

/// One message of an opened run: sender, kind, first line and time; opened, its full text.
fn peer_line_row(
    message: &Message,
    row: &Row,
    head: &str,
    open: &OpenPeers,
    p: Palette,
    view: WeakEntity<Transcript>,
) -> Div {
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
                .id(row.line_id.clone())
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
                        .child(row.sender.clone()),
                )
                .when_some(row.pill.as_ref(), |d, (kind, trouble)| {
                    d.child(pill(kind.clone(), if *trouble { p.red } else { p.focus }))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .child(row.first_line.clone()),
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
                    .children(
                        row.body
                            .as_ref()
                            .map(|body| body.view().text_size(px(12.5))),
                    ),
            )
        })
}

/// A page of messages being prepared: which ask it answers, for which session and page.
struct PageAsk {
    number: u64,
    session_id: String,
    page: Option<i64>,
}
impl PageAsk {
    /// Shown only if no page was asked for since and the reader is still on its session and page.
    fn wanted(&self, latest: u64, selected: Option<&str>, page: Option<i64>) -> bool {
        self.number == latest && selected == Some(self.session_id.as_str()) && self.page == page
    }
}

/// The selected session's messages, a page at a time, and its live activity.
pub(super) struct Transcript {
    bridge: Bridge,
    shared: Shared,
    /// The composer's draft, which an empty conversation's suggestions fill in.
    input: Entity<TextareaState>,
    shown: Loaded,
    open_peers: Rc<OpenPeers>,
    list: ListState,
    /// The newest page of the sessions shown lately, so going back to one draws at once.
    cached_pages: BTreeMap<String, Loaded>,
    cache_order: VecDeque<String>,
    /// The older page being read, by the sequence it ends before; `None` for the newest.
    page: Option<i64>,
    /// Pages are prepared off the UI thread; counts the asks so only the latest is shown.
    preparing: u64,
    /// The selected session's latest run, shown as the activity card while it works.
    activity: Option<RunActivity>,
    step_fetch: StepFetch,
    activity_clock: bool,
    /// The open turn history, which follows step changes while it is shown.
    history: Option<WeakEntity<history_sheet::HistorySheet>>,
    /// Finished runs' durations and step counts, keyed by run.
    run_summaries: Rc<BTreeMap<String, RunSummary>>,
}
impl EventEmitter<Intent> for Transcript {}
impl Transcript {
    pub(super) fn new(
        bridge: Bridge,
        input: Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_global_in::<HumanWrote>(window, |view, window, cx| {
            view.back_to_latest(window, cx)
        })
        .detach();
        Self {
            bridge,
            shared: Shared::default(),
            input,
            shown: Loaded::default(),
            open_peers: Rc::default(),
            list: ListState::new(0, ListAlignment::Bottom, px(300.)),
            cached_pages: BTreeMap::new(),
            cache_order: VecDeque::new(),
            page: None,
            preparing: 0,
            activity: None,
            step_fetch: StepFetch::default(),
            activity_clock: false,
            history: None,
            run_summaries: Rc::default(),
        }
    }

    /// A new selection shows its last page at once and loads its newest; each snapshot loads
    /// the shown page again and follows the agent's work.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let (selection, moved) = shared.moved_from(&self.shared);
        self.shared = shared;
        if selection {
            self.show_selected(window, cx);
        } else if moved {
            self.sync_transcript_rows();
            self.load_messages(window, cx);
            // A turn that just started has no steps yet; ask before the first one arrives.
            if self.selected_working() && !self.activity.as_ref().is_some_and(|a| a.running) {
                self.fetch_steps(window, cx);
            }
            self.sync_activity_clock(window, cx);
        }
        if selection || moved {
            cx.notify();
        }
    }

    fn show_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.shared.selected.clone() else {
            return;
        };
        self.shown = self.cached_pages.get(&id).cloned().unwrap_or_default();
        self.page = None;
        self.list.reset(self.transcript_rows());
        self.load_messages(window, cx);
        self.activity = None;
        self.fetch_steps(window, cx);
        self.sync_activity_clock(window, cx);
    }

    /// The messages shown, the newest page unless an older one is being read.
    pub(super) fn messages(&self) -> &[Message] {
        &self.shown.messages
    }

    /// Messages from the human that wait for a stopped project.
    pub(super) fn waiting_messages(&self) -> usize {
        (self.shown.messages.iter())
            .filter(|m| from_human(m) && m.receipt == Receipt::Queued)
            .count()
    }

    /// Transcript rows are the loaded messages plus a trailing activity row while the agent works.
    fn transcript_rows(&self) -> usize {
        self.shown.messages.len() + usize::from(self.selected_working())
    }

    fn sync_transcript_rows(&mut self) {
        let (messages, rows) = (self.shown.messages.len(), self.transcript_rows());
        let shown = self.list.item_count();
        if shown < messages {
            self.list.reset(rows);
        } else if shown != rows {
            self.list.splice(messages..shown, rows - messages);
        }
    }

    pub(super) fn load_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session_id) = self.shared.selected.clone() {
            self.request(
                Command::Messages {
                    session_id,
                    before: self.page,
                    limit: MAX_PAGE,
                },
                window,
                cx,
            );
        }
    }

    /// Goes back to the newest page, as after something the human sent.
    pub(super) fn show_latest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = None;
        self.load_messages(window, cx);
    }

    /// After a change the human made, a reader of an older page goes back to the newest.
    fn back_to_latest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.take().is_some() {
            self.load_messages(window, cx);
        }
    }

    /// Prepares a page's rows off the UI thread, then shows it if it is still the one wanted.
    fn prepare(
        &mut self,
        session_id: String,
        page: Option<i64>,
        messages: Vec<Message>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preparing += 1;
        let ask = PageAsk {
            number: self.preparing,
            session_id,
            page,
        };
        let senders = Senders::of(&self.shared, &ask.session_id, &messages);
        let rows = cx.background_spawn(async move {
            let rows = prepare(&messages, &senders);
            (messages, rows)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (messages, rows) = rows.await;
            let _ = this.update_in(cx, |view, window, cx| {
                if !ask.wanted(view.preparing, view.shared.selected.as_deref(), view.page) {
                    return;
                }
                let shown = Loaded {
                    messages: Rc::new(messages),
                    rows: Rc::new(rows),
                };
                view.replace_page(shown.clone());
                cx.emit(Intent::MessagesShown);
                view.load_run_summaries(window, cx);
                if ask.page.is_none() {
                    view.cache(ask.session_id, shown);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn cache(&mut self, session_id: String, page: Loaded) {
        self.cached_pages.insert(session_id.clone(), page);
        self.cache_order.retain(|id| id != &session_id);
        self.cache_order.push_back(session_id);
        while self.cache_order.len() > 4 {
            if let Some(old) = self.cache_order.pop_front() {
                self.cached_pages.remove(&old);
            }
        }
    }

    /// Splices only what a refresh changed, so a reader scrolled up keeps their place.
    fn replace_page(&mut self, page: Loaded) {
        let shown = std::mem::replace(&mut self.shown, page);
        let Some((dropped, unchanged)) = transcript_overlap(&shown.messages, &self.shown.messages)
        else {
            self.list.reset(self.transcript_rows());
            return;
        };
        if dropped > 0 {
            self.list.splice(0..dropped, 0);
        }
        // An unchanged message whose run grew, or lost its head, draws a different row.
        for row in 0..unchanged {
            let placed = |page: &Loaded, row: usize| page.rows.get(row).map(|r| r.placement);
            if placed(&shown, row + dropped) != placed(&self.shown, row) {
                self.list.remeasure_items(row..row + 1);
            }
        }
        let (shown_rows, rows) = (self.list.item_count(), self.transcript_rows());
        if unchanged < self.shown.messages.len() || shown_rows != rows {
            self.list.splice(unchanged..shown_rows, rows - unchanged);
        }
    }

    /// Opens or closes a peer-message run, or one line within it, and measures its row again.
    fn toggle_peers(&mut self, head: &str, line: Option<&str>, cx: &mut Context<Self>) {
        let peers = Rc::make_mut(&mut self.open_peers);
        let (open, id) = match line {
            Some(line) => (&mut peers.lines, line),
            None => (&mut peers.runs, head),
        };
        if !open.remove(id) {
            open.insert(id.to_owned());
        }
        if let Some(row) = self.shown.messages.iter().position(|m| m.id == head)
            && row < self.list.item_count()
        {
            self.list.remeasure_items(row..row + 1);
        }
        cx.notify();
    }

    /// Asks for the selected session's steps changed since the last page.
    pub(super) fn fetch_steps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.shared.selected.clone() else {
            return;
        };
        let Some(wait) = self.step_fetch.begin(STEP_PACE) else {
            return;
        };
        let after = self
            .activity
            .as_ref()
            .filter(|a| a.session_id == session_id)
            .map_or(0, |a| a.revision);
        let command = Command::Steps {
            session_id: session_id.clone(),
            run_id: None,
            after,
        };
        let page = paced_reply(
            self.bridge.clone(),
            command,
            wait,
            cx.background_executor().clone(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let result = page.await;
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| {
                    view.accept_steps(&session_id, result, window, cx)
                })
            });
        })
        .detach();
        // The open history sheet follows the same changes.
        if let Some(sheet) = self.history.as_ref().and_then(WeakEntity::upgrade) {
            sheet.update(cx, |sheet, cx| sheet.fetch(window, cx));
        }
    }

    fn accept_steps(
        &mut self,
        session_id: &str,
        result: Result<StepPage, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owed = self.step_fetch.finish();
        match result {
            Ok(page) if self.shared.selected.as_deref() == Some(session_id) => {
                let activity = match &mut self.activity {
                    Some(activity) if activity.session_id == session_id => activity,
                    other => other.insert(RunActivity::new(session_id)),
                };
                activity.merge(page);
                cx.notify();
            }
            Ok(_) => {}
            Err(error) => eprintln!("Work steps: {error}"),
        }
        if owed {
            self.fetch_steps(window, cx);
        }
    }

    /// Ticks the elapsed time once a second, only while the selected session works.
    fn sync_activity_clock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity_clock || !self.selected_working() {
            return;
        }
        self.activity_clock = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let ticking = this.update(cx, |view, cx| {
                    view.activity_clock = view.selected_working();
                    if view.activity_clock {
                        cx.notify();
                        if let Some(sheet) = view.history.as_ref().and_then(WeakEntity::upgrade) {
                            sheet.update(cx, |_, cx| cx.notify());
                        }
                    }
                    view.activity_clock
                });
                if !matches!(ticking, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn selected_working(&self) -> bool {
        self.page.is_none()
            && (self.shared.selected_session()).is_some_and(|s| s.status == Status::Working)
    }

    fn activity_card(&self) -> Card {
        self.activity
            .as_ref()
            .filter(|a| self.shared.selected.as_ref() == Some(&a.session_id))
            .map(RunActivity::card)
            .unwrap_or_default()
    }

    /// Asks for the summaries of finished turns on the loaded page that are not known yet.
    fn load_run_summaries(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.shared.selected.clone() else {
            return;
        };
        let run_ids: Vec<String> = (self.shown.messages.iter())
            .filter_map(|m| reply_run(&m.id))
            .filter(|run| !self.run_summaries.contains_key(*run))
            .map(str::to_owned)
            .collect();
        if !run_ids.is_empty() {
            self.request(
                Command::RunSummaries {
                    session_id,
                    run_ids,
                },
                window,
                cx,
            );
        }
    }

    fn accept_run_summaries(&mut self, summaries: Vec<RunSummary>) {
        let cache = Rc::make_mut(&mut self.run_summaries);
        // Finished runs never change, so the cache only needs a bound.
        if cache.len() > 1_000 {
            cache.clear();
        }
        for summary in summaries {
            let reply = reply_id(&summary.run_id);
            cache.insert(summary.run_id.clone(), summary);
            // The reply's row gains a summary line; the list measures it again.
            if let Some(row) = self.shown.messages.iter().position(|m| m.id == reply)
                && row < self.list.item_count()
            {
                self.list.splice(row..row + 1, 1);
            }
        }
    }

    /// Opens a turn's history over the conversation; the draft and selection stay as they are.
    pub(super) fn open_history(
        &mut self,
        run_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.shared.selected_session().cloned() else {
            return;
        };
        let link_bases = self.shared.link_bases(&session.id).into();
        let sheet = cx.new(|_| {
            history_sheet::HistorySheet::new(self.bridge.clone(), &session.id, run_id, link_bases)
        });
        sheet.update(cx, |sheet, cx| sheet.fetch(window, cx));
        self.history = Some(sheet.downgrade());
        let title = format!("{} · this turn", session.name);
        window.open_sheet(cx, move |panel, _, _| {
            panel
                .size(px(560.))
                .title(title.clone())
                .child(sheet.clone())
        });
    }

    pub(super) fn toggle_activity(&mut self, cx: &mut Context<Self>) {
        let preferences = cx.global_mut::<Preferences>();
        preferences.minimize_activity = !preferences.minimize_activity;
        if let Err(error) = preferences.save() {
            eprintln!("Saving preferences: {error}");
        }
        // The trailing row changed height; the list measures it again.
        let shown = self.shown.messages.len();
        if self.list.item_count() > shown {
            self.list.splice(shown..shown + 1, 1);
        }
        cx.notify();
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
        let Loaded { messages, rows } = self.shown.clone();
        let card = self.activity_card();
        let summaries = self.run_summaries.clone();
        let minimized = cx.global::<Preferences>().minimize_activity;
        let view = cx.entity().downgrade();
        let role = session.role;
        let open = self.open_peers.clone();
        let transcript = list(self.list.clone(), move |ix, _, _| {
            let content = match (messages.get(ix), rows.get(ix)) {
                (Some(message), Some(row)) => match (row.placement, row.shape) {
                    // A run draws in its head's row; the rest of its rows stay empty.
                    (Placement::Folded, _) => return div().into_any_element(),
                    (Placement::Heads(count), _) => {
                        let run = ix..(ix + count).min(messages.len());
                        peer_run(&messages[run.clone()], &rows[run], &open, p, view.clone())
                    }
                    (_, Shape::Human) => human_message(message, row, p),
                    (_, Shape::Notice) => notice_message(message, row, p),
                    (_, Shape::Compaction) => {
                        compaction_message(CompactionLine::parse(&message.body), p)
                    }
                    (_, Shape::Agent { own }) => {
                        let reply = agent_message(message, row, own, p);
                        // A reply whose turn did something shows how long and how much, above it.
                        let summary = reply_run(&message.id)
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
                },
                // The row after the last message appears only while the agent works.
                _ => div()
                    .w_full()
                    .flex()
                    .gap_3()
                    .child(avatar(session_icon(role), p.accent))
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
        let older = self.shown.messages.len() == MAX_PAGE;
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
                                v.page = v.shown.messages.first().map(|m| m.sequence);
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
                            .on_click(cx.listener(|v, _, w, c| v.show_latest(w, c))),
                    )
                })
        })
    }
}

impl Requests for Transcript {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        command: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = match reply {
            Ok(value) => value,
            Err(error) => return toast_error(error, window, cx),
        };
        match command {
            Command::Messages {
                session_id, before, ..
            } if self.shared.selected.as_deref() == Some(&session_id) && self.page == before => {
                match serde_json::from_value::<Vec<Message>>(value) {
                    Ok(messages) => self.prepare(session_id, before, messages, window, cx),
                    Err(error) => toast_error(error.to_string(), window, cx),
                }
            }
            Command::RunSummaries { .. } => match serde_json::from_value(value) {
                Ok(summaries) => self.accept_run_summaries(summaries),
                Err(error) => toast_error(error.to_string(), window, cx),
            },
            _ => {}
        }
    }
}

impl Render for Transcript {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Transcript");
        let p = palette(is_dark(cx));
        let Some(session) = self.shared.selected_session().cloned() else {
            return div();
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .children(self.pagination(p, cx))
            .child(if self.list.item_count() == 0 {
                self.empty_conversation(session.role, p, cx)
                    .into_any_element()
            } else {
                self.transcript(&session, p, cx).into_any_element()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PageAsk,
        Placement::{Alone, Folded, Heads},
        Senders, Shape, from_human, peer_line, placements, prepare, run_heading, run_troubles,
        transcript_overlap,
    };
    use gpui::SharedString;
    use std::{collections::HashMap, sync::Arc};
    use workspace_core::{Message, Receipt, Role};

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
    fn a_prepared_page_is_shown_only_if_it_is_still_the_one_wanted() {
        let ask = PageAsk {
            number: 2,
            session_id: "s".into(),
            page: Some(40),
        };
        assert!(ask.wanted(2, Some("s"), Some(40)));
        assert!(
            !ask.wanted(3, Some("s"), Some(40)),
            "a later page was asked for"
        );
        assert!(
            !ask.wanted(2, Some("t"), Some(40)),
            "another session is selected"
        );
        assert!(
            !ask.wanted(2, Some("s"), None),
            "the reader went back to the newest"
        );
        assert!(!ask.wanted(2, None, Some(40)), "nothing is selected");
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
            run_heading(1, &[SharedString::from("Coordinator")]),
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
        let senders = ["Implementer", "Tester", "Implementer"].map(SharedString::from);
        assert_eq!(
            run_heading(3, &senders),
            "3 agent messages · Implementer, Tester"
        );
    }

    /// A page prepared for session `me`, with the senders' names and `bases` for every link.
    fn prepared(messages: &[Message], bases: &std::path::Path) -> Vec<super::Row> {
        let bases: Arc<[std::path::PathBuf]> = vec![bases.to_owned()].into();
        let names = [
            ("impl", "Implementer", Role::Implementer),
            ("me", "Coordinator", Role::ProjectOrchestrator),
        ];
        let senders = Senders {
            session_id: "me".into(),
            names: (names.iter())
                .map(|(id, name, role)| (id.to_string(), (SharedString::from(*name), *role)))
                .collect(),
            bases: HashMap::new(),
            current: bases,
        };
        prepare(messages, &senders)
    }

    #[test]
    fn a_page_is_prepared_with_what_each_row_draws() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("notes.md"), "").unwrap();
        let page = [
            from(None, "m1", "Look at `notes.md`"),
            from(None, "pr:t:merged:141", "PR #141 merged."),
            from(
                Some("impl"),
                "report:impl:1",
                "[failed] Implementer\nBlocking",
            ),
            from(
                Some("me"),
                "timer:t1:5",
                "[timer] Watch CI\nYour timer fired.",
            ),
            from(Some("me"), "output:r1", "Done."),
            from(Some("me"), "compaction:r2", "Context compacted"),
        ];
        let rows = prepared(&page, folder.path());
        let shapes: Vec<_> = rows.iter().map(|row| (row.placement, row.shape)).collect();
        assert_eq!(
            shapes,
            [
                (Alone, Shape::Human),
                (Alone, Shape::Notice),
                (Heads(2), Shape::Agent { own: false }),
                (Folded, Shape::Agent { own: true }),
                (Alone, Shape::Agent { own: true }),
                (Alone, Shape::Compaction),
            ]
        );
        // The file the human named is a link already, so drawing it reads nothing from disk.
        let linked = rows[0].body.as_ref().unwrap().text();
        assert!(linked.starts_with("Look at [`notes.md`](<") && linked.contains("notes.md>)"));
        assert_eq!(rows[1].sender.as_ref(), "Wiffletree PR watcher");
        // A run's lines: a report says it failed; a timer is named by its label.
        assert_eq!(rows[2].pill, Some(("Failed".into(), true)));
        assert_eq!(rows[2].body.as_ref().unwrap().text(), "Blocking");
        assert_eq!(rows[3].sender.as_ref(), "Watch CI");
        assert_eq!(rows[3].first_line.as_ref(), "Your timer fired.");
        assert_eq!(
            rows[2].run,
            Some((
                "2 agent messages · Implementer, Watch CI".into(),
                vec!["1 failed".into()]
            ))
        );
        assert_eq!(
            (rows[4].sender.as_ref(), rows[4].pill.is_none()),
            ("Coordinator", true)
        );
        assert!(rows[5].body.is_none());
    }
}
