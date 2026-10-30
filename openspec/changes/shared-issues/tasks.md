Each numbered phase is one pull request and ships on its own. Every phase needs `spaces`, which provides storage repositories, their tracker config, the shared repository module (`repos.rs`, `github_access.rs`, formerly task 1.2 here) and the scans before a push. A phase leaves nothing visible until its UI exists: phase 1 adds the issue count to the Space page only.

## 1. Model, tracker store and sync for a single user

- [x] 1.1 `workspace-core`: add `issues.rs` with `Issue`, `IssueState`, `Priority`, `Comment`, `IssueEvent`, `PausedIssueFile` and `IssueId` (number or temporary); export it from `lib.rs`. Add `Command` variants `SyncIssues`, `Issues`, `Issue`, `CreateIssue`, `EditIssue`, `Comment`, `MoveIssue`. Add issue summaries (tracker, prefix, login, sync status) per Space to `Snapshot`.
- [x] 1.2 `workspace-host`: new `tracker.rs`, the store over a storage repository's clone (`spaces`): parse and write `wiffletree-issues.yaml`'s states and labels, `issue.md` (fixed field order, unknown fields kept), comment files and `events.jsonl`; atomic writes; record what the host wrote. Verify round trips, unknown fields, a file that does not parse, and the refused states list.
- [x] 1.3 `workspace-host`: new `issues.sql` (per-file sync record as in `sync.sql`, paused parts, temporary IDs, index of issues for list and search, keyed by storage repository) and its migration in `lib.rs`; rebuild the index from files when it is missing.
- [x] 1.4 `workspace-host`: new `issues_sync.rs`, the issue rule set on `repos.rs`: passes at launch, every minute, on Sync now and 2 seconds after a change; push only `wiffletree-issues.yaml` and `issues/`, after the scans; whole-file merge for now; ID allocation with up to 5 retries and temporary ID rewriting; numbering at filing in a local tracker; accepting remote changes from anyone; holding back local edits Wiffletree did not make. Verify against a local bare repository: two clones filing at once, offline filing, a hand-added issue, an outside edit in the clone, a held file, a local tracker, and a Space edit and an issue edit in one pass making two commits.
- [x] 1.5 `workspace-host`: `service.rs` and `socket.rs`: answer the issue commands; make every tracker write human-only. Verify an uncredentialed connection is refused for writes and answered for reads.
- [x] 1.6 `workspace-desktop`: the issue count and Open Issues in `spaces`' Storage and issues box.

## 2. Issues UI

- [x] 2.1 `workspace-desktop`: `Page::Issues` in `main.rs` and `ui.rs`; an Issues row with its new-count before Repositories in `sidebar.rs`, and the footer naming the filtered Space's tracker.
- [x] 2.2 New `issues_view.rs`: list grouped by state and board with drag to move, filters (Space, open, assignee, priority, label, repository), search, unread marks kept on this Mac.
- [x] 2.3 New `issue_view.rs`: description, timeline of comments and events, composer with mentions and issue IDs, properties, Work box (Start project from issue disabled until phase 3), related issues, raw view with warning for a file that does not parse.
- [x] 2.4 New `new_issue.rs`: the form, the Space defaulting to the current Space, temporary ID, and the permanent-history and personal-data warning naming the storage repository, or the stays-on-this-Mac line for a local tracker.
- [x] 2.5 `workspace-host`: search over titles, descriptions and comments from the index; read state per issue in `issues.sql`.
- [x] 2.6 Docs: README section on shared issues and their limits; `docs/DEVELOPMENT.md` notes.

## 3. Links between issues and projects or tickets, and automatic states

- [x] 3.1 `workspace-core`: `IssueLink` (tracker, issue, project, optional ticket) and `Command::StartProjectFromIssue`, `LinkIssue`, `UnlinkIssue`.
- [x] 3.2 `workspace-host`: links in `issues.sql`; the issue's Space; Start project from issue in `service.rs` (create the project and coordinator in the issue's Space with the issue's repositories on this Mac, naming those not here, or the Space's only when the issue lists none, link, first message framing the issue as untrusted context, `work` event, In progress). Verify the Space chosen for a tracker two Spaces share, the repositories, the first message, and that no project or ticket detail reaches the tracker.
- [x] 3.3 `workspace-host`: derived state and manual override in `tracker.rs`; `pr_watch.rs` appends `pr` events and re-derives state when a linked ticket's PR changes; ticket accept and close re-derive too. Verify open, merged, closed without merge, manual override and its release, and Canceled.
- [x] 3.4 `workspace-host`: coordinator tools `list_issues`, `read_issue` and `link_issue` over the project's Space's tracker in `mcp.rs`, `live.rs` and `service.rs`, with authors marked on every passage; the coordinator's core in `skills.rs` (`workspace-core/src/skills`) gains the untrusted-text rule. Verify an implementer is refused and another tracker's issue is refused.
- [x] 3.5 `workspace-desktop`: Work box lists the linked project, tickets and PRs, or who started work on another Mac; `creation.rs` gains Start project from issue with the Space picker limited to the Spaces sharing the tracker.

## 4. Issue threads

Moved to `assistant-spaces-jobs-boards` stage 6 (decided by Michael, 2026-10-29): Ask the assistant opens an issue thread of the one assistant, with the issue tools, web off and proposals applied through phase 1's write commands. This phase has no tasks.

## 5. Multi-user: identity, assignment, Inbox and field-level conflicts

- [x] 5.1 `workspace-host`: read the login from `gh api user` when a Space's storage is chosen or joined and when `gh`'s account changes; refuse writes without one. Assignee candidates from the tracker and, where readable, the repository's collaborators; display names from profiles.
- [x] 5.2 `workspace-host`: three-way merge per front matter field and description in `issues_sync.rs`, union merge of `events.jsonl`, comment pauses; paused parts in `issues.sql`; Keep mine, Use theirs and Compare. Verify two people changing different fields, the same field, the description, and comments at once.
- [x] 5.3 `workspace-host`: follows and Inbox entries (assigned, mentioned, followed comments, urgent in my repositories, automatic moves, paused and outside-edited files), settled once for all projects.
- [x] 5.4 `workspace-desktop`: issue entries under an "Issues" label at the top of every project's Inbox in `inbox.rs`, with Compare and the settle buttons from `workspace_notes.rs`; Assign to me and the assignee picker; Follow on the issue page.

## 6. Linear import

- [ ] 6.1 `workspace-host`: new `linear_import.rs`: the key in `keychain.rs` for the import only, teams, a dry-run summary, state and user mapping, commits of at most 100 issues, Linear identifiers in front matter so a rerun adds only new issues. Verify against recorded API responses; nothing is ever sent to Linear but reads.
- [ ] 6.2 `workspace-desktop`: Import from Linear… in `spaces`' Storage and issues box, with the summary, the user mapping and progress.

## 7. Docs for this change

- [x] 7.1 PRD: shared issues replace "multi-user collaboration" as out of scope, and tickets are said to stay local while issues are shared.
- [x] 7.2 Design README: link the study.
- [x] 7.3 PRD: the tracker is the Space's storage repository, filtered by Space, with issue threads instead of an issue assistant.
