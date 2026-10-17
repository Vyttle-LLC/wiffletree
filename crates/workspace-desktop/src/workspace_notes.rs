//! A workspace note about a shared setting sync paused, as the Inbox and the Settings window
//! show it: its words, Compare's two versions, and the choices that settle it.
use super::*;
use gpui_component::button::ButtonVariants;
use ui::hint;
use workspace_core::skills::Line;

/// What the human chose on a note.
#[derive(Clone, Copy)]
pub(crate) enum NoteAction {
    /// Opens Compare, or hides it when open.
    Compare,
    Settle(Settle),
}

/// Acts on the note whose path it is given; the view decides how.
pub(crate) type OnNote = Rc<dyn Fn(String, NoteAction, &mut Window, &mut App)>;

/// The note open in Compare, and its lines once the host answers.
pub(crate) type Compared = Option<(String, Option<Vec<Line>>)>;

/// Opens the note's comparison, answering with the request for its lines, or hides it when open.
pub(crate) fn toggle_compare(compared: &mut Compared, path: String) -> Option<Command> {
    if compared.as_ref().is_some_and(|(open, _)| *open == path) {
        *compared = None;
        return None;
    }
    *compared = Some((path.clone(), None));
    Some(Command::ComparePausedFile { path })
}

/// After a sync pass or a change to the notes, asks again for the open comparison, since either
/// version may have changed and settling uses the newest; closes it once its note is gone.
pub(crate) fn recompare<'a>(
    compared: &mut Compared,
    notes: impl IntoIterator<Item = &'a PausedSetting>,
) -> Option<Command> {
    let (path, lines) = compared.as_mut()?;
    if !notes.into_iter().any(|note| note.path == *path) {
        *compared = None;
        return None;
    }
    *lines = None;
    Some(Command::ComparePausedFile { path: path.clone() })
}

/// Closes the comparison a failed request was asking for, such as one whose note was settled in
/// the other window; the caller shows the host's error.
pub(crate) fn compare_failed(compared: &mut Compared, command: &Command) {
    if let Command::ComparePausedFile { path } = command
        && compared.as_ref().is_some_and(|(open, _)| open == path)
    {
        *compared = None;
    }
}

/// Keeps lines that answer the note still open.
pub(crate) fn compared_arrived(compared: &mut Compared, path: &str, lines: Vec<Line>) {
    if let Some((open, shown)) = compared
        && open == path
    {
        *shown = Some(lines);
    }
}

/// "MacBook Air" of "Mike’s MacBook Air", as a button names the other machine.
fn short(machine: &str) -> &str {
    (machine
        .split_once("’s ")
        .or_else(|| machine.split_once("'s ")))
    .map_or(machine, |(_, rest)| rest)
}

/// The choices a note offers, the main one last.
fn choices(note: &PausedSetting) -> Vec<(Settle, String, bool)> {
    match &note.paused {
        Paused::TwoMachines { machine } => vec![
            (
                Settle::UseTheOthers,
                format!("Use {}’s", short(machine)),
                false,
            ),
            (Settle::KeepThisMachines, "Keep this Mac’s".into(), true),
        ],
        Paused::OutsideWiffletree => vec![
            (
                Settle::PutBackSynced,
                "Put back the synced version".into(),
                false,
            ),
            (Settle::SyncThisVersion, "Sync this version".into(), true),
        ],
        Paused::CannotBeRead { .. } => {
            vec![(Settle::KeepThisMachines, "Keep this Mac’s".into(), false)]
        }
    }
}

/// The buttons that settle a note.
pub(crate) fn settle_buttons(note: &PausedSetting, on: &OnNote) -> Vec<Button> {
    let buttons = choices(note).into_iter().map(|(settle, label, main)| {
        let (on, path) = (on.clone(), note.path.clone());
        let id = SharedString::from(format!("settle-{}-{label}", note.path));
        let button = button(id).label(label);
        let button = if main {
            button.primary()
        } else {
            button.ghost()
        };
        button
            .on_click(move |_, window, cx| on(path.clone(), NoteAction::Settle(settle), window, cx))
    });
    buttons.collect()
}

/// Compare, or Hide `open` when the comparison shows.
pub(crate) fn compare_link(
    note: &PausedSetting,
    label: &'static str,
    open: &'static str,
    compared: &Compared,
    on: &OnNote,
) -> Button {
    let shown = compared
        .as_ref()
        .is_some_and(|(path, _)| *path == note.path);
    let (on, path) = (on.clone(), note.path.clone());
    button(SharedString::from(format!("compare-{}", note.path)))
        .link()
        .label(if shown { open } else { label })
        .on_click(move |_, window, cx| on(path.clone(), NoteAction::Compare, window, cx))
}

/// This Mac's version beside the other's, each with the lines only it has marked, once the host
/// has answered for this note.
pub(crate) fn comparison(note: &PausedSetting, compared: &Compared, p: Palette) -> Option<Div> {
    let (path, lines) = compared.as_ref()?;
    if *path != note.path {
        return None;
    }
    let Some(lines) = lines else {
        return Some(hint("Comparing…", p));
    };
    let other = match &note.paused {
        Paused::TwoMachines { machine } | Paused::CannotBeRead { machine, .. } => machine.clone(),
        Paused::OutsideWiffletree => "Last synced".into(),
    };
    let side = |title: String, mine: bool| {
        let rows = lines.iter().filter_map(|line| {
            let (text, only) = match line {
                Line::Unchanged(text) => (text, false),
                Line::Removed(text) if mine => (text, true),
                Line::Added(text) if !mine => (text, true),
                _ => return None,
            };
            let text = SharedString::from(if text.is_empty() { " " } else { text }.to_owned());
            let row = div().px_2().child(text);
            Some(match only {
                true => row.bg(p.yellow.opacity(0.18)),
                false => row.text_color(p.subtle),
            })
        });
        div()
            .flex_1()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(p.edge.opacity(0.55))
            .bg(p.base)
            .overflow_hidden()
            .child(
                div()
                    .px_2()
                    .py(px(3.))
                    .border_b_1()
                    .border_color(p.edge.opacity(0.45))
                    .bg(p.surface)
                    .text_size(px(11.))
                    .text_color(p.subtle)
                    .child(title),
            )
            .child(
                div()
                    .py_1()
                    .font_family(".SystemMonoFont")
                    .text_size(px(11.5))
                    .line_height(relative(1.55))
                    .children(rows),
            )
    };
    Some(
        div()
            .flex()
            .items_start()
            .gap_2()
            .child(side("This Mac".into(), true))
            .child(side(other, false)),
    )
}

/// Where the version not kept stays, under an open comparison of two machines' versions.
pub(crate) fn kept_note(note: &PausedSetting, compared: &Compared) -> Option<String> {
    let open = compared
        .as_ref()
        .is_some_and(|(path, _)| *path == note.path);
    if !open || !matches!(note.paused, Paused::TwoMachines { .. }) {
        return None;
    }
    let history = match note.setting.split_once(" · ") {
        Some((skill, _)) => format!(" and in {skill}’s history"),
        None => String::new(),
    };
    Some(format!(
        "The version you don’t keep stays in the repository’s history{history}."
    ))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{choices, compare_failed, kept_note, recompare, short};
    use workspace_core::skills::Line;
    use workspace_core::*;

    fn note(paused: Paused) -> PausedSetting {
        PausedSetting {
            path: "skills/implementing/".into(),
            setting: "Implementing · your rules".into(),
            paused,
            text: String::new(),
        }
    }

    #[test]
    fn each_note_offers_the_choices_its_reason_allows() {
        let machine = "Mike’s MacBook Air".to_owned();
        let offered = |paused| {
            let choices = choices(&note(paused));
            choices
                .into_iter()
                .map(|(s, label, _)| (s, label))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            offered(Paused::TwoMachines {
                machine: machine.clone()
            }),
            [
                (Settle::UseTheOthers, "Use MacBook Air’s".into()),
                (Settle::KeepThisMachines, "Keep this Mac’s".into())
            ]
        );
        assert_eq!(
            offered(Paused::OutsideWiffletree),
            [
                (Settle::PutBackSynced, "Put back the synced version".into()),
                (Settle::SyncThisVersion, "Sync this version".into())
            ]
        );
        let reason = "two verifiers have the focus Tests".to_owned();
        assert_eq!(
            offered(Paused::CannotBeRead { machine, reason }),
            [(Settle::KeepThisMachines, "Keep this Mac’s".into())]
        );
        assert_eq!(short("Build box"), "Build box");
    }

    #[test]
    fn an_open_comparison_says_where_the_version_not_kept_stays() {
        let two = note(Paused::TwoMachines {
            machine: "Mike’s MacBook Air".into(),
        });
        let open = Some((two.path.clone(), None));
        assert_eq!(
            kept_note(&two, &open).unwrap(),
            "The version you don’t keep stays in the repository’s history and in Implementing’s history."
        );
        assert_eq!(kept_note(&two, &None), None);
        assert_eq!(kept_note(&note(Paused::OutsideWiffletree), &open), None);
    }

    #[test]
    fn a_sync_asks_again_for_the_open_comparison_and_closes_it_with_its_note() {
        let two = note(Paused::TwoMachines {
            machine: "Mike’s MacBook Air".into(),
        });
        let mut compared = Some((two.path.clone(), Some(vec![Line::Added("Older.".into())])));
        let asked = recompare(&mut compared, std::slice::from_ref(&two));
        assert!(matches!(asked, Some(Command::ComparePausedFile { path }) if path == two.path));
        assert_eq!(compared, Some((two.path.clone(), None)));
        assert!(recompare(&mut compared, &[]).is_none());
        assert_eq!(compared, None);
    }

    #[test]
    fn a_failed_compare_closes_the_comparison_it_asked_for() {
        let ask = |path: &str| Command::ComparePausedFile { path: path.into() };
        let mut compared = Some(("skills/a/".to_owned(), None));
        compare_failed(&mut compared, &ask("skills/b/"));
        assert!(compared.is_some());
        compare_failed(&mut compared, &Command::Snapshot);
        assert!(compared.is_some());
        compare_failed(&mut compared, &ask("skills/a/"));
        assert_eq!(compared, None);
    }
}
