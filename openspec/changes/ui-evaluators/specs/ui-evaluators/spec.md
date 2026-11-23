## ADDED Requirements

### Requirement: A coordinator starts an evaluator run

The workspace SHALL offer `start_evaluation` with `run_folder`, `provider`, `model`, `effort` and `mode` (`app` or `desktop`), with `target` for an app run and `apps` for a desktop run, to the project coordinator only. The run folder SHALL be an absolute path to an existing folder holding a `prompt.md` file, and SHALL NOT be inside a Git repository or worktree, a workspace repository or Wiffletree's data folder. A valid call SHALL create a run of the caller's project and return its id; a refused call SHALL create nothing and SHALL name the reason.

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

A run SHALL have no shell, web, repository, brain, memory, history or workspace tools, and none of the human's provider instructions, settings, skills or MCP servers. It SHALL read and write only inside its run folder. Its first message SHALL be the content of `prompt.md` with nothing added, and no instructions SHALL be appended to its system prompt. Its environment SHALL carry no Wiffletree session variable. A Claude run that starts with another tool, an MCP server other than its driver, another permission mode or auto memory SHALL be stopped; so SHALL a Codex run that calls any MCP tool but its driver's. Neither provider's built-in computer use SHALL be offered, and Codex SHALL ignore the user's config, so the user's `notify` hook never runs.

#### Scenario: Claude's launch

- **WHEN** a Claude run starts
- **THEN** it runs with no setting sources, a strict MCP configuration holding only its driver, `dontAsk`, only the read and write file tools, and allow rules for its folder alone

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

App-only runs SHALL run side by side. Full-desktop runs SHALL share one queue for every project and one screen lock: the run at the front takes the lock, the others wait queued in the order they were started, and the lock passes on when the holder ends. A run holding the lock SHALL wait for the human's Start on an Inbox card offering Start now, Later and Skip these runs, and SHALL NOT touch the screen before Start now. Start now SHALL start the run, unless its provider is at its usage limit, when it SHALL be refused with the reset time and the card stays. Later SHALL leave the card and the run first in line. Skip these runs SHALL stop the run and its project's queued desktop runs, telling their coordinator. The card SHALL NOT be dismissed. A full-desktop run whose turn reaches its usage limit SHALL end as failed rather than wait, so it never takes the screen again without a Start.

#### Scenario: Two desktop runs

- **WHEN** two desktop runs are admitted
- **THEN** the first holds the screen and waits for the human's Start, and the second stays queued until the first ends

#### Scenario: Start now

- **WHEN** the human presses Start now on the card of the run holding the screen
- **THEN** the run gets its session and starts, and only then

#### Scenario: Skip these runs

- **WHEN** the human presses Skip these runs
- **THEN** the run and its project's queued desktop runs end as stopped, "Skipped by you.", the coordinator is told of each, and the screen passes to the next project's run

#### Scenario: A limit mid-run

- **WHEN** a desktop run's turn ends at its provider's usage limit
- **THEN** the run fails and the screen passes on; it does not wait for the reset

### Requirement: A full-desktop run sees and touches only the apps it names

`start_evaluation` in desktop mode SHALL take `apps`, one to ten bundle IDs, and no target, and SHALL refuse an empty list, a malformed ID and Wiffletree Evaluator itself. Nothing SHALL be allowed that the coordinator does not name. The run's only MCP server SHALL be the desktop driver, named `desktop`, for Claude and Codex alike, with the tools screenshot, click, double_click, right_click, move, type_text, key, scroll, drag, open_app and list_windows, in screen points; every screenshot SHALL report its scale and be saved inside the run folder by the app driver's rules. The helper, not the model, SHALL enforce:

- a screenshot captures only the allowed apps' windows, through a ScreenCaptureKit content filter limited to them, never the whole display;
- a click, move, scroll or drag is refused unless the topmost on-screen window at its point, at that moment, belongs to an allowed app;
- keys and text are refused unless an allowed app is frontmost, and go to that app's process alone;
- open_app opens only an allowed app, and list_windows lists only their windows.

#### Scenario: Input over another app

- **WHEN** an evaluator allowed only Calculator clicks a point where a notification banner covers Calculator
- **THEN** the click is refused, and the reply does not name the other app

#### Scenario: Keys while another app is frontmost

- **WHEN** Terminal is frontmost and the evaluator types text
- **THEN** nothing is typed and the tool says no allowed app is frontmost

#### Scenario: No apps

- **WHEN** a coordinator starts a desktop run with no apps
- **THEN** the call is refused and no run is created

### Requirement: Only Wiffletree Evaluator holds the screen permissions

A full-desktop run SHALL drive the screen through Wiffletree Evaluator, a helper app shipped in Wiffletree with its own bundle ID, signed with Wiffletree's identity and the hardened runtime. Because macOS charges Screen Recording to the outermost bundle, the host SHALL install it outside Wiffletree's bundle, in its data folder, and SHALL start that copy per run through LaunchServices, so that macOS charges the helper's own grants and never Wiffletree's. Before the provider starts, the host SHALL read the helper's Screen Recording and Accessibility grants, as the helper sees them, and whether it can hear Esc, and SHALL fail the run, naming the System Settings pane to open, when any is missing. The helper SHALL listen on a socket of the run's own with mode 600 and quit when the run ends or the host's connection closes; the host SHALL kill it if it stays. Settings › Screen control (ticket 5) SHALL describe the helper's permissions and no provider's computer use.

#### Scenario: The helper is charged as itself

- **WHEN** the host starts and the shipped helper differs from its installed copy, or there is none
- **THEN** it installs the helper in its data folder, and a run's helper is checked for Screen Recording and Accessibility as `com.vyttle.wiffletree.evaluator`, never as Wiffletree

#### Scenario: A missing grant

- **WHEN** a desktop run starts and the helper lacks Screen Recording
- **THEN** the run fails before anything touches the screen, naming System Settings › Privacy & Security › Screen & System Audio Recording

#### Scenario: The run ends

- **WHEN** a desktop run's turn ends, however it ends
- **THEN** the helper quits, and is killed if it has not quit within three seconds

### Requirement: The human stops a full-desktop run on the screen

While a full-desktop run holds the screen, the helper SHALL show "Evaluator using this Mac" in the menu bar with Stop, and SHALL listen for Esc anywhere, ignoring the keys it posts itself. Esc, Stop or the helper quitting SHALL stop the run at once, as `stop_evaluation` does, and the helper SHALL refuse every later request.

#### Scenario: Esc

- **WHEN** the human presses Esc during a desktop run
- **THEN** the turn is cancelled at once and the run ends as stopped, "Stopped: you pressed Esc.", and its coordinator is told

### Requirement: The coordinator learns the outcome, never the content

When a run ends, Wiffletree SHALL message its coordinator with only the run's id, state, model, folder and duration. `list_evaluations` SHALL list the project's runs with their state, mode, the model asked for and the one reported, folder, times and a waiting run's reset, and no transcript, reply or failure text. `stop_evaluation` SHALL stop one of the project's runs; the human can stop any run.

#### Scenario: A run finishes

- **WHEN** a run's turn ends on its own
- **THEN** the coordinator receives "Run <id> finished · model <model> · folder <folder> · <minutes>", with none of the run's reply
