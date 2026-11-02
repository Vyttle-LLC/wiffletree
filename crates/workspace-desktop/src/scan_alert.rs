//! An alert's card (OpenSpec change `one-space-repository`, decision 5), as the top of each
//! project's Inbox and the Brains page show it: the check, rule, file, line and who pushed it,
//! never the matched text. It offers Open the file, Not sensitive and How to clean history.
use super::*;
use gpui_component::Sizable;
use gpui_component::button::ButtonVariants;
use gpui_component::input::{Input, InputState};
use ui::{card, hint, pill};

/// Wiffletree's page on cleaning a slip out of Git history.
const CLEAN_HISTORY: &str =
    "https://github.com/Vyttle-LLC/wiffletree/blob/main/docs/clean-history.md";

/// What the human did on a card.
pub(super) enum AlertAction {
    /// Opens the reason's line.
    NotSensitive,
    SaveMark,
    Cancel,
}

pub(super) type OnAlert = Rc<dyn Fn(AlertAction, &mut Window, &mut App)>;

/// The one card's reason line open in a view, by the card's key.
pub(super) struct AlertForm {
    open: Option<String>,
    reason: Entity<InputState>,
}
impl AlertForm {
    pub(super) fn new<V: 'static>(window: &mut Window, cx: &mut Context<V>) -> Self {
        Self {
            open: None,
            reason: cx.new(|cx| InputState::new(window, cx).placeholder("Why it is not sensitive")),
        }
    }
    pub(super) fn close(&mut self) {
        self.open = None;
    }
}

/// An alert's key among the cards of a view.
fn key(alert: &ScanAlert) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        alert.repository, alert.path, alert.check, alert.rule
    )
}

/// Whether `command` settles the form it came from.
pub(super) fn settles(command: &Command) -> bool {
    matches!(command, Command::MarkAlertNotSensitive { .. })
}

/// Acts on a card for `view`, whose form `form` reaches.
pub(super) fn act<V: Requests>(
    view: &mut V,
    form: fn(&mut V) -> &mut AlertForm,
    alert: ScanAlert,
    action: AlertAction,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    match action {
        AlertAction::NotSensitive => {
            let form = form(view);
            form.open = Some(key(&alert));
            form.reason
                .update(cx, |input, cx| input.set_value("", window, cx));
            cx.notify();
        }
        AlertAction::SaveMark => {
            let reason = form(view).reason.read(cx).value().trim().to_owned();
            let command = Command::MarkAlertNotSensitive {
                repository: alert.repository,
                path: alert.path,
                check: alert.check,
                rule: alert.rule,
                reason,
            };
            view.request(command, window, cx);
        }
        AlertAction::Cancel => {
            form(view).close();
            cx.notify();
        }
    }
}

/// The card.
pub(super) fn alert_card(alert: &ScanAlert, form: &AlertForm, on: OnAlert, p: Palette) -> Div {
    let opened = form.open.as_deref() == Some(key(alert).as_str());
    let id = |name: &str| SharedString::from(format!("alert-{name}-{}", key(alert)));
    let action = |name: &str, label: &'static str, action: fn() -> AlertAction| {
        let on = on.clone();
        button(id(name))
            .ghost()
            .xsmall()
            .label(label)
            .on_click(move |_, window, cx| on(action(), window, cx))
    };
    let link = |name: &str, label: &'static str, url: String| {
        button(id(name))
            .ghost()
            .xsmall()
            .label(label)
            .on_click(move |_, _, cx| cx.open_url(&url))
    };
    let mut actions = div().flex().flex_wrap().gap_2();
    if opened {
        actions = actions
            .child(action("mark", "Mark it", || AlertAction::SaveMark))
            .child(action("cancel", "Cancel", || AlertAction::Cancel));
    } else {
        actions = actions
            .child(link("open", "Open the file", alert.url()))
            .child(action("not-sensitive", "Not sensitive", || {
                AlertAction::NotSensitive
            }))
            .child(link(
                "clean",
                "How to clean history",
                CLEAN_HISTORY.to_owned(),
            ));
    }
    card(p)
        .bg(p.surface)
        .border_color(p.red.opacity(0.55))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(px(font::TEXT))
                        .child("Something on main failed a scan"),
                )
                .child(pill("Reached GitHub", p.red)),
        )
        .child(hint(alert.repository.clone(), p))
        .child(hint(alert.found(), p))
        .child(hint(
            "Removing it adds a new commit. The old text stays in Git history until someone cleans the history.",
            p,
        ))
        .when(opened, |d| d.child(Input::new(&form.reason).small()))
        .child(actions)
}

/// The note that a check cannot run on what this Mac pulls.
pub(super) fn stopped_card(stopped: &ScanStopped, p: Palette) -> Div {
    card(p)
        .bg(p.surface)
        .border_color(p.yellow.opacity(0.55))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(hint(format!("Workspace · {}", stopped.check), p))
                .child(pill("Not scanning", p.yellow)),
        )
        .child(
            div()
                .text_size(px(font::TEXT))
                .line_height(relative(1.5))
                .child(stopped.text.clone()),
        )
}
