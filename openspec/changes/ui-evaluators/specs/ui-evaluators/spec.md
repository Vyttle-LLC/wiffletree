## ADDED Requirements

### Requirement: A coordinator starts an evaluator run

The workspace SHALL offer `start_evaluation` with `run_folder`, `provider`, `model`, `effort` and `mode` (`app` or `desktop`) to the project coordinator only. The run folder SHALL be an absolute path to an existing folder holding a `prompt.md` file, and SHALL NOT be inside a Git repository or worktree, a workspace repository or Wiffletree's data folder. A valid call SHALL create a run of the caller's project and return its id; a refused call SHALL create nothing and SHALL name the reason.

#### Scenario: A valid start

- **WHEN** a coordinator whose project allows app-only evaluators starts an app run on a signed-in provider's listed model and effort, with a valid folder
- **THEN** the tool returns a new run id, and the run starts in a fresh session

#### Scenario: A bad folder

- **WHEN** the folder is relative, missing, has no `prompt.md`, or is inside a repository, worktree or Wiffletree's data folder
- **THEN** the call is refused with the reason and no run is created

#### Scenario: A worker asks

- **WHEN** an implementer, tester or reviewer calls `start_evaluation`
- **THEN** the call is refused

### Requirement: A run uses exactly the model it was started with

A run SHALL use only the provider, model and effort it was started with, and SHALL NOT consult Who runs what or fall back to another model. A provider not signed in on this Mac, a model its CLI did not list and an effort the model does not offer SHALL be refused at start. A run whose provider is at its usage limit SHALL wait, showing the reset time, and SHALL continue on the same model after the reset.

#### Scenario: Unknown model

- **WHEN** `start_evaluation` names a model the provider does not offer
- **THEN** the call is refused, naming the model and the provider

#### Scenario: At a limit

- **WHEN** a run is admitted while its provider's limit is reached
- **THEN** the run is waiting with the reset time, and no other model starts it

### Requirement: A run is blind and isolated by its provider

A run SHALL have no shell, web, repository, brain, memory, history or workspace tools, and none of the human's provider instructions, settings, skills or MCP servers. It SHALL read and write only inside its run folder. Its first message SHALL be the content of `prompt.md` with nothing added, and no instructions SHALL be appended to its system prompt. Its environment SHALL carry no Wiffletree session variable. A Claude run that starts with another tool, an MCP server, another permission mode or auto memory SHALL be stopped; so SHALL a Codex run that calls any MCP tool.

#### Scenario: Claude's launch

- **WHEN** a Claude run starts
- **THEN** it runs with no setting sources, a strict and empty MCP configuration, `dontAsk`, only the read and write file tools, and allow rules for its folder alone

#### Scenario: Codex's launch

- **WHEN** a Codex run starts
- **THEN** it ignores the user's config and rules, runs in a workspace-write sandbox rooted at its folder with no network or web search, has no shell, and uses a Codex home of its own that holds only the user's sign-in

### Requirement: The human authorizes evaluators per project

Each project SHALL have an evaluator permission of off, app or desktop, which only the human sets; a project with no choice is off. The first `start_evaluation` in a project with no choice SHALL raise one Inbox card offering App only, Full desktop and Not in this project, and SHALL hold the run until it is answered. Closing the project SHALL turn the permission off. A refusal SHALL name the reason, such as "Full desktop is not allowed in this project. Ask the human."

#### Scenario: First run in a project

- **WHEN** a coordinator starts a run in a project with no choice
- **THEN** an Inbox card is raised, the run is waiting, and the tool says the human decides

#### Scenario: The human allows app only

- **WHEN** the human answers the card with App only
- **THEN** the held app runs start and the held desktop runs are stopped

#### Scenario: Refused mode

- **WHEN** a project allows app only and a coordinator starts a desktop run
- **THEN** the call is refused: "Full desktop is not allowed in this project. Ask the human."

#### Scenario: Closing the project

- **WHEN** the human archives the project's coordinator
- **THEN** the permission is off and the project's unfinished runs are stopped

### Requirement: Full-desktop runs take turns at the screen

App-only runs SHALL run side by side. Full-desktop runs SHALL share one queue for every project and one screen lock: only the run holding the lock runs, the others wait queued in the order they were started, and the lock passes on when the holder ends.

#### Scenario: Two desktop runs

- **WHEN** two desktop runs are admitted
- **THEN** the first runs and the second stays queued until the first ends

### Requirement: The coordinator learns the outcome, never the content

When a run ends, Wiffletree SHALL message its coordinator with only the run's id, state, folder and duration. `list_evaluations` SHALL list the project's runs with their state, mode, model, folder, times and a waiting run's reset, and no transcript, reply or failure text. `stop_evaluation` SHALL stop one of the project's runs; the human can stop any run.

#### Scenario: A run finishes

- **WHEN** a run's turn ends on its own
- **THEN** the coordinator receives "Run <id> finished · folder <folder> · <minutes>", with none of the run's reply
