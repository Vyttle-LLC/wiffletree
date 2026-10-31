## MODIFIED Requirements

<!-- Delta order for these requirements: 1 brain-log-capture (archived), which adds them for the coordinator and implementers → 2 assistant-spaces-jobs-boards (this), which gives assistant threads log_work (moved from brain-log-capture task 2.4). Each restates brain-log-capture's version with the thread's additions. -->

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
- A coordinator MAY name one of its project's repositories; without one, the entry SHALL be a planning entry in the folder named after the brain repository.
- An assistant thread's entries SHALL go in the `assistant` folder, and a `repository` or `ticket_id` it names SHALL be refused.

The tool SHALL refuse, storing nothing, when the caller's Space has no brain or when gh reports no GitHub login. The human's capture form in the Memory panel SHALL use the same path as the project coordinator.

#### Scenario: An implementer logs
- **WHEN** the implementer of a `sagechat-call-watch-relay` ticket calls `log_work` with a decision, title and the three fields
- **THEN** the host stores one entry for `sagechat-call-watch-relay` in `sagehaven-docs`'s open batch for that ticket, and returns its id and file path

#### Scenario: No brain
- **WHEN** an implementer in a project whose Space names no brain calls `log_work`
- **THEN** the call is refused, saying the project's Space has no brain, and nothing is stored

#### Scenario: An assistant thread logs
- **WHEN** an assistant thread in the Vyttle Space calls `log_work`
- **THEN** the host stores one entry in `vyttle-docs` at `second-brain/logs/assistant/YYYY/MM/YYYY-MM-DD-<id>.md` with `repo: assistant`, and records the thread's id and Space on the stored entry

#### Scenario: A reviewer has no tool
- **WHEN** the host builds a reviewer's tool list
- **THEN** `log_work` is not in it

### Requirement: The host fills in what the agent cannot know
The host SHALL mint the entry's id as a lowercase ULID and take its date from the host's local date. It SHALL set `author` to the GitHub login gh reports on this machine and `source` to `wiffletree`. It SHALL compute `ref` from the ticket's worktree: `<branch> @ <sha>` for one commit since the ticket's base, `<branch> @ <first>…<last>` for several, `<branch> @ —` when there is no commit yet, and `—` for a coordinator entry with no ticket or an assistant thread's entry, appending ` · PR #NN` when the ticket has a known PR. SHAs SHALL be 7 lowercase hex characters. The tool SHALL NOT accept `ref`, `author`, `source`, `id` or a date from the agent. The host SHALL record the provider, model, machine, Space, project, ticket, session and, for a thread, the thread's id on the stored entry.

#### Scenario: A branch with commits and a PR
- **WHEN** a ticket's branch `wiffletree/retry-telnyx-timeout` has two commits since its base and PR #92 is open
- **THEN** the entry's `ref` is `wiffletree/retry-telnyx-timeout @ 3c1e9a0…7d03b1e · PR #92`

#### Scenario: Uncommitted work
- **WHEN** an implementer logs before its first commit
- **THEN** the entry's `ref` is `<branch> @ —`

### Requirement: Entries follow the per-entry file contract
Each entry SHALL be rendered as one file at `<brain folder>/logs/<repo>/YYYY/MM/YYYY-MM-DD-<id>.md`, in exactly the gate's entry contract: frontmatter with exactly `id`, `repo`, `author` and `source`, then the `## YYYY-MM-DD — Title` heading and the five fields `type`, `ref`, `changed`, `why` and `state` in that order. `<repo>` SHALL be the GitHub repository's name, or `assistant` for an assistant thread's entry; names the contract forbids SHALL be refused. No assistant thread SHALL write the rolling file `logs/assistant.md`. The host SHALL refuse an entry whose rendered body exceeds 2,000 characters, saying how many characters to cut, and SHALL NOT truncate an agent's entry. A merged entry file SHALL never be changed by the host.

#### Scenario: Rendering
- **WHEN** an entry is stored for `sagechat-app-watchos` on October 29, 2026
- **THEN** its file is `second-brain/logs/sagechat-app-watchos/2026/10/2026-10-29-<id>.md`, and the gate's `log-schema` check passes on it

#### Scenario: Over the cap
- **WHEN** an agent's entry would render to 2,450 characters of body
- **THEN** the call is refused, saying to cut at least 450 characters, and nothing is stored

### Requirement: Entries reach the brain in batches
The host SHALL collect entries into batches: an implementer's entries per brain and ticket, the coordinator's and the human's entries per brain and coordinator session, whichever ticket they name, and an assistant thread's entries per brain and thread. A batch SHALL close and go to the brain's batch flow (`brain-registry`) when its ticket is accepted or closed, when its session is archived or its thread deleted, 24 hours after its first entry, or when the human presses Send now. A batch SHALL hold at most 50 entries. An entry captured after its batch closed, or beyond 50, SHALL start a new batch. A batch's PR SHALL only add entry files.

#### Scenario: One PR per ticket
- **WHEN** an implementer logs three entries and the coordinator then accepts the ticket
- **THEN** one batch with the three entries opens one PR that adds three files

#### Scenario: A quiet coordinator
- **WHEN** a coordinator logs one planning entry and nothing else happens
- **THEN** its batch opens its PR 24 hours after that entry

#### Scenario: A deleted thread
- **WHEN** an assistant thread logs two entries and the user deletes the thread
- **THEN** its batch closes and opens one PR that adds the two files
