## MODIFIED Requirements

### Requirement: Agents log work through the log_work tool
The host SHALL offer a `log_work` MCP tool to the project coordinator, implementers and the Assistant, and SHALL NOT offer it to testers, reviewers, the PR keeper, issue threads or job runs. The tool SHALL take `type` (`decision` or `observation`), `title` (one line of at most 80 characters), `changed`, `why` and `state` (each folded to one line), and optionally `repository` and `milestone`. The coordinator MAY also give `ticket_id`, one of its project's tickets, which makes the entry that ticket's: its repository's folder and its `ref`.

The entry's brain SHALL be the brain of the caller's Space, the project's Space for a coordinator or implementer (`brain-registry`, "A project's brain comes from its Space"), never chosen by them. The Assistant, which serves every Space, SHALL name one with `space`, and SHALL be refused a Space this Mac does not have or, in a turn where the human picked a Space in the composer, any other Space (see assistant, "The Assistant infers each request's Space"). Its folder SHALL be chosen as follows:
- An implementer's entry SHALL be for its ticket's repository, and a `repository` it names SHALL be refused.
- A coordinator MAY name one of its project's repositories; without one, the entry SHALL be a planning entry in the folder `planning`.
- The Assistant's entries SHALL go in the `assistant` folder, and a `repository` or `ticket_id` it names SHALL be refused.

For a Space on GitHub the tool SHALL refuse, storing nothing, when gh reports no GitHub login. For a Space on this Mac alone it SHALL not need one. The human's capture form in the Memory panel SHALL use the same path as the project coordinator.

#### Scenario: An implementer logs
- **WHEN** the implementer of a `sagechat-call-watch-relay` ticket calls `log_work` with a decision, title and the three fields
- **THEN** the host stores one entry for `sagechat-call-watch-relay` in Sage's folder, sends it to the sync engine at once, and returns its id and file path

#### Scenario: A Space on this Mac alone
- **WHEN** an implementer in a project of Personal, which named no brain in an earlier release, calls `log_work` with `gh` signed out
- **THEN** the entry is committed in Personal's local repository, with this Mac's account name as `author`

#### Scenario: An assistant thread logs
- **WHEN** the Assistant calls `log_work` with `space` Vyttle
- **THEN** the host stores one entry in Vyttle's brain at `wiffletree/second-brain/logs/assistant/YYYY/MM/YYYY-MM-DD-<id>.md` with `repo: assistant`, and records the Assistant's session id and Vyttle on the stored entry

#### Scenario: No Space named
- **WHEN** the Assistant calls `log_work` without `space`
- **THEN** it is refused, saying to name the Space, and nothing is stored

#### Scenario: An issue thread has no tool
- **WHEN** the host builds an issue thread's tool list
- **THEN** `log_work` is not in it

#### Scenario: A reviewer has no tool
- **WHEN** the host builds a reviewer's tool list
- **THEN** `log_work` is not in it
