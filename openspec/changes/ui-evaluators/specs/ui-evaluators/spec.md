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

A run SHALL use only the provider, model and effort it was started with, and SHALL NOT consult Who runs what or fall back to another model. A provider not signed in on this Mac, a model its CLI did not list, whether a full ID or an alias, and an effort the model does not offer SHALL be refused at start. The run SHALL record the model its provider names when it starts, and a run started on a full ID whose provider names another model SHALL fail at once. A run whose provider is at its usage limit SHALL wait, showing the reset time, and SHALL continue on the same model after the reset.

#### Scenario: Unknown model

- **WHEN** `start_evaluation` names a model the provider does not offer
- **THEN** the call is refused, naming the model and the provider

#### Scenario: The list is not read yet

- **WHEN** `start_evaluation` names a provider whose CLI's model list Wiffletree has not read
- **THEN** the call is refused with that reason, and Wiffletree reads the list so a repeated call can succeed

#### Scenario: The model moved

- **WHEN** a run started on `claude-opus-5-5` and Claude names another model at its start
- **THEN** the run fails, with the reason recorded for the human, and runs no further

#### Scenario: An alias

- **WHEN** a run started on `opus` and Claude names `claude-opus-5-5`
- **THEN** the run goes on and records `claude-opus-5-5`

#### Scenario: At a limit

- **WHEN** a run is admitted while its provider's limit is reached
- **THEN** the run is waiting with the reset time, and no other model starts it

### Requirement: A run is blind and isolated by its provider

A run SHALL have no shell, web, repository, brain, memory, history or workspace tools, and none of the human's provider instructions, settings, skills or MCP servers. It SHALL read and write only inside its run folder. Its first message SHALL be the content of `prompt.md` with nothing added, and no instructions SHALL be appended to its system prompt. Its environment SHALL carry no Wiffletree session variable. A Claude run that starts with another tool, an MCP server other than an app run's driver, another permission mode or auto memory SHALL be stopped; so SHALL a Codex run that calls any MCP tool but its driver's.

#### Scenario: Claude's launch

- **WHEN** a Claude run starts
- **THEN** it runs with no setting sources, a strict MCP configuration holding only an app run's driver, `dontAsk`, only the read and write file tools, and allow rules for its folder alone

#### Scenario: Codex's launch

- **WHEN** a Codex run starts
- **THEN** it ignores the user's config and rules, runs in a workspace-write sandbox rooted at its folder with no network or web search, has no shell, and uses a Codex home of its own that holds only the user's sign-in

### Requirement: An app run drives a test copy the host starts

An app run SHALL name its target: an absolute executable, its arguments with a test data folder given by `--data-dir`, and the variables it needs. The data folder SHALL exist when the run is started and SHALL be compared by its canonical path. The workspace SHALL refuse a target without its own data folder, one whose data folder does not exist, overlaps Wiffletree's own or is used by another run that has not ended, one with more than one `--data-dir` or the `--data-dir=<path>` spelling, one that gives `--automation-socket`, and one given a Wiffletree session variable or a token. Before the run's turn, the host SHALL start the copy with `--automation-socket` on a socket of the run's own, from a clean environment holding the given variables, a given `PATH` before its own and the switch that keeps the copy's host from adopting the login shell's environment, and SHALL fail the run with the reason when the socket does not answer. When the turn ends or is stopped, the host SHALL stop the copy and everything it started.

#### Scenario: The copy starts

- **WHEN** an app run's turn starts
- **THEN** its copy runs on its own socket with no `WORKSPACE_TOKEN`, `WORKSPACE_SOCKET`, `GH_TOKEN` or `GITHUB_TOKEN`, with the given `PATH` first, and the provider starts only once the socket answers

#### Scenario: The socket never answers

- **WHEN** the copy exits or does not answer within the wait
- **THEN** the run fails with that reason and the copy is stopped

#### Scenario: Two runs on one data folder

- **WHEN** a coordinator starts an app run on the data folder of a run that has not ended, however its path is spelled or linked
- **THEN** the call is refused, naming that run

#### Scenario: A data folder not created yet

- **WHEN** a coordinator starts an app run whose `--data-dir` does not exist
- **THEN** the call is refused: the coordinator creates the folder first

### Requirement: The app driver reaches only the copy's window

An app run's only MCP server SHALL be the app driver, connected to its copy's socket, with the tools screenshot, click, double_click, right_click, move, type_text, key, scroll, drag, resize_window and window, in window points that match a screenshot at scale 1. A screenshot SHALL be saved as a PNG inside the run folder under the name the model gives and returned as an image with its pixel size and the window's scale. resize_window SHALL report the size reached. Because Claude downsizes an image over 2000 pixels on its long edge, the screenshot tool SHALL tell the model to sample pixels from a shot at one pixel per point, or a native shot small enough, rather than a native shot of a large window. The app SHALL capture its own window, at one pixel per point or the display's scale, without Screen Recording permission; on macOS 27.0.1 this was measured from a launch with no grant.

#### Scenario: A screenshot

- **WHEN** the evaluator calls screenshot with the name `screenshots/models-dark` and scale native
- **THEN** `screenshots/models-dark.png` in its folder holds the window at the display's scale, and the tool returns that image with its size and scale

#### Scenario: A name outside the folder

- **WHEN** the evaluator names a screenshot `../x.png`
- **THEN** the tool refuses it and writes nothing

#### Scenario: Another server or tool

- **WHEN** a Claude app run starts with any MCP server but the connected driver, or a tool beyond its files and the driver's, or a Codex app run calls another server's tool
- **THEN** the run is stopped

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

When a run ends, Wiffletree SHALL message its coordinator with only the run's id, state, model, folder and duration. `list_evaluations` SHALL list the project's runs with their state, mode, the model asked for and the one reported, folder, times and a waiting run's reset, and no transcript, reply or failure text. `stop_evaluation` SHALL stop one of the project's runs; the human can stop any run.

#### Scenario: A run finishes

- **WHEN** a run's turn ends on its own
- **THEN** the coordinator receives "Run <id> finished · model <model> · folder <folder> · <minutes>", with none of the run's reply
