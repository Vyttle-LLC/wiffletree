## Purpose

Decides, before each message to the assistant, which thread and which Space it belongs to, through fixed typed questions answered with confidence, and asks the user when the answer is not clear.

## ADDED Requirements

### Requirement: The router answers typed questions
Before each message the user sends to the assistant, the host SHALL ask the router two fixed questions: which thread (continue the current thread, switch to a named open thread, or open a new one) and which Space. Each answer SHALL be a pick with a confidence from 0 to 1 and at most two alternatives with their confidences. A new-thread answer SHALL carry a title of at most 60 characters. The router SHALL be skipped when the user chose the thread or Space for that message by hand, and the Space question SHALL be answered by the host when only one Space exists. When no thread is current, such as the first message or a message sent from an empty tab, the thread question SHALL offer only switching to a named open thread or opening a new one, never continuing, and the router's input SHALL have no current thread.

#### Scenario: The first message
- **WHEN** the user sends their first message to the assistant, with no thread open and Spaces Sage and Personal
- **THEN** the router is asked with no current thread and no open threads, so its only thread answer is a new thread; at or above the threshold a new tab opens in the Space it answered, and below it the user is asked "Is this Sage or Personal?"

#### Scenario: A clear continuation
- **WHEN** the user, in a Sage thread about release notes, writes "and the watch app?"
- **THEN** the router answers continue with high confidence and Sage, and the message goes to the current thread

#### Scenario: Chosen by hand
- **WHEN** the user picks the Personal Space in the composer before sending
- **THEN** no router run starts and the message goes where the user chose

### Requirement: The router sees only what it needs
The router's input SHALL be the new message, when a thread is current the current thread's title, Space and its last six messages up to 1,500 characters, the open threads' ids, titles and Spaces, and each Space's id, name and description. It SHALL NOT receive brains, outputs, scratch files, other threads' text or Space instructions.

#### Scenario: Another thread's text
- **WHEN** the router runs while a Vyttle thread holds a long conversation
- **THEN** the router's input names that thread's title and Space and none of its messages

### Requirement: Host rules act on the answers
The host SHALL act on the router's answers by fixed rules. An answer that fails, times out after 10 seconds, does not match the answer shape, names an unknown thread or Space, or answers continue when no thread is current SHALL count as unsure. An answer below the confidence threshold on either question, or a thread whose Space differs from the Space answer, SHALL count as unsure. Otherwise the host SHALL continue the current thread, switch to the named thread and send the message there, or open a new tab in the answered Space. The threshold SHALL default to 0.8, SHALL be a setting from 0.5 to 1.0, and SHALL be shared (see settings-repository). A routed message that left the current thread SHALL show a line naming where it went, with Change.

#### Scenario: A new topic
- **WHEN** the router answers new thread at 0.93 and Personal at 0.9 with the threshold at 0.8
- **THEN** a new tab opens in Personal titled with the router's title, the message is sent there, and the line reads "New thread in Personal · Change"

#### Scenario: Below the threshold
- **WHEN** the router answers Sage at 0.62 with Personal at 0.31 as an alternative
- **THEN** the host asks the user instead of sending the message

#### Scenario: The router fails
- **WHEN** the Router row's every choice is spent
- **THEN** the message is not sent, and the user is asked which thread and Space it belongs to

### Requirement: Unsure asks with one-click choices
When the answer is unsure, the host SHALL hold the message unsent and show an inline question in the assistant, such as "Is this Sage or Personal?", with one button per pick and alternative, a button to continue the current thread when it is not among them, and "Somewhere else…" listing every Space and open thread. One click SHALL send the message where the user chose. The held message SHALL survive a restart.

#### Scenario: The user answers
- **WHEN** the question offers Sage, Personal and Somewhere else…, and the user clicks Personal
- **THEN** the message is sent to a new Personal thread, or the Personal thread the router named, and no brain entry was written before the click

### Requirement: Changing a routed message
Change on a routed message SHALL offer the same choices as an unsure question. Choosing one SHALL stop the turn the message started if it still runs, mark the message and its reply in that thread as moved, and send the message to the chosen thread. Log entries that turn already added SHALL stay, and the moved line SHALL name how many and in which brain.

#### Scenario: Moved after a log entry
- **WHEN** a message routed to Sage added one log entry and the user changes it to Personal
- **THEN** the message is sent to Personal, and the Sage thread shows it as moved with "1 entry stays in the Sage brain"

### Requirement: The router never writes
A router run SHALL be started with every tool off, no MCP server, no workspace tools, no stored session and an empty working directory, and SHALL return only an answer of the router's shape. The host SHALL discard an answer from a run that reports any other tool or MCP server, and count it as unsure. The router SHALL NOT write to a brain, a thread, a file or a setting.

#### Scenario: A run that reports a tool
- **WHEN** a router run reports a `Bash` tool at its start
- **THEN** its answer is discarded and the user is asked

### Requirement: Router providers behind one interface
The router SHALL be reached through one interface that takes the router's input and returns its answer. Version 1 SHALL provide it on the user's installed Claude and Codex CLIs with structured output, on the model the Router row picks, with no API key. Another provider, such as Jev through OpenRouter or Vercel, MAY be added later behind the same interface without changing the host rules.

#### Scenario: Router on Codex
- **WHEN** the Router row's first choice is a Codex model
- **THEN** the router runs through Codex with an output schema, and the host rules are the same as on Claude
