## ADDED Requirements

### Requirement: Compact action on the Context card
The Context card SHALL show, under its measurement, one row with a short hint and a Compact button, for every agent. For a Claude agent with a provider session the hint SHALL read "Replace the conversation with a summary." and the button "Compact…", or "Compact after this turn" while the agent is working. Activating it SHALL open a form in the card, not a dialog, with one optional field labelled "What to keep", empty by default; a line saying that Claude replaces the agent's conversation with a summary, that this can't be undone and that it uses one model call; Cancel; and a confirm button reading "Compact now", or "Compact after this turn" while the agent is working, when the line SHALL also say that it runs when the turn ends, before waiting messages are delivered. Nothing SHALL be sent to the host before the confirm button. For a Codex agent the button SHALL be disabled beside "Not available for Codex agents: the Codex CLI has no compact command." For an agent with no provider session it SHALL be disabled beside "Available after this agent's first turn." A request the host refuses SHALL show the host's reason in the form and keep what was typed.

#### Scenario: Confirm on an idle agent
- **WHEN** the human presses "Compact…" on an idle Claude agent's card, types "the failing test names" and presses "Compact now"
- **THEN** the host receives one request with those instructions, and the card shows the compaction running

#### Scenario: Cancel the form
- **WHEN** the human opens the form and presses Cancel
- **THEN** the form closes and the host has received nothing

#### Scenario: Working agent
- **WHEN** the human opens the form on an agent whose turn is running
- **THEN** the confirm button reads "Compact after this turn" and the line says it runs when the turn ends, before waiting messages are delivered

#### Scenario: Codex agent
- **WHEN** the human opens a Codex agent's Context card
- **THEN** the Compact button is disabled and the row says the Codex CLI has no compact command

#### Scenario: New agent
- **WHEN** the human opens the Context card of an agent that has not run a turn
- **THEN** the card says its measurement appears after the next turn, and the Compact button is disabled beside "Available after this agent's first turn."

### Requirement: The Context card follows a compaction
While an agent has a queued compaction, its Context card SHALL replace the Compact row with "Compacts when this turn ends", or "Compacts when this agent runs again" when the agent is not in a turn, the instructions after "Keeping:" when some were given, and a Cancel button that withdraws the request. While the compaction runs, the card SHALL show "Compacting…", an indeterminate bar and "Messages you send now are delivered when it finishes.", with no Cancel, and the card's refresh button and the conversation header's Pause button SHALL be disabled. After every outcome the card SHALL be measured again once, also when the agent's next turn has already started, and SHALL NOT be measured while a compaction runs. That measurement SHALL be the only one made during a turn; the refresh button SHALL stay disabled while the agent is working. An outcome of nothing to compact SHALL show a notice in the subtle color titled "Nothing to compact" with the provider's reason and "The context is unchanged." A failed outcome SHALL show a notice in the failure color titled "Compaction failed" with the reason and "The context is unchanged." An interrupted outcome SHALL show a notice in the failure color titled "Compaction interrupted" saying that Wiffletree stopped while Claude was compacting and that the figure was measured again. Each notice SHALL have a Dismiss button, SHALL survive a restart until dismissed or until the next request, and SHALL leave the Compact row available. A compacted outcome SHALL show no notice.

#### Scenario: Queued behind a turn
- **WHEN** the human confirms "Compact after this turn" with a note
- **THEN** the card shows "Compacts when this turn ends", the note after "Keeping:", and Cancel

#### Scenario: Cancel a queued compaction
- **WHEN** the human presses Cancel on a queued compaction
- **THEN** the card shows the Compact row again, and no compaction runs when the turn ends

#### Scenario: Running
- **WHEN** a compaction is running
- **THEN** the card shows "Compacting…" with no Cancel, and refresh and Pause are disabled

#### Scenario: Measured again
- **WHEN** a compaction from 812k succeeds
- **THEN** the card measures once more and shows the new figure and percentage without the human pressing refresh

#### Scenario: Measured again with a message waiting
- **WHEN** a compaction queued behind a turn succeeds while a message waits, so the agent's next turn starts at once
- **THEN** the card and the chip show the new figure during that turn, and the badge reads "Working"

#### Scenario: Nothing to compact
- **WHEN** Claude reports "Not enough messages to compact."
- **THEN** the card shows "Nothing to compact" with that text and "The context is unchanged.", and the Compact button is still available

#### Scenario: Failed
- **WHEN** a compaction fails
- **THEN** the card shows "Compaction failed" in the failure color with the reason, until the human dismisses it or compacts again

### Requirement: The header shows a queued or running compaction
The context chip in the conversation header SHALL add "compact queued" after its percentage while the agent has a queued compaction and "compacting" while one runs, and SHALL show neither otherwise. While a compaction runs, the agent's status badge SHALL read "Compacting" with the working glyph and color; before and after, it SHALL show the agent's own status. The sidebar SHALL stay unchanged. The Usage page SHALL gain no control, figure or label for compactions.

#### Scenario: Queued
- **WHEN** an agent at 81% has a compaction queued behind its running turn
- **THEN** its chip reads "81%" and "compact queued", and its badge reads "Working"

#### Scenario: Running
- **WHEN** that compaction is running
- **THEN** the chip reads "81%" and "compacting", and the badge reads "Compacting"

#### Scenario: Finished
- **WHEN** it has succeeded and the card was measured again at 4%
- **THEN** the chip reads "4%" with no note, and the badge shows the agent's status

#### Scenario: Usage page
- **WHEN** the human opens the Usage page after a compaction
- **THEN** its totals include the compaction's tokens and nothing on the page names compactions
