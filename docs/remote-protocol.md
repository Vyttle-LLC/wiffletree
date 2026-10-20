# Remote protocol

How a device drives a Wiffletree host over the network: another Mac today, and later the iPhone client. This page covers what a client in any language needs: TLS and pinning, pairing, the challenge, the framing and what a device may do. The commands and events are those of `docs/host-protocol.md`, which this page does not repeat. The Rust types are in `workspace-core`: `remote.rs` for the messages, `pairing.rs` for the cryptography and `remote_client.rs` for the device's side.

Binary values are standard base64 with padding in JSON, unless this page says hex.

## Keys and ids

- Every host and every device has one long-term P-256 key pair. A public key travels as SEC1 uncompressed, 65 bytes starting `04`, which CryptoKit calls the X9.63 representation.
- A key's id is the SHA-256 of that public key, in lowercase hex. The first 16 hex characters are the short id that Bonjour and logs show.
- Signatures are ECDSA P-256 with SHA-256, raw `r ‖ s`, 64 bytes, which is CryptoKit's `rawRepresentation`.

## Finding a host

- While remote access is on, the host listens on TCP port 47821 on every IPv4 interface and advertises `_wiffletree._tcp` through DNS-SD. The TXT record holds `id` (the short host id), `name`, `version` and `pairing`, which is `1` while pairing mode is open and `0` otherwise.
- A typed address is `name`, `name:port`, `address:port` or `[ipv6]:port`; the port defaults to 47821. Over Tailscale or a VPN nothing is advertised, so the human types the host's name, such as `studio.tail4c2e.ts.net`. A client tries each address the name resolves to in turn.
- A client reconnects a paired host by the addresses it last answered at, newest first, and only then by browsing for its id. A DNS name, such as a typed Tailscale name or `studio.local`, and a Tailscale address (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`) name one machine, so another key there means the host's key has changed, at once. Any other IP address whose certificate shows another key is taken for another machine, since a LAN may hand the address on, and the next is tried. When nothing shows the pinned key, the host's key also counts as changed if an address found under the host's id shows another key, or a host advertises the paired host's name under another id, as after a reinstall.

## TLS and pinning

- TLS 1.3 only. The host's certificate is self-signed for the name `wiffletree-host` and carries the host's long-term key. Send `wiffletree-host` as the server name; no certificate authority is involved.
- **While pairing**, the device accepts any certificate but notes its public key, which goes into the pairing transcript. A machine in the middle shows another key, so the confirmations fail.
- **Once paired**, the device accepts the certificate only if its public key is the one stored at pairing. Another key is refused before anything is sent: "Studio Mac's key has changed. Wiffletree won't connect until you pair again." The client never replaces a stored key on its own.
- The handshake signature is checked against the certificate's key as usual, so only the holder of that key can complete it.

## WebSocket framing

- Over TLS, the device opens a WebSocket on one of two paths: `/pair` while pairing mode is open, and `/v1` for a paired device. Any other path is closed.
- Each message is one JSON value in one text frame. Binary frames are ignored.
- A message from the device is at most 256 KiB, as a line on the host's socket is. A longer one closes the connection. Messages from the host have no such bound: a `read_file` reply carries up to 50 MB as base64, so accept at least 70 MiB.
- Each step before `Hello` must come within 30 seconds.
- The host answers WebSocket pings. To tell a quiet host from one that is gone, send the `ping` command, which the connection answers without waiting for the host's work.

## Pairing

The human opens pairing mode at the host: Settings › Remote access › Pair a device. It stays open for 2 minutes. The first message on `/pair` is `PairStart`, naming a method; a method the host does not offer is refused without counting a wrong try:

```json
{"method": "code", "device_name": "Michael's MacBook Air", "device_key": "BCLPdewVJEdcQR628…", "spake": "Qe7h3aCbmDx3IQJG…"}
```

Every method ends the same way: both sides hold each other's public key, the host stores the device with level Control, and pairing mode closes, so whatever was shown works once. Each failed check is a wrong try; the third closes pairing mode, and the human opens it again at the host. One exchange runs at a time; a second device is told to wait.

The host's messages are `PairReply`: `{"spake": {…}}`, `{"paired": {…}}` or `{"refused": {"reason": "…"}}`. A refusal's reason is a sentence for a person, and the host closes the connection after it.

### The transcript

Both methods confirm the same transcript: the bytes `wiffletree-pair-v1`, then four fields, each behind its length as 4 bytes big-endian:

1. the host's public key: as the device saw it in the TLS certificate, and its own key on the host;
2. the device's public key;
3. the device's name, UTF-8, trimmed and cut to 100 characters;
4. the host's name, UTF-8.

A confirmation is HMAC-SHA256 over the side's label, the bytes `device` or `host`, followed by the transcript. In CryptoKit:

```swift
HMAC<SHA256>.authenticationCode(for: Data("device".utf8) + transcript, using: SymmetricKey(data: key))
```

### The `code` method

For Macs. The host shows a random 4-digit code.

1. The device opens TLS to the host without a stored key and connects to `/pair`, noting the certificate's key.
2. Both sides run SPAKE2 in the Ed25519 group, as the RustCrypto `spake2` crate 0.4 implements it, with the code's 4 ASCII digits as the password, the device as side A with the identity `wiffletree-device` and the host as side B with `wiffletree-host`. A message is 33 bytes: `0x41` (A) or `0x42` (B), then a compressed Edwards point. The shared key is 32 bytes.
3. The device sends `PairStart` with `method: "code"`, its name, its key and its SPAKE2 message in `spake`.
4. The host answers `{"spake": {"spake": "…", "host_name": "Studio Mac"}}`.
5. The device sends `{"confirm": "…"}`: its confirmation under the shared key.
6. The host checks it. A wrong code gives a different key, so the confirmation fails; the host counts a wrong try and refuses with "The code did not match". Otherwise it stores the device and answers `{"paired": {"confirm": "…", "host_id": "…"}}` with its own confirmation.
7. The device checks the host's confirmation and that `host_id` is the id of the key it saw, then stores the host.

The code is never checked any other way. It is safe at 4 digits only because SPAKE2 gives one guess per exchange and the third wrong one closes pairing mode.

### The `qr` method

For the iPhone, which has no SPAKE2 in CryptoKit. It is specified here and built with the iPhone client; until then a host refuses it with "Studio Mac does not offer pairing by qr", counting no wrong try.

1. The host's Remote access page shows a QR code holding one JSON value: `{"host_key": "…", "host_name": "Studio Mac", "addresses": ["192.168.1.20", "studio.tail4c2e.ts.net"], "port": 47821, "secret": "…"}`. `secret` is a new one-time secret of 16 random bytes, valid once and for 2 minutes.
2. The device opens TLS pinned to `host_key` from the QR code, so no machine in the middle can answer, and connects to `/pair`.
3. The device sends `PairStart` with `method: "qr"`, its name and key, and `proof`: its confirmation keyed with the secret.
4. The host checks the proof. A wrong one counts as a wrong try. Otherwise it stores the device and answers `{"paired": {"confirm": "…", "host_id": "…"}}`, where `confirm` is the host's confirmation keyed with the secret.
5. The device checks the host's confirmation, then stores the host.

### Test vectors

The complete vectors, with the keys' secrets and the random bytes each SPAKE2 side drew, are in `crates/workspace-core/tests/fixtures/pairing_vectors.json`. A Rust test regenerates them and fails if they change. `swift scripts/check_pairing_vectors.swift crates/workspace-core/tests/fixtures/pairing_vectors.json` checks every HMAC, the host's id and the challenge signature below with CryptoKit; all match.

Keys, from secret scalars that are the SHA-256 of `wiffletree device key` and `wiffletree host key`:

| | Hex |
| --- | --- |
| Device public key | `0422cf75ec1524475c411eb6f8c62e7cc7bf4c4d03196974bdfc425d0c21cca1f70fe85cf36525bc54d39fb9120c0451a47e889f5c9912c8544b72ca81da48f438` |
| Device id | `6d23aaa2769b03ede587f79616627e40ba12f88ef0d9033a767195266ce007c2` |
| Host public key | `04f9cfa2481609a2edab1b2f1f5c1968a3acf99f5ee98850cc8945f8a97ddc4886dd9240dba9c252083dd79389bf1a8224948a302e62353c00080a1e4de4e72f13` |
| Host id | `bb5287baa8da8c4e9f995cdb6de312e4b9b7f2571af61f116cf0118d124843d3` |

The transcript, for the device `Michael's MacBook Air` and the host `Studio Mac`:

```
776966666c65747265652d706169722d76310000004104f9cfa2481609a2edab1b2f1f5c1968a3acf99f5ee98850cc8945f8a97ddc4886dd9240dba9c252083dd79389bf1a8224948a302e62353c00080a1e4de4e72f13000000410422cf75ec1524475c411eb6f8c62e7cc7bf4c4d03196974bdfc425d0c21cca1f70fe85cf36525bc54d39fb9120c0451a47e889f5c9912c8544b72ca81da48f438000000154d69636861656c2773204d6163426f6f6b204169720000000a53747564696f204d6163
```

`code`, with the code `4817`:

| | Hex |
| --- | --- |
| Device message (A) | `41eee1dda09b983c772102465a4bb7ab8635ff1d58a711cc6c4b0c06d1fdd4bd64` |
| Host message (B) | `42f3fc108418f5ca637f45b494384963ab498ef754a1356d732366e471e067eac4` |
| Shared key | `494c77dacfa5119f01ddbc4d411e16ff7af326e9b5514aa1942adf85916ad85c` |
| Device confirmation | `f26ce7a917260defc5a3356ac0cb79e4252c1a67f0cf654cffc99b729d5ff414` |
| Host confirmation | `c6370810f33096d04a406ab6c1150a7c6eeb88a1846eac8bdfd33a0f2fbcdbc9` |

`qr`, with the first 16 bytes of the SHA-256 of `wiffletree qr secret` as the secret:

| | Hex |
| --- | --- |
| Secret | `011099c6bde34934f959faa21b3aeedf` |
| Device proof | `63d9e0095bfff430cefececacdf5ada871d3b1570091951c2d9fa49af850b54c` |
| Host confirmation | `a13995734713c50f9bb2f209d2cc0941bacfe33e410369e9a11389a904efd1db` |

## The challenge

Every connection of a paired device on `/v1` opens with it:

1. The host sends `{"challenge": "…", "host_id": "…"}`, with a fresh random challenge.
2. The device checks that `host_id` is the id of its stored key, then signs the bytes `wiffletree-challenge-v1`, the challenge, the host's id and its own id, both ids as raw bytes rather than hex. It answers `{"device_id": "…", "signature": "…"}`.
3. The host checks the signature against the stored device key and answers `{"accepted": {"level": "control"}}`, or `"view_only"`, so the device knows what it may do before it sends anything. A device the host does not know, or removed, gets `"unpaired"` and the connection closes with the reason `unpaired`: "Studio Mac no longer accepts this Mac. Pair again from Studio Mac."

The vector: the challenge is the SHA-256 of `wiffletree challenge`, signed with the device key above.

| | Hex |
| --- | --- |
| Challenge | `12c9ebccaa528df96d3704dac3b2c8935914e1d82f8cd44eade7662a3aa0d489` |
| Signed bytes | `776966666c65747265652d6368616c6c656e67652d763112c9ebccaa528df96d3704dac3b2c8935914e1d82f8cd44eade7662a3aa0d489bb5287baa8da8c4e9f995cdb6de312e4b9b7f2571af61f116cf0118d124843d36d23aaa2769b03ede587f79616627e40ba12f88ef0d9033a767195266ce007c2` |
| Signature | `be8f33c9928ad37795b00af817c90b95d1fe149bc26b80aadac3be63c64e05674f996ea74c63b2b9adb6b56bc4cb1a2333ec840a4708e01dd31a6f940a8d198d` |

ECDSA signatures are randomized, so a device's own signature differs; check that it verifies.

The device authenticates inside the WebSocket rather than with a TLS client certificate, which would need a Keychain identity that is awkward to build on an iPhone.

## After the challenge

An accepted connection carries the host protocol of `docs/host-protocol.md`, one message per text frame: `Hello` with no `credential`, then requests, responses and events. The desktop client subscribes to every event for the host the window shows, and only to `attention` for the others. It changes that on the live connection with a second `subscribe` where the host lists `resubscribe`.

Each reply carries `host_ms`, the time from the host reading the request to writing the reply. A client measures each request's round trip; the network's delay is the round trip less `host_ms`. For example, a 240 ms round trip with `host_ms` 12 is 12 ms on the host and 228 ms of network delay.

## What a device may do

The level is read from the store for every command, so a change at the host applies to the next one. Changing a device's level or removing it also closes its connections, so it reconnects under the new level, or is refused.

| Commands | The host itself | Control | View only |
| --- | --- | --- | --- |
| Reads, `Hello`, `subscribe`, `ping` | yes | yes | yes, except the rows below |
| `read_file`, `resolve_paths`, `list_folder` | yes | within the host's roots; `list_folder` also under its home folder | within the roots only |
| `discover_repositories` | yes | yes | refused |
| `inspect_attachments` | yes | refused; devices upload instead | refused |
| Every other workspace command, settings and `upload_attachment` included | yes | yes | refused |
| Remote access, pairing, devices and levels | yes | refused | refused |
| `stop_agents_and_quit`, `restart_host` | yes | refused | refused |

`docs/host-protocol.md` lists the roots. Every path is resolved, symlinks and `..` included, before it is checked. A refused command runs nothing and answers with one of these errors:

- "View only: Studio Mac refuses changes from this device"
- "Remote devices attach files by uploading them", for `inspect_attachments` and for a `send` with `attachments`
- "Only Studio Mac itself can change remote access"
- "Only Studio Mac itself can stop or restart Wiffletree's host"

Control devices can still stop agents' turns, as the human at the host can.

## Uploads

A device attaches a file by uploading it, since the host cannot read the device's paths:

1. Choose an `upload_id` and send the file in pieces of at most 128 KiB with `upload_attachment`: `upload_id`, the file's `name`, the piece's `offset` and its `bytes` in base64. This keeps each request under the 256 KiB bound.
2. The host stores a piece only where the upload ends, so a piece sent again changes nothing. It answers `{"upload_id": "…", "stored": 131072}`: where the next piece starts. After a dropped connection, resume from the `stored` of the last answer.
3. Name the finished upload in `send`'s `uploads`, in place of `attachments`. Sent again with the same message `id`, the message is stored once, with its uploads.

The 50 MB attachment limit applies. An upload no `send` names is deleted after an hour.

## Errors

- A command that fails or is refused answers with `error`, a sentence for a person, and changes nothing.
- A pairing step that fails answers `{"refused": {"reason": "…"}}` and closes the connection.
- The host closes a connection with the reason `unpaired` when it no longer knows the device, and closes every remote connection when remote access is turned off. Paired devices stay paired.
- Nothing the host sends carries a provider, GitHub or Git credential, a token, or the host's environment.
