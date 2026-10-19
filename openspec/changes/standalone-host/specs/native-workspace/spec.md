## ADDED Requirements

### Requirement: Quitting asks while agents work
When the app quits with a launchd host while at least one agent is working, it SHALL ask "Quit Wiffletree?", saying "N agents are working on this Mac. They can keep going after the window closes, and you can check in from your other devices.", with the buttons **Stop agents and quit** and **Keep them running**, the default. Keep them running SHALL leave the host and its turns running. Stop agents and quit SHALL ask the host to stop its agents and exit, and SHALL quit the app once the host has answered. A desktop that started its host as a child SHALL NOT ask.

#### Scenario: Three agents working
- **WHEN** the human presses ⌘Q while three agents are working
- **THEN** the prompt says "3 agents are working on this Mac." and Return chooses Keep them running

#### Scenario: Stop agents and quit
- **WHEN** the human chooses Stop agents and quit
- **THEN** every turn is stopped as a quit stops it, the host process exits, and then the app exits

### Requirement: A menu bar icon while agents run with no window
After the human chooses Keep them running, the app SHALL close its windows, leave the Dock, and keep running in the background, connected to its host. It SHALL show a menu bar icon whenever any agent is working, including turns that start after the windows closed, and SHALL hide it while none is. The icon's menu SHALL show how many agents are working, **Open Wiffletree** and **Stop agents and quit**.

#### Scenario: Reopening from the menu bar
- **WHEN** the human chooses Open Wiffletree from the icon while two agents work
- **THEN** the main window opens on the same host with both agents still working

#### Scenario: The last agent finishes
- **WHEN** the last working agent's turn ends while no window is open
- **THEN** the icon disappears, and the app and the host keep running in the background

#### Scenario: A turn starts after the windows closed
- **WHEN** no agent is working and no window is open, and a timer or a paired device starts a turn
- **THEN** the menu bar icon appears and shows one agent working

### Requirement: The desktop reconnects to its host
When its connection to the host closes or stops answering, the desktop SHALL keep the last state on screen, SHALL try again with a growing delay, and on reconnecting SHALL resume its event subscription from its last cursor or take a fresh snapshot. A message send whose reply never arrived SHALL be sent again with the same message id, so the host stores it once.

#### Scenario: The host restarts under the window
- **WHEN** the host is restarted by launchd while the window is open
- **THEN** the window reconnects without the human doing anything and shows the restarted host's state

#### Scenario: A send across the drop
- **WHEN** the socket closes after a message was written and before its reply
- **THEN** the desktop sends it again with the same id, and the conversation shows it once
