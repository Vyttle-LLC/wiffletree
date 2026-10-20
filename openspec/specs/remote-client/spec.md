# remote-client Specification

## Purpose
How the desktop app pairs with other Macs and drives them: Connect a computer, one connection per paired host, the host switcher, labels for what is remote, files and folders on a host, settings that belong to the host, and what the app shows while a host is away.

## Requirements

### Requirement: Connect a computer
The switcher's **Connect a computer…** SHALL open a sheet titled "Connect a computer" that, while finding other Macs is on, lists the Wiffletree hosts found on this network, each with its name and either "Pairing now · Wiffletree <version>" with **Pair**, or "Pairing is off there", under "Wiffletree hosts on this network. Turn on pairing on the other Mac first." It SHALL also accept a typed name or address, with an optional port, under "Not listed, or on Tailscale or a VPN? Enter its name or address." Pair SHALL ask "Pair with <host>", "Type the 4 digits <host> is showing.", saying "<host>'s key will be remembered. If it ever changes, Wiffletree will refuse to connect and ask you to pair again." A paired host SHALL stay in the switcher until the human removes it, and SHALL be reconnected by its last address and by its id on the network without the human typing anything again. Finding other Macs SHALL be this Mac's app setting "Find Wiffletree on other Macs on this network" in Settings › This Mac · the app, under OTHER MACS, off by default, including for a preferences file saved before it existed. While it is off, the app SHALL NOT browse the network, so macOS does not ask about the local network. The sheet SHALL then say "Finding Wiffletree on this network is off. Turn it on to list the Macs nearby, or type a name or address.", where **Turn it on** turns on the same setting. Typing a name or address SHALL work either way, and paired hosts SHALL still be reconnected by their stored addresses. Turning the setting on SHALL start browsing at once; turning it off SHALL stop browsing and forget what it found.

#### Scenario: A host on the same network
- **WHEN** finding other Macs is on and Studio Mac has pairing open on the same network
- **THEN** the sheet lists "Studio Mac · Pairing now · Wiffletree 0.4" with Pair, without anything typed

#### Scenario: Finding other Macs is off
- **WHEN** the human opens Connect a computer while finding other Macs is off, as it is after installing or updating
- **THEN** no host is listed and the app has not browsed the network, the sheet says "Finding Wiffletree on this network is off. Turn it on to list the Macs nearby, or type a name or address.", and a typed name or address still pairs

#### Scenario: Turning finding on from the sheet
- **WHEN** the human chooses Turn it on in Connect a computer
- **THEN** the setting in Settings is on too, browsing starts at once, and the hosts it finds are listed

#### Scenario: A host over Tailscale
- **WHEN** the human types `studio.tail4c2e.ts.net` and the 4 digits Studio Mac shows
- **THEN** Studio Mac is paired and appears in the switcher

#### Scenario: A paired host changes address
- **WHEN** a paired host on the same network gets a new address
- **THEN** the client finds it again by its id and reconnects

### Requirement: The host switcher
The sidebar's "Local workspace" line SHALL open a host switcher listing **This Mac** ("Local workspace · N projects") and each paired host with its connection state ("Connected · you can control it", "Connected · view only", "Reconnecting…", or "Offline · last seen <time>"), a check mark on the selected one, and the number of things that need the human on each host, followed by **Connect a computer…** and **Remote access settings…**. While a remote host is selected, the line SHALL read "<host> · connected" or its current state. The sidebar SHALL list only the selected host's projects under the heading "Projects · N · on <host>". Switching hosts SHALL NOT stop, pause or change anything on any host.

#### Scenario: A question waiting elsewhere
- **WHEN** This Mac is selected and Studio Mac's coordinator asks the human a question
- **THEN** Studio Mac's row in the switcher shows 1 without Studio Mac being selected

#### Scenario: Switching away
- **WHEN** the human switches from Studio Mac to This Mac while Studio Mac's agents work
- **THEN** they keep working, and This Mac's projects are listed

### Requirement: Remote things say where they are
While a remote host is selected, the breadcrumb SHALL start with the host's name, the composer's model line SHALL end "on <host>", and every path SHALL be labelled "<host>: <path>". Opening such a path SHALL read the file through that host and open a read-only copy on this Mac, never a file of the same path on this Mac. Reveal in Finder SHALL be offered only for This Mac's paths. Choosing a folder on a remote host SHALL browse that host's folders, not this Mac's. On This Mac, nothing SHALL gain a host label.

#### Scenario: A worktree path on Studio Mac
- **WHEN** a reply on Studio Mac names `~/dev/tidepool` and this Mac also has a `~/dev/tidepool`
- **THEN** the path reads "Studio Mac: ~/dev/tidepool", and opening a file under it shows Studio Mac's copy

#### Scenario: Adding a repository on a remote host
- **WHEN** the human adds a repository while Studio Mac is selected
- **THEN** the folder browser lists Studio Mac's folders, starting at its home folder

### Requirement: Settings belong to the Mac that runs them
While a remote host is selected, the Workspace section's Repositories, Models, Skills and Usage and limits SHALL show and save that host's settings, accounts and usage, and the Models page SHALL say "Saving changes <host>'s agents only. Your <this device> keeps its own table for its own projects." A View only device SHALL see them with saving turned off and the reason. The Settings window SHALL show "This Mac · the app" with appearance and theme, the computers paired with, and window layout, and "<host> · the host you're driving" with the host's settings, ending "Remote access (pairing and who can control it) changes only at <host> itself." These client settings SHALL stay on the device: appearance, the list of paired hosts and window layout. The shared and per-Mac split of the settings repository SHALL be unchanged.

#### Scenario: Personal MacBook driving the work Mac
- **WHEN** a MacBook with Control opens Models while Work Studio is selected and saves a change
- **THEN** Work Studio's table changes, and the MacBook's own table for its own projects does not

#### Scenario: View only opens Models
- **WHEN** a View only device opens Models on a host
- **THEN** it shows the host's table and cannot save it

#### Scenario: Appearance stays here
- **WHEN** the human switches to Dark while Work Studio is selected
- **THEN** only this device's appearance changes, and Work Studio's own windows are unchanged

### Requirement: A dropped connection freezes the view without losing work
When a selected remote host stops answering, the client SHALL keep showing the last state it received, under "Reconnecting to <host>…" and "Showing what it sent <age> ago. Its agents keep working.", with the age counting up and **Try now**. The composer SHALL keep working, saying "You can write; it sends when <host> is back.", and drafts SHALL be kept. A message sent while disconnected SHALL get its id when it is written and SHALL be sent with that id when the host answers again, and SHALL be kept only while the app runs. A message whose reply was lost SHALL be sent again with its id. The host's existing deduplication SHALL store each such message once. Other actions SHALL be disabled until the host answers.

#### Scenario: Writing during a drop
- **WHEN** the connection drops and the human writes and sends a reply to the coordinator
- **THEN** the reply waits in the composer area and is delivered once when the host is back

#### Scenario: The app quits during a drop
- **WHEN** a message waits to be sent and the human quits the app before the host is back
- **THEN** the message is not sent after the app opens again

#### Scenario: A send whose answer was lost
- **WHEN** the connection drops after a message reached the host and before its reply arrived
- **THEN** after reconnecting the conversation shows the message once

#### Scenario: Try now
- **WHEN** the human presses Try now during a 30-second wait
- **THEN** the client tries to connect at once

### Requirement: Reconnecting backs off and resumes
The client SHALL retry a lost host connection after 1 second, doubling the wait up to 30 seconds with some jitter, and SHALL try at once when this device's network changes or the human presses Try now. On reconnecting, it SHALL resume its event subscription from its cursor or take a fresh snapshot.

#### Scenario: A host restarted while away
- **WHEN** the client reconnects to a host that restarted during the drop
- **THEN** it takes a fresh snapshot, and no message, usage entry or operation is repeated

### Requirement: Latency is reported apart from network delay
Each host reply SHALL carry the time the host spent on it. The client SHALL record, per request kind, the round trip and the network delay, which is the round trip less the host's time, SHALL measure the network delay while idle, and SHALL show the latest network delay for a connected host in its switcher row's tooltip.

#### Scenario: A slow network, a fast host
- **WHEN** a snapshot takes 240 ms end to end and the host reports 12 ms
- **THEN** the record shows 12 ms on the host and 228 ms of network delay
