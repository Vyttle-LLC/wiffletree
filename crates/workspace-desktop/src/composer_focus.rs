//! Keeps typing in the composer while the transcript's text is clicked or selected.
//!
//! A press in selectable text focuses that text view, so keys would stop reaching the composer.
//! The text view must hold focus while a selection exists, since only it handles ⌘C. So a click
//! that selects nothing gives focus straight back, and a selection keeps it only until the next
//! typed character, which moves to the composer. A dead key, such as ⌥E, or an input method
//! sends no typed character: macOS hands it to the focused view's text input, so the selection's
//! text view takes the composer's text input until the composer has focus again.

use gpui::{
    App, Bounds, ElementInputHandler, InputHandler, Keystroke, Pixels, Point,
    TextInputConfiguration, UTF16Selection, Window,
};
use gpui_component::input::TextareaState;
use std::ops::Range;

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

    /// A key went down. Returns the text the composer takes from it: a typed character while the
    /// composer is owed focus. The composer having focus again, by any route, settles the debt.
    pub(crate) fn typed<'a>(
        &mut self,
        composer_focused: bool,
        keystroke: &'a Keystroke,
    ) -> Option<&'a str> {
        if composer_focused {
            self.owed = false;
        }
        let text = typed_text(keystroke).filter(|_| self.owed)?;
        self.owed = false;
        Some(text)
    }

    pub(crate) fn is_owed(&self) -> bool {
        self.owed
    }

    /// A dead key or an input method wrote into the composer, which now takes focus.
    pub(crate) fn composed(&mut self) {
        self.owed = false;
    }
}

/// The composer's text input, for the text view that holds a selection while the composer is
/// owed focus. Text it receives goes to the composer, and then `took_text` gives it focus.
pub(crate) struct ComposerInput<F: FnMut(&mut Window, &mut App) + 'static> {
    pub(crate) composer: ElementInputHandler<TextareaState>,
    pub(crate) took_text: F,
}

impl<F: FnMut(&mut Window, &mut App) + 'static> InputHandler for ComposerInput<F> {
    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        (self.composer).selected_text_range(ignore_disabled_input, window, cx)
    }
    fn marked_text_range(&mut self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.composer.marked_text_range(window, cx)
    }
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        (self.composer).text_for_range(range_utf16, adjusted_range, window, cx)
    }
    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        (self.composer).replace_text_in_range(replacement_range, text, window, cx);
        (self.took_text)(window, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        (self.composer).replace_and_mark_text_in_range(
            range_utf16,
            new_text,
            new_selected_range,
            window,
            cx,
        );
        (self.took_text)(window, cx);
    }
    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        self.composer.unmark_text(window, cx);
    }
    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        self.composer.bounds_for_range(range_utf16, window, cx)
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize> {
        self.composer.character_index_for_point(point, window, cx)
    }
    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) {
        (self.composer).set_selected_text_range(range_utf16, window, cx);
    }
    fn element_bounds(&mut self, window: &mut Window, cx: &mut App) -> Option<Bounds<Pixels>> {
        self.composer.element_bounds(window, cx)
    }
    fn text_length_utf16(&mut self, window: &mut Window, cx: &mut App) -> Option<usize> {
        self.composer.text_length_utf16(window, cx)
    }
    fn accepts_text_input(&mut self, window: &mut Window, cx: &mut App) -> bool {
        self.composer.accepts_text_input(window, cx)
    }
    fn text_input_editable_range(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Range<usize>> {
        self.composer.text_input_editable_range(window, cx)
    }
    fn prefers_ime_for_printable_keys(&mut self, window: &mut Window, cx: &mut App) -> bool {
        self.composer.prefers_ime_for_printable_keys(window, cx)
    }
    fn text_input_configuration(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> TextInputConfiguration {
        self.composer.text_input_configuration(window, cx)
    }
}

/// The text a keystroke types, if it types any: a printable character, without ⌃, ⌘ or fn,
/// which make shortcuts. ⌥ stays, since it types accents and symbols.
fn typed_text(keystroke: &Keystroke) -> Option<&str> {
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
        assert_eq!(focus.typed(false, &copy()), None);
    }

    #[test]
    fn the_next_typed_character_goes_to_the_composer_once() {
        let mut focus = ComposerFocus::default();
        focus.pressed(true);
        assert!(!focus.released(true));
        let escape = keystroke("escape", None, Modifiers::default());
        assert_eq!(focus.typed(false, &escape), None);
        assert_eq!(focus.typed(false, &typed("a")), Some("a"));
        assert_eq!(
            focus.typed(false, &typed("b")),
            None,
            "the composer has it now"
        );
    }

    #[test]
    fn a_dead_key_after_a_selection_composes_in_the_composer() {
        let mut focus = ComposerFocus::default();
        focus.pressed(true);
        assert!(!focus.released(true));
        // ⌥E types nothing itself; macOS hands its accent to the text input the composer lends.
        let dead_key = keystroke("e", None, Modifiers::alt());
        assert_eq!(focus.typed(false, &dead_key), None);
        assert!(focus.is_owed(), "the composer still takes the accent");
        focus.composed();
        assert!(!focus.is_owed());
        assert_eq!(focus.typed(false, &typed("e")), None);
    }

    #[test]
    fn a_selection_made_while_the_composer_was_not_focused_owes_it_nothing() {
        let mut focus = ComposerFocus::default();
        focus.pressed(false);
        assert!(!focus.released(false));
        assert_eq!(focus.typed(false, &typed("a")), None);
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
        assert_eq!(focus.typed(true, &typed("a")), None);
        assert_eq!(focus.typed(false, &typed("b")), None);
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
