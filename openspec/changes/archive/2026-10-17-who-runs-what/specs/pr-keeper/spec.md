## MODIFIED Requirements

### Requirement: The comments trigger needs a Claude model
The keeper's settings SHALL refuse to turn the comments trigger on unless the table's PR keeper row (see model-routing) has a Claude choice in its main line, saying that sorting runs with every tool turned off and that Codex cannot be started that way. A table that removes that row, or leaves it without a Claude choice, while the trigger is on SHALL be refused in the same way.

#### Scenario: No model
- **WHEN** the human ticks "Sort new comments" with no PR keeper row
- **THEN** the save is refused, saying to choose a Claude model in Models first

#### Scenario: A Codex model
- **WHEN** the PR keeper row names only Codex models and the human ticks "Sort new comments"
- **THEN** the save is refused, saying sorting needs a Claude model

### Requirement: The PR keeper is an agent role no coordinator controls
Each model run the keeper starts SHALL be a turn of an agent of the ticket with the role "PR keeper", whose parent is the ticket's coordinator and whose model is what the PR keeper row resolves to for that run, or the row's Claude choice for a sorting run; with no PR keeper row, no PR keeper agent SHALL be created. A ticket's try session and its sorting session SHALL be distinct agents, so neither is ever given the other's conversation. A PR keeper agent SHALL NOT hold the ticket's worktree: no agent of the ticket SHALL wait to start a turn because a PR keeper agent is in one. No coordinator tool SHALL create, message, resume or assign a PR keeper agent, and the human's new-agent dialog SHALL NOT offer the role. The PR keeper's instructions SHALL be its own, bundled with the app.

#### Scenario: A coordinator cannot make one
- **WHEN** a coordinator calls `assign_ticket` naming the PR keeper role
- **THEN** the call is refused and no agent is created

#### Scenario: A coordinator cannot message one
- **WHEN** a coordinator calls `send_message` or `resume_agent` naming a PR keeper agent of its ticket
- **THEN** the call is refused and nothing is queued for that agent

#### Scenario: Two sessions of one ticket
- **WHEN** a ticket has a sorting session and the keeper creates a try session for it
- **THEN** the try session is a new PR keeper agent, not the sorting session

#### Scenario: The worktree is not held
- **WHEN** a PR keeper agent of a ticket is in a turn and the ticket's implementer has queued input
- **THEN** the implementer's turn starts

#### Scenario: A try while Claude is spent
- **WHEN** the PR keeper row is Sonnet · medium then GPT 6.1 Sol · medium, Claude is spent and a conflict gets a try
- **THEN** the try runs on GPT 6.1 Sol · medium in a Codex PR keeper agent

### Requirement: The conflicts trigger needs a model and a re-check level
The keeper's settings SHALL hold the review level a fix is reviewed at. They SHALL refuse to turn the conflicts trigger on unless the rebase trigger is on, the table has a PR keeper row and the review level names a configured level whose kind is re-check, and SHALL refuse to turn the rebase trigger off while the conflicts trigger is on: a conflict is found by the keeper's own rebase, so without the rebase trigger no try could ever start. A verification setting that removes that level or makes it a full review while the trigger is on SHALL be refused, naming the trigger.

#### Scenario: No re-check level
- **WHEN** the only review level is "Full" and the human ticks "Try to resolve conflicts"
- **THEN** the save is refused, saying to add a re-check level in Models → Reviews

#### Scenario: Without the rebase trigger
- **WHEN** the rebase trigger is off and the human ticks "Try to resolve conflicts"
- **THEN** the save is refused, saying it needs "Rebase when the base branch moves"

#### Scenario: Turning the rebase trigger off
- **WHEN** both triggers are on and the human unticks the rebase trigger alone
- **THEN** the save is refused and both stay on

#### Scenario: The level is removed later
- **WHEN** the conflicts trigger is on with the level "Light" and the human removes "Light"
- **THEN** the verification setting is refused and stays as it was
