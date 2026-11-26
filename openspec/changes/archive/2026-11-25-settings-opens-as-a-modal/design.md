## Context

Settings opened a separate 900 × 820 window and kept it in a `SettingsWindow` global so ⌘, could focus it. Its confirmations are drawn inside its body with `over_dimmed`, not as dialogs. gpui-component 0.7.1's `Dialog` rebuilds from its builder on every frame, and routes Esc, a click on the overlay and its own ✕ through one `on_cancel` callback, which may return `false` to keep the dialog open.

## Decisions

- **Settings is the dialog's content, with its own header.** The dialog has no padding, title or close button; Settings draws "Settings" and a ✕ that closes the dialog outright, as the study draws them. Its body keeps its own scroll, so scrolling to a section is unchanged.
- **Size from the window on every frame.** The builder reads the window's size each time, so the dialog follows a resized window: width `min(900, width − 48)`, top 24 below the title bar, as the study draws it, and height to 24 above the bottom.
- **A linked section stays in view.** Settings, remote access and disk answers arrive after Settings opens and grow the sections above, so a section scrolled to once drifts down. The page scrolls to the chosen section on every frame until the human scrolls.
- **No "/" search under Settings.** Settings now sits in the main window, where "/" outside a field opens Brain search. That shortcut is skipped while any dialog is open, since the search would open hidden under it.
- **One `on_cancel` guards the confirmation.** While a confirmation is open, `on_cancel` closes it and returns `false`, so Esc and a click outside leave Settings open. Otherwise it returns `true`. The header's ✕ is not a cancel: it closes Settings whatever is open in it.
- **Pairing ends when Settings is released.** Every way the dialog closes drops its builder, which holds the only strong handle to Settings. Settings' release callback cancels a pairing that is showing or still opening (the host takes a client's commands in the order sent, so the cancel lands after the open), so ✕, Esc, a click outside, a ↗ entry and closing the main window are all covered by one rule. The Remote access watch already stops once Settings is gone.
- **The global holds the open Settings weakly.** `SettingsDialog(WeakEntity<Settings>)` answers "is Settings open" for ⌘, and for `host_selected`; it upgrades only while the dialog holds Settings.

## Risks / Trade-offs

- A click outside loses a half-typed field, such as a repository name. Settings save as they change, so nothing saved is lost; the human approved this.
- Changing host needs Settings closed first, because the switcher sits under the dimmed window.
