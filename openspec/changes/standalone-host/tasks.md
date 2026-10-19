Three implementation tickets, in order: the host serves a socket (1–2), the desktop uses only that socket (3–4), then quitting, updates and staying awake (5–6). Each ticket ends with `cargo test --workspace` passing.

## 1. Core and protocol (ticket A)

- [ ] 1.1 `workspace-core`: `PROTOCOL_VERSION` 2; `Hello` and its answer; `Event` and `Cursor`; `Command::Subscribe`, `ResolvePaths`, `ReadFile`, `ListFolder`, `StopAgentsAndQuit`, `RestartHost`, each classified in `is_read`; move `OwnDescriptors` here from `workspace-host::runtime`.
- [ ] 1.2 Write the protocol down in `docs/host-protocol.md`: framing, the handshake, every command's JSON shape and whether it is a read, the events and the cursor rules. It is the reference `remote-hosts` and a later iPhone client build on.

## 2. Host process (ticket A)

- [ ] 2.1 `main.rs`: `workspace-host serve --data-dir <dir>` binds `<dir>/host.sock` at 0600, checks the peer's user, and refuses a too-long socket path. The stdin mode stays as it is. Test: a second user is simulated by a rejecting peer check; a request before `Hello` is refused; `scripts/test_communications.py` still passes.
- [ ] 2.2 The handshake and version rule (own version and the one before). Test: same version, one behind, none in common.
- [ ] 2.3 `service.rs`: replace the four signals with the event log (1,024 events, epoch, per-subscriber queue of 256, `resync`). Test: resume replays exactly the missed events; an earlier epoch gets `resync`; a subscriber that never reads does not block the actor or other subscribers; one provider event yields at most one event per kind.
- [ ] 2.4 The host adopts the login-shell environment (moved from `login_env.rs`) and runs the `Agent Workspace` folder migration (moved from `data_dir.rs`) before opening the store. Move their tests with them.
- [ ] 2.5 `ResolvePaths`, `ReadFile` and `ListFolder` off the host thread, with the allowed roots and the 50 MB bound. Test: a worktree file resolves; an executable is marked as running when opened; `/etc/hosts` is refused; a listing names folders and repositories only.
- [ ] 2.6 The skills reply carries the core instructions' names and versions.

## 3. Desktop on the socket (ticket B)

- [ ] 3.1 `bridge.rs`: a socket client with `Hello`, request correlation by id, and the event subscription feeding the existing reload paths in `main.rs`, `progress.rs` and the usage view. Keep `Bridge`'s `request`, `send` and `ask` signatures so views do not change.
- [ ] 3.2 Launching: install and bootstrap the LaunchAgent for the default data folder; start a child host for `--data-dir`. Test with `--data-dir`: no plist is written and the child stops with the app.
- [ ] 3.3 Reconnect with growing delay, resume from the cursor or snapshot, and resend an unanswered `Send` with its id. Test: kill the child host mid-send and restart it; the message is stored once.
- [ ] 3.4 Move the desktop's host-file reads behind commands as in design.md 6: `file_links.rs`, `attachment_chips.rs`, `skills_view.rs`; remove `login_env.rs` from the app. Test: link resolution and thumbnails through a real host in a temporary folder.
- [ ] 3.5 `Cargo.toml`: `workspace-host` only under `[dev-dependencies]`; `cargo build --release -p workspace-desktop` passes.

## 4. Desktop checks (ticket B)

- [ ] 4.1 `scripts/drive_desktop.py` runs unchanged against a desktop started with `--data-dir`.
- [ ] 4.2 Measure a `Snapshot` and a `Messages` page over the socket against the in-process figures in `docs/performance/host-baseline.md` and record both.

## 5. Quitting and the menu bar (ticket C)

- [ ] 5.1 The quit prompt with its wording, Keep them running as the default, and no prompt for a child host.
- [ ] 5.2 Stop agents and quit: `StopAgentsAndQuit` runs the shutdown and exits 0; the app exits after the reply. Test through `Service`: running turns are recorded with `QUIT_DURING_TURN` and their groups stopped with reason `quit`.
- [ ] 5.3 The menu bar icon, its menu, leaving the Dock while windows are closed, and exiting when no agent works.

## 6. Updates and staying awake (ticket C)

- [ ] 6.1 The versioned host copy under `<data folder>/host/<version>/`, the plist pointing at it, helpers started from it, and removal of unused copies. Test: replacing the bundle's binary while a child host runs leaves the running host's helper path unchanged.
- [ ] 6.2 `RestartHost { when: Idle | Now }` and the sidebar's update line with Restart now; the no-common-version screen. Test: an idle restart waits for a running fake turn to end, then exits cleanly.
- [ ] 6.3 The power assertion while any turn or compaction runs. Test the assertion count around a fake turn on macOS.

## 7. Docs

- [ ] 7.1 `docs/DEVELOPMENT.md`: the host as a LaunchAgent, its log, how to stop or remove it, `--data-dir` hosts, and the open questions settled.
- [ ] 7.2 `docs/PRD.md` stays in step with what shipped.
