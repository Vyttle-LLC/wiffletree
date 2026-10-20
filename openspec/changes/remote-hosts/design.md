## Context

See proposal.md for why and `ui/mockups.html` for the approved look and wording. This change builds on `standalone-host`: the host is its own process, every client speaks the JSON protocol in `docs/host-protocol.md` with a handshake and a resumable event stream, and the desktop reads host files only through commands. `Command::is_read` classifies every command as a read or a change. The host already deduplicates messages by id and rejects a changed payload under an existing id.

Termic 1.17.2 pairs machines on a LAN the same way (Bonjour `_termic._tcp`, a SPAKE2 4-digit code, per-pair keys, drive and reply grants). It is a reference for the exchange, not a dependency.

## Decisions

### 1. Where a connection comes from decides what it may do
A connection on the host's Unix socket is **the host itself**: the human at that Mac. It may do everything, including pairing and changing who has access; the commands only the human should give also need the app's client credential there, as `standalone-host` design 2 defines, because agents can reach that socket too. A connection over the network is a **paired device**, with that device's access level. Nothing a remote device sends can make it the host itself.

| Commands | The host itself | Control | View only |
| --- | --- | --- | --- |
| Reads (`is_read`), `Hello`, `Subscribe` | yes | yes | yes, except the rows below |
| Reads that take a host path: `ReadFile`, `ResolvePaths`, `ListFolder` | yes | within the `standalone-host` roots; `ListFolder` also under the home folder | within the roots only |
| `DiscoverRepositories`, which scans the folders it is given | yes | yes | refused |
| `InspectAttachments`, which reads host paths the client names | yes | refused; remote devices upload instead (8) | refused |
| `ComparePausedFile` | yes | yes | yes: it only accepts a file the host itself recorded as paused in its shared folder |
| Every other workspace command, settings included, and `UploadAttachment` | yes | yes | refused |
| Remote access, pairing, devices and access levels | yes | refused | refused |
| `StopAgentsAndQuit`, `RestartHost` | yes | refused | refused |

Every path is resolved, symlinks and `..` included, before it is checked against the roots.

The level is read from the store for each command, so a change at the host applies to the next command. Changing a device's level or removing it also closes its open connections at once, so it reconnects under the new level or not at all. A refused command answers `"View only: <host> refuses changes from this device"` or `"Only <host> itself can change remote access"`, and runs nothing.

### 2. Keys
Every host and every client device has one long-term P-256 key pair, created the first time it is needed and kept in the login Keychain on macOS. P-256 because it is what the Secure Enclave, CryptoKit and Apple's certificate trust checks support, so an iPhone can hold its key in hardware and check the host's certificate without extra libraries. A host's id is the SHA-256 of its public key; the first 8 bytes in hex identify it in Bonjour and in logs.

The host stores each paired device in the store: its id, public key, name, level, when it paired and when it last connected. A client stores each paired host in its own client settings: the host's id, public key, name, and the addresses it was last reached at. Neither stores the other's private key, and neither key ever leaves its Keychain.

### 3. The listener
While "Let other devices use this Mac's Wiffletree" is on, the host listens on TCP port 47821 on every interface (a typed address may name another port), TLS 1.3 only, with rustls. Its certificate is self-signed and carries the host's key. Two WebSocket paths: `/pair` while pairing mode is open, and `/v1` for paired devices. Turning the setting off closes the listener, every remote connection and the advertisement; paired devices stay paired. The setting is kept in this Mac's host settings and is never synced through a settings repository.

### 4. Pairing
Pairing methods are pluggable. The first message on `/pair` names the method, and the host refuses a method it does not offer. Every method ends the same way: both sides hold each other's long-term public key, the host stores the device with level **Control**, and pairing mode closes. Two methods are defined:

- **`code`**, for Macs, built in this change: a 4-digit code run through SPAKE2, described below.
- **`qr`**, for the iPhone, specified in the protocol now and built with the iPhone client, including the QR screen on the host's Remote access page. Apple provides no SPAKE2, so this method needs only CryptoKit. The host shows a QR code holding its public key, its addresses and port, and a one-time secret of at least 128 random bits that works once, for 2 minutes. The iPhone opens TLS pinned to the key from the QR code, so no machine in the middle can answer, sends its public key and name, and proves it holds the secret with an HMAC-SHA256 under the secret over the transcript (both public keys and both names). The host answers with its own HMAC over the same transcript. A wrong HMAC counts as a wrong try, as for `code`.

**The `code` method.** At the host, Settings › Remote access › Pair a device opens pairing mode with a random 4-digit code for 2 minutes. On the other device, Connect a computer › Pair asks for the code.

1. The device opens TLS to the host without checking its certificate yet, and connects to `/pair`. It notes the certificate's public key.
2. Both sides run SPAKE2 (the Ed25519 group, as the RustCrypto `spake2` crate implements it) with the code as the password and fixed identities `wiffletree-device` and `wiffletree-host`.
3. Each side sends a confirmation, an HMAC over the transcript under the derived key: the device first, and the host, which counts a failed one as a wrong try, answers with its own only once the device's holds. The transcript holds the host's public key as each side sees it (from the TLS certificate on the device, its own key on the host), the device's public key, its name and the host's name. A wrong code, or a machine in the middle presenting another certificate, makes a confirmation fail.
4. On success the host stores the device with level **Control** and closes pairing mode, so the code is used once. The device stores the host. Each side shows the other's name.

Each failed confirmation counts as a wrong try. After 3 wrong tries, pairing mode closes and the code no longer works; the human starts pairing again at the host. One pairing exchange runs at a time; a second device trying meanwhile is told to wait.

### 5. Every later connection
The device opens TLS 1.3 to the host and accepts the certificate only if its public key is the one stored at pairing. The host then sends a random challenge; the device answers with its id and an ECDSA signature over the challenge, the host's id and its own id. The host checks it against the stored device key and closes the connection with `unpaired` if the device is unknown or removed. Only then does the `Hello` handshake of `standalone-host` run.

A certificate with another key is refused before anything is sent: "Studio Mac's key has changed. Wiffletree won't connect until you pair again." A host that answers `unpaired` shows: "Studio Mac no longer accepts this Mac. Pair again from Studio Mac." The app never replaces a stored key on its own.

The device authenticates inside the WebSocket rather than with a TLS client certificate, because a client certificate needs a Keychain identity that is awkward to build on an iPhone, while a signature over a challenge is a few lines of CryptoKit. Pinning the host's key means the challenge can only be answered to the real host.

### 6. Finding a host
While its setting is on, the host advertises `_wiffletree._tcp` through the system's DNS-SD service with the TXT fields `id` (the short host id), `name`, `version` and `pairing` (`1` while pairing mode is open). Connect a computer browses for it and lists each host as "Studio Mac · Pairing now · Wiffletree 0.4" or "Mac mini · Pairing is off there". A typed name or address, with an optional `:port`, connects directly; this is how Tailscale and VPNs work, since nothing is discovered there. A paired host is reconnected by its last address first and then by browsing for its id.

The app's `Info.plist` gains `NSLocalNetworkUsageDescription` and `NSBonjourServices` with `_wiffletree._tcp`.

### 7. One connection per paired host
The client keeps a connection to every paired host it can reach while it runs. For the selected host it subscribes to every event. For the others it subscribes only to a new `attention` event, which carries the host's count of things that need the human: its attention items plus its workspace notes, as the Inbox rail counts them today (`sidebar.rs::needs_you` without the project filter), so the switcher shows counts for every host without loading their snapshots. `Subscribe` gains an optional list of event kinds for this.

Switching hosts never stops anything; each host keeps running on its own, and This Mac keeps its own projects.

### 8. Files between devices
- **Attachments.** A file dropped on a remote host's conversation is uploaded with `UploadAttachment { upload_id, offset, bytes }` in 128 KiB pieces, base64 in JSON, which keeps each request under the 256 KiB line bound. `Send` names uploaded ids in place of paths. The host keeps an upload until a send uses it or for one hour; an upload cut off by a dropped connection resumes from the host's last stored offset. The 50 MB attachment limit applies.
- **Opening a host's file.** Clicking a path on a remote host asks the host with `ResolvePaths`, reads the file with `ReadFile`, and opens a read-only copy from the client's cache folder for that host; the file is not shown inside Wiffletree. Reveal in Finder is offered only for This Mac. Copy path copies the host's path.
- **Choosing a folder on the host.** Adding a repository, choosing the workspace folder or a brain folder on a remote host opens an in-app folder browser over `ListFolder`, starting at the host's home folder, in place of the system panel.

### 9. Labels
While the switcher is on a remote host, the sidebar heading reads "Projects · 2 · on Studio Mac", the breadcrumb starts with the host ("Studio Mac › Tide alerts › Coordinator"), the composer's model line ends "on Studio Mac", and paths read "Studio Mac: ~/dev/tidepool". On This Mac nothing gains a label, as today.

### 10. Settings
A setting belongs to the Mac whose agents it runs. With the switcher on a remote host, the Workspace section's Repositories, Models, Skills and Usage and limits are that host's, read and saved through its commands; the Models page reads "each job's model on Work Studio" and "Saving changes Work Studio's agents only. Your MacBook keeps its own table for its own projects." A View only device sees them with saving turned off and the reason.

The Settings window has two parts: "This Mac · the app" (appearance and theme, computers you've paired with, window layout) and, while the switcher is on a remote host, "<host> · the host you're driving" (models, repositories and checks, workspace folder, PR owners allowed, check-ins, reviews, skills, settings repository), ending with "Remote access (pairing and who can control it) changes only at <host> itself." On This Mac, the second part is This Mac's host settings and includes Remote access. The per-Mac and shared split of `settings-repository` is unchanged.

### 11. Disconnected
When a host stops answering, its view stays as last received and a banner says "Reconnecting to Studio Mac…" and "Showing what it sent 40 seconds ago. Its agents keep working.", with **Try now**. The age counts up. The composer still works: "You can write; it sends when Studio Mac is back." A message sent meanwhile gets its id when it is written and is sent with that id once the host answers, so a send already received is stored once. Messages waiting to be sent are kept only while the app runs: like drafts today, they are lost if the app quits before the host is back. Other actions are disabled until the host is back. A host offline when the app starts shows "Offline · last seen 2 hours ago" in the switcher.

Reconnecting tries after 1 s, doubling to at most 30 s, with up to 20% jitter. A change of network on the client, and Try now, try at once and restart the delay. On reconnecting, the client resumes its subscription from its cursor or takes a snapshot, as `standalone-host` defines.

### 12. Latency apart from network delay
Each reply gains `host_ms`, the time from the host reading the request to writing the reply. The client measures each request's round trip; network delay is the round trip less `host_ms`. A WebSocket ping every 15 s measures the network while idle. The client logs both figures per request kind, and the switcher's row for a connected host shows the latest network delay in its tooltip. This gives the PRD's comparison of local and remote latency with network delay reported separately.

### 13. Credentials stay on the host
No command or event carries a provider, GitHub or Git credential, a token, or the host's environment. Provider sign-in states and account labels are shown as the host reports them, as today. Publishing, the PR keeper and their checks run on the host, as today. A test scans every reply and event of a scripted session for planted secret values.

### 14. The protocol for other clients
`docs/remote-protocol.md` documents everything a client needs: TLS and pinning, the pairing methods (`code` and `qr`) with test vectors for each, the challenge, the WebSocket framing (one JSON message per text frame), `Hello`, every command and event as in `docs/host-protocol.md`, the access table above, uploads and errors. Binary data is base64. Nothing in it requires Rust. The iPhone client and push notifications are not part of this change; its first job will be answering the Inbox, which the protocol already serves.

### 15. Settled with the human on October 19, 2026
- The default port is 47821 (3).
- Pairing mode closes after 3 wrong tries (4).
- A remote device may not stop or restart the host process (1). Control devices can still stop agents' turns, as today.
- Pairing is pluggable: Macs use the 4-digit code with SPAKE2; the iPhone scans a QR code and needs only CryptoKit (4).
- Messages written while disconnected do not survive a restart of the app (11).
- Opening a host's file gives a read-only copy on this Mac (8).
- Network delay is shown in the switcher row's tooltip and in the log (12).

## Risks

- **Local Network privacy.** macOS asks before an app uses the local network. The host binary runs from a copy outside the app bundle under launchd, and how macOS attributes its advertising and listening must be checked first (task 1.1). If macOS does not prompt for it, the app advertises on the host's behalf while the setting is on. What task 1.1 found is under Local Network privacy, checked below.
- **Four digits.** The code is safe only because SPAKE2 allows one guess per try and pairing closes after 3 wrong ones; the code must never be checked any other way.
- **Agents on a driving Mac.** The client's own host socket and its paired hosts are reachable by that Mac's agents, which run as the same user. Device private keys stay in a Keychain item limited to the signed app, so an agent cannot sign the challenge as the device; this is defence in depth, not a sandbox.
- **A lost device.** Anyone holding a paired Mac with Control can run commands on the host, since agents run with permission prompts off. Remove at the host is the remedy, and it applies at once.

## Local Network privacy, checked

Checked on October 20, 2026, on macOS 27.0.1 (26A434), with `crates/workspace-host/examples/local_network_probe.rs` copied out of the build, signed with *Apple Development: Michael Hohlios (PF6B6HV656)* and the hardened runtime under its own identifier, and run as a `gui/<uid>` launchd agent as the host copy is. Not with a Developer ID and not notarized. The probe listens on every interface, registers `_wiffletree._tcp`, and opens a TCP connection to the router; `nehelper` and `UserEventAgent` log macOS's decisions.

- **The host copy is prompted for, not silently denied.** Its first connection to the local network is refused with `EHOSTUNREACH` while macOS shows the Local Network alert, even though the agent is not in the foreground. The alert names the copy's file name (`workspace-host`) and is keyed by its signing identifier. Once allowed, the same binary connects at every later start.
- **Listening and advertising do not prompt.** Binding a TCP port on `0.0.0.0` and `DNSServiceRegister` succeed before any decision, and a browse on the same Mac lists the service. Accepting a connection from the same Mac's own network addresses works too.
- **A rebuilt copy is prompted again.** A build with a new Mach-O UUID under the same identifier and identity was asked about afresh ("Local network preference not yet set"); `nehelper` found no team ID for a binary outside a bundle. Each host update, a new copy under `host/<version>/`, would therefore ask again with this signature.
- **An embedded `Info.plist`** (`-sectcreate __TEXT __info_plist`) with `CFBundleIdentifier`, `NSLocalNetworkUsageDescription` and `NSBonjourServices` changes nothing above: same identifier keying, same re-prompt for a new UUID.

So the host advertises and listens itself, as decisions 3 and 6 say, and needs no outgoing local-network connection. Still to confirm:

- **With a second Mac** (Michael's): that it browses the host's advertisement and connects to 47821 both before the host copy has a Local Network decision and after one is denied. If an undecided or denied copy is not reachable from another Mac, the listener moves into the app's process as the fallback above describes, since the app keeps one decision across updates.
- **With a Developer ID and notarization:** whether the decision then follows the team and identifier across host updates, or every new `host/<version>/` copy asks again. If it asks again, advertising and listening move to the app.

## Open questions

None.
