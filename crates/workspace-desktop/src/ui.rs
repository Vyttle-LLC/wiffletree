//! Shared visual primitives and the window layout.
use super::*;
use crate::welcome::StartScreen;
use gpui_component::{
    Icon, IconName, Selectable, Size,
    button::ButtonVariants,
    resizable::{h_resizable, resizable_panel},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::LazyLock,
    time::{Duration, Instant},
};

/// Each animation step redraws the views that show one, so the spinners and pulses step at this
/// rate instead of the display's.
const ANIMATION_FPS: u32 = 15;
/// One turn of a spinner.
const SPIN: Duration = Duration::from_millis(800);
/// The moment every animation's phase counts from, so they all step together.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// The views that drew an animation since the last step. One timer steps them all, so the
/// frame rate holds however many animations run, and it stops once none is drawn.
#[derive(Default)]
struct AnimationClock {
    views: HashSet<EntityId>,
    running: bool,
}
impl Global for AnimationClock {}

/// How far through a `period` the shared clock is, from 0 to 1. Asks for the current view to be
/// drawn again at the next step; with reduced motion it stays at 0 and asks for nothing.
pub(super) fn animation_phase(period: Duration, window: &Window, cx: &mut App) -> f32 {
    if cx.reduce_motion() {
        return 0.;
    }
    let clock = cx.default_global::<AnimationClock>();
    clock.views.insert(window.current_view());
    if !clock.running {
        clock.running = true;
        cx.spawn(async move |cx| {
            let step = Duration::from_secs(1) / ANIMATION_FPS;
            loop {
                let since = EPOCH.elapsed().as_nanos() % step.as_nanos();
                let wait = step - Duration::from_nanos(since as u64);
                cx.background_executor().timer(wait).await;
                let stepped = cx.update(|cx| {
                    let clock = cx.global_mut::<AnimationClock>();
                    let views = std::mem::take(&mut clock.views);
                    clock.running = !views.is_empty();
                    for view in &views {
                        cx.notify(*view);
                    }
                    !views.is_empty()
                });
                if !stepped {
                    break;
                }
            }
        })
        .detach();
    }
    let period = period.as_nanos();
    (EPOCH.elapsed().as_nanos() % period) as f32 / period as f32
}

/// gpui-component's loading spinner, turning on the shared animation clock.
#[derive(IntoElement)]
pub(super) struct Spinner {
    size: Size,
    icon: Icon,
    color: Option<Hsla>,
}
impl Spinner {
    pub(super) fn new() -> Self {
        Self {
            size: Size::Medium,
            icon: Icon::new(IconName::Loader),
            color: None,
        }
    }
    pub(super) fn icon(mut self, icon: Icon) -> Self {
        self.icon = icon;
        self
    }
    pub(super) fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}
impl Sizable for Spinner {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}
impl RenderOnce for Spinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let turn = ease_in_out(animation_phase(SPIN, window, cx));
        div().child(
            self.icon
                .with_size(self.size)
                .when_some(self.color, |icon, color| icon.text_color(color))
                .transform(Transformation::rotate(percentage(turn))),
        )
    }
}

/// A button showing `icon`, or while `loading` this module's spinner in its place, sized as
/// gpui-component sizes button icons. It keeps gpui-component's loading state, inert and faded,
/// but its stock spinner draws at the display's full rate, so the button is given no icon then.
pub(super) fn icon_or_spinner(
    button: Button,
    icon: Icon,
    label: Option<&'static str>,
    loading: bool,
    size: Size,
) -> Button {
    if !loading {
        return button
            .icon(icon)
            .when_some(label, |b, label| b.label(label));
    }
    button
        .loading(true)
        .child(Spinner::new().with_size(size))
        .when_some(label, |b, label| {
            b.child(
                div()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(label),
            )
        })
}

/// An icon from `assets/icons`. Each name's path is made once, so drawing one allocates nothing.
pub(super) fn icon(name: &'static str) -> Icon {
    thread_local! {
        static PATHS: RefCell<HashMap<&'static str, &'static str>> = RefCell::default();
    }
    let path = PATHS.with_borrow_mut(|paths| {
        *(paths.entry(name)).or_insert_with(|| format!("icons/{name}.svg").leak())
    });
    Icon::default().path(path).size(px(16.))
}

// Use the full-color image renderer; GPUI's SVG icon element applies a single tint.
pub(super) fn brand_image(path: &'static str, width: f32, height: f32) -> Img {
    img(ImageSource::Resource(Resource::Embedded(path.into())))
        .w(px(width))
        .h(px(height))
        .flex_none()
        .object_fit(ObjectFit::Contain)
}
pub(super) fn eyebrow(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.subtle)
        .child(text.into())
}
pub(super) fn hint(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .text_size(px(12.))
        .line_height(relative(1.5))
        .text_color(p.subtle)
        .child(text.into())
}
/// A setting in plain words: its label and control on one line, and a one-line caption.
pub(super) fn setting(
    label: &'static str,
    control: impl IntoElement,
    caption: &'static str,
    p: Palette,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(label)
                .child(control),
        )
        .child(hint(caption, p))
}
/// A bar's fill color: green when finished, red when blocked, otherwise `active`.
pub(super) fn bar_color(state: BarState, active: Hsla, p: Palette) -> Hsla {
    match state {
        BarState::Finished => p.green,
        BarState::Blocked => p.red,
        BarState::InProgress | BarState::Indeterminate => active,
    }
}
/// A progress track `height` px tall, filled to `position` in its state's color. An
/// indeterminate bar is striped across its whole width; the stripes do not move.
pub(super) fn progress_bar(position: Option<u16>, state: BarState, height: f32, p: Palette) -> Div {
    let track = div()
        .w_full()
        .flex_none()
        .h(px(height))
        .rounded_full()
        .overflow_hidden()
        .bg(p.edge.opacity(0.6));
    if state == BarState::Indeterminate {
        let stripes = pattern_slash(p.subtle.opacity(0.75), height, height * 2.);
        return track.child(div().size_full().bg(stripes));
    }
    let filled = f32::from(position.unwrap_or(0)) / f32::from(FULL_POSITION);
    track.child(
        div()
            .h_full()
            .w(relative(filled))
            .rounded_full()
            .bg(bar_color(state, p.focus, p)),
    )
}
/// One bar of equal segments, each filled to its own position in its own state.
pub(super) fn segmented_bar(segments: &[(u16, BarState)], height: f32, p: Palette) -> Div {
    let segment = |&(position, state): &(u16, BarState)| {
        div()
            .flex_1()
            .min_w_0()
            .child(progress_bar(Some(position), state, height, p))
    };
    div()
        .w_full()
        .flex()
        .gap(px(2.))
        .children(segments.iter().map(segment))
}
pub(super) fn card(p: Palette) -> Div {
    div()
        .w_full()
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(p.edge.opacity(0.4))
        .bg(p.base)
}
pub(super) fn property(label: &'static str, value: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .flex()
        .gap_3()
        .justify_between()
        .text_size(px(12.))
        .child(div().flex_none().text_color(p.subtle).child(label))
        .child(div().min_w_0().text_right().child(value.into()))
}
/// A section heading; callers append a trailing action when the section has one.
pub(super) fn section(title: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .min_h(px(24.))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(eyebrow(title, p))
}
pub(super) fn pill(text: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(7.))
        .py(px(1.))
        .rounded_full()
        .bg(color.opacity(0.12))
        .text_color(color)
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
}
pub(super) fn mono(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .min_w_0()
        .font_family(".SystemMonoFont")
        .text_size(px(11.))
        .text_color(p.subtle)
        .text_ellipsis()
        .child(text.into())
}
pub(super) fn empty_state(
    icon_name: &'static str,
    title: &'static str,
    body: impl Into<SharedString>,
    p: Palette,
) -> Div {
    div()
        .w_full()
        .py_8()
        .px_4()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .text_center()
        .child(icon(icon_name).size(px(22.)).text_color(p.subtle))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(hint(body, p))
}
/// A tinted, full-width notice. Callers add the message and its actions.
pub(super) fn banner(color: Hsla) -> Div {
    div()
        .w_full()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(color.opacity(0.35))
        .bg(color.opacity(0.08))
        .text_size(px(12.5))
}
/// One option in a small either/or control.
pub(super) fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
) -> Button {
    let option = button(id).xsmall().label(label);
    if on {
        option.primary().selected(true)
    } else {
        option
    }
}
pub(super) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub(super) fn age(at: i64) -> String {
    let minutes = (now() - at).max(0) / 60_000;
    if minutes == 0 {
        "Just now".into()
    } else if minutes < 60 {
        format!("{minutes}m ago")
    } else if minutes < 1440 {
        format!("{}h ago", minutes / 60)
    } else {
        format!("{}d ago", minutes / 1440)
    }
}
/// Time until a future moment, "in 25m"; "now" once it has come.
pub(super) fn from_now(at: i64) -> String {
    let minutes = ((at - now()).max(0) as u64).div_ceil(60_000);
    match minutes {
        0 => "now".into(),
        1..60 => format!("in {minutes}m"),
        60..1440 => format!("in {}h {:02}m", minutes / 60, minutes % 60),
        _ => format!("in {}d", minutes / 1440),
    }
}
/// Turns a host identifier such as `ready_for_testing` into a readable label.
pub(super) fn humanize(identifier: &str) -> String {
    let text = identifier.replace('_', " ");
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
/// Reasoning effort as shown in menus; provider identifiers such as `xhigh` are not words.
pub(super) fn effort_label(effort: &str) -> String {
    match effort {
        "xhigh" => "Extra high".into(),
        other => humanize(other),
    }
}
pub(super) fn session_icon(role: Role) -> &'static str {
    match role {
        Role::ProjectOrchestrator => "project",
        Role::TaskOrchestrator => "folder",
        _ => "agent",
    }
}
/// Only activity gets color. Yellow is reserved for what needs a person, and a blocked worker
/// reports to its coordinator rather than to you.
pub(super) fn status_color(status: Status, p: Palette) -> Hsla {
    match status {
        Status::Working => p.focus,
        _ => p.subtle,
    }
}
fn status_icon(status: Status) -> &'static str {
    match status {
        Status::Ready => "ready",
        Status::Working => "loader-circle",
        Status::Blocked => "blocked",
        Status::Done => "check",
        Status::Paused => "pause",
        Status::Disconnected => "disconnected",
    }
}
/// The host calls a session whose turn was cut short "disconnected"; to the person who pressed
/// Stop or Pause, it was interrupted.
pub(super) fn status_label(status: Status) -> &'static str {
    match status {
        Status::Disconnected => "Interrupted",
        other => other.label(),
    }
}
/// Working spins so activity is visible at a glance; reduced motion leaves it still.
pub(super) fn status_glyph(status: Status, size: f32, p: Palette) -> AnyElement {
    let color = status_color(status, p);
    if status == Status::Working {
        Spinner::new()
            .icon(icon(status_icon(status)))
            .with_size(px(size))
            .color(color)
            .into_any_element()
    } else {
        icon(status_icon(status))
            .size(px(size))
            .text_color(color)
            .into_any_element()
    }
}
/// The project orchestrator's open questions and approvals: the one signal that asks for you.
pub(super) fn needs_you_badge(count: usize, p: Palette) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(3.))
        .pl(px(4.))
        .pr(px(6.))
        .py(px(1.))
        .rounded_full()
        .bg(p.yellow)
        .text_color(p.on_accent)
        .text_size(px(10.5))
        .font_weight(FontWeight::BOLD)
        .child(icon("attention").size(px(12.)))
        .child(count.to_string())
}
fn status_badge(status: Status, label: String, p: Palette) -> Div {
    let color = status_color(status, p);
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(5.))
        .px_2()
        .py(px(3.))
        .rounded_full()
        .bg(color.opacity(0.10))
        .text_color(color)
        .text_size(px(11.))
        .child(status_glyph(status, 12., p))
        .child(label)
}
/// What a session's badge shows. A running compaction reads as work, though the agent's own
/// status is unchanged under it; an idle coordinator whose team is working says so.
fn badge_state(own: Status, shown: Status, compacting: bool) -> (Status, &'static str) {
    if compacting {
        (Status::Working, "Compacting")
    } else if shown == own {
        (shown, status_label(shown))
    } else {
        (shown, "Team working")
    }
}
pub(super) fn ticket_state_color(state: TicketState, p: Palette) -> Hsla {
    match state {
        TicketState::Accepted | TicketState::Passed => p.green,
        TicketState::Blocked | TicketState::Failed => p.red,
        TicketState::Assigned | TicketState::ReadyForTesting | TicketState::Completed => p.focus,
        TicketState::Planned | TicketState::Verifying | TicketState::Closed => p.subtle,
    }
}

/// A session's status badge; an idle coordinator whose team is working says so. Questions
/// it still waits on share the status color: they go to its coordinator, not to you.
pub(super) fn session_badge(shared: &Shared, session: &Session, p: Palette) -> Div {
    let (status, label) = badge_state(
        session.status,
        shared.shown_status(session),
        shared.compacting(&session.id),
    );
    let questions = shared
        .snapshot()
        .map_or(0, |s| s.questions_waiting(&session.id));
    status_badge(status, crate::tree::badge_label(label, questions), p)
}

/// How a cached child view is laid out: it fills its slot, which its parent sizes.
pub(super) fn full() -> StyleRefinement {
    StyleRefinement::default().size_full()
}

/// A full-width page about the whole workspace, shown in place of the conversation.
pub(super) fn page<V: EventEmitter<Intent>>(
    title: &'static str,
    subtitle: &'static str,
    body: impl IntoElement,
    footer: Option<Div>,
    p: Palette,
    cx: &mut Context<V>,
) -> Div {
    const COLUMN: f32 = 980.;
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(p.base)
        .child(
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
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(p.subtle)
                                .child(format!("Workspace  ›  {subtitle}")),
                        )
                        .child(
                            div()
                                .text_size(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        ),
                )
                .child(
                    button("close-page")
                        .ghost()
                        .icon(icon("close"))
                        .tooltip("Back to the conversation")
                        .on_click(
                            cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Conversation))),
                        ),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .px_6()
                .py_5()
                .child(div().w_full().max_w(px(COLUMN)).mx_auto().child(body)),
        )
        .when_some(footer, |d, footer| {
            d.child(
                div()
                    .flex_none()
                    .px_6()
                    .py_3()
                    .border_t_1()
                    .border_color(p.edge.opacity(0.3))
                    .child(footer.w_full().max_w(px(COLUMN)).mx_auto()),
            )
        })
}

impl Workspace {
    /// Shown in place of the conversation until a session can be selected.
    fn start_screen(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let kind = self.start_screen_kind(cx);
        let content = if let Some(error) = &self.load_error {
            empty_state(
                "triangle-alert",
                "The workspace could not open",
                error.clone(),
                p,
            )
        } else if kind == StartScreen::Opening {
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_color(p.subtle)
                .child(Spinner::new().small())
                .child("Opening your workspace…")
        } else if kind == StartScreen::Checking {
            Self::checking_clis(p)
        } else {
            let welcome = kind != StartScreen::StartWithProject;
            div()
                .max_w(px(420.))
                .when(welcome, |d| d.w_full().max_w(px(440.)))
                .flex()
                .flex_col()
                .items_start()
                .gap_3()
                .child(brand_image(
                    if is_dark(cx) {
                        "brand/symbol-dark.svg"
                    } else {
                        "brand/symbol-light.svg"
                    },
                    80.,
                    40.,
                ))
                .child(
                    div()
                        .mt_2()
                        .text_size(px(26.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(if welcome {
                            "Welcome to Wiffletree."
                        } else {
                            "Start with a project."
                        }),
                )
                .map(|d| {
                    if welcome {
                        let signed_in = kind == StartScreen::WelcomeWithCli;
                        return d.child(self.welcome_steps(signed_in, p, cx));
                    }
                    d.child(hint(
                        "A project is one outcome: a feature, a bug, a migration. Its coordinator plans the work with you and runs each ticket in its own repository worktree with its own agents.",
                        p,
                    ))
                    .child(
                        button("first-project")
                            .primary()
                            .icon(icon("plus"))
                            .label("New project")
                            .mt_2()
                            .on_click(cx.listener(|v, _, w, c| v.new_project(w, c))),
                    )
                })
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(p.base)
            .child(content)
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::render_counts::count("Workspace");
        let p = self.colors(cx);
        let main = match self.shared.page {
            Page::Usage => self.usage.clone().cached(full()).into_any_element(),
            Page::Models => self.models.clone().cached(full()).into_any_element(),
            Page::Skills => self.skills.clone().cached(full()).into_any_element(),
            Page::Repositories => self.repositories.clone().cached(full()).into_any_element(),
            Page::Conversation if self.shared.selected_session().is_some() => {
                // Not cached: GPUI draws every view inside a cached view that renders again,
                // so the transcript is cached only below an uncached conversation.
                self.conversation.clone().into_any_element()
            }
            Page::Conversation => self.start_screen(p, cx).into_any_element(),
        };
        // Session tools make no sense beside a page about the whole workspace.
        let session_tools = self.shared.page == Page::Conversation;
        div()
            .size_full()
            .bg(p.base)
            .text_color(p.text)
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .flex()
            .flex_col()
            .child(TitleBar::new())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div().flex_1().min_w_0().h_full().child(
                            h_resizable("workspace-split-v2")
                                .child(
                                    resizable_panel()
                                        .size(px(264.))
                                        .size_range(px(240.)..px(500.))
                                        .child(self.sidebar.clone().cached(full())),
                                )
                                .child(
                                    resizable_panel().size_range(px(620.)..Pixels::MAX).child(
                                        h_resizable("conversation-split-v2")
                                            .child(
                                                resizable_panel()
                                                    .size_range(px(340.)..Pixels::MAX)
                                                    .child(main),
                                            )
                                            .child(
                                                resizable_panel()
                                                    .size(px(372.))
                                                    .size_range(px(300.)..px(560.))
                                                    .visible(
                                                        session_tools
                                                            && self.shared.panel.is_some(),
                                                    )
                                                    .when(
                                                        session_tools
                                                            && self.shared.panel.is_some(),
                                                        |d| {
                                                            d.child(
                                                                self.inspector
                                                                    .clone()
                                                                    .cached(full()),
                                                            )
                                                        },
                                                    ),
                                            ),
                                    ),
                                ),
                        ),
                    )
                    .when(session_tools, |d| d.child(self.inspector_rail(p, cx))),
            )
            .children(file_links::link_menu(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{badge_state, humanize};
    use workspace_core::Status;

    #[test]
    fn a_running_compaction_reads_as_compacting_over_the_agents_own_status() {
        for own in [Status::Ready, Status::Blocked, Status::Paused] {
            assert_eq!(badge_state(own, own, true), (Status::Working, "Compacting"));
        }
        // Before and after, the badge is the agent's own; a queued request changes nothing.
        assert_eq!(
            badge_state(Status::Working, Status::Working, false),
            (Status::Working, "Working")
        );
        assert_eq!(
            badge_state(Status::Blocked, Status::Blocked, false),
            (Status::Blocked, "Blocked")
        );
        assert_eq!(
            badge_state(Status::Disconnected, Status::Disconnected, false),
            (Status::Disconnected, "Interrupted")
        );
        assert_eq!(
            badge_state(Status::Ready, Status::Working, false),
            (Status::Working, "Team working")
        );
    }

    #[test]
    fn host_identifiers_read_as_labels() {
        assert_eq!(humanize("ready_for_testing"), "Ready for testing");
        assert_eq!(humanize("accepted"), "Accepted");
        assert_eq!(humanize(""), "");
        assert_eq!(super::effort_label("xhigh"), "Extra high");
        assert_eq!(super::effort_label("medium"), "Medium");
    }
}
