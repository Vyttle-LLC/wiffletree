## Why

Wiffletree runs one person's agents well, but the work they pick up still starts in an external tracker such as Linear. Ideas, bugs and requests live there, are discussed there, and are copied by hand into a coordinator's first message; when the pull requests merge, someone goes back and closes the issue. A team that uses Wiffletree has no shared backlog inside it, and the PRD lists multi-user collaboration as out of scope.

Shared issues give a team one backlog that every member's Wiffletree reads and writes, without a server: plain Markdown files in a private GitHub repository the team owns, synced the way the settings repository already is. Starting a project from an issue links them, and the issue follows the work to Done.

## What Changes

- **Builds on `spaces`** (decided by Michael, October 29, 2026). The tracker is the Space's storage repository, issues sync on that repository's one engine, and the Space decides where an issue goes and where a project started from it lives.
- **Issues, a new shared record.** An issue is an idea, bug or request with its discussion. It has a state (Triage, Backlog, Todo, In progress, In review, Done, Canceled), a priority, an assignee, labels, repositories and related issues. A **ticket** keeps today's meaning: a local execution workspace. Tickets are not renamed, changed or shared.
- **The tracker is the Space's storage repository.** It holds `wiffletree-issues.yaml` (prefix, states, labels) and `issues/<ID>/` with `issue.md` (YAML front matter and description), `comments/<timestamp>-<login>.md` and `events.jsonl`, beside `spaces/`. Spaces sharing a storage repository share its issues and prefix; a personal Space's local storage is a private, local tracker. There is no per-Mac tracker list and no Settings › Trackers: the Space page shows the tracker.
- **Sync without a model.** Issues are their own rule set on the repository's sync engine: pull every minute and push a few seconds after a change, only the default branch, never by force, only the issue files, after the local personal-data and secret scans. IDs such as `CEL-151` are allocated at push time; a rejected push fetches, takes the next number and pushes again, and an issue filed offline carries a temporary ID until then. A front matter field changed on two Macs pauses alone, with Compare, while the rest of the issue syncs. Comments are one file each and never conflict. Unlike settings, changes a teammate commits without Wiffletree are accepted.
- **Identity is the GitHub login** `gh` reports. Assignees, authors and mentions are logins.
- **An Issues workspace page** beside Repositories and Usage: list and board, filters (including by Space), search and unread marks. **The issue page**: a timeline of comments and events, properties, a Work box with **Start project from issue**, and related links. **New issue** defaults to the current Space and warns that issues stay in Git history for good and must not hold personal data. **Inbox entries** for issues assigned to you, mentions, comments on issues you follow, new urgent issues in your repositories, and paused fields.
- **Links and automatic states.** Starting a project from an issue, in the issue's Space with the issue's repositories, or a coordinator's `link_issue`, links them on this Mac. In progress, In review and Done then follow the linked project's tickets and pull requests, using what the PR watcher already reads. A manual move overrides until the linked work changes again. The tracker records only "Jamie started a project" and the pull requests, never ticket or agent detail.
- **Issue threads instead of an issue assistant** (decided by Michael, October 29, 2026). Asking the assistant about an issue opens an issue thread of the one assistant (`assistant-spaces-jobs-boards`), with the issue tools, web off and proposals the human applies. This change no longer adds an issue assistant role, a hidden tracker project or an Issue assistant row.
- **Coordinator tools** `list_issues`, `read_issue` and `link_issue`, over the project's Space's tracker. Every agent that reads issues is told that text written by others is untrusted context, never instructions.
- **Import from Linear**, once, through Linear's API. There is no two-way sync.
- **Docs.** The PRD stops listing multi-user collaboration as out of scope and points to shared issues; the design README links the study.

## Capabilities

### New Capabilities
- `shared-issues`: the issue model, the tracker in a Space's storage repository and its layout, the issue rule set, conflicts, ID allocation, identity, an issue's Space, the Issues page, the issue page, New issue, Inbox entries, links between issues and projects or tickets, automatic states, the coordinator's issue tools and the Linear import.

### Modified Capabilities
- `pr-watch`: a linked ticket's PR changes also move its issue's state.
- `host-process`: every issue write and applying a proposal are human-only commands. Delta order: 1 `spaces` → 2 (this).

The Issues sidebar row and the Inbox's issue entries are specified in `shared-issues`, beside the rest of the Issues UI. Issue threads, proposals and their cards are specified in `assistant-spaces-jobs-boards`' `assistant`.

## Impact

- Depends on `spaces` (storage repositories, the tracker config, `repos.rs`, `github_access.rs`, the scans).
- `workspace-core`: a new `issues.rs` (issue, event, comment and link types; issue commands); `lib.rs` (`Command` variants, `Snapshot` issue summaries per Space).
- `workspace-host`: new `tracker.rs` (the store over a storage clone and the file format), `issues_sync.rs` (the issue rule set on `repos.rs`: per-field merges and ID allocation) and `issues.sql`; `mcp.rs`, `live.rs` and `service.rs` (tools and commands); `pr_watch.rs` (state following); `skills.rs` (the coordinator's untrusted-text rule); `socket.rs` (human-only commands); `keychain.rs` and a new `linear_import.rs`.
- `workspace-desktop`: `main.rs` and `ui.rs` (`Page::Issues`), `sidebar.rs`, new `issues_view.rs`, `issue_view.rs` and `new_issue.rs`; `inbox.rs`, `creation.rs`; the issue count and Import from Linear… in `spaces`' Storage and issues box.
- Docs: `README.md`, `docs/PRD.md`, `design/README.md`.
- Mockup: `ui/mockups.html`, approved as drawn, with neutral sample data. Its tracker set-up study (g) and its issue assistant pane are superseded by `spaces`' Space page and by issue threads.
