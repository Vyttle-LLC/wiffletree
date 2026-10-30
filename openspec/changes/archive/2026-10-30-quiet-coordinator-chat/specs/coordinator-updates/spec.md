## ADDED Requirements

### Requirement: Pinned live ticket table
The project coordinator's conversation SHALL pin a table above its transcript, filled by Wiffletree from its records and never written by the model. It SHALL list the coordinator's tickets that are in progress, meaning open and not planned, or with an open PR, each with its title, its status in the Tickets inspector's wording with the verification round while a cycle runs, and its PR's number and state linked to the PR, or "Not published". Accepted and closed tickets SHALL collapse to one "N done" line that opens to list them. Backlog items such as shared issues SHALL NOT appear. The table SHALL collapse to its title, and the choice SHALL be a client preference that survives relaunch. It SHALL redraw from the snapshot the client already reloads on ticket, cycle and PR changes, and SHALL NOT poll or reload the snapshot itself. Worker conversations SHALL NOT show it.

#### Scenario: In progress and done
- **WHEN** a coordinator owns a ticket implementing, one verifying in round 2, one planned, one accepted and one closed
- **THEN** the table lists the implementing and verifying tickets, the latter reading "Verifying · round 2", and a "2 done" line, and not the planned one

#### Scenario: A planned ticket with an open PR
- **WHEN** a planned ticket has an open PR #12
- **THEN** it is listed with "#12 open"

#### Scenario: Live without a turn
- **WHEN** a ticket moves from implementing to ready for testing while the coordinator is idle
- **THEN** the table shows the new status after the next snapshot, with no coordinator turn

#### Scenario: Collapsed survives relaunch
- **WHEN** the human collapses the table and relaunches the app
- **THEN** the table is still collapsed

### Requirement: The host compares a coordinator's background reply with its last full one
When a project coordinator's turn ends with a reply and the human did not start it, the host SHALL compare the project's recorded facts with those taken at the coordinator's last reply shown in full, and SHALL store the result on the reply message. A turn is human-started when its input holds a message from the human, including an Inbox answer, or no message at all. The facts SHALL be each owned ticket's state, each open ticket's branch head, the latest verification cycle's number, outcome, rounds and verifier results, the ledger's entries and statuses, the PR's number, state, checks and unresolved threads, the project's open questions other than the host's check-in notes, and which of the project's agents exist and are stopped or hold input. A changed result SHALL list each difference in words. The result SHALL be unchanged only when nothing differs and the turn's input held no failed, blocked or error message and the coordinator did not mark the reply as news. Every reply shown in full SHALL become the new baseline; an unchanged reply SHALL NOT. The stored reply text SHALL NOT change, and the result and baseline SHALL survive a host restart.

#### Scenario: Nothing changed
- **WHEN** a check-in starts a coordinator turn and no recorded fact differs from its last full reply
- **THEN** the reply is stored unchanged

#### Scenario: Each fact kind
- **WHEN** between two background replies a ticket's state, its branch head, its cycle, its ledger, its PR, an open question or an agent's stopped or held state changes
- **THEN** the later reply is stored changed with a line naming that difference

#### Scenario: Human-started
- **WHEN** the human's message starts a coordinator turn
- **THEN** its reply has no result and becomes the baseline

#### Scenario: Failure input
- **WHEN** a turn's input holds a failed report and nothing else changed
- **THEN** the reply has no result and is shown in full

#### Scenario: News
- **WHEN** the coordinator calls `set_plan` with `news: true` during an unchanged turn
- **THEN** the reply has no result and is shown in full

#### Scenario: Restart
- **WHEN** the host restarts after storing an unchanged reply
- **THEN** the reply still reads unchanged, and the next background turn compares with the same baseline

### Requirement: Unchanged replies fold beside agent messages
The conversation SHALL fold a coordinator reply stored unchanged into the run of peer messages around it. The run's heading SHALL count such replies as "N replies, no change" and SHALL NOT name the coordinator among the senders. Opened, the run SHALL say Wiffletree found no change since the last update, and each such reply's line SHALL show a "No change" pill and its first line and open to the full reply. A reply stored changed SHALL stay full and open with a muted "Since last update:" line listing the changes. Replies without a result SHALL draw as before. Folding SHALL be display only.

#### Scenario: Check-ins with no change
- **WHEN** a check-in, an unchanged reply, a check-in and an unchanged reply arrive in a row
- **THEN** they show as one row reading "2 agent messages, 2 replies, no change" with the implementer's name, and opening a reply's line shows its full text

#### Scenario: A changed reply
- **WHEN** a reply is stored changed with "Stage 1 committed 684cd0f"
- **THEN** it shows in full under "Since last update: Stage 1 committed 684cd0f"

### Requirement: Coordinator instructions keep routine updates short
The Coordinating skill SHALL tell the coordinator that a routine update, such as letting a check-in continue or acknowledging a report, says what is new since its last update in one or two sentences and repeats nothing already said, that the pinned table already shows live status, and that the per-ticket table and review accounting are for a ticket finished, accepted, published or ready for the human, or when the human asks for status. The coordinator contract SHALL say that Wiffletree folds a background reply its records show unchanged and that `set_plan` with `news: true` keeps a reply full.

#### Scenario: Bundled instructions
- **WHEN** the host builds the project coordinator's instructions
- **THEN** they describe routine updates in one or two sentences, the pinned table, when the per-ticket table applies, and the `news` mark
