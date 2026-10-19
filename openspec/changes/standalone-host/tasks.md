Three implementation tickets, in order: the host serves a socket (1–2), the desktop uses only that socket (3–4), then quitting, updates and staying awake (5–6). Each ticket ends with `cargo test --workspace` passing.

## 1. Core and protocol (ticket A)

- [ ] 1.1 `workspace-core`: `PROTOCOL_VERSION` 2; `Hello` and its answer; `Event` and `Cursor`; `Command::Subscribe`, `ResolvePaths`, `ReadFile`, `ListFolder`, `StopAgentsAndQuit`, `RestartHost`, each classified in `is_read`; move `OwnDescriptors` here from `workspace-host::runtime`.
- [ ] 1.2 Write the protocol down in `docs/host-protocol.md`: framing, the handshake, every command's JSON shape and whether it is a read, the events and the cursor rules. It is the reference `remote-hosts` and a later iPhone client build on.

## 2. Host process (ticket A)

- [ ] 2.1 `main.rs`: `workspace-host serve --data-dir <dir>` binds `<dir>/host.sock` at 0600, checks the peer's user, and refuses a too-long socket path. The stdin mode stays as it is. Test: a second user is simulated by a rejecting peer check; a request before `Hello` is refused; `scripts/test_communications.py` still passes.
- [ ] 2.2 The handshake and version rule (own version and the one before). Test: same version, one behind, none in common.
- [ ] 2.3 `service.rs`: replace the four signals with the event log (1,024 events, epoch, per-subscriber queue of 256, `resync`). Test: resume replays exactly the missed events; an earlier epoch gets `resync`; a subscriber that never reads does not block the actor or other subscribers; one provider event yields at most one event per kind.
- [ ] 2.4 The host adopts the login-shell environment (moved from `login_env.rs`) and runs the `Agent Workspace` folder migration (moved from `data_dir.rs`) before opening the store. Move their tests with them.
- [ ] 2.5 `ResolvePaths`, `ReadFile` and `ListFolder` off the host thread, with the allowed roots and the 50 MB bound. Test: a worktree file resolves; an executable is marked as running when opened; `/etc/hosts`, `workspace.sqlite3`, its `-wal` file and `backups/` are refused; an attachment under `<data folder>/projects/<id>/attachments/` of a project whose home is in the workspace folder is served; a symlink in a worktree to a file outside every root is refused; a 60 MB file is refused; `ListFolder` outside the roots is refused except under the home folder for the host itself; a listing names folders and repositories only.
- [ ] 2.6 The skills reply carries the core instructions' names and versions.
- [ ] 2.7 The client credential: created at first start in a Keychain item limited to the signed app and the host, required on `host.sock` for the human-only commands, passed to a child host on its standard input. Tests: without it, `ResolveAttention` is refused and the question stays open; with it, the answer is recorded; `Snapshot` works either way; the child host's environment does not contain it.

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

- [ ] 5.1 The quit prompt with its wording, Keep them running as the default, no prompt with nothing working, and no prompt for a child host. Tests: the prompt's default choice is Keep them running; with nothing working, quitting asks nothing and sends no stop; a desktop started with `--data-dir` asks nothing and its child host has exited afterwards.
- [ ] 5.2 Stop agents and quit: `StopAgentsAndQuit` runs the shutdown and exits 0; the app exits after the reply. Test through `Service`: running turns are recorded with `QUIT_DURING_TURN` and their groups stopped with reason `quit`.
- [ ] 5.3 The menu bar icon and its menu, leaving the Dock while windows are closed, and staying in the background so the icon also appears for a turn that starts later from a timer or a paired device.

## 6. Updates and staying awake (ticket C)

- [ ] 6.1 The versioned host copy under `<data folder>/host/<version>/`, the plist pointing at it, helpers started from it, and removal of unused copies. Child hosts are not copied and run from the bundle (or `target/`). Test, on the launchd path without launchd: `serve` started from a versioned copy in a temporary data folder keeps its own path as the helper path after the source binary it was copied from is replaced, and an unused copy is removed at the next start.
- [ ] 6.2 `RestartHost { when: Idle | Now }` and the sidebar's update line with Restart now; the no-common-version screen. Test: an idle restart waits for a running fake turn to end, then exits cleanly; with no common protocol, the refusal carries the working count and `RestartHost` on the refused connection is accepted while any other command is not.
- [ ] 6.3 The power assertion while any turn or compaction runs. Test the assertion count around a fake turn on macOS.

## 7. Docs

- [ ] 7.1 `docs/DEVELOPMENT.md`: the host as a LaunchAgent that does not start at login, its log, how to stop or remove it, `--data-dir` hosts, the client credential, and that it is not a sandbox.
- [ ] 7.2 `docs/PRD.md` stays in step with what shipped.
- [ ] 7.3 When archiving this change, repoint the links to `openspec/changes/standalone-host/` in `docs/PRD.md` and `design/README.md` to its archived path under `openspec/changes/archive/`.
