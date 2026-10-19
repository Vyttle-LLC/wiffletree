## MODIFIED Requirements

### Requirement: Portable local command boundary
The host SHALL expose versioned bounded commands over JSON-lines and build without GPUI. It SHALL serve them on standard input for scripts and on its private Unix socket for clients, where each connection begins with a handshake. Database ownership SHALL be exclusive per store.

#### Scenario: Competing host
- **WHEN** another process attempts to open an already owned store
- **THEN** it fails visibly instead of becoming a competing writer

#### Scenario: A script on standard input
- **WHEN** a script writes a version-1 `Snapshot` request to `workspace-host --data-dir <dir>` without a handshake
- **THEN** the host answers it as before

### Requirement: A quit stops running turns
The host SHALL offer a shutdown that cancels every running turn, looks afresh at what each running turn has started, stops every process group recorded for any agent under the confirmation rule, and returns only once each of those turns' provider process groups has been stopped, as at any turn's end, or a bounded wait has passed. The host SHALL run it when the human chooses Stop agents and quit, when the human chooses to restart the host now, when launchd stops it, when a desktop that started it as a child exits, and, for the host on standard input, when its input closes. Closing the desktop app while the human chose to keep agents running SHALL NOT run it. A run SHALL be recorded as finished only after its process group has been stopped; a turn still running when the wait ends SHALL stay unfinished, and a group still running then SHALL stay recorded as running, so the next start handles both. Each group the shutdown stops SHALL be recorded with the reason `quit`.

#### Scenario: Quit during a turn
- **WHEN** the human chooses Stop agents and quit while a provider is mid-turn
- **THEN** the provider's process group no longer runs when the shutdown returns
- **AND** the run is finished with the quit as its error and its input is held for a retry

#### Scenario: Quit with no turn running
- **WHEN** the host is shut down while no turn runs and no agent has a process recorded
- **THEN** the shutdown returns without waiting

#### Scenario: Quit while an agent's process runs
- **WHEN** the host is shut down while an agent between turns has a process it started still running
- **THEN** that process no longer runs when the shutdown returns
- **AND** its group is recorded as stopped with the reason `quit`

#### Scenario: Keep them running
- **WHEN** the human quits the app and chooses Keep them running while a provider is mid-turn
- **THEN** the turn continues and its process group still runs after the app has exited
