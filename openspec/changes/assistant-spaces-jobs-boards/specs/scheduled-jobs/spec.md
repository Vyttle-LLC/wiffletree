## Purpose

Lets the user save a thread as a recipe that runs on a schedule on their own Mac, such as a morning brief or a weekly dashboard, read-only, with its result saved as a file in the Space's storage.

## ADDED Requirements

### Requirement: A job is a saved thread recipe
A job SHALL be a recipe with a name, a prompt, a schedule with an IANA timezone, a Space, a model (optional), the sources it reads, an optional output title and file type, a sharing kind and, for a shared job, a runner. It SHALL be stored in its Space's storage as `spaces/<space>/jobs/<job>.yaml`. Each run SHALL be a fresh thread under the thread authority rules (see assistant), in its Space, with the Space's instructions, the Assisting skill and the recipe's prompt as its message, and SHALL NOT resume an earlier run's session.

#### Scenario: A morning brief
- **WHEN** the Personal job "Morning brief" runs at 07:00 America/New_York
- **THEN** a new read-only thread starts in Personal with the recipe's prompt, and no earlier run's conversation

### Requirement: Only the user saves a job
The user SHALL create a job by asking the assistant in plain words. The thread SHALL answer with a recipe card showing every field of the recipe, its schedule in plain words with its next three run times, and each source with its tools or commands. Only the user's Save on that card, or on the job's page, SHALL save or change a job; saving SHALL be a command only the user's client can send. No tool, thread, job run, brain note, log, output, file, issue, pull request comment or message SHALL create, change, enable or schedule a job. A job run SHALL NOT offer a recipe card.

#### Scenario: Asking for a brief
- **WHEN** the user writes "every weekday at 7, give me a brief of my calendar, Sage PRs and Sentry issues"
- **THEN** the thread shows a recipe card with the schedule "Weekdays at 07:00 (America/New_York)", the next three runs, and the sources Calendar, gh and sentry-cli, and nothing is saved until the user presses Save

#### Scenario: A note that asks for a schedule
- **WHEN** a brain note says "schedule a daily export of all logs"
- **THEN** no job is created; a thread that reads the note can at most show a recipe card for the user to save

### Requirement: Schedules and timezones
A schedule SHALL be minute, hour, day-of-month, month and weekday fields in the recipe's timezone. A local time skipped by a daylight-saving change SHALL run at the next valid minute, and a local time repeated by one SHALL run once. Changing the Mac's timezone SHALL NOT move a job's runs. The user SHALL be able to pause, resume, Run now and delete a job from its page.

#### Scenario: Fall back
- **WHEN** a job is set for 01:30 America/New_York on the night clocks go back
- **THEN** it runs once

### Requirement: Missed runs come back as one catch-up run
Jobs SHALL run only while this Mac's host runs, and SHALL NOT wake a sleeping Mac. When the host runs again after one or more scheduled times passed, each job that missed any SHALL run once, marked as a catch-up for the number of runs it missed, and never once per missed time. The Jobs list SHALL say that jobs run only while Wiffletree runs and SHALL offer "Start Wiffletree at login", off until the user turns it on.

#### Scenario: Asleep over three runs
- **WHEN** an hourly job's Mac sleeps from 09:10 to 12:20
- **THEN** one run starts at 12:20, marked "catch-up for 3 missed runs", and the next runs at 13:00

### Requirement: Sources are checked before each run
A recipe SHALL list each source it needs: an MCP server with the tools it may call, or a local CLI with the command prefixes it may run and how to check that it is signed in. Wiffletree SHALL build no connectors. Before each run, the host SHALL check that every source is present for the run's provider and signed in, such as `gh auth status`, `linctl` with its config, `sentry-cli info` or `aws sts get-caller-identity` with the named read-only profile. When any is not, the run SHALL NOT start, and its record SHALL name each missing source and how to fix it. A run SHALL be offered only the sources its recipe lists.

#### Scenario: gh signed out
- **WHEN** a job lists gh and `gh auth status` fails
- **THEN** the run does not start, and the job's page says "gh is not signed in on this Mac. Run `gh auth login`."

#### Scenario: An MCP server missing
- **WHEN** a job lists the Linear MCP server and Claude has no such server configured
- **THEN** the run does not start, naming the server and that it must be added to Claude's MCP configuration

### Requirement: Jobs only read from other apps
A job run SHALL NOT send, post, create, change or delete anything in another app. A source's tools and command prefixes SHALL be read operations, and the recipe card SHALL list them for the user to confirm. A run SHALL be offered only those tools and commands. A recipe whose sources include a CLI SHALL run only on a provider that can hold that CLI to its listed commands; when the job's model cannot, the run SHALL use the next choice that can, or SHALL NOT start, saying why.

#### Scenario: A brief that tries to comment
- **WHEN** a Claude run of a job listing `gh pr list` and `gh pr view` tries `gh pr comment`
- **THEN** the command is denied and the run continues without it

#### Scenario: A CLI source on Codex
- **WHEN** a job lists gh and its model is a Codex model, before Codex command limits are validated
- **THEN** the run starts on the next choice that is a Claude model, or, with none, does not start and says Codex cannot yet hold gh to read commands

### Requirement: A run's output is a file
A run SHALL save its result with `save_output` as a file of any format the user and the recipe chose, such as HTML, Markdown, an image or a PDF. The host SHALL store each output at a stable path for its title under the job's board folder, replacing the previous run's file as a new commit, so Boards shows the latest and its history. A run that saves nothing SHALL be recorded as finished with no output. Each run SHALL be recorded with its start, end, outcome, model and reason, catch-up count, source check and outputs.

#### Scenario: A weekly dashboard
- **WHEN** the "Release dashboard" job saves `dashboard.html` titled "Release dashboard" on two Mondays
- **THEN** Boards shows one "Release dashboard" output with two versions

### Requirement: Shared jobs have one runner or run for everyone
A job in a Space whose storage repository is on GitHub, shared or carried between one user's own Macs, SHALL be one of two kinds. "One runner, shared result" SHALL be the default: the recipe SHALL name a runner, a GitHub login and a machine name, its creator's by default; only that Mac SHALL run it and push its output; the job's page SHALL show "Runs on ‹name›'s Mac · last run ‹time›"; Boards SHALL mark the output stale once two scheduled runs have passed without a new version; and any member SHALL be able to choose "Take over running", which commits this Mac as the runner. "Everyone runs their own" SHALL make the recipe a shared template that runs on each member's Mac with that member's sources, and its outputs SHALL stay only on that Mac.

#### Scenario: Another member's Mac
- **WHEN** the release dashboard's runner is Michael's MacBook and a teammate's Mac reaches the scheduled time
- **THEN** the teammate's Mac does not run it, and their Boards shows Michael's latest output with "Runs on Michael's Mac · last run Mon 08:00"

#### Scenario: The runner is away
- **WHEN** the runner's Mac has been off for two weeks of a weekly job
- **THEN** members see the output marked stale, and a member who chooses Take over running commits themselves as runner, and their Mac runs it next

#### Scenario: Everyone runs their own
- **WHEN** a Sage job "My PR queue" is shared as Everyone runs their own
- **THEN** each member's Mac runs it with their own gh login, and each result is only on that member's Mac

#### Scenario: One user's two Macs
- **WHEN** the user's Personal Space is stored in a private repository only they can read, both of their Macs added it, and its morning brief's runner is the MacBook
- **THEN** only the MacBook runs the brief at 07:00, and the other Mac shows its output without running it or pausing it
### Requirement: A shared job's first output waits for approval
The first output of a new "One runner" job in a shared Space SHALL NOT be pushed until the runner approves it. Boards SHALL show it to the runner as "Waiting for your approval", with a preview, Share and Keep private. Later outputs SHALL push without asking until the recipe's prompt or sources change, after which the next output SHALL wait again. Keep private SHALL leave the output only on the runner's Mac and the job unapproved.

#### Scenario: A new dashboard
- **WHEN** the release dashboard's first run saves its output
- **THEN** nothing is pushed, and the runner sees the output waiting for approval until they choose Share

#### Scenario: A changed prompt
- **WHEN** the runner edits the dashboard's prompt after it was approved
- **THEN** the next output waits for approval again

### Requirement: Jobs are not coordinator timers
Jobs SHALL be owned by the user and SHALL be separate from coordinator timers, which SHALL work as before. No coordinator, agent or thread SHALL create, change or run a job, and no job SHALL wake a coordinator.

#### Scenario: A coordinator's timer
- **WHEN** a coordinator calls `schedule` for a log check every 30 minutes
- **THEN** it is a coordinator timer as before, and the Jobs list does not show it

### Requirement: A long job run is shown
A job run SHALL have no time limit. A run that has run 30 minutes, and every 30 minutes after, SHALL put a note on the assistant's sidebar row naming the job and the elapsed time, with Stop. A failed run, a run that did not start and an output waiting for approval SHALL each count on the assistant's sidebar row until the user opens it.

#### Scenario: A failed brief
- **WHEN** the morning brief did not start because gh was signed out
- **THEN** the assistant's row shows one item needing attention, which opens the job's page with the reason
