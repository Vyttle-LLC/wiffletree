Four stages, each one reviewable ticket, stacked on a feature branch that the human merges into `main`. Order: 1 → 2 → 3 → 4. Stage 3 may start once stage 1 has merged into the feature branch, beside stage 2, and merges after it. Task 1.1 is a prototype that reports before the rest of stage 1 builds on it.

## 1. Host: the Assistant session, work and tools

- [ ] 1.1 Prototype the managed-path sandbox (design §1): a Seatbelt profile built from the host's records, around Claude with `--dangerously-skip-permissions` and Codex with `--dangerously-bypass-approvals-and-sandbox`. Ignored real-provider tests: a shell write, an Edit/Write tool write, a link and a `git commit` into a managed clone, a worktree and the data folder are refused; writes to `~/Downloads` and the Assistant's folder succeed; a nested `claude -p` and a background process inherit it; a LaunchAgent write is refused. Record the macOS and provider versions. Report before 1.4 builds on it.
- [ ] 1.2 Core: `SessionOwner::Install` and the Assistant role; the work item record on a project (Assistant work, small flag, state Running/Paused/Stopped/Done); `ask_human` decision records; loop change records for Undo.
- [ ] 1.3 Host: `start_work` beside `create_project_from` (atomic project, coordinator with the Assistant as parent, brief as a Wiffletree message naming the Assistant, small-flag text); hierarchy checks; `create_project` refused for Assistant-parented coordinators.
- [ ] 1.4 Host: the Assistant's launch: install-owned session in `<data folder>/assistant/workspace/`, YOLO inside the 1.1 profile rebuilt each turn, Claude deny rules for managed paths, every Space's listed MCP servers with only their tools; refuse the turn when the profile cannot apply.
- [ ] 1.5 Host: reports, `ask_user` (as a message, no attention item) and check-ins of Assistant-parented coordinators go to the Assistant; the "Human" check-in rule only for parentless coordinators; the Assistant is told of each approval the host creates for its work. Verify a publish approval for a disallowed owner still lands in the Inbox.
- [ ] 1.6 Host: work tools `message_work`, `pause_work`, `resume_work`, `stop_work`, `list_work`, limited to the Assistant's own work; `ask_human`; the routing tag on the human message a `message_work` call follows, and the Change command.
- [ ] 1.7 Host: loop tools over every Space's loops with the human-turn rule, read-only command checks at save, Undo; issue tools `list_issues`, `read_issue`, `file_issue`, `edit_issue`, `comment_issue` with `assistant_for`; `save_output`, `query_brain`, `log_work` taking a Space; the composer's hand-picked Space guard. Verify a turn woken only by a report cannot create a loop.

- [ ] 1.8 Core and host: sources gain act lists and "Do without asking" marks, old lists reading as read lists; the Assistant's provider offered read tools only; `read_source` and `act` run through the host (the host as MCP client, CLIs under the shell-text rule); the sandbox refuses exec of listed CLIs; action and done cards with Run and Decline; turns held to cards (not started by the human, or after a source read, issue read, web search or fetch, or report). Verify the email-forwarding, invite and report scenarios, and that a loop never receives an act tool.

## 2. One conversation: remove the router, threads and tabs

- [ ] 2.1 Host: the human's messages go to the one conversation, first in a turn's input; delete `router.rs`, `provider/router.rs`, the router's core types and `Job::Router`.
- [ ] 2.2 Host: migration: non-issue threads become read-only history (searchable, deletable, never resumed); a held router message becomes an unsent draft; `project_brief` and `propose_job` leave every tool list; issue threads keep their tools and restricted launch otherwise unchanged.
- [ ] 2.3 Models and settings: drop the Router row at load and save; ignore a Router row in `space.yaml` under the Brain compiler rule's banner; stop reading the router threshold and keep it in `settings/assistant.json`; the conversation resolves this Mac's Assistant row only.
- [ ] 2.4 Host: the Assistant core v5 (tools, managed-path rule and what the sandbox cannot stop, human-turn rule for loops, reports as context) and every Space's additions under their headings; the Coordinating core's paragraph for Assistant-parented coordinators.

## 3. Desktop: the chat, cards and rail

- [ ] 3.1 `assistant_view`: one chat with no tabs; composer chips Space · auto and Goes to · auto; the "Can act on this Mac · repositories change through work" line; long-turn note with Stop; History.
- [ ] 3.2 Cards: work (title, Space, stage, PRs, Stop, Open details), decision (options, recommendation, free answer), approval (settles the Inbox item), loop (what changed, schedule in words, sources, Undo, Edit, Run now), issue written; the routing tag with Change.
- [ ] 3.3 The rail: Needs you, Running, Loops, Done recently; the Loops page replacing the Jobs list.
- [ ] 3.4 Sidebar: hide Assistant work from Projects and Space sections; the assistant row's working mark and Needs you count; Open details opens the project view with Back to chat; Inbox selection of Assistant work opens it from the Assistant.
- [ ] 3.5 Action cards (target, exact content, what others' text the turn read, Run, Decline, result) and done cards; Add source on the Space's page (configured MCP servers and CLIs, tools with suggested read or act kinds, Do without asking, the server's own sign-in instructions and Try again).
- [ ] 3.6 Models page without the Router row; Settings without the router threshold.

## 4. Skill and docs

- [ ] 4.1 Assisting v3 (`skills/stock/assisting.md`): when to answer, act, start work or take the fast path; when to ask the human versus answer a coordinator; how to phrase decision cards; keeping turns short.
- [ ] 4.2 Docs: rewrite `docs/FEATURES.md` "Assistant, recurring jobs and Boards" for the shipped behaviour (one conversation, work, loops, the sandbox and its gaps), `docs/DEVELOPMENT.md` on the Assistant's launch, the README's Assistant bullets in the present tense, and refresh `docs/images/assistant.png`.
- [ ] 4.3 At archive: sync the deltas and delete `openspec/specs/assistant-router`, whose every requirement this change removes.
