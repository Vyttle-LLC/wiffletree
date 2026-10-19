## Why

Michael's agents run on one Mac, and he wants to follow and steer them from another: a MacBook on the couch now, an iPhone later. Today the only way in is the window on the Mac that runs them. Once `standalone-host` makes the host a process of its own with a handshake and a resumable event stream, other paired devices can use the same commands over the network.

## What Changes

- **One Mac is the host.** It runs the agents and keeps the repositories, worktrees, Claude, Codex, GitHub and Git credentials, and the database. Other paired devices drive its projects; they never hold a copy of its database. One host per project.
- **The host listens only when asked.** "Let other devices use this Mac's Wiffletree", in a new Settings › Remote access, is off by default. While it is on, the host listens on TCP and advertises `_wiffletree._tcp` over Bonjour.
- **Pairing in person.** At the host, the human turns on pairing and reads out a one-time 4-digit code, valid for 2 minutes. The other device runs it through SPAKE2; a few wrong tries close pairing mode. The two sides exchange long-term keys, and every later connection is TLS 1.3 (rustls) pinned to the paired key. A changed key is refused until the device pairs again.
- **Access levels.** A newly paired device has **Control**; the other level is **View only**, which the host enforces on every command by refusing changes. Pairing, access levels and removal change only at the host itself, never from a remote device. Remove takes effect at once, even while the device is offline.
- **The host switcher.** The sidebar's "Local workspace" line becomes a switcher listing This Mac and each paired host, with how many things need the human on each, plus Connect a computer… and Remote access settings…. Connect a computer lists nearby hosts and accepts a typed name or address, for Tailscale or a VPN.
- **It looks local, but says where things are.** Paths are labelled with their host and open through that host. The Workspace pages, Models, Skills, Repositories and Usage, show and edit the selected host's settings; a Control device may save them. The Settings window splits into "This Mac · the app" and "<host> · the host you're driving".
- **Disconnected.** The view freezes with its age, the host's agents keep going, drafts are kept, Try now reconnects, and nothing sent is sent twice. Reconnection backs off; latency is reported apart from network delay.
- **A documented JSON protocol over WebSocket and TLS**, usable from SwiftUI without shared Rust code, so an iPhone client can follow.
- **Phases.** Phase 1: same network, found by Bonjour. Phase 2: a typed name or address, over Tailscale or a VPN. No relay service.
- **Not included:** the iPhone client and push notifications; peer coordinators between hosts, projects spanning hosts and moving sessions between hosts; a relay; Linux hosts.

## Capabilities

### New Capabilities
- `remote-access`: the host side: the listener, discovery, pairing, keys and TLS, devices and access levels, enforcement per command, uploads and keeping credentials on the host.
- `remote-client`: the client side: Connect a computer, the host switcher and attention counts, labelled paths, host settings from another device, the disconnected state, reconnecting and latency.

### Modified Capabilities
<!-- None. settings-repository's per-Mac and shared split is unchanged: a setting still belongs to the Mac that runs it. -->

## Impact

- `workspace-core`: access levels; commands for remote access, pairing and devices (host-only), `UploadAttachment`; the attention count event.
- `workspace-host`: a `remote` module: the TCP and WebSocket listener with rustls, the SPAKE2 pairing exchange, the device table in the store, enforcement per connection, Bonjour advertising, staged uploads.
- `workspace-desktop`: a client connection per paired host; the switcher in `sidebar.rs`; Connect a computer and pairing sheets; Settings › Remote access; the split Settings window; host labels on paths in `file_links.rs` and the leftovers sheet; the in-app folder browser for remote hosts; the disconnected banner in `conversation.rs`.
- Packaging: `NSLocalNetworkUsageDescription` and `NSBonjourServices` (`_wiffletree._tcp`) in `Info.plist`.
- Docs: `docs/remote-protocol.md`, `docs/DEVELOPMENT.md`, `docs/PRD.md`.
- Mockup: `ui/mockups.html`, the study approved on 2026-10-19, copied unchanged.
- Depends on `standalone-host`.
