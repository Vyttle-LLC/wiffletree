## Why

The host lives inside the desktop app. `Bridge::start` calls `Service::start` in the app's own process, the app reads host files straight from disk, and quitting the app stops every turn. Nothing can reach the host except that one window, so it cannot be driven from another computer, and closing the app or installing an update interrupts agents mid-turn.

This change is the first step of remote control (see `remote-hosts`). It makes the host a process of its own that the desktop talks to only through commands, so a remote device can later use exactly the same commands.

## What Changes

- **The host runs on its own.** `workspace-host` runs as a per-user process under launchd, from a copy of its binary that an app update does not replace. It keeps owning its data folder exclusively, as it does today. A desktop started with `--data-dir` starts its own host as a child instead, so tests and scratch stores never install a background item.
- **The desktop connects over a Unix socket.** `<data folder>/host.sock`, mode 0600, open only to the same user. Every request is the existing JSON-lines `Request` / `Response`. The desktop stops linking the host crate except in its tests.
- **A handshake.** The first message on a connection names the protocol versions and capabilities each side has. A host accepts its own protocol version and the one before it. What happens when the app updates while turns run is defined: the old host keeps running until no turn runs, then restarts into the new version, or sooner if the human chooses.
- **One sequenced event stream.** A subscription with a resume cursor replaces the four change pings (`changes`, `step_changes`, `progress_changes`, `usage_changes`). A client that cannot resume takes a fresh snapshot, as today.
- **Host files only through the host.** Opening a linked file, attachment thumbnails, the folder pickers' contents, the skills' core versions, the login environment and the data folder's migration move behind host commands or into the host. The audit is in design.md.
- **Quitting asks.** While agents are working, quitting asks "Keep them running" (the default) or "Stop agents and quit". A menu bar icon shows while agents run with no window open.
- **The Mac stays awake** while a turn or a compaction runs.
- **Not included:** listening on the network, pairing, other devices and the host switcher (`remote-hosts`); Linux service management; any change to how turns, tickets or verification work.

## Capabilities

### New Capabilities
- `host-process`: the host as its own process, the socket, the handshake, updates under running turns, the event stream, host-file commands and keeping the Mac awake.

### Modified Capabilities
- `workspace-host`: the command boundary gains the socket and the handshake; a quit stops turns only when the human asks for it.
- `native-workspace`: the quit prompt, the menu bar icon and the desktop's reconnect to its host.

## Impact

- `workspace-core`: `PROTOCOL_VERSION` 2; `Hello`, `Event`, `Cursor`; commands `Subscribe`, `ResolvePaths`, `ReadFile`, `ListFolder`, `StopAgentsAndQuit`, `RestartHost`; the skills' core versions in the skills reply; `OwnDescriptors`, which `update.rs` uses, moves here from `workspace-host::runtime`.
- `workspace-host`: a `serve` mode in `main.rs` with the socket listener; the event log in `service.rs` in place of the four signals; the login environment and `data_dir` migration at startup; a power assertion while turns run; the versioned binary copy.
- `workspace-desktop`: `bridge.rs` becomes a socket client; `main.rs` (launch, quit prompt, menu bar icon, event loop); `file_links.rs`, `attachment_chips.rs`, `skills_view.rs` and `login_env.rs` change as audited; `Cargo.toml` keeps `workspace-host` only as a dev-dependency.
- Packaging: `scripts/package_macos.sh` is unchanged except for the LaunchAgent label; the plist is written by the app at first launch.
- Docs: `docs/DEVELOPMENT.md` (running and stopping the host, its log, `--data-dir`).
