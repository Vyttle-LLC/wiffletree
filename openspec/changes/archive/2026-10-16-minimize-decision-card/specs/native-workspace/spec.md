## ADDED Requirements

### Requirement: The decision card can be minimized
The decision card above the project coordinator's composer SHALL have a Minimize button, tooltip "Minimize", at the right end of the row above the card, after Open inbox. Minimized, the card SHALL become one row with the card's yellow edge, a small yellow dot, the current decision's prompt on one line with an ellipsis, "N of M" only when there are several open decisions, and an Expand button, tooltip "Expand". Clicking anywhere on the row SHALL restore the full card on the same decision. Questions and approvals SHALL minimize alike. The minimized state SHALL be kept for each project, in memory only: it SHALL survive reading, typing, sending and switching to another conversation and back, and an app restart SHALL start expanded. A decision arriving SHALL NOT expand it; the row's count SHALL update. When a project has no open decision, its minimized state SHALL be cleared, so the next decision starts expanded. Answering, choices, Dismiss, Open inbox and the Inbox panel SHALL behave as before.

#### Scenario: Minimizing
- **WHEN** the human selects Minimize on a card showing "1 of 2"
- **THEN** one row shows the prompt on a single line with an ellipsis, "1 of 2" and Expand, and the conversation above it has the card's space

#### Scenario: Restoring
- **WHEN** the human clicks the minimized row or its Expand button
- **THEN** the full card returns showing the same decision

#### Scenario: Staying minimized
- **WHEN** the human types, sends a message, or selects another conversation and returns
- **THEN** the card is still minimized

#### Scenario: A new decision arrives
- **WHEN** the card is minimized and the coordinator asks another question
- **THEN** the card stays minimized and the row shows the new count

#### Scenario: The last decision is settled
- **WHEN** the human settles the last open decision while the card is minimized
- **THEN** the row disappears
- **AND** the next decision that arrives shows the full card

#### Scenario: Restart
- **WHEN** the app restarts while a decision is open
- **THEN** the card is expanded
