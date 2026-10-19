Starts once `standalone-host` is archived. Three implementation tickets: the host side (1–3), the client side (4–6), then the second phase and the protocol document (7–8). Phase 1 ends with two Macs on one network; phase 2 with the same over Tailscale. Each ticket ends with `cargo test --workspace` passing.

## 1. Spike (ticket A)

- [ ] 1.1 On a notarized build, check how macOS Local Network privacy treats the launchd host copy when it listens and advertises, and record the result in design.md. If it is not prompted for, move advertising to the app as design.md's Risks describe.
- [ ] 1.2 Settle the open questions in design.md with the human before 2.x.

## 2. Host: keys, listener and pairing (ticket A)

- [ ] 2.1 `workspace-core`: access levels, the remote-access, pairing and device commands, `UploadAttachment`, the `attention` event and `Subscribe`'s list of kinds, `host_ms` on replies.
- [ ] 2.2 The host key in the Keychain; the device table in the store (a schema version bump with its migration test).
- [ ] 2.3 The remote-access setting in this Mac's host settings; the rustls listener and WebSocket paths, opened and closed with it. Test: off by default binds nothing; turning it off closes a live connection.
- [ ] 2.4 Pairing mode: code, 2 minutes, one exchange at a time, SPAKE2 with key confirmation over the transcript, the wrong-try limit, Control for a new device. Tests: success; reuse of a code; the wrong-try limit closes pairing; expiry; a relaying machine with its own certificate fails confirmation on both sides. Record the test vectors for the protocol document.
- [ ] 2.5 The challenge on `/v1` and pinning on the client side. Tests: an unknown device gets `unpaired`; a changed host key is refused before any request.
- [ ] 2.6 Bonjour advertising with the TXT fields while the setting is on.

## 3. Host: enforcement, uploads and credentials (ticket A)

- [ ] 3.1 The access table of design.md 1, checked per command from the store; closing a device's connections when its level changes or it is removed. Tests: every command in `Command` is covered by one table row (a test fails when a new command is not classified); View only refuses a send and stores nothing; a remote device cannot open pairing or change levels, or stop or restart the host.
- [ ] 3.2 Staged uploads in 128 KiB pieces, the 50 MB limit, expiry after an hour, and `Send` naming uploads. Test: an upload is stored with its message; an unused one is deleted.
- [ ] 3.3 The credentials test: a scripted remote session against a host with planted secrets finds none of them in replies or events.
- [ ] 3.4 Settings › Remote access at the host: the switch, "This Mac is called …" with its addresses, Pair a device with the code and timer, Paired devices with level and Remove, and the explanation text, as in the mockup's study 2.

## 4. Client: connecting and switching (ticket B)

- [ ] 4.1 The device key; the client's list of paired hosts in its own settings; a connection per paired host with the pinning and challenge; full subscription for the selected host and `attention` only for the others.
- [ ] 4.2 Connect a computer: Bonjour browsing, a typed name or address, and the pairing sheet, worded as in study 3. `Info.plist` keys in `scripts/package_macos.sh`.
- [ ] 4.3 The host switcher in the sidebar footer with states, counts, Connect a computer… and Remote access settings…, as in study 1. Test the row states and counts from fake connections.

## 5. Client: working on a remote host (ticket B)

- [ ] 5.1 Host labels: the sidebar heading, the breadcrumb, the composer's model line, and paths in `file_links.rs` and the leftovers sheet, as in study 4.
- [ ] 5.2 Opening a host's file as a read-only copy; Reveal in Finder only on This Mac; the in-app folder browser over `ListFolder` for repositories, the workspace folder and the brain folder on a remote host.
- [ ] 5.3 Uploading attachments to a remote host from the composer.
- [ ] 5.4 The Workspace pages bound to the selected host; saving turned off with the reason on View only; the split Settings window, as in study 7. Test: a save on a remote host reaches that host only.

## 6. Client: disconnected (ticket B)

- [ ] 6.1 The frozen view with its age, the banner, Try now, and other actions disabled.
- [ ] 6.2 Messages written while disconnected, sent once with their ids; resending a send whose reply was lost. Test with a host whose connection is cut after it stored a message: the conversation shows it once.
- [ ] 6.3 Backoff (1 s doubling to 30 s with jitter), an immediate try on a network change, and resuming from the cursor or a snapshot.

## 7. Phase 2: outside one network (ticket C)

- [ ] 7.1 Reconnecting a paired host by its last address before browsing, so a host reached by name over Tailscale reconnects without Bonjour.
- [ ] 7.2 Latency: `host_ms`, the round trip and the network delay per request kind in the log; the tooltip on a connected host's row.
- [ ] 7.3 Two Macs over Tailscale on different networks: pair by name, drive a project, drop and restore the connection. Record the local and remote latency figures in `docs/performance/`.

## 8. Docs (ticket C)

- [ ] 8.1 `docs/remote-protocol.md` as design.md 14 lists, with the pairing test vectors from 2.4.
- [ ] 8.2 `docs/DEVELOPMENT.md`: turning on remote access, pairing, removing a device, and what View only and Control allow. `docs/PRD.md` stays in step with what shipped.
