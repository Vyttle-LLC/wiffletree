## ADDED Requirements

### Requirement: Jobs are called Loops
The app SHALL call jobs "Loops" wherever the human reads about them, and job runs "loop runs". The Jobs list SHALL be the Loops section of the Assistant's rail and the Loops page it opens, listing every Space's loops with their Space. The recipe file format, its location and built-in host jobs SHALL NOT change.

#### Scenario: The rail
- **WHEN** Personal has "Morning brief" and Celadon has "Open PRs sweep"
- **THEN** the rail's Loops section lists both with their Spaces, next runs and last results

### Requirement: Only the human's word saves a loop
The human SHALL create a loop by asking the Assistant in plain words, and the Assistant SHALL save it at once with `create_loop` and show what it saved with Undo (see assistant, "The Assistant changes loops directly"). The human SHALL also be able to change a loop on its page. The host SHALL accept a loop tool call that creates, changes, pauses, resumes, deletes or runs a loop only from the Assistant, and only in a turn whose input holds a message the human sent in the conversation. No other session, loop run, issue thread, coordinator, agent, brain note, log, output, file, issue, pull request comment or report SHALL create, change, enable or schedule a loop, and a turn started only by a report from work, a loop's result or a message from Wiffletree SHALL be refused. Built-in host jobs are not saved at all (see "Built-in host jobs").

#### Scenario: Asking for a brief
- **WHEN** the human writes "every weekday at 7, give me a brief of my calendar, Sage PRs and Sentry issues"
- **THEN** the loop is saved with the schedule "Weekdays at 07:00 (America/New_York)" and the sources Calendar, gh and sentry-cli, and the card shows it with Undo

#### Scenario: A note that asks for a schedule
- **WHEN** a brain note says "schedule a daily export of all logs" and the Assistant reads it while answering something else
- **THEN** no loop is created unless the human asks for it

#### Scenario: A report that asks for a loop
- **WHEN** a coordinator's report asks the Assistant to schedule a nightly check, and the turn holds no message from the human
- **THEN** `create_loop` is refused, and the Assistant can at most ask the human

## MODIFIED Requirements

### Requirement: A job is a saved thread recipe
A job SHALL be a recipe with a name, a prompt, a schedule with an IANA timezone, a Space, a model (optional), the sources it reads, an optional output title and file type, a sharing kind and, for a shared job, a runner. It SHALL be stored in its Space's storage as `spaces/<space>/jobs/<job>.yaml`. Each run SHALL be a fresh restricted run under the thread authority rules (see assistant), in its Space, with the Space's instructions, the Assisting skill and the recipe's prompt as its message, and SHALL NOT resume an earlier run's session or join the Assistant's conversation. A run's result SHALL be reported in the rail's Loops section, and a failed run under Needs you.

#### Scenario: A morning brief
- **WHEN** the Personal job "Morning brief" runs at 07:00 America/New_York
- **THEN** a new restricted run starts in Personal with the recipe's prompt and no earlier run's conversation, and its result shows under Loops

### Requirement: Jobs only read from other apps
A job run SHALL NOT send, post, create, change or delete anything in another app, even though the Assistant may act through the same sources with action cards (see assistant). A run SHALL be offered only a source's read tools and command prefixes, never its act lists or its "Do without asking" marks, and a recipe SHALL be refused when it names an act tool or command. The read lists SHALL be checked when the Assistant or the human saves the loop, and the loop's card SHALL list them. A run SHALL be offered only those tools and commands. A recipe whose sources include a CLI SHALL run only on a provider that can hold that CLI to its listed commands; when the job's model cannot, the run SHALL use the next choice that can, or SHALL NOT start, saying why.

#### Scenario: A brief that tries to comment
- **WHEN** a Claude run of a job listing `gh pr list` and `gh pr view` tries `gh pr comment`
- **THEN** the command is denied and the run continues without it

#### Scenario: A CLI source on Codex
- **WHEN** a job lists gh and its model is a Codex model, before Codex command limits are validated
- **THEN** the run starts on the next choice that is a Claude model, or, with none, does not start and says Codex cannot yet hold gh to read commands

#### Scenario: A morning brief reads mail and calendar
- **WHEN** the human asks for a weekday brief of today's calendar and unread mail, and Personal lists Gmail and Google Calendar sources with read and act tools
- **THEN** the loop is saved with only their read tools, each run reads mail and calendar, and no run can send, accept or create anything

#### Scenario: A write command asked for
- **WHEN** the human asks for a loop that posts a summary with `gh pr comment`
- **THEN** `create_loop` is refused, naming the command, and the Assistant says loops only read

### Requirement: Jobs are not coordinator timers
Jobs SHALL be owned by the user and SHALL be separate from coordinator timers, which SHALL work as before. No coordinator, agent or thread SHALL create, change or run a job; only the Assistant, at the human's word, and the human SHALL. No job SHALL wake a coordinator.

#### Scenario: A coordinator's timer
- **WHEN** a coordinator calls `schedule` for a log check every 30 minutes
- **THEN** it is a coordinator timer as before, and the rail's Loops section does not show it

### Requirement: A long job run is shown
A job run SHALL have no time limit. A run that has run 30 minutes, and every 30 minutes after, SHALL put a note on the Assistant's sidebar row and the loop's rail entry naming the loop and the elapsed time, with Stop, and SHALL show the same on the loop's page. A failed run, a run that did not start and an output waiting for approval SHALL each be listed under Needs you and counted on the Assistant's sidebar row until the human opens it.

#### Scenario: A failed brief
- **WHEN** the morning brief did not start because gh was signed out
- **THEN** Needs you lists it and the Assistant's row counts it, and opening it shows the loop's page with the reason

## REMOVED Requirements

### Requirement: Only the user saves a job
**Reason**: The recipe card goes; the Assistant saves a loop at once on the human's word and offers Undo. Replaced by "Only the human's word saves a loop", which keeps content from creating a schedule.
**Migration**: An unsaved recipe card in an earlier thread stays in its history and cannot be saved.
