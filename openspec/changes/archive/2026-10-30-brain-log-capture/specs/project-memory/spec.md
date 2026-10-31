## MODIFIED Requirements

### Requirement: Stable scoped milestones
Logs SHALL carry project, ticket, session, milestone identity, type, title, reference, changed, why and state, and the host SHALL also record the entry's id, Space, brain, repository folder, author, provider, model and machine. Repeated identical capture under one session and milestone identity SHALL return the original record, and changed content under the same identity SHALL fail. A capture without a milestone identity SHALL always create a new entry. Retrieval SHALL NOT expose another project's entries through a project's own views; the brain browser shows a brain's entries only within the scope rules of `brain-browser`.

#### Scenario: Same day milestones
- **WHEN** a session captures two milestones on one day and repeats the first
- **THEN** both distinct entries remain once, with distinct ids, and neither is skipped

#### Scenario: Changed content under one milestone
- **WHEN** a session repeats a milestone identity with a different `changed` text
- **THEN** the capture is refused and the first entry is unchanged

## REMOVED Requirements

<!-- Delta order for "Safe Markdown export": 1 spaces (exports to the Space's brain) → 2 brain-log-capture (this), which removes it. -->

### Requirement: Safe Markdown export
**Reason**: Writing newest-first into shared `logs/<repo>.md` files conflicts between machines and people. Brains are now team-shared and change only through PRs, so the host writes one new file per entry and never edits an existing file.
**Migration**: The requirements "Entries follow the per-entry file contract" and "Entries reach the brain in batches" replace it, in the version `spaces` left, which exports to the project's Space's brain. Rows already exported to a legacy file are brought over by the legacy import, and `log_exports` is dropped after migrating.

### Requirement: Inspectable maintenance
**Reason**: The export it describes is removed, and with it the maintenance runs. Compilation becomes scheduled compile runs on the brain's compiler host, specified in the `brain-compile` change.
**Migration**: `maintenance_runs` is dropped after migrating. `brain-compile` specifies compile runs, their history and the rule that a run with nothing pending makes no model call.

## ADDED Requirements

### Requirement: Agents log work through the log_work tool
The host SHALL offer a `log_work` MCP tool to the project coordinator and implementers, and SHALL NOT offer it to testers, reviewers, the PR keeper or job runs. The tool SHALL take `type` (`decision` or `observation`), `title` (one line of at most 80 characters), `changed`, `why` and `state` (each folded to one line), and optionally `repository` and `milestone`. The coordinator MAY also give `ticket_id`, one of its project's tickets, which makes the entry that ticket's: its repository's folder and its `ref`.

The entry's brain SHALL be the brain of the caller's Space: the project's Space for a coordinator or implementer (`brain-registry`, "A project's brain comes from its Space"). Its folder SHALL be chosen as follows:
- An implementer's entry SHALL be for its ticket's repository, and a `repository` it names SHALL be refused.
- A coordinator MAY name one of its project's repositories; without one, the entry SHALL be a planning entry in the folder named after the brain repository.

The tool SHALL refuse, storing nothing, when the caller's Space has no brain or when gh reports no GitHub login. The human's capture form in the Memory panel SHALL use the same path as the project coordinator.

#### Scenario: An implementer logs
- **WHEN** the implementer of a `sagechat-call-watch-relay` ticket calls `log_work` with a decision, title and the three fields
- **THEN** the host stores one entry for `sagechat-call-watch-relay` in `sagehaven-docs`'s open batch for that ticket, and returns its id and file path

#### Scenario: No brain
- **WHEN** an implementer in a project whose Space names no brain calls `log_work`
- **THEN** the call is refused, saying the project's Space has no brain, and nothing is stored

#### Scenario: A reviewer has no tool
- **WHEN** the host builds a reviewer's tool list
- **THEN** `log_work` is not in it

### Requirement: The host fills in what the agent cannot know
The host SHALL mint the entry's id as a lowercase ULID and take its date from the host's local date. It SHALL set `author` to the GitHub login gh reports on this machine and `source` to `wiffletree`. It SHALL compute `ref` from the ticket's worktree: `<branch> @ <sha>` for one commit since the ticket's base, `<branch> @ <first>…<last>` for several, `<branch> @ —` when there is no commit yet, and `—` for a coordinator entry with no ticket, appending ` · PR #NN` when the ticket has a known PR. SHAs SHALL be 7 lowercase hex characters. The tool SHALL NOT accept `ref`, `author`, `source`, `id` or a date from the agent. The host SHALL record the provider, model, machine, Space, project, ticket and session on the stored entry.

#### Scenario: A branch with commits and a PR
- **WHEN** a ticket's branch `wiffletree/retry-telnyx-timeout` has two commits since its base and PR #92 is open
- **THEN** the entry's `ref` is `wiffletree/retry-telnyx-timeout @ 3c1e9a0…7d03b1e · PR #92`

#### Scenario: Uncommitted work
- **WHEN** an implementer logs before its first commit
- **THEN** the entry's `ref` is `<branch> @ —`

### Requirement: Entries follow the per-entry file contract
Each entry SHALL be rendered as one file at `<brain folder>/logs/<repo>/YYYY/MM/YYYY-MM-DD-<id>.md`, in exactly the gate's entry contract: frontmatter with exactly `id`, `repo`, `author` and `source`, then the `## YYYY-MM-DD — Title` heading and the five fields `type`, `ref`, `changed`, `why` and `state` in that order. `<repo>` SHALL be the GitHub repository's name; names the contract forbids SHALL be refused. The host SHALL refuse an entry whose rendered body exceeds 2,000 characters, saying how many characters to cut, and SHALL NOT truncate an agent's entry. A merged entry file SHALL never be changed by the host.

#### Scenario: Rendering
- **WHEN** an entry is stored for `sagechat-app-watchos` on October 29, 2026
- **THEN** its file is `second-brain/logs/sagechat-app-watchos/2026/10/2026-10-29-<id>.md`, and the gate's `log-schema` check passes on it

#### Scenario: Over the cap
- **WHEN** an agent's entry would render to 2,450 characters of body
- **THEN** the call is refused, saying to cut at least 450 characters, and nothing is stored

### Requirement: Entries reach the brain in batches
The host SHALL collect entries into batches: an implementer's entries per brain and ticket, and the coordinator's and the human's entries per brain and coordinator session, whichever ticket they name. A batch SHALL close and go to the brain's batch flow (`brain-registry`) when its ticket is accepted or closed, when its session is archived, 24 hours after its first entry, or when the human presses Send now. A batch SHALL hold at most 50 entries. An entry captured after its batch closed, or beyond 50, SHALL start a new batch. A batch's PR SHALL only add entry files.

#### Scenario: One PR per ticket
- **WHEN** an implementer logs three entries and the coordinator then accepts the ticket
- **THEN** one batch with the three entries opens one PR that adds three files

#### Scenario: A quiet coordinator
- **WHEN** a coordinator logs one planning entry and nothing else happens
- **THEN** its batch opens its PR 24 hours after that entry

### Requirement: Milestones prompt one log entry
When an implementer's `report` of kind ready_for_testing, or a successful `accept_ticket` or `publish_ticket`, concerns a ticket whose project's Space has a brain, and the calling session has made no `log_work` call for that ticket since its previous prompted milestone, the tool result SHALL end with one line asking for one terse `log_work` call for this milestone. Each milestone SHALL be prompted at most once, and no prompt SHALL be added when no brain applies.

#### Scenario: Ready for testing
- **WHEN** an implementer reports ready_for_testing without having logged for its ticket
- **THEN** the report's result ends with the line asking for one `log_work` call

#### Scenario: Already logged
- **WHEN** the implementer called `log_work` for the ticket earlier in the same turn and then reports ready_for_testing
- **THEN** no prompt is added

### Requirement: Agents are told how to log
The coordinator's and implementer's fixed core instructions SHALL say to log with `log_work` at meaningful milestones (a decision, a finished assignment, a handoff, a blocker), in one terse entry each. They SHALL forbid transcripts, step-by-step narration and pasted output, and SHALL forbid child personal information (names, message text, phone numbers, locations), credentials and secrets.

#### Scenario: Bundled instructions
- **WHEN** the host builds the implementer's instructions
- **THEN** they name `log_work`, say when to use it, and state the terseness and sensitive-information rules

### Requirement: Legacy logs are imported once
For each brain whose `main` holds legacy `logs/<repo>.md` files, the Brains page SHALL offer Import legacy logs until the import is recorded as done. The import SHALL write one entry file per legacy entry in log-only PRs of at most 200 files, with `source: skill`. Each id SHALL be derived deterministically from the repository, date, title and body, so that a repeated import adds no file. The author SHALL be the GitHub login of the commit that added the entry's heading, or the importing human's login when none is known. An entry over the cap SHALL keep its heading and fields, with `changed` shortened to fit and ending with a pointer to the legacy file and commit. A `ref` that does not fit the contract SHALL become `—`, with its text moved into `state`. Entries at or before a file's `compiled-through` marker SHALL be recorded as already compiled. The import SHALL NOT change or delete legacy files.

#### Scenario: Repeated import
- **WHEN** the import's PRs merge and the human imports again
- **THEN** no file is added and the action is no longer offered

#### Scenario: An oversized transcript
- **WHEN** a legacy `simpatico-sage` entry holds 18,000 characters
- **THEN** its entry file fits the cap, keeps its heading and fields, and points to the legacy file at the imported commit

### Requirement: Existing captured logs carry over
On upgrade, every stored `work_logs` row SHALL be kept and gain the new fields. A row with an export receipt SHALL be marked as in a legacy file and SHALL NOT be written again. A row never exported SHALL be queued into a batch when its project's Space has a brain, and otherwise SHALL be listed as unsent in the project's Memory panel until a brain applies. A queued row that breaks the entry contract or the cap as it stands SHALL be held, its text unchanged, and SHALL NOT be sent; the project's coordinator SHALL get one Inbox item naming how many rows it holds, and the Memory panel SHALL name the rule each held row breaks. The human SHALL either fix a held row, which is then checked again and sent with its batch once it fits, or release it, which keeps it in the store and never sends it. The Inbox item SHALL clear once no row of the project stays held. The host's export command, the export receipts and the maintenance runs SHALL be removed, and the Memory panel SHALL offer no export.

#### Scenario: An unexported row
- **WHEN** a project has one row that was never exported and its Space names `vyttle-docs`
- **THEN** after the upgrade that row is in an open batch for `vyttle-docs`, and nothing is written to `logs/wiffletree.md`

#### Scenario: A held row is fixed
- **WHEN** a carried-over row is held for its ref and the human edits it in the Memory panel so that it fits
- **THEN** the row is no longer held, it is sent with its batch, and the project's Inbox item clears when no other row is held

#### Scenario: A held row is released
- **WHEN** the human releases a held row
- **THEN** the row stays in the project's memory, is never sent, and the project's Inbox item clears when no other row is held
