## ADDED Requirements

### Requirement: Settings opens as a modal over the main window
Settings SHALL open as a modal over the main window, with the main window dimmed behind it and taking no clicks. It SHALL be 900 points wide, or the window's width less 24 points each side if that is narrower, and as tall as the window below its title bar less 24 points top and bottom, following the window as it is resized. Its header SHALL say "Settings" and have a ✕. Its sidebar and its single scrolling page SHALL keep their groups, order, sections and wording, except that "Builds & remote access" SHALL be two entries, Builds and Remote access. Choosing an entry for a section SHALL scroll to that section and highlight the entry, and the page SHALL keep that section in place as answers arrive above it, until the human scrolls. The entries that open a page of the main window SHALL be marked ↗: Behaviors, Reviews, PR keeper, each Space or Spaces, Brains, Who runs what, Subscriptions, Repositories and Allowed owners. Choosing one SHALL close Settings and then open that page.

#### Scenario: A section entry
- **WHEN** the human chooses Disk in the Settings sidebar
- **THEN** the page scrolls to Disk and the Disk entry is highlighted

#### Scenario: A main-window entry
- **WHEN** the human chooses Who runs what in the Settings sidebar
- **THEN** Settings closes and the main window shows Who runs what on Models

### Requirement: Links open Settings at their section
The host menu's "Remote access settings…" SHALL open Settings at Remote access, the sidebar's sync mark at Settings repository, and the Inbox's disk card at Disk, each with its entry highlighted. ⌘, and Wiffletree › Settings… SHALL open Settings at its top.

#### Scenario: Remote access settings
- **WHEN** the human chooses "Remote access settings…" in the host menu
- **THEN** Settings opens scrolled to Remote access, with Remote access highlighted

### Requirement: Settings closes with ✕, Esc or a click outside
Settings SHALL close with the ✕ in its header, with Esc, or with a click on the dimmed main window. While a confirmation is open over Settings, such as "Stop syncing with …?", Esc or a click on the dimmed main window SHALL close only that confirmation; the ✕ SHALL still close Settings.

#### Scenario: Esc over a confirmation
- **WHEN** "Stop syncing with …?" is open over Settings and the human presses Esc
- **THEN** the confirmation closes and Settings stays open; a second Esc closes Settings

### Requirement: There is at most one Settings
While Settings is open, ⌘, and Wiffletree › Settings… SHALL do nothing. With no main window open, they SHALL open the main window first and then Settings over it. A link to a section while Settings is open SHALL scroll the open Settings to that section.

#### Scenario: No main window
- **WHEN** the main window is closed and the human presses ⌘,
- **THEN** the main window opens with Settings over it

### Requirement: Closing Settings ends a pairing nobody can see
Closing Settings in any way while a pairing code is showing, or while pairing is still opening, SHALL cancel that pairing. Remote access SHALL be asked for again on a timer only while Settings is open. Settings SHALL follow the host the switcher selected; while it is open the switcher is covered, so changing host means closing Settings first.

#### Scenario: Closing during pairing
- **WHEN** Remote access shows a pairing code and the human closes Settings
- **THEN** the host cancels pairing and the code no longer works
