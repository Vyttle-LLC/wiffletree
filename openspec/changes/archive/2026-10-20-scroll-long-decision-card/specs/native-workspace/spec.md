## ADDED Requirements

### Requirement: A long decision card scrolls inside a capped card
The decision card above the project coordinator's composer SHALL be at most half the window's height. Everything between its header row (asker and Approval or Question pill) and its button row SHALL scroll as one region: the prompt, the detail rows, the Description, a question's choices and its answer box. The header row, the button row and the composer below SHALL stay visible. A card shorter than the cap SHALL look as before, with no scroll bar. Questions, permission approvals and pull request approvals SHALL behave alike. Stepping to another decision with previous or next SHALL show it from its top. The Inbox panel and minimizing SHALL behave as before.

#### Scenario: A long pull request approval
- **WHEN** a pull request approval's Description is taller than half the window
- **THEN** the card stops at half the window's height, its middle scrolls, and Deny, Open pull request and the composer stay visible

#### Scenario: A short decision
- **WHEN** a decision fits within half the window's height
- **THEN** the card is as tall as its content and shows no scroll bar

#### Scenario: Stepping to another decision
- **WHEN** the human has scrolled a card and selects next or previous
- **THEN** the decision shown starts at its top

#### Scenario: The Inbox panel
- **WHEN** the human opens the Inbox panel
- **THEN** its cards are drawn as before and the panel scrolls as one list
