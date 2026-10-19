## Context

See proposal.md for why. At origin/main f67ce3f:

- `crates/workspace-desktop/src/bridge.rs:24` calls `Service::start(home, helper)` in the app's process, with `helper` the `workspace-host` binary beside the app's own. Replies come back on `async_channel` receivers.
- `Service::start` opens the store, which takes `host.lock` with `try_lock`, so a second host on the same data folder fails with "Another host already owns this store". It also binds a Unix socket in a fresh `/tmp/aw-*` directory, mode 0600, which only agents' `workspace-host agent-mcp` helpers use for their tools.
- The host wakes clients with four bounded(1) signals that carry nothing: `changes` (reload the snapshot), `step_changes`, `progress_changes` and `usage_changes`. `Actor::signal` coalesces any number of changes into one.
- `workspace-host --data-dir <dir>` already serves version-1 JSON-line `Request`s on stdin and stops the host when stdin closes. `scripts/test_communications.py` drives it that way.
- `main.rs` registers `cx.on_app_quit` to call `bridge.shutdown()`, which stops every running turn and its processes (the "A quit stops running turns" requirement).
- `update.rs` swaps the app bundle after the app exits: the old bundle is renamed to `.previous` and deleted.
- `Command::is_read` already classifies every command as a read or a change.

## Decisions

### 1. One host process per data folder
For the default data folder (`~/Library/Application Support/Wiffletree`, or the beta's) the host runs under launchd as the LaunchAgent `com.vyttle.wiffletree.host` (`com.vyttle.wiffletree.beta.host` for a beta), so the release and the beta keep separate hosts as they keep separate stores. The app writes `~/Library/LaunchAgents/<label>.plist` and loads it with `launchctl bootstrap gui/<uid>` the first time it finds no host, and starts it with `launchctl kickstart` after that. The plist runs `workspace-host serve --data-dir <folder>` with `KeepAlive` set to restart only after an unsuccessful exit, and sends standard error to `<data folder>/logs/host.log`.

When the desktop is started with `--data-dir`, as in tests, `drive_desktop.py` and scratch stores, it starts `workspace-host serve` as its own child and stops it when it exits. No plist is written. The quit prompt and the menu bar icon do not apply to such a host.

The store lock stays as it is and remains the single guarantee that one host owns a store.

*Why not `SMAppService`:* it requires the program to stay inside the app bundle, which an update replaces under the running host (see 4).

### 2. The socket
`serve` listens on `<data folder>/host.sock`, created with mode 0600 inside the data folder, which the host keeps at 0700. Each accepted connection is checked with `getpeereid`; a different user is closed at once. A socket path longer than the platform's limit (104 bytes on macOS) stops the host at start with that reason, rather than falling back to another folder. The agents' tool socket in `/tmp/aw-*` is unchanged; it is a different protocol with its own per-turn tokens.

Messages are the existing JSON lines, at most 256 KiB per request line. A connection carries any number of requests, answered in any order and matched by `id`, plus the events of a subscription (5).

### 3. The handshake
The first line on a connection is `Hello { protocols: [min, max], app_version, client: { name, kind }, capabilities }`. The host answers with the protocol it chose, its `host_id`, its `host_version`, its `epoch` (a new random id each time it starts), its capabilities, and the `binary` it runs from. A host accepts its own protocol version and the one before it, so an app one release ahead or behind still works. With no common version the host refuses with both ranges, and the app says which side needs updating.

Capabilities are short strings, one per optional feature (`events`, `files`, `folders`, `restart-when-idle`). A client never sends a command for a capability the host did not list. A request before `Hello` is refused. `PROTOCOL_VERSION` becomes 2; the stdin mode keeps answering version 1 without a handshake, so existing scripts keep working.

### 4. Updates while turns run
The host runs from a copy of its binary: on first install and after each update, the app copies `Contents/MacOS/workspace-host` to `<data folder>/host/<version>/workspace-host` and points the plist at it. Agents' `agent-mcp` helpers are started from the same copy, so a helper always matches its host. Swapping the app bundle therefore never touches a running host. A copy no host uses is removed at the next start.

After an update the new app connects to the old host. If the handshake finds a common protocol, everything works and the sidebar's update line reads "The host restarts into <version> once no agent is working", with **Restart now**. The app asks the host `RestartHost { when: Idle }`; the host then exits cleanly at the first moment no turn and no compaction runs, and the app starts the new copy. **Restart now** sends `RestartHost { when: Now }`, which runs the same shutdown as a quit, recording interrupted turns with `QUIT_DURING_TURN` exactly as today. New turns are not held back while a restart waits.

If there is no common protocol, the app shows only that the host must restart, how many agents are working, **Restart now**, and **Wait until they finish**, which sends `RestartHost { when: Idle }` and shows the projects again once the new host answers.

### 5. One event stream with a resume cursor
`Command::Subscribe { after: Option<Cursor> }` turns the connection into a subscriber. Events are sent as lines `{"event": {...}}` beside replies:

| Event | Replaces | Carries |
| --- | --- | --- |
| `state` | `changes` | nothing; reload the snapshot |
| `steps` | `step_changes` | the ids of the sessions whose steps changed |
| `progress` | `progress_changes` | nothing; fetch progress |
| `usage` | `usage_changes` | nothing; fetch usage and quotas |
| `resync` | — | the subscriber must take a snapshot |

Each event has a `Cursor { epoch, seq }`; `seq` grows by one per event within an epoch. The host keeps the last 1,024 events in memory. `Subscribe` with a cursor of the current epoch still in that log replays every later event, then streams. Any other cursor, including one from an earlier epoch, gets `resync` first. Events stay invalidations, as the pings are today; no state is carried, so missing one can never leave a wrong value on screen, only a stale one until the next fetch.

Coalescing stays: the host emits at most one event of each kind per actor event, as `Actor::signal` does now. Each subscriber has a bounded queue of 256 events. A subscriber whose queue is full is sent `resync` once and its queue is cleared, so a slow client never holds the host thread. Nothing is persisted; a restart is a new epoch.

### 6. Host files only through the host
The audit of the desktop, by file:

| File | Today | After |
| --- | --- | --- |
| `file_links.rs` | `resolve` checks `is_file()` under the worktree and project home; `runs_when_opened` reads the file's mode; opens or reveals the path | `Command::ResolvePaths { references, bases }` answers which references exist and whether opening would run them. A client on the host's Mac then opens or reveals the path itself. |
| `attachment_chips.rs` | thumbnails `img(attachment.path)` from the host's attachment store | `Command::ReadFile { path, max_bytes }`, cached per message attachment |
| `conversation.rs` drop, `InspectAttachments`, `Send` | sends the client's paths; the host reads and copies them | unchanged here: on the host's Mac they are the same files. `remote-hosts` adds uploads. |
| `creation.rs`, `repositories_view.rs`, `settings.rs`, `memory_view.rs` | native folder panels | unchanged on the host's Mac. `Command::ListFolder { path }` is added for the in-app browser `remote-hosts` uses elsewhere. |
| `skills_view.rs` | `workspace_host::provider::CORE` compiled into the app | the core instructions' names and versions come in the host's skills reply, so the page shows the running host's contract |
| `login_env.rs` | the app adopts the login shell's environment so the in-process host inherits it | the host adopts it at its own start; launchd gives it the same bare `PATH` the Dock does |
| `data_dir.rs` | moves the pre-rename `Agent Workspace` folder | the host does it before opening the store; the app only names the folder |
| `leftovers.rs`, `verifiers.rs`, `tree.rs`, `inspector.rs` | only commands; their tests open `Host` | unchanged |
| `preferences.rs` | `desktop-preferences.json` in the data folder | stays the app's own file; the host never reads it |
| `update.rs` | uses `workspace_host::runtime::OwnDescriptors` | the trait moves to `workspace-core` |

`ReadFile` reads only files under the data folder, a project home, a ticket worktree, an attached repository or the workspace folder, and at most `MAX_ATTACHMENT_BYTES` (50 MB). `ListFolder` lists only folders, names and whether each is a Git repository, and reads nothing inside them. Both run off the host thread, as Git reads do.

With these, `workspace-desktop` needs `workspace-host` only as a dev-dependency.

### 7. Quitting and the menu bar
The quit prompt applies when the app quits with a launchd host and at least one agent is working: "Quit Wiffletree?", "N agents are working on this Mac. They can keep going after the window closes, and you can check in from your other devices.", **Stop agents and quit** and **Keep them running**, the default.

- **Keep them running** closes the windows, removes the Dock icon, and leaves the app running as a menu bar icon while any agent is working. The icon's menu shows how many are working, **Open Wiffletree** and **Stop agents and quit**. When no agent has been working for a short while, the app exits and the icon goes. The host keeps running either way.
- **Stop agents and quit** sends `StopAgentsAndQuit`. The host runs the existing shutdown, exits successfully so launchd does not restart it, and the app exits once the host's reply arrives.

The menu bar icon lives in the app, not the host, so the host keeps building without any GUI dependency.

### 8. Keeping the Mac awake
While at least one turn or compaction runs, the host holds one IOKit power assertion of type `PreventUserIdleSystemSleep`, named "Wiffletree agents are working", and releases it when none runs. The display may still sleep. A MacBook with its lid closed and no external display still sleeps; macOS gives a background process no way to prevent that. On Linux the host holds nothing for now.

### 9. Losing the host
If the socket closes or the host does not answer, the desktop keeps the last snapshot on screen, tries again after 0.5 s, doubling up to 30 s (and at once if launchd has just restarted the host), and on success sends `Hello` and `Subscribe` with its last cursor. A send whose reply never came is retried with the same message id, which the host's existing dedup accepts once.

## Risks

- A LaunchAgent shows a "Background item added" notification and appears in System Settings › Login Items, where the human can turn it off. The app then says the host could not start and how to allow it.
- Copying the host binary out of the bundle keeps its code signature, but Gatekeeper's treatment of the copy must be checked on a notarized build before release.
- Every reply now crosses a socket. The local transport target in the PRD (low overhead, bounded buffering) is to be measured with the existing host benchmark, not assumed.

## Open questions

1. **Does the host start at login?** Proposed: no. The host starts when the app opens and runs until it is stopped, so after a restart of the Mac nothing runs until the app is opened. Starting at login matters most once other devices drive this Mac.
2. **Quitting with no agent working.** The approved prompt covers quitting while agents work. When none is working, should the app quit without asking and leave the host running (timers and other devices keep working), or stop the host too? Proposed: quit without asking and leave the host running.
3. **Holding new turns while an update waits.** Proposed: no; the host restarts at the first idle moment. A busy host may then wait a long time, which **Restart now** covers.
4. **The menu bar icon's grace period** before the app exits once no agent is working. Proposed: one minute.
