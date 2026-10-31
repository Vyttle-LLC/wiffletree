//! Keeps typing in the composer while the transcript's text is clicked or selected.
//!
//! A press in selectable text focuses that text view, so keys would stop reaching the composer.
//! The text view must hold focus while a selection exists, since only it handles ⌘C. So a click
//! that selects nothing gives focus straight back, and a selection keeps it only until the next
//! typed character, which moves to the composer.

use gpui::Keystroke;

/// Whether the composer is owed focus back from the transcript's text.
#[derive(Default)]
pub(crate) struct ComposerFocus {
    owed: bool,
}

impl ComposerFocus {
    /// A press landed in the transcript; the composer is owed focus if it held it.
    pub(crate) fn pressed(&mut self, composer_focused: bool) {
        self.owed |= composer_focused;
    }

    /// The press ended. Returns whether focus goes back to the composer now: it does unless text
    /// is selected, which keeps focus for ⌘C.
    pub(crate) fn released(&mut self, selection_active: bool) -> bool {
        let give_back = self.owed && !selection_active;
        if give_back {
            self.owed = false;
        }
        give_back
    }

    /// A key went down. Returns whether the composer takes it: a typed character while the
    /// composer is owed focus. The composer having focus again, by any route, settles the debt.
    pub(crate) fn typed(&mut self, composer_focused: bool, keystroke: &Keystroke) -> bool {
        if composer_focused {
            self.owed = false;
        }
        let take = self.owed && typed_text(keystroke).is_some();
        if take {
            self.owed = false;
        }
        take
    }
}

/// The text a keystroke types, if it types any: a printable character, without ⌃, ⌘ or fn,
/// which make shortcuts. ⌥ stays, since it types accents and symbols.
pub(crate) fn typed_text(keystroke: &Keystroke) -> Option<&str> {
    let modifiers = &keystroke.modifiers;
    if modifiers.control || modifiers.platform || modifiers.function {
        return None;
    }
    let text = keystroke.key_char.as_deref()?;
    (!text.is_empty() && text.chars().all(|c| !c.is_control())).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::{ComposerFocus, typed_text};
    use gpui::{Keystroke, Modifiers};

    fn keystroke(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.into(),
            key_char: key_char.map(Into::into),
        }
    }

    fn typed(character: &str) -> Keystroke {
        keystroke(character, Some(character), Modifiers::default())
    }

    fn copy() -> Keystroke {
        keystroke("c", None, Modifiers::command())
    }

    #[test]
    fn a_click_that_selects_nothing_gives_focus_back_to_the_composer() {
        let mut focus = ComposerFocus::default();
        focus.pressed(true);
        assert!(focus.released(false));
        assert!(!focus.released(false), "focus is given back once");
    }

    #[test]
    fn a_selection_keeps_focus_on_the_text_so_copy_works() {
        let mut focus = ComposerFocus::default();
        focus.pressed(true);
        assert!(!focus.released(true));
        assert!(!focus.typed(false, &copy()));
    }

    #[test]
    fn the_next_typed_character_goes_to_the_composer_once() {
        let mut focus = ComposerFocus::default();
        focus.pressed(true);
        assert!(!focus.released(true));
        let escape = keystroke("escape", None, Modifiers::default());
        assert!(!focus.typed(false, &escape));
        assert!(focus.typed(false, &typed("a")));
        assert!(!focus.typed(false, &typed("b")), "the composer has it now");
    }

    #[test]
    fn a_selection_made_while_the_composer_was_not_focused_owes_it_nothing() {
        let mut focus = ComposerFocus::default();
        focus.pressed(false);
        assert!(!focus.released(false));
        assert!(!focus.typed(false, &typed("a")));
    }

    #[test]
    fn a_later_press_in_the_text_keeps_what_is_owed() {
        let mut focus = ComposerFocus::default();
        focus.pressed(true);
        assert!(!focus.released(true));
        focus.pressed(false);
        assert!(focus.released(false));
    }

    #[test]
    fn focusing_the_composer_again_settles_what_is_owed() {
        let mut focus = ComposerFocus::default();
        focus.pressed(true);
        assert!(!focus.released(true));
        assert!(!focus.typed(true, &typed("a")));
        assert!(!focus.typed(false, &typed("b")));
    }

    #[test]
    fn only_printable_keys_without_shortcut_modifiers_type_text() {
        assert_eq!(typed_text(&typed("a")), Some("a"));
        let accent = keystroke("e", Some("´"), Modifiers::alt());
        assert_eq!(typed_text(&accent), Some("´"));
        assert_eq!(typed_text(&copy()), None);
        let control_a = keystroke("a", Some("a"), Modifiers::control());
        assert_eq!(typed_text(&control_a), None);
        let enter = keystroke("enter", Some("\n"), Modifiers::default());
        assert_eq!(typed_text(&enter), None);
    }
}
