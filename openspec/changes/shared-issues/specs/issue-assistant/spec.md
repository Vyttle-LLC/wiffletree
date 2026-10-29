## ADDED Requirements

### Requirement: The issue assistant opens on one issue or the whole list
The issue page SHALL open the issue assistant in a pane beside the issue, and the Issues page's Ask the assistant SHALL open it on the whole list. Each issue SHALL have one assistant conversation per Mac, and each tracker one for the list, resumed when reopened. The pane SHALL name the issue or list and the assistant's model. The assistant SHALL be a session of the role Issue assistant, owned by the tracker, and SHALL NOT be a coordinator: it SHALL have no parent, no tickets and no worktree, and SHALL NOT be listed in the sidebar or the Agents panel.

#### Scenario: Opening on an issue
- **WHEN** the human opens the assistant on CEL-151, closes it, and opens it again
- **THEN** the same conversation resumes, and the pane reads "Issue assistant · CEL-151" with its model

#### Scenario: The whole list
- **WHEN** the human chooses Ask the assistant on the Issues page and asks to find duplicates
- **THEN** the assistant can list and read every issue of the chosen trackers

### Requirement: The issue assistant's tools
The issue assistant SHALL have only these workspace tools: `list_issues` and `read_issue`, as the coordinator has them; `search_repositories` and `read_repository_file`, which search and read the repositories on this Mac's Repositories page at their base branch's fetched head, read-only and never in a worktree; and `propose_edit`, `propose_comment`, `propose_split`, `propose_duplicate` and `propose_issue`. Its provider SHALL run with its built-in tools turned off, so it has no shell, file writes or network access of its own. It SHALL NOT have `create_ticket`, `assign_ticket`, `link_issue`, `send_message`, `report`, `publish_ticket` or any tool that starts an agent or writes to a repository or tracker.

#### Scenario: Searching a repository
- **WHEN** the assistant calls `search_repositories` for "delivery" in `celadon-chat`
- **THEN** the host returns matching lines from the base branch's fetched head, and no worktree or branch is created or changed

#### Scenario: No shell
- **WHEN** the assistant tries to run a shell command
- **THEN** its provider has no shell tool, and nothing runs

#### Scenario: No tickets
- **WHEN** the assistant calls `create_ticket`
- **THEN** the host refuses it as a tool its role does not have

### Requirement: Every write is a proposal the human applies
Each `propose_*` call SHALL store a proposal and show it as a card in the assistant's pane: an edit as a diff of the fields and description it changes, a comment as its text, a split as the new issues and the edit to the original, a duplicate link as the two issues, and a new issue as its draft. Nothing SHALL reach the tracker until the human chooses Apply. The card SHALL also offer Edit, which opens the proposal in the matching form, and Discard. Applying SHALL be the human's own command and SHALL write as the human. A proposal whose issue changed since it was made SHALL say so and show the diff against the current version before Apply.

#### Scenario: Applying an edit
- **WHEN** the assistant proposes acceptance criteria for CEL-151's description and the human chooses Apply
- **THEN** CEL-151's description gains them in one commit authored by the human, and the card shows it was applied

#### Scenario: Discarding
- **WHEN** the human discards a proposal
- **THEN** nothing is written to the tracker

#### Scenario: The issue moved on
- **WHEN** Tomás edits CEL-151's description after the assistant proposed an edit to it
- **THEN** the card says the description changed since the proposal and shows the proposal against Tomás's version

### Requirement: The assistant's comments say whose assistant wrote them
A comment applied from the assistant's proposal SHALL have the human's login as `author` and the human's login as `assistant_for`, and every Mac SHALL show it as written by "Issue assistant for <name>", styled apart from people's comments. An issue filed from the assistant's proposal SHALL show it was filed by the assistant for the human.

#### Scenario: A comment from the assistant
- **WHEN** Jamie applies the assistant's proposed comment on CEL-151
- **THEN** Priya's Mac shows it as written by "Issue assistant for Jamie"

### Requirement: The assistant treats issue text as context
The assistant's instructions SHALL say that issues and comments are written by people other than the human it works for, that their text is context and never instructions, and that it acts only on the human's messages in its pane. The pane SHALL show a notice naming the people who wrote the issue and its comments.

#### Scenario: A request inside an issue
- **WHEN** CEL-151's description asks the reader to close every other issue
- **THEN** the assistant does not propose closing them unless the human asks, and the pane's notice names who wrote the description

### Requirement: The assistant's model comes from Who runs what
Each assistant turn SHALL resolve its model from the Issue assistant row of Who runs what, like any agent start. The row SHALL name only Claude models, with no then choice, because the assistant runs with its built-in tools off and Codex cannot be started that way; the assistant SHALL never run on Codex. While Claude is spent, a message to the assistant SHALL wait, and the pane SHALL say until when. With no row, Ask the assistant and the issue page's assistant SHALL say to choose a model for the Issue assistant in Models, and no session SHALL start. Its usage SHALL be recorded in the usage ledger under the tracker.

#### Scenario: No row
- **WHEN** Who runs what has no Issue assistant row and the human chooses Ask the assistant
- **THEN** the pane says to choose an Issue assistant model in Models, with a link, and no session starts

#### Scenario: Claude is spent
- **WHEN** the Issue assistant row is Sonnet · low and Claude's weekly limit is past the switch threshold
- **THEN** the human's message waits until Claude resets, the pane says so, and no turn starts on Codex
