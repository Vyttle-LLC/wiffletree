# host-process Specification

## Purpose
Run the host as its own long-lived process that owns its store, reached only through a private socket with a versioned handshake and one event stream, so agents keep working while the app is closed and through an app update.

## Requirements

### Requirement: The host runs as its own process
For the default data folder of a signed build, the host SHALL run as a per-user launchd process separate from the desktop app, SHALL NOT start at login, labelled `com.vyttle.wiffletree.host` (`com.vyttle.wiffletree.beta.host` for a beta), and SHALL keep running when the app's windows close or the app exits. launchd SHALL restart it after an unsuccessful exit and SHALL NOT restart it after a clean one. A desktop started with `--data-dir` SHALL instead start the host as its own child process, SHALL NOT write a launchd plist, and SHALL stop that host when it exits. The host SHALL keep exclusive ownership of its store.

#### Scenario: First launch
- **WHEN** the app opens on a Mac where no host runs for the default data folder
- **THEN** the app loads the host's launchd job from the data folder, the host starts under launchd, and the app connects to it

#### Scenario: The app's windows close
- **WHEN** the human quits and chooses to keep agents running
- **THEN** the host process is still running and its turns continue, whether or not the app is running

#### Scenario: The host crashes
- **WHEN** the host process exits unexpectedly
- **THEN** launchd starts it again, and the unfinished turns are recovered as after any unexpected stop

#### Scenario: A scratch store
- **WHEN** the desktop is started with `--data-dir` pointing at a temporary folder
- **THEN** no plist is written and no launchd job is loaded, the host runs from the app's own binary with no versioned copy, and the host stops when the desktop exits

#### Scenario: Not at login
- **WHEN** the host has run under launchd, and then the Mac restarts and the human logs in without opening Wiffletree
- **THEN** no host runs and launchd has no job with the host's label until the app is opened

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

### Requirement: Human-only commands need the app's credential
On its own socket, the host SHALL run the commands only the human should give, namely sending a message as the human (`Send` with no sender), opening or cancelling pairing mode, turning remote access on or off, changing or removing a device's access, answering the Inbox (`ResolveAttention`, `DismissAttention`, which also approve a publish or a merge), granting publish or push authority without the Inbox (`SetOwnerAllowed`, which lets an owner's PRs open without asking, `SetKeeper`, whose rebases the PR keeper pushes, `SetProtectedBranches`, whose removed entries the host may then merge into, and `SetUpSettingsRepository` and `JoinSettingsRepository`, which choose a repository the host pushes settings to), creating, editing, removing or joining a Space, choosing its storage repository or setting up its issues, which choose a repository the host pushes Space files and issues to, and writing to a tracker (creating, editing, commenting on, moving, assigning or linking an issue, applying a proposal from an assistant thread, settling a paused issue file and importing from Linear), only for a connection whose `Hello` carried the app's client credential. For a signed build's launchd host, the host SHALL create that credential at its first start and keep it in a login Keychain item whose access list lets only the signed Wiffletree app and the host read it without asking; any other program SHALL get it only after the human enters the login keychain password in the dialog macOS shows. For a child host, the app SHALL create a fresh credential each launch and pass it through the child's standard-input pipe, never in its environment or in a file. An unsigned development build SHALL always start a child host, never a launchd job. Every other command SHALL work without it. The agents' tool socket SHALL keep requiring each turn's own token.

#### Scenario: An agent opens the host socket
- **WHEN** an agent's shell connects to `host.sock`, sends `Hello` without the credential and answers an Inbox question
- **THEN** the host refuses the answer and the question stays open

#### Scenario: An agent writes as the human
- **WHEN** a connection without the credential sends `Send` with no sender to a coordinator
- **THEN** the host refuses it and stores no message

#### Scenario: The human writes
- **WHEN** the app, which sent the credential in `Hello`, sends the same message
- **THEN** the host stores it

#### Scenario: An agent allows an owner
- **WHEN** a connection without the credential sends `SetOwnerAllowed` for an owner
- **THEN** the host refuses it and the owner is not allowed

#### Scenario: An agent removes a protected branch
- **WHEN** a connection without the credential sends `SetProtectedBranches` without `release/*`
- **THEN** the host refuses it and `release/*` stays protected

#### Scenario: The app answers
- **WHEN** the app, which sent the credential in `Hello`, answers the same question
- **THEN** the host records the answer

#### Scenario: An unsigned build
- **WHEN** an unsigned development build of the app opens with the default data folder
- **THEN** it starts a child host, loads no launchd job, writes no file holding the credential, and passes the credential through the child's standard-input pipe

#### Scenario: Other commands
- **WHEN** a connection without the credential asks for the snapshot
- **THEN** the host answers it

#### Scenario: An agent edits a Space
- **WHEN** a connection without the credential sends a command that changes the Sage Space's instructions or storage repository
- **THEN** the host refuses it and the Space is unchanged

#### Scenario: An agent comments on an issue
- **WHEN** a connection without the credential sends a command that comments on CEL-151 or applies a thread's proposal
- **THEN** the host refuses it and the tracker is unchanged

#### Scenario: An agent reads issues
- **WHEN** a connection without the credential asks for the Issues list
- **THEN** the host answers it

### Requirement: Connections begin with a handshake
Every client connection SHALL begin with `Hello`, naming the protocol versions and capabilities the client supports. The host SHALL answer with the protocol version it chose, its host id, its version, its epoch and its capabilities, and SHALL refuse any request sent before `Hello`. A host SHALL accept its own protocol version and the one before it. With no common version, the host SHALL refuse with both version ranges, its version and how many agents are working, and the app SHALL say which side needs updating. `Hello`, that refusal and `RestartHost` SHALL keep the same shape in every protocol version, and after a refusal the host SHALL accept `RestartHost`, and no other command, from a connection on its own socket. A client SHALL NOT send a command that needs a capability the host did not list.

#### Scenario: Same version
- **WHEN** an app of the host's version connects
- **THEN** the handshake chooses that protocol version and lists the host's capabilities

#### Scenario: One version behind
- **WHEN** an app whose newest protocol is one above the host's connects
- **THEN** the handshake chooses the host's version and the app works with the host's capabilities

#### Scenario: No common version
- **WHEN** an app and a host share no protocol version
- **THEN** the host refuses the connection with both ranges, and the app names which side to update

#### Scenario: Restarting a host no version matches
- **WHEN** an updated app shares no protocol version with the running host while two agents work, and the human chooses Wait until they finish
- **THEN** the app shows that two agents are working, sends `RestartHost` with `Idle` on the refused connection, and the host restarts into the new version after both turns end

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
The desktop SHALL learn whether a linked path exists and whether opening it would run it, read attachment thumbnails, list folders for a chooser and read the core instructions' versions only through host commands. The host SHALL read files for a client only under the attachment store (`<data folder>/projects/<project-id>/attachments/`), a project home, a ticket worktree, an attached repository or the workspace folder, and at most 50 MB of one file. It SHALL resolve symlinks and `..` before checking, and SHALL refuse a path whose resolved form is not under a resolved allowed root. Over every root, it SHALL refuse a resolved path inside a fixed deny list of credential locations: `~/.ssh`, `~/.config/gh`, `~/.gnupg`, `~/.aws`, `~/.netrc`, `~/.git-credentials`, `~/.config/git/credentials` (and `$XDG_CONFIG_HOME/git/credentials` when that is set), the Claude and Codex auth files (`~/.claude/.credentials.json`, `~/.claude.json` and `$CODEX_HOME/auth.json`, by default `~/.codex/auth.json`) and `~/Library/Keychains`. Folder listings SHALL apply the same deny list. It SHALL NOT read for a client the store, its journal files or earlier copies, its backups, or any other file in the data folder outside the attachment store and project homes. A folder listing SHALL name only folders and whether each is a Git repository, SHALL be resolved and checked the same way, and SHALL be limited to the same roots, except that the host itself and a Control device may list folders under the host's home folder. The host SHALL adopt the user's login-shell environment at its own start.

#### Scenario: A link to a worktree file
- **WHEN** an agent's reply names `src/lib.rs` and the ticket worktree holds that file
- **THEN** the host answers that it exists and opens normally, and the desktop on the host's Mac opens it

#### Scenario: A file outside what the host serves
- **WHEN** a client asks the host to read `/etc/hosts`
- **THEN** the host refuses and reads nothing

#### Scenario: An attachment of a project whose home is elsewhere
- **WHEN** a project's home is `~/wiffletree/projects/tide-alerts` and the desktop asks for the thumbnail of an image stored with one of its messages under `<data folder>/projects/<project-id>/attachments/`
- **THEN** the host returns the image

#### Scenario: A symlink out of a worktree
- **WHEN** a ticket worktree holds `creds`, a symlink to `~/.config/gh/hosts.yml`, and a client asks the host to read `creds` in that worktree
- **THEN** the host refuses and reads nothing

#### Scenario: A credential under a root
- **WHEN** the workspace folder is set to `~` and a client asks the host to read `~/.config/gh/hosts.yml`
- **THEN** the host refuses and reads nothing

#### Scenario: Too large
- **WHEN** a client asks the host to read a 60 MB file in a worktree
- **THEN** the host refuses it as over 50 MB

#### Scenario: The database
- **WHEN** a client asks the host to read `workspace.sqlite3`, its `-wal` file or a file under `backups/` in the data folder
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
