## Why

Settings is its own window. When the human paired a device from Settings › Remote access, the main window came forward and hid Settings, so the pairing code was never seen. "Remote access settings…" in the host menu also opened Settings at its top, not at Remote access.

On November 25, 2026 the human approved the study `design/mockups/current/settings-modal.html` as drawn.

## What Changes

- Settings opens as a modal over the main window, as Connect a computer does: 900 points wide, or the window's width less 24 points each side, and as tall as the window less 24 points top and bottom. The main window behind is dimmed.
- The sidebar and every section are unchanged, except that "Builds & remote access" becomes two entries, Builds and Remote access. The entry last chosen, or the one a link opened at, is highlighted. Entries that open a main-window page are marked ↗.
- Deep links land on their section: the host menu's "Remote access settings…" at Remote access, the sync mark at Settings repository, the Inbox's disk card at Disk.
- Entries marked ↗ close Settings, then open their page in the main window.
- Settings closes with ✕, Esc or a click on the dimmed main window. While a confirmation is open over it, Esc or a click outside closes only the confirmation.
- ⌘, and Wiffletree › Settings… do nothing while Settings is open. With no main window, they open one first and then Settings over it.
- Closing Settings while a pairing code is showing cancels that pairing.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `native-workspace`: Settings is a modal over the main window, with its own rules for opening, closing, deep links and pairing.

## Impact

- `crates/workspace-desktop/src/settings.rs`: opens with `window.open_dialog`, draws its own header, and keeps the open Settings in a reshaped global.
- `crates/workspace-desktop/src/main.rs`: ⌘, opens the main window first when none is open.
- `crates/workspace-desktop/src/sidebar.rs`: "Remote access settings…" opens at Remote access.
