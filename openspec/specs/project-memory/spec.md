# project-memory Specification

## Purpose
Preserve meaningful work across replacement sessions and providers using durable scoped records and portable Markdown.

## Requirements

### Requirement: Stable scoped milestones
Logs SHALL carry project, ticket, session, milestone identity, type, title, reference, changed, why and state, and the host SHALL also record the entry's id, Space, brain, repository folder, author, provider, model and machine, and for an assistant thread its thread id. Repeated identical capture under one session and milestone identity SHALL return the original record, and changed content under the same identity SHALL fail. A capture without a milestone identity SHALL always create a new entry. Retrieval SHALL NOT expose another project's entries through a project's own views; the brain browser shows a brain's entries only within the scope rules of `brain-browser`.

#### Scenario: Same day milestones
- **WHEN** a session captures two milestones on one day and repeats the first
- **THEN** both distinct entries remain once, with distinct ids, and neither is skipped

#### Scenario: Changed content under one milestone
- **WHEN** a session repeats a milestone identity with a different `changed` text
- **THEN** the capture is refused and the first entry is unchanged

### Requirement: Agents log work through the log_work tool
The host SHALL offer a `log_work` MCP tool to the project coordinator, implementers and assistant threads, and SHALL NOT offer it to testers, reviewers, the PR keeper or job runs. The tool SHALL take `type` (`decision` or `observation`), `title` (one line of at most 80 characters), `changed`, `why` and `state` (each folded to one line), and optionally `repository` and `milestone`. The coordinator MAY also give `ticket_id`, one of its project's tickets, which makes the entry that ticket's: its repository's folder and its `ref`.

The entry's brain SHALL be the brain of the caller's Space: the project's Space for a coordinator or implementer, and the thread's own Space for an assistant thread (`brain-registry`, "A project's brain comes from its Space"). Its folder SHALL be chosen as follows:
- An implementer's entry SHALL be for its ticket's repository, and a `repository` it names SHALL be refused.
- A coordinator MAY name one of its project's repositories; without one, the entry SHALL be a planning entry in the folder `planning`.
- An assistant thread's entries SHALL go in the `assistant` folder, and a `repository` or `ticket_id` it names SHALL be refused.

For a Space on GitHub the tool SHALL refuse, storing nothing, when gh reports no GitHub login. For a Space on this Mac alone it SHALL not need one. The human's capture form in the Memory panel SHALL use the same path as the project coordinator.

#### Scenario: An implementer logs
- **WHEN** the implementer of a `sagechat-call-watch-relay` ticket calls `log_work` with a decision, title and the three fields
- **THEN** the host stores one entry for `sagechat-call-watch-relay` in Sage's folder, sends it to the sync engine at once, and returns its id and file path

#### Scenario: A Space on this Mac alone
- **WHEN** an implementer in a project of Personal, which named no brain in an earlier release, calls `log_work` with `gh` signed out
- **THEN** the entry is committed in Personal's local repository, with this Mac's account name as `author`

#### Scenario: An assistant thread logs
- **WHEN** an assistant thread in the Vyttle Space calls `log_work`
- **THEN** the host stores one entry at `wiffletree/second-brain/logs/assistant/YYYY/MM/YYYY-MM-DD-<id>.md` with `repo: assistant`, and records the thread's id and Space on the stored entry

#### Scenario: A reviewer has no tool
- **WHEN** the host builds a reviewer's tool list
- **THEN** `log_work` is not in it

### Requirement: The host fills in what the agent cannot know
The host SHALL mint the entry's id as a lowercase ULID and take its date from the host's local date. It SHALL set `author` to the GitHub login gh reports on this machine, or on a local repository with none, this Mac's account name, and `source` to `wiffletree`. It SHALL compute `ref` from the ticket's worktree: `<branch> @ <sha>` for one commit since the ticket's base, `<branch> @ <first>…<last>` for several, `<branch> @ —` when there is no commit yet, and `—` for a coordinator entry with no ticket or an assistant thread's entry, appending ` · PR #NN` when the ticket has a known PR. SHAs SHALL be 7 lowercase hex characters. The tool SHALL NOT accept `ref`, `author`, `source`, `id` or a date from the agent. The host SHALL record the provider, model, machine, Space, project, ticket, session and, for a thread, the thread's id on the stored entry.

#### Scenario: A branch with commits and a PR
- **WHEN** a ticket's branch `wiffletree/retry-telnyx-timeout` has two commits since its base and PR #92 is open
- **THEN** the entry's `ref` is `wiffletree/retry-telnyx-timeout @ 3c1e9a0…7d03b1e · PR #92`

#### Scenario: Uncommitted work
- **WHEN** an implementer logs before its first commit
- **THEN** the entry's `ref` is `<branch> @ —`

### Requirement: Entries follow the per-entry file contract
Each entry SHALL be rendered as one file at `second-brain/logs/<repo>/YYYY/MM/YYYY-MM-DD-<id>.md` in the folder, in exactly the entry contract (see `brain-gate`): frontmatter with exactly `id`, `repo`, `author` and `source`, then the `## YYYY-MM-DD — Title` heading and the five fields `type`, `ref`, `changed`, `why` and `state` in that order. `<repo>` SHALL be the GitHub repository's name, `assistant` for an assistant thread's entry, or `planning` for a planning entry; names the contract forbids SHALL be refused. No assistant thread SHALL write the rolling file `logs/assistant.md`. The host SHALL refuse an entry whose rendered body exceeds 2,000 characters, saying how many characters to cut, and SHALL NOT truncate an agent's entry. A landed entry file SHALL never be changed by the host.

#### Scenario: Rendering
- **WHEN** an entry is stored for `sagechat-app-watchos` on October 29, 2026
- **THEN** its file is `wiffletree/second-brain/logs/sagechat-app-watchos/2026/10/2026-10-29-<id>.md`, and it meets the writer's contract test

#### Scenario: Over the cap
- **WHEN** an agent's entry would render to 2,450 characters of body
- **THEN** the call is refused, saying to cut at least 450 characters, and nothing is stored

### Requirement: Milestones prompt one log entry
When an implementer's `report` of kind ready_for_testing, or a successful `accept_ticket` or `publish_ticket`, concerns a ticket whose calling session has made no `log_work` call for that ticket since its previous prompted milestone, the tool result SHALL end with one line asking for one terse `log_work` call for this milestone. Each milestone SHALL be prompted at most once.

#### Scenario: Ready for testing
- **WHEN** an implementer reports ready_for_testing without having logged for its ticket
- **THEN** the report's result ends with the line asking for one `log_work` call

#### Scenario: Already logged
- **WHEN** the implementer called `log_work` for the ticket earlier in the same turn and then reports ready_for_testing
- **THEN** no prompt is added

### Requirement: Agents are told how to log
The coordinator's and implementer's fixed core instructions SHALL say to log with `log_work` at meaningful milestones (a decision, a finished assignment, a handoff, a blocker), in one terse entry each, and that an entry lands within seconds. They SHALL NOT mention batches or pull requests for entries. They SHALL forbid transcripts, step-by-step narration and pasted output, and SHALL forbid child personal information (names, message text, phone numbers, locations), credentials and secrets.

#### Scenario: Bundled instructions
- **WHEN** the host builds the implementer's instructions
- **THEN** they name `log_work`, say when to use it, say that an entry lands straight in, and state the terseness and sensitive-information rules

### Requirement: Existing captured logs carry over
On upgrade, every stored `work_logs` row SHALL be kept and gain the new fields. A row with an export receipt SHALL be marked as in a legacy file and SHALL NOT be written again. A row never exported, or in a batch never pushed, SHALL land in its project's Space's folder. A row that breaks the entry contract or the cap as it stands SHALL be held, its text unchanged, and SHALL NOT be sent; the project's coordinator SHALL get one Inbox item naming how many rows it holds, and the Memory panel SHALL name the rule each held row breaks. The human SHALL either fix a held row, which is then checked again and lands once it fits, or release it, which keeps it in the store and never sends it. The Inbox item SHALL clear once no row of the project stays held. The host's export command, the export receipts and the maintenance runs SHALL be removed, and the Memory panel SHALL offer no export.

#### Scenario: An unexported row
- **WHEN** a project in Vyttle has one row that was never exported
- **THEN** after the upgrade that row lands in Vyttle's folder, and nothing is written to `logs/wiffletree.md`

#### Scenario: A held row is fixed
- **WHEN** a carried-over row is held for its ref and the human edits it in the Memory panel so that it fits
- **THEN** the row is no longer held, it lands, and the project's Inbox item clears when no other row is held

#### Scenario: A held row is released
- **WHEN** the human releases a held row
- **THEN** the row stays in the project's memory, is never sent, and the project's Inbox item clears when no other row is held

### Requirement: Entries land straight in
A captured entry SHALL be written to the clone and SHALL go through the logs rule set of the repository's sync engine at once: scanned on this Mac, then pushed to `main` 2 seconds after the last change, in commits that only add entry files. There SHALL be no batches, no waiting for a ticket to close and no Send now. A local repository SHALL commit it at once. An entry that fails the scan SHALL be held (see `brain-registry`, "A held entry can be edited, marked or discarded"), and every other entry SHALL keep landing. An entry waiting because GitHub cannot be reached SHALL stay on this Mac and land at the first pass that does.

#### Scenario: One entry, seconds later
- **WHEN** an implementer logs a decision and nothing else happens
- **THEN** one commit on `main` adds that file within seconds, and a teammate's Mac has it within a minute

#### Scenario: A burst of entries
- **WHEN** an implementer logs three entries in one second
- **THEN** one commit adds the three files

#### Scenario: A held entry among others
- **WHEN** one of three entries fails `sensitive-info`
- **THEN** two land, and the third stays on this Mac with its card

#### Scenario: A deleted thread
- **WHEN** an assistant thread logs two entries and the user deletes the thread
- **THEN** both entries have already landed, and nothing else happens to them
