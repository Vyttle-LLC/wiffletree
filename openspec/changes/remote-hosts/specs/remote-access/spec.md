## ADDED Requirements

### Requirement: The host listens only while remote access is on
Settings › Remote access SHALL have the switch "Let other devices use this Mac's Wiffletree", off by default, saved in this Mac's host settings and never synced through a settings repository. While it is off, the host SHALL NOT listen on any network port or advertise itself. While it is on, the host SHALL accept TLS 1.3 connections on one TCP port and advertise `_wiffletree._tcp` with its short id, name, version and whether pairing mode is open. Turning it off SHALL close the listener and every remote connection at once and stop advertising, and SHALL keep the paired devices.

#### Scenario: A fresh install
- **WHEN** Wiffletree is installed and opened for the first time
- **THEN** the host listens on no TCP port and nothing is advertised on the network

#### Scenario: Turning it off while a device is connected
- **WHEN** the human turns the switch off while a paired MacBook is connected
- **THEN** the MacBook's connection closes at once, it cannot reconnect, and it is still listed under Paired devices

### Requirement: Pairing uses a one-time code at the host
Only the host itself SHALL open pairing mode, from Settings › Remote access › Pair a device. Pairing mode SHALL show a random 4-digit code, "Type this code on the other Mac or iPhone. It works once, for 2 minutes.", the time left and Cancel. The code SHALL be checked only through a SPAKE2 exchange whose key confirmation covers both sides' long-term public keys, the host's key as the device saw it in TLS, and both names, and SHALL never be sent or compared in any other form. A successful pairing SHALL store the device with the access level Control and close pairing mode. Each failed confirmation SHALL count as a wrong try, and a fixed small number of wrong tries SHALL close pairing mode. Pairing mode SHALL also close when its 2 minutes end or the human cancels it. One exchange SHALL run at a time.

#### Scenario: Pairing a MacBook
- **WHEN** the human opens pairing on Studio Mac and types its code on a MacBook within 2 minutes
- **THEN** Studio Mac lists the MacBook under Paired devices with Control, the MacBook lists Studio Mac in its switcher, and pairing mode is closed

#### Scenario: The code is used once
- **WHEN** a second device types the same code after the first paired
- **THEN** pairing fails and nothing is stored

#### Scenario: Wrong guesses
- **WHEN** a device keeps sending wrong codes until the limit of wrong tries
- **THEN** pairing mode closes, the code no longer works even if typed correctly, and no device is stored

#### Scenario: A machine in the middle
- **WHEN** another machine relays the pairing while presenting its own certificate to the device
- **THEN** the key confirmation fails on both sides and nothing is stored

#### Scenario: Expired
- **WHEN** the code is typed 2 minutes and 1 second after pairing mode opened
- **THEN** pairing fails because pairing mode is closed

### Requirement: Every connection is pinned to the paired keys
After pairing, a device SHALL accept a connection to a host only if the host's TLS certificate carries the public key stored at pairing, and SHALL send nothing otherwise. The host SHALL accept a device only after it signs a fresh challenge with the key stored at pairing. A host whose key changed SHALL be refused until the device pairs again, and the client SHALL say so; the client SHALL NOT replace a stored key on its own. Private keys SHALL stay in the Keychain of the device that made them.

#### Scenario: The host's key changed
- **WHEN** Studio Mac's key is replaced and a paired MacBook connects
- **THEN** the MacBook refuses the connection before sending any request and shows "Studio Mac's key has changed. Wiffletree won't connect until you pair again."

#### Scenario: An unknown device
- **WHEN** a device that never paired connects to the host's listener with a valid TLS client
- **THEN** the host closes the connection with `unpaired` before the handshake

### Requirement: Access levels are enforced per command
A paired device SHALL have the access level Control or View only. The host SHALL check the level of the connection's device for every command. View only SHALL allow only reads, the handshake and the event subscription, and SHALL refuse every other command, running nothing. Control SHALL allow every workspace command, including saving the host's settings and uploading attachments. Commands that change remote access, pairing, devices or access levels, and commands that stop or restart the host process, SHALL be refused from every remote device. A connection on the host's own socket SHALL be the host itself and SHALL be allowed everything.

#### Scenario: View only sends a message
- **WHEN** a View only device sends a message to a coordinator
- **THEN** the host refuses it, stores nothing and wakes no one

#### Scenario: View only reads
- **WHEN** a View only device asks for the snapshot, a conversation page and the Models settings
- **THEN** the host answers all three

#### Scenario: Control saves the host's models
- **WHEN** a Control device saves a change to the host's Who runs what table
- **THEN** the host saves it and its agents use it from their next assignment

#### Scenario: A remote device tries to grant access
- **WHEN** a Control device asks the host to open pairing mode or change a device's level
- **THEN** the host refuses with "Only Studio Mac itself can change remote access" and changes nothing

### Requirement: Paired devices are managed at the host
Settings › Remote access SHALL list the paired devices with their name, what each can do (Control or View only), when each last connected, and Remove, and SHALL explain "Control means the device can start and message agents here. Agents run with permission prompts off, so that is the same as running commands on this Mac. Remove takes effect at once, even if the device is offline." Changing a device's level or removing it SHALL close that device's open connections at once. A removed device SHALL be refused at its next connection, whenever that is, and SHALL need to pair again.

#### Scenario: Removing a connected device
- **WHEN** the human removes Michael's MacBook Air while it is connected
- **THEN** its connection closes at once and its next attempt is refused with `unpaired`

#### Scenario: Removing an offline device
- **WHEN** the human removes Michael's iPhone while it is offline, and the iPhone connects a day later
- **THEN** the host refuses it and the iPhone is told to pair again

#### Scenario: Lowering a device to View only
- **WHEN** the human changes a connected device from Control to View only
- **THEN** its next send is refused

### Requirement: Credentials never leave the host
No command reply or event SHALL carry a provider, GitHub or Git credential, a token, or the host's environment. Provider runs, Git operations, publishing and the PR keeper SHALL run on the host for a remote device as they do for the host itself.

#### Scenario: A scripted remote session
- **WHEN** a Control device runs a scripted session of snapshots, settings, usage, sends and a publish, on a host whose environment and credential files hold planted secret values
- **THEN** none of those values appears in any reply or event the device received

### Requirement: Attachments are uploaded to the host
A Control device SHALL send an attached file to the host in bounded pieces before the message that carries it, and the message SHALL name the upload in place of a path. The host SHALL keep an unused upload for at most one hour, and SHALL apply the existing 50 MB attachment limit.

#### Scenario: A screenshot from a MacBook
- **WHEN** a MacBook attaches a 3 MB screenshot to a message for a remote coordinator
- **THEN** the host stores it with the message as it would a local file, and the coordinator's turn receives it

#### Scenario: An upload never sent
- **WHEN** an upload finishes and no message names it for an hour
- **THEN** the host deletes it

### Requirement: The protocol is documented for other clients
The repository SHALL document the remote protocol in `docs/remote-protocol.md`: TLS and pinning, the pairing exchange with test vectors, the challenge, the WebSocket framing with one JSON message per text frame, the handshake, every command and event, the access levels, uploads and errors. Following it SHALL NOT require Rust code.

#### Scenario: The pairing test vectors
- **WHEN** the documented pairing test vectors are run against the host's pairing code
- **THEN** they produce the documented confirmation values
