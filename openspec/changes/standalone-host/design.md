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
For the default data folder (`~/Library/Application Support/Wiffletree`, or the beta's) a signed build's host runs under launchd as the LaunchAgent `com.vyttle.wiffletree.host` (`com.vyttle.wiffletree.beta.host` for a beta), so the release and the beta keep separate hosts as they keep separate stores. Whenever the app finds no host running, it writes `<data folder>/host/<label>.plist` and loads it with `launchctl bootstrap gui/<uid> <that path>`. The plist runs `workspace-host serve --data-dir <folder>` from the versioned copy (4), with `KeepAlive` set to restart only after an unsuccessful exit. `KeepAlive` implies `RunAtLoad` (launchd.plist(5)), so loading the job starts the host, which is what the app wants at that moment. The plist is deliberately not in `~/Library/LaunchAgents`: launchd loads only that folder at login, and a job bootstrapped from any other path lasts until logout. So the host does not start at login: after the Mac restarts or the human logs out, nothing runs until the app is opened, and the app then bootstraps the job again. A host that exits cleanly is unloaded with `launchctl bootout` so the next start is a fresh bootstrap. It sends standard error to `<data folder>/logs/host.log`.

An unsigned development build always starts a child host as below, even for the default data folder, and never loads a launchd job; testing the launchd path needs a signed build.

When the desktop is started with `--data-dir`, as in tests, `drive_desktop.py` and scratch stores, it starts `workspace-host serve` as its own child, straight from the bundle's `Contents/MacOS/workspace-host` (or `target/<profile>/` in development), and stops it when it exits. No plist is written and no versioned copy is made: a child host never outlives the app that started it, so an update cannot replace it mid-turn. The quit prompt and the menu bar icon do not apply to such a host.

The store lock stays as it is and remains the single guarantee that one host owns a store.

*Why not `SMAppService`:* it requires the program to stay inside the app bundle, which an update replaces under the running host (see 4).

### 2. The socket
`serve` listens on `<data folder>/host.sock`, created with mode 0600 inside the data folder, which the host keeps at 0700. Each accepted connection is checked with `getpeereid`; a different user is closed at once. A socket path longer than the platform's limit (104 bytes on macOS) stops the host at start with that reason, rather than falling back to another folder. The agents' tool socket in `/tmp/aw-*` is unchanged: it is a different protocol, and each call still needs the calling turn's own token, as today.

Agents run as the same user with permission prompts off, so an agent can connect to `host.sock` as well as the app can. The socket's permissions keep out other users, not agents. As defence in depth, the commands only the human should give also require the app's **client credential**: opening or cancelling pairing mode, turning remote access on or off, changing or removing a device's access (all from `remote-hosts`), and answering the Inbox (`ResolveAttention`, `DismissAttention`), which is also how a publish is approved. The host creates the credential, a random 256-bit secret, at its first start and keeps it in a login Keychain item whose access list holds only the signed Wiffletree app and the host's own copy; Keychain refuses other programs, an agent's shell among them, without asking. The app reads it and sends it in `Hello`; a connection without it may use every other command. For a child host, the app creates a fresh credential each time it starts one and passes it on the child's standard input, never in its environment, which agents inherit; no Keychain item is written for it. An unsigned development build cannot be named in a Keychain access list, so it always runs a child host (1) and passes the per-launch credential the same way, through the child's standard-input pipe; nothing is written to a file. How macOS treats other programs asking for the Keychain item (a silent refusal, or an allow/deny dialog) and what the copied host binary needs to read it are checked by a spike before this work starts. This raises the bar for an agent that tries to answer its own questions or pair a device of its own; it is not a sandbox. As the README warns, agents can still run any command as the user.

Messages are the existing JSON lines, at most 256 KiB per request line. A connection carries any number of requests, answered in any order and matched by `id`, plus the events of a subscription (5).

### 3. The handshake
The first line on a connection is `Hello { protocols: [min, max], app_version, client: { name, kind }, capabilities }`. The host answers with the protocol it chose, its `host_id`, its `host_version`, its `epoch` (a new random id each time it starts), its capabilities, and the `binary` it runs from. A host accepts its own protocol version and the one before it, so an app one release ahead or behind still works. With no common version the host refuses with both ranges, its `host_version` and how many agents are working, and the app says which side needs updating. The shape of `Hello`, of that refusal and of `RestartHost` is frozen across protocol versions: after a refusal, a connection on the host's own socket may still send `RestartHost` and nothing else, so an updated app can always restart an older host.

Capabilities are short strings, one per optional feature (`events`, `files`, `folders`, `restart-when-idle`). A client never sends a command for a capability the host did not list. A request before `Hello` is refused. `PROTOCOL_VERSION` becomes 2; the stdin mode keeps answering version 1 without a handshake, so existing scripts keep working.

### 4. Updates while turns run
The host runs from a copy of its binary: on first install and after each update, the app copies `Contents/MacOS/workspace-host` to `<data folder>/host/<version>/workspace-host` and points the plist at it. Agents' `agent-mcp` helpers are started from the same copy, so a helper always matches its host. Swapping the app bundle therefore never touches a running host. A copy no host uses is removed at the next start.

After an update the new app connects to the old host. If the handshake finds a common protocol, everything works and the sidebar's update line reads "The host restarts into <version> once no agent is working", with **Restart now**. The app asks the host `RestartHost { when: Idle }`; the host then exits cleanly at the first moment no turn and no compaction runs, and the app starts the new copy. **Restart now** sends `RestartHost { when: Now }`, which runs the same shutdown as a quit, recording interrupted turns with `QUIT_DURING_TURN` exactly as today. New turns are not held back while a restart waits.

If there is no common protocol, the app shows only that the host must restart, how many agents are working (from the refusal), **Restart now**, and **Wait until they finish**, which sends `RestartHost { when: Idle }` on the refused connection, as 3 allows, and shows the projects again once the new host answers.

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
| `update.rs` | uses `workspace_host::runtime::OwnDescriptors`; installs a staged update on an ordinary quit | the trait moves to `workspace-core`; the install runs when the app process exits, which is now Quit Wiffletree from the menu bar icon or Stop agents and quit (7) |

`ReadFile` reads only files under these roots: the attachment store, `<data folder>/projects/<project-id>/attachments/`, where `attachments::directory` keeps every project's attachments whatever its home; a project home; a ticket worktree; an attached repository; and the workspace folder. It reads at most `MAX_ATTACHMENT_BYTES` (50 MB). The host resolves the requested path first, following symlinks and `..`, and checks the resolved path against the resolved roots, so a symlink in a worktree that points at `~/.config/gh/hosts.yml` is refused. It never reads the store (`workspace.sqlite3` and its `-wal`, `-shm` and earlier copies such as `.pre-flatten`), `backups/`, host settings, `host/`, `logs/` or anything else in the data folder outside the attachment store and project homes, so no client can hold a copy of the database. Over every root, a fixed deny list of credential locations applies, checked after the path is resolved: `~/.ssh`, `~/.config/gh`, `~/.gnupg`, `~/.aws`, `~/.netrc`, `~/.git-credentials`, `~/.config/git/credentials` (and `$XDG_CONFIG_HOME/git/credentials` when that is set), the Claude and Codex auth files (`~/.claude/.credentials.json`, `~/.claude.json` and `$CODEX_HOME/auth.json`, by default `~/.codex/auth.json`) and `~/Library/Keychains`. A root that contains one of them, such as a workspace folder set to `~`, still never serves it.

`ListFolder` lists only folders, with their names and whether each is a Git repository, and reads nothing inside them. It applies the same resolution, and lists only folders under the same roots, except that the host itself and a `remote-hosts` Control device may list folders under the host's home folder, since choosing a new repository needs that. Both run off the host thread, as Git reads do.

With these, `workspace-desktop` needs `workspace-host` only as a dev-dependency.

### 7. Quitting and the menu bar
The quit prompt applies when the app quits with a launchd host and at least one agent is working: "Quit Wiffletree?", "N agents are working on this Mac. They can keep going after the window closes, and you can check in from your other devices.", **Stop agents and quit** and **Keep them running**, the default.

- **Keep them running** closes the windows, removes the Dock icon, and leaves the app running in the background, connected to the host. While the app is in the background its menu bar icon is always shown: quiet while no agent works, and with the count while any does, including turns that start later from a timer or a paired device. The icon's menu shows how many are working, **Open Wiffletree**, **Stop agents and quit** and **Quit Wiffletree**. Quit Wiffletree quits the app and leaves the host and its turns running.
- **Stop agents and quit** sends `StopAgentsAndQuit`. The host runs the existing shutdown, exits successfully so launchd does not restart it, and the app exits once the host's reply arrives.

Quitting while no agent is working asks nothing: the app closes its windows and leaves the Dock exactly as Keep them running does, so the host keeps running and the quiet icon stays in the menu bar. To end the app itself, the human chooses Quit Wiffletree from the icon.

A staged update (`update.rs`) installs when the app process exits: on Quit Wiffletree and on Stop agents and quit. Closing the windows into the background does not install it.

The menu bar icon lives in the app, not the host, so the host keeps building without any GUI dependency.

### 8. Keeping the Mac awake
While at least one turn or compaction runs, the host holds one IOKit power assertion of type `PreventUserIdleSystemSleep`, named "Wiffletree agents are working", and releases it when none runs. The display may still sleep. A MacBook with its lid closed and no external display still sleeps; macOS gives a background process no way to prevent that. On Linux the host holds nothing for now.

### 9. Losing the host
If the socket closes or the host does not answer, the desktop keeps the last snapshot on screen, tries again after 0.5 s, doubling up to 30 s (and at once if launchd has just restarted the host), and on success sends `Hello` and `Subscribe` with its last cursor. A send whose reply never came is retried with the same message id, which the host's existing dedup accepts once.

### 10. Settled with the human on October 19, 2026
- The host does not start at login (1).
- Quitting with no agent working asks nothing and leaves the host running (7).
- While the app is in the background, its menu bar icon is always shown, quiet when idle and with a count while agents work; its menu's Quit Wiffletree quits the app and leaves the host running, and a staged update installs on that quit and on Stop agents and quit (7).
- New turns are not held while an update's restart waits (4); Restart now covers a host that stays busy.

## Risks

- macOS may show a "Background item added" notification for the job and list it in System Settings › Login Items, where the human can turn it off. The app then says the host could not start and how to allow it.
- Copying the host binary out of the bundle keeps its code signature, but Gatekeeper's treatment of the copy must be checked on a notarized build before release.
- **The Keychain guarantee is unproven.** If macOS answers another program's request for the credential with an allow/deny dialog rather than a silent refusal, a human who clicks Allow hands it to that program, which is weaker than a refusal. Until the spike (task 0.1) settles it, treat the credential as a dialog-guarded secret, not a refusal-guarded one.
- Agents are same-user peers of `host.sock`. The client credential (2) guards the human-only commands; everything else an agent could do through the socket it can already do as the user.
- Every reply now crosses a socket. The local transport target in the PRD (low overhead, bounded buffering) is to be measured with the existing host benchmark, not assumed.

## Open questions

None.
