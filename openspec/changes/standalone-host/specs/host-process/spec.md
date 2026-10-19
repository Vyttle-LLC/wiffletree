## ADDED Requirements

### Requirement: The host runs as its own process
For the default data folder, the host SHALL run as a per-user launchd process separate from the desktop app, labelled `com.vyttle.wiffletree.host` (`com.vyttle.wiffletree.beta.host` for a beta), and SHALL keep running when the app's windows close or the app exits. launchd SHALL restart it after an unsuccessful exit and SHALL NOT restart it after a clean one. A desktop started with `--data-dir` SHALL instead start the host as its own child process, SHALL NOT write a launchd plist, and SHALL stop that host when it exits. The host SHALL keep exclusive ownership of its store.

#### Scenario: First launch
- **WHEN** the app opens on a Mac where no host runs for the default data folder
- **THEN** the app installs the LaunchAgent, the host starts under launchd, and the app connects to it

#### Scenario: The app exits
- **WHEN** the app exits while the human chose to keep agents running
- **THEN** the host process is still running and its turns continue

#### Scenario: The host crashes
- **WHEN** the host process exits unexpectedly
- **THEN** launchd starts it again, and the unfinished turns are recovered as after any unexpected stop

#### Scenario: A scratch store
- **WHEN** the desktop is started with `--data-dir` pointing at a temporary folder
- **THEN** no file is written under `~/Library/LaunchAgents`, and the host stops when the desktop exits

#### Scenario: Two hosts, one store
- **WHEN** a second host is started for a data folder whose host is running
- **THEN** it fails with "Another host already owns this store" and the running host is unaffected

### Requirement: The desktop reaches its host only through a private socket
The host SHALL accept client connections on `<data folder>/host.sock`, created with mode 0600, and SHALL close any connection whose peer is not the same user. The desktop SHALL send every request to its host through that socket or, for a child host, an equivalent private channel, and SHALL NOT call into the host's code in its own process. A socket path longer than the platform allows SHALL stop the host at start with that reason.

#### Scenario: Another user
- **WHEN** a process running as another user connects to the socket
- **THEN** the connection is closed before any request is read

#### Scenario: The desktop does not link the host
- **WHEN** `workspace-desktop` is built for release
- **THEN** it does not depend on the `workspace-host` crate

### Requirement: Connections begin with a handshake
Every client connection SHALL begin with `Hello`, naming the protocol versions and capabilities the client supports. The host SHALL answer with the protocol version it chose, its host id, its version, its epoch and its capabilities, and SHALL refuse any request sent before `Hello`. A host SHALL accept its own protocol version and the one before it. With no common version, the host SHALL refuse with both version ranges, and the app SHALL say which side needs updating. A client SHALL NOT send a command that needs a capability the host did not list.

#### Scenario: Same version
- **WHEN** an app of the host's version connects
- **THEN** the handshake chooses that protocol version and lists the host's capabilities

#### Scenario: One version behind
- **WHEN** an app whose newest protocol is one above the host's connects
- **THEN** the handshake chooses the host's version and the app works with the host's capabilities

#### Scenario: No common version
- **WHEN** an app and a host share no protocol version
- **THEN** the host refuses the connection with both ranges, and the app names which side to update

#### Scenario: Request without a handshake
- **WHEN** a client sends `Snapshot` as its first line
- **THEN** the host refuses it and runs nothing

### Requirement: An update does not interrupt running turns
The host SHALL run from a copy of its binary that an app update does not replace, and SHALL start agents' tool helpers from that same copy. After an update, the app SHALL ask the running host to restart once no turn and no compaction runs, and SHALL say so with an option to restart now. A restart when idle SHALL exit the host cleanly so the new version starts. Restarting now SHALL stop running turns exactly as a quit does. While a restart waits, new turns SHALL start as usual.

#### Scenario: Update during a turn
- **WHEN** the app is updated and reopened while two agents are mid-turn
- **THEN** both turns continue under the old host, the app shows that the host restarts into the new version once no agent is working, and the host restarts after the second turn ends

#### Scenario: Restart now
- **WHEN** the human chooses Restart now while one turn runs
- **THEN** that turn is recorded as interrupted by the quit with its input held, and the new host version starts

#### Scenario: The bundle is replaced
- **WHEN** the app bundle is swapped while a host runs
- **THEN** the running host and the helpers it starts afterwards still run the host's own version

### Requirement: One sequenced event stream
The host SHALL offer one subscription per connection that sends the events `state`, `steps` (with the ids of the sessions whose steps changed), `progress`, `usage` and `resync`, in place of the four change signals. Each event SHALL carry a cursor made of the host's epoch and a sequence number that grows by one per event. The host SHALL keep the most recent events in memory, and a subscription given a cursor still among them SHALL first receive every later event in order. A subscription given any other cursor SHALL receive `resync`, after which the client takes a fresh snapshot. A subscriber that falls behind its bounded queue SHALL receive `resync` and SHALL NOT slow the host thread. Events SHALL carry no workspace state, only what to fetch again.

#### Scenario: Resume after a short drop
- **WHEN** a client reconnects with the cursor of the last event it read, and the host has emitted three events since
- **THEN** the client receives exactly those three events, in order, and then live events

#### Scenario: The host restarted
- **WHEN** a client subscribes with a cursor from before the host's restart
- **THEN** it first receives `resync`, and its next snapshot reflects the restarted host

#### Scenario: A stalled client
- **WHEN** a subscriber stops reading while agents produce thousands of steps
- **THEN** other clients' events and the host's work continue, and the stalled subscriber receives `resync` when it reads again

#### Scenario: Many changes at once
- **WHEN** one provider event changes state, steps and progress
- **THEN** the subscriber receives at most one event of each kind for it

### Requirement: The desktop reads host files only through the host
The desktop SHALL learn whether a linked path exists and whether opening it would run it, read attachment thumbnails, list folders for a chooser and read the core instructions' versions only through host commands. The host SHALL read files for a client only under its data folder, a project home, a ticket worktree, an attached repository or the workspace folder, and at most 50 MB of one file. A folder listing SHALL name only folders and whether each is a Git repository. The host SHALL adopt the user's login-shell environment at its own start.

#### Scenario: A link to a worktree file
- **WHEN** an agent's reply names `src/lib.rs` and the ticket worktree holds that file
- **THEN** the host answers that it exists and opens normally, and the desktop on the host's Mac opens it

#### Scenario: A file outside what the host serves
- **WHEN** a client asks the host to read `/etc/hosts`
- **THEN** the host refuses and reads nothing

#### Scenario: Core versions
- **WHEN** the Skills page opens
- **THEN** the core instructions' names and versions it shows are those of the running host

#### Scenario: Launched by launchd
- **WHEN** the host starts under launchd with launchd's minimal `PATH`
- **THEN** the providers it starts find the tools on the user's login-shell `PATH`

### Requirement: The Mac stays awake while agents work
While at least one turn or compaction runs, the host SHALL hold a power assertion that prevents idle system sleep, and SHALL release it as soon as none runs. It SHALL hold at most one such assertion at a time.

#### Scenario: A turn runs
- **WHEN** a turn starts on an otherwise idle host
- **THEN** `pmset -g assertions` lists one Wiffletree assertion preventing idle sleep

#### Scenario: The last turn ends
- **WHEN** the last running turn ends and no compaction runs
- **THEN** the host holds no power assertion
