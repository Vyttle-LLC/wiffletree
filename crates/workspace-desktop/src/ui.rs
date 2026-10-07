//! Shared visual primitives and the window layout.
use super::*;
use gpui_component::{
    Icon, Selectable,
    button::ButtonVariants,
    resizable::{h_resizable, resizable_panel},
    spinner::Spinner,
};

pub(super) fn icon(name: &str) -> Icon {
    Icon::default()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(16.))
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
    icon_name: &str,
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
pub(super) fn status_color(status: Status, p: Palette) -> Hsla {
    match status {
        Status::Working => p.focus,
        Status::Done => p.green,
        Status::Blocked => p.yellow,
        _ => p.subtle,
    }
}
pub(super) fn status_icon(status: Status) -> &'static str {
    match status {
        Status::Ready => "ready",
        Status::Working => "working",
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
pub(super) fn status_badge(status: Status, p: Palette) -> Div {
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
        .child(icon(status_icon(status)).size(px(12.)))
        .child(status_label(status))
}
pub(super) fn ticket_state_color(state: &str, p: Palette) -> Hsla {
    match state {
        "accepted" | "passed" => p.green,
        "blocked" | "failed" => p.red,
        "assigned" | "ready_for_testing" | "completed" => p.focus,
        _ => p.subtle,
    }
}

impl Workspace {
    /// Sessions to list: archived ones appear only while the sidebar is showing them.
    pub(super) fn shown<'a>(&'a self, snapshot: &'a Snapshot) -> impl Iterator<Item = &'a Session> {
        snapshot
            .sessions
            .iter()
            .filter(|s| self.show_archived || !s.archived)
    }
    pub(super) fn owner_name(&self) -> String {
        self.selected_session()
            .map(|s| s.name.clone())
            .unwrap_or_default()
    }
    pub(super) fn project_name(&self) -> String {
        let project = self.project_id();
        self.snapshot
            .iter()
            .flat_map(|s| &s.projects)
            .find(|p| Some(&p.id) == project.as_ref())
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }
    pub(super) fn repository(&self) -> Option<&Repository> {
        let id = self.selected_session()?.repository_id.as_ref()?;
        self.snapshot
            .as_ref()?
            .repositories
            .iter()
            .find(|r| &r.id == id)
    }
    /// The ticket a worker session belongs to.
    pub(super) fn ticket_of(&self, session_id: &str) -> Option<&Ticket> {
        let ticket_id = self.runtime(session_id)?.ticket_id.as_ref()?;
        self.snapshot
            .as_ref()?
            .tickets
            .iter()
            .find(|t| &t.id == ticket_id)
    }
    /// A full-width page about the whole workspace, shown in place of the conversation.
    fn page(
        &self,
        title: &'static str,
        subtitle: &'static str,
        body: Div,
        footer: Option<Div>,
        p: Palette,
        cx: &mut Context<Self>,
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
                            .on_click(cx.listener(|v, _, _, c| {
                                v.view = Page::Conversation;
                                c.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("page-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
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

    /// Shown in place of the conversation until a session can be selected.
    fn start_screen(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let content = if let Some(error) = &self.load_error {
            empty_state(
                "triangle-alert",
                "The workspace could not open",
                error.clone(),
                p,
            )
        } else if self.snapshot.is_none() {
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_color(p.subtle)
                .child(Spinner::new().small())
                .child("Opening your workspace…")
        } else {
            div()
                .max_w(px(420.))
                .flex()
                .flex_col()
                .items_start()
                .gap_3()
                .child(brand_image(
                    if self.is_dark(cx) {
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
                        .child("Start with a project."),
                )
                .child(hint(
                    "A project is one outcome: a feature, a bug, a migration. Its main coordinator plans the work with you and runs a team in each repository.",
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.colors(cx);
        let main = match self.view {
            Page::Usage => self.page(
                "Usage",
                "Account limits and tokens across every project",
                self.usage_page(p, cx),
                None,
                p,
                cx,
            ),
            Page::Models => self.page(
                "Models",
                "Defaults for new agents in every project",
                self.models_panel(p, cx),
                Some(self.models_footer(p, cx)),
                p,
                cx,
            ),
            Page::Repositories => self.page(
                "Repositories",
                "Repositories every project can use",
                self.repositories_page(p, cx),
                None,
                p,
                cx,
            ),
            Page::Conversation if self.selected_session().is_some() => {
                self.conversation(p, window, cx)
            }
            Page::Conversation => self.start_screen(p, cx),
        };
        // Session tools make no sense beside a page about the whole workspace.
        let session_tools = self.view == Page::Conversation;
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
                                        .child(self.sidebar(p, cx)),
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
                                                    .visible(session_tools && self.panel.is_some())
                                                    .when_some(
                                                        self.panel.filter(|_| session_tools),
                                                        |d, panel| {
                                                            d.child(
                                                                self.panel_view(
                                                                    panel, p, window, cx,
                                                                ),
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
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::humanize;

    #[test]
    fn host_identifiers_read_as_labels() {
        assert_eq!(humanize("ready_for_testing"), "Ready for testing");
        assert_eq!(humanize("accepted"), "Accepted");
        assert_eq!(humanize(""), "");
        assert_eq!(super::effort_label("xhigh"), "Extra high");
        assert_eq!(super::effort_label("medium"), "Medium");
    }
}
