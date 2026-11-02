//! A held log entry's card (OpenSpec change `one-space-repository`), as the Inbox of the project
//! that wrote it and the Brains page show it: the file, line, check and rule, never the matched
//! text. It offers Edit it and Discard, with Not sensitive for a `sensitive-info` finding only.
use super::*;
use gpui_component::Sizable;
use gpui_component::button::ButtonVariants;
use gpui_component::input::{Input, InputState};
use ui::{card, hint, mono, pill};

/// What the human did on a card.
pub(super) enum HeldAction {
    /// Opens the entry's text.
    Edit,
    SaveEdit,
    /// Opens the reason's line.
    NotSensitive,
    SaveMark,
    Discard,
    Cancel,
}

pub(super) type OnHeld = Rc<dyn Fn(HeldAction, &mut Window, &mut App)>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Opened {
    Edit,
    NotSensitive,
}

/// The one card's form open in a view, by the entry's path.
pub(super) struct HeldForm {
    open: Option<(String, Opened)>,
    text: Entity<TextareaState>,
    reason: Entity<InputState>,
}
impl HeldForm {
    pub(super) fn new<V: 'static>(window: &mut Window, cx: &mut Context<V>) -> Self {
        Self {
            open: None,
            text: cx.new(|cx| TextareaState::new(window, cx).auto_grow(6, 20)),
            reason: cx.new(|cx| InputState::new(window, cx).placeholder("Why it is not sensitive")),
        }
    }
    pub(super) fn close(&mut self) {
        self.open = None;
    }
}

/// Whether `command` settles the form it came from.
pub(super) fn settles(command: &Command) -> bool {
    matches!(
        command,
        Command::EditHeldEntry { .. }
            | Command::MarkEntryNotSensitive { .. }
            | Command::DiscardHeldEntry { .. }
    )
}

/// What the card says was found: the check, rule and line, or why the check could not run.
pub(super) fn found_line(entry: &HeldEntry) -> String {
    let at = entry
        .line
        .map_or(String::new(), |line| format!(", line {line}"));
    match (&entry.rule, &entry.reason) {
        (Some(rule), _) => format!("{}, rule {rule}{at}", entry.check),
        (None, Some(reason)) => format!("{} could not run: {reason}", entry.check),
        (None, None) => format!("{} could not run", entry.check),
    }
}

/// Acts on a card for `view`, whose form `held` reaches.
pub(super) fn act<V: Requests>(
    view: &mut V,
    held: fn(&mut V) -> &mut HeldForm,
    entry: HeldEntry,
    action: HeldAction,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let HeldEntry {
        repository, path, ..
    } = entry;
    match action {
        HeldAction::Edit => {
            let ask = ask::held_entry_text(repository, path.clone());
            view.ask(ask, window, cx, move |view, text, window, cx| {
                match text {
                    Ok(text) => {
                        let form = held(view);
                        form.open = Some((path, Opened::Edit));
                        form.text
                            .update(cx, |input, cx| input.set_value(text, window, cx));
                    }
                    Err(error) => toast_error(error, window, cx),
                }
                cx.notify();
            });
        }
        HeldAction::SaveEdit => {
            let text = held(view).text.read(cx).value().to_string();
            let command = Command::EditHeldEntry {
                repository,
                path,
                text,
            };
            view.request(command, window, cx);
        }
        HeldAction::NotSensitive => {
            let form = held(view);
            form.open = Some((path, Opened::NotSensitive));
            form.reason
                .update(cx, |input, cx| input.set_value("", window, cx));
            cx.notify();
        }
        HeldAction::SaveMark => {
            let reason = held(view).reason.read(cx).value().trim().to_owned();
            let command = Command::MarkEntryNotSensitive {
                repository,
                path,
                reason,
            };
            view.request(command, window, cx);
        }
        HeldAction::Discard => {
            view.request(Command::DiscardHeldEntry { repository, path }, window, cx);
        }
        HeldAction::Cancel => {
            held(view).close();
            cx.notify();
        }
    }
}

/// The card.
pub(super) fn held_entry_card(entry: &HeldEntry, form: &HeldForm, on: OnHeld, p: Palette) -> Div {
    let opened = (form.open.as_ref())
        .filter(|(path, _)| *path == entry.path)
        .map(|(_, opened)| *opened);
    let id = |name: &str| SharedString::from(format!("held-{name}-{}", entry.path));
    let action = |name: &str, label: &'static str, action: fn() -> HeldAction| {
        let on = on.clone();
        button(id(name))
            .ghost()
            .xsmall()
            .label(label)
            .on_click(move |_, window, cx| on(action(), window, cx))
    };
    let mut actions = div().flex().flex_wrap().gap_2();
    if let Some(opened) = opened {
        let save = match opened {
            Opened::Edit => action("save", "Save and scan again", || HeldAction::SaveEdit),
            Opened::NotSensitive => action("mark", "Mark it", || HeldAction::SaveMark),
        };
        actions = actions
            .child(save)
            .child(action("cancel", "Cancel", || HeldAction::Cancel));
    } else {
        actions = actions.child(action("edit", "Edit it", || HeldAction::Edit));
        if entry.offers_not_sensitive() {
            actions = actions.child(action("not-sensitive", "Not sensitive", || {
                HeldAction::NotSensitive
            }));
        }
        actions = actions.child(action("discard", "Discard", || HeldAction::Discard));
    }
    card(p)
        .bg(p.surface)
        .border_color(p.yellow.opacity(0.55))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(px(font::TEXT))
                        .child("A log entry was held on this Mac"),
                )
                .child(pill("Held", p.yellow)),
        )
        .child(hint(found_line(entry), p))
        .child(mono(entry.path.clone(), p))
        .when(opened == Some(Opened::Edit), |d| {
            d.child(Textarea::new(&form.text).w_full())
        })
        .when(opened == Some(Opened::NotSensitive), |d| {
            d.child(Input::new(&form.reason).small())
        })
        .child(actions)
}

#[cfg(test)]
mod tests {
    use super::found_line;
    use workspace_core::HeldEntry;

    fn held(rule: Option<&str>, reason: Option<&str>) -> HeldEntry {
        HeldEntry {
            repository: "Sage/x".into(),
            path: "wiffletree/second-brain/logs/app/2026/11/a.md".into(),
            project_id: None,
            check: "sensitive-info".into(),
            rule: rule.map(str::to_owned),
            line: rule.map(|_| 9),
            reason: reason.map(str::to_owned),
        }
    }

    #[test]
    fn the_card_names_the_rule_and_line_and_what_could_not_run() {
        assert_eq!(
            found_line(&held(Some("phone"), None)),
            "sensitive-info, rule phone, line 9"
        );
        let missing = held(None, Some("gitleaks is not installed"));
        assert_eq!(
            found_line(&missing),
            "sensitive-info could not run: gitleaks is not installed"
        );
        assert!(held(Some("phone"), None).offers_not_sensitive());
        assert!(!missing.offers_not_sensitive());
        let secret = HeldEntry {
            check: "secret-scan".into(),
            ..held(Some("generic-api-key"), None)
        };
        assert!(!secret.offers_not_sensitive());
    }
}
