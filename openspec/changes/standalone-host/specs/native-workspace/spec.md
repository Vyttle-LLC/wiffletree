## ADDED Requirements

### Requirement: Quitting asks while agents work
When the app quits with a launchd host while at least one agent is working, it SHALL ask "Quit Wiffletree?", saying "N agents are working on this Mac. They can keep going after the window closes, and you can check in from your other devices.", with the buttons **Stop agents and quit** and **Keep them running**, the default. Keep them running SHALL leave the host and its turns running. Stop agents and quit SHALL ask the host to stop its agents and exit, and SHALL quit the app once the host has answered. Quitting while no agent is working SHALL NOT ask, and SHALL close the windows and leave the Dock as Keep them running does, leaving the host running and the app in the background. A desktop that started its host as a child SHALL NOT ask, and quitting it SHALL stop that host.

#### Scenario: Three agents working
- **WHEN** the human presses ⌘Q while three agents are working
- **THEN** the prompt says "3 agents are working on this Mac." and Return chooses Keep them running

#### Scenario: Nothing working
- **WHEN** the human presses ⌘Q while no agent is working
- **THEN** no prompt appears, the windows close, the host keeps running, and the quiet menu bar icon is shown

#### Scenario: Stop agents and quit
- **WHEN** the human chooses Stop agents and quit
- **THEN** every turn is stopped as a quit stops it, the host process exits, and then the app exits

### Requirement: A menu bar icon while the app runs in the background
After the human chooses Keep them running, or quits while no agent is working, the app SHALL close its windows, leave the Dock, and keep running in the background, connected to its host. While it is in the background, its menu bar icon SHALL always be shown: quiet while no agent is working, and with the number working while any is, including turns that start after the windows closed. The icon's menu SHALL show how many agents are working, **Open Wiffletree**, **Stop agents and quit** and **Quit Wiffletree**. Quit Wiffletree SHALL quit the app and leave the host and its turns running. A staged update SHALL install when the app quits this way or through Stop agents and quit, and not when it only goes into the background.

#### Scenario: Reopening from the menu bar
- **WHEN** the human chooses Open Wiffletree from the icon while two agents work
- **THEN** the main window opens on the same host with both agents still working

#### Scenario: The last agent finishes
- **WHEN** the last working agent's turn ends while no window is open
- **THEN** the icon stays, quiet with no count, and the app and the host keep running in the background

#### Scenario: A turn starts after the windows closed
- **WHEN** no agent is working, no window is open and the quiet icon is shown, and a timer or a paired device starts a turn
- **THEN** the menu bar icon shows one agent working

#### Scenario: Quit Wiffletree with nothing working
- **WHEN** no agent is working, the app is in the background with an update staged, and the human chooses Quit Wiffletree
- **THEN** the app exits, the host keeps running, and the update is installed

### Requirement: The desktop reconnects to its host
When its connection to the host closes or stops answering, the desktop SHALL keep the last state on screen, SHALL try again with a growing delay, and on reconnecting SHALL resume its event subscription from its last cursor or take a fresh snapshot. A message send whose reply never arrived SHALL be sent again with the same message id, so the host stores it once.

#### Scenario: The host restarts under the window
- **WHEN** the host is restarted by launchd while the window is open
- **THEN** the window reconnects without the human doing anything and shows the restarted host's state

#### Scenario: A send across the drop
- **WHEN** the socket closes after a message was written and before its reply
- **THEN** the desktop sends it again with the same id, and the conversation shows it once
