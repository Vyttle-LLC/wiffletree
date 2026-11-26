## 1. Settings as a modal

- [x] 1.1 Open Settings with `window.open_dialog` over the main window, sized from the window on every frame, with its own header and ✕; remove the separate window and its TitleBar.
- [x] 1.2 Reshape the `SettingsWindow` global into a weak handle to the open Settings, used by ⌘, and `host_selected`.
- [x] 1.3 `on_cancel` closes an open confirmation instead of Settings.
- [x] 1.4 Cancel a showing pairing when Settings is released.
- [x] 1.5 Split "Builds & remote access" into Builds and Remote access; highlight the chosen entry; mark main-window entries ↗ and close Settings before opening their page.
- [x] 1.6 "Remote access settings…" opens at Remote access; the sync mark and the disk card keep their sections.
- [x] 1.7 ⌘, and Settings… do nothing while Settings is open, and open the main window first when none is open.

## 2. Checks

- [x] 2.1 Update the sidebar tests for the split entry; test which entries close Settings and where each deep link lands.
- [x] 2.2 Copy the study to `design/mockups/current/settings-modal.html`.
- [x] 2.3 fmt, clippy and tests for `workspace-desktop`; launch the app and check the modal by eye.
