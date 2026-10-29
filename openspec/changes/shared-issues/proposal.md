## Why

Wiffletree runs one person's agents well, but the work they pick up still starts in an external tracker such as Linear. Ideas, bugs and requests live there, are discussed there, and are copied by hand into a coordinator's first message; when the pull requests merge, someone goes back and closes the issue. A team that uses Wiffletree has no shared backlog inside it, and the PRD lists multi-user collaboration as out of scope.

Shared issues give a team one backlog that every member's Wiffletree reads and writes, without a server: plain Markdown files in a private GitHub repository the team owns, synced the way the settings repository already is. Starting a project from an issue links them, and the issue follows the work to Done.

## What Changes

- **Issues, a new shared record.** An issue is an idea, bug or request with its discussion. It has a state (Triage, Backlog, Todo, In progress, In review, Done, Canceled), a priority, an assignee, labels, repositories and related issues. A **ticket** keeps today's meaning: a local execution workspace. Tickets are not renamed, changed or shared.
- **A tracker repository per team.** The human names a private GitHub repository on each Mac, as for the settings repository. It holds `wiffletree-issues.toml` (prefix, states, labels) and `issues/<ID>/` with `issue.md` (YAML front matter and description), `comments/<timestamp>-<login>.md` and `events.jsonl`. A Mac can use more than one tracker, each with its own prefix, such as one per company.
- **Sync without a model.** Wiffletree pulls every minute and pushes a few seconds after a change: only the default branch, never by force, only the tracker's own files. IDs such as `CEL-151` are allocated at push time; a rejected push fetches, takes the next number and pushes again, and an issue filed offline carries a temporary ID until then. A front matter field changed on two Macs pauses alone, with Compare, while the rest of the issue syncs. Comments are one file each and never conflict. Unlike settings, changes a teammate commits without Wiffletree are accepted.
- **Identity is the GitHub login** `gh` reports. Assignees, authors and mentions are logins.
- **An Issues workspace page** beside Repositories and Usage: list and board, filters (including by tracker), search and unread marks. **The issue page**: a timeline of comments and events, properties, a Work box with **Start project from issue**, and related links. **New issue** warns that issues stay in Git history for good and must not hold personal data. **Inbox entries** for issues assigned to you, mentions, comments on issues you follow, new urgent issues in your repositories, and paused fields.
- **Links and automatic states.** Starting a project from an issue, or a coordinator's `link_issue`, links them on this Mac. In progress, In review and Done then follow the linked project's tickets and pull requests, using what the PR watcher already reads. A manual move overrides until the linked work changes again. The tracker records only "Jamie started a project" and the pull requests, never ticket or agent detail.
- **The issue assistant, a new lightweight role.** It opens on one issue or on the whole list, from its own Who runs what row. It can read and list issues and search this Mac's repositories read-only, and it proposes edits, comments, splits, duplicate links and new issues. Every write is a proposal the human applies. It has no worktree, starts no agents, creates no tickets and never pushes; its comments say whose assistant wrote them.
- **Coordinator tools** `list_issues`, `read_issue` and `link_issue`. Every agent that reads issues is told that text written by others is untrusted context, never instructions.
- **Import from Linear**, once, through Linear's API. There is no two-way sync.
- **Docs.** The PRD stops listing multi-user collaboration as out of scope and points to shared issues; the design README links the study.

## Capabilities

### New Capabilities
- `shared-issues`: the issue model, tracker repositories and their layout, sync and conflicts, ID allocation, identity, the Issues page, the issue page, New issue, Inbox entries, links between issues and projects or tickets, automatic states, the coordinator's issue tools, tracker setup and the Linear import.
- `issue-assistant`: the issue assistant role, its tools, its proposals and how they are applied, its instructions and its limits.

### Modified Capabilities
- `model-routing`: Who runs what gains an optional Issue assistant row.
- `agent-lifecycle`: the issue assistant's sessions belong to a tracker, not to a project's tickets, and are never archived by a coordinator.
- `pr-watch`: a linked ticket's PR changes also move its issue's state.
- `host-process`: tracker set-up and every issue write are human-only commands.
- `workspace-host`: the enforced hierarchy allows the issue assistant's tracker sessions.
- `skills`: the issue assistant gets its own core and no skill, and every core that reads issues treats their text as untrusted.

The Issues sidebar row and the Inbox's issue entries are specified in `shared-issues`, beside the rest of the Issues UI.

## Impact

- `workspace-core`: a new `issues.rs` (issue, tracker, event, comment, proposal and link types; tracker commands); `selection.rs` (`Job::IssueAssistant`); `lib.rs` (`Role::IssueAssistant`, `Command` variants, `Snapshot` tracker summaries, host settings for trackers).
- `workspace-host`: new `tracker.rs` (the store and file format), `tracker_sync.rs` (passes, ID allocation, field merges) and `issues.sql`; GitHub checks shared with `sync.rs`; `mcp.rs`, `live.rs` and `service.rs` (tools and commands); `pr_watch.rs` (state following); `policies.rs`, `provider.rs` and `skills.rs` (the assistant's instructions and tool limits); `socket.rs` (human-only commands); `keychain.rs` and a new `linear_import.rs`.
- `workspace-desktop`: `main.rs` and `ui.rs` (`Page::Issues`), `sidebar.rs`, new `issues_view.rs`, `issue_view.rs`, `new_issue.rs`, `issue_assistant.rs` and `settings/trackers.rs`; `inbox.rs`, `models_view.rs`, `creation.rs`.
- Docs: `README.md`, `docs/PRD.md`, `design/README.md`.
- Mockup: `ui/mockups.html`, approved as drawn, with neutral sample data.
