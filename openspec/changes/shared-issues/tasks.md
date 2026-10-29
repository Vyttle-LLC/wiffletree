Each numbered phase is one pull request and ships on its own. A phase leaves nothing visible until its UI exists: phase 1 adds trackers to Settings only.

## 1. Model, tracker store and sync for a single user

- [ ] 1.1 `workspace-core`: add `issues.rs` with `Tracker`, `TrackerConfig` (format, prefix, states, labels), `Issue`, `IssueState`, `Priority`, `Comment`, `IssueEvent`, `PausedIssueFile` and `IssueId` (number or temporary); export it from `lib.rs`. Add tracker choices to `HostSettings` and `Command` variants `CheckTracker`, `SetUpTracker`, `UseTracker`, `RemoveTracker`, `SyncTracker`, `Issues`, `Issue`, `CreateIssue`, `EditIssue`, `Comment`, `MoveIssue`. Add tracker summaries (repository, prefix, login, sync status) to `Snapshot`.
- [ ] 1.2 `workspace-host`: move the GitHub checks `sync.rs` uses (`candidates`, `gh_protocol`, `gh_login`, `first_readable`, `gh_json` and the visibility and readers check) into a shared `github_access.rs`, with `sync.rs` unchanged in behavior. Verify with the existing settings repository tests.
- [ ] 1.3 `workspace-host`: new `tracker.rs`, the store rooted at `<data folder>/trackers/<owner>-<name>/`: parse and write `wiffletree-issues.toml`, `issue.md` (fixed field order, unknown fields kept), comment files and `events.jsonl`; atomic writes; record what the host wrote. Verify round trips, unknown fields, a file that does not parse, and the refused states list.
- [ ] 1.4 `workspace-host`: new `issues.sql` (trackers, per-file sync record as in `sync.sql`, paused parts, temporary IDs, index of issues for list and search) and its migration in `lib.rs`; rebuild the index from files when it is missing.
- [ ] 1.5 `workspace-host`: new `tracker_sync.rs`: set up and use a tracker, passes at launch, every minute, on Sync now and 2 seconds after a change, off the command thread, with `sync.rs`'s backoff; push only the default branch, never forced, only `wiffletree-issues.toml` and `issues/`; whole-file merge for now; ID allocation with up to 5 retries and temporary ID rewriting; accepting remote changes from anyone; holding back local edits Wiffletree did not make. Verify against a local bare repository: two clones filing at once, offline filing, a hand-added issue, an outside edit in the clone, a refused public repository.
- [ ] 1.6 `workspace-host`: `service.rs` and `socket.rs`: answer the tracker commands; make every tracker write human-only. Verify an uncredentialed connection is refused for writes and answered for reads.
- [ ] 1.7 `workspace-desktop`: new `settings/trackers.rs`, the Trackers section after Settings repository in `settings.rs`: check, who can read, prefix, login, Set up / Use this repository, status, Sync now and Remove.

## 2. Issues UI

- [ ] 2.1 `workspace-desktop`: `Page::Issues` in `main.rs` and `ui.rs`; an Issues row with its new-count before Repositories in `sidebar.rs`, and the tracker footer.
- [ ] 2.2 New `issues_view.rs`: list grouped by state and board with drag to move, filters (open, assignee, priority, label, repository, tracker), search, unread marks kept on this Mac.
- [ ] 2.3 New `issue_view.rs`: description, timeline of comments and events, composer with mentions and issue IDs, properties, Work box (Start project from issue disabled until phase 3), related issues, raw view with warning for a file that does not parse.
- [ ] 2.4 New `new_issue.rs`: the form, tracker choice, temporary ID, and the permanent-history and personal-data warning naming the tracker repository.
- [ ] 2.5 `workspace-host`: search over titles, descriptions and comments from the index; read state per issue in `issues.sql`.
- [ ] 2.6 Docs: README section on shared issues and their limits; `docs/DEVELOPMENT.md` notes.

## 3. Links between issues and projects or tickets, and automatic states

- [ ] 3.1 `workspace-core`: `IssueLink` (tracker, issue, project, optional ticket) and `Command::StartProjectFromIssue`, `LinkIssue`, `UnlinkIssue`.
- [ ] 3.2 `workspace-host`: links in `issues.sql`; Start project from issue in `service.rs` (create the project and coordinator, link, first message framing the issue as untrusted context, `work` event, In progress). Verify the first message and that no project or ticket detail reaches the tracker.
- [ ] 3.3 `workspace-host`: derived state and manual override in `tracker.rs`; `pr_watch.rs` appends `pr` events and re-derives state when a linked ticket's PR changes; ticket accept and close re-derive too. Verify open, merged, closed without merge, manual override and its release, and Canceled.
- [ ] 3.4 `workspace-host`: coordinator tools `list_issues`, `read_issue` and `link_issue` in `mcp.rs`, `live.rs` and `service.rs`, with authors marked on every passage; the coordinator's core in `skills.rs` (`workspace-core/src/skills`) gains the untrusted-text rule. Verify an implementer is refused.
- [ ] 3.5 `workspace-desktop`: Work box lists the linked project, tickets and PRs, or who started work on another Mac; `creation.rs` gains Start project from issue.

## 4. Issue assistant

- [ ] 4.1 `workspace-core`: `Role::IssueAssistant` in `lib.rs`; `Job::IssueAssistant` in `selection.rs` with its row checks; `Proposal` and its kinds in `issues.rs`; `Command::ApplyProposal`, `DiscardProposal`.
- [ ] 4.2 `workspace-host`: the tracker's host-owned project and its hierarchy checks; assistant sessions per issue and per list; built-in tools off in `provider.rs` as for comment sorting; its own core in `skills.rs`; its tool list in `policies.rs` and `mcp.rs` (`list_issues`, `read_issue`, `search_repositories`, `read_repository_file`, `propose_*`). Verify it has no shell and is refused every other tool.
- [ ] 4.3 `workspace-host`: `search_repositories` and `read_repository_file` read the base branch's fetched head with `git grep` and `git show`, off the command thread, bounded in size.
- [ ] 4.4 `workspace-host`: proposals stored in `issues.sql`; Apply writes as the human, `assistant_for` on comments; a proposal whose issue changed shows the new diff. Usage recorded under the tracker in `usage.rs`.
- [ ] 4.5 `workspace-desktop`: new `issue_assistant.rs` pane with proposal cards (Apply, Edit, Discard) and the authors notice; Ask the assistant on the Issues page; Draft with the assistant in New issue; the Issue assistant row in `models_view.rs` and `models.rs`.

## 5. Multi-user: identity, assignment, Inbox and field-level conflicts

- [ ] 5.1 `workspace-host`: read the login from `gh api user` when a tracker is chosen and when `gh`'s account changes; refuse writes without one. Assignee candidates from the tracker and, where readable, the repository's collaborators; display names from profiles.
- [ ] 5.2 `workspace-host`: three-way merge per front matter field and description in `tracker_sync.rs`, union merge of `events.jsonl`, comment pauses; paused parts in `issues.sql`; Keep mine, Use theirs and Compare. Verify two people changing different fields, the same field, the description, and comments at once.
- [ ] 5.3 `workspace-host`: follows and Inbox entries (assigned, mentioned, followed comments, urgent in my repositories, automatic moves, paused and outside-edited files), settled once for all projects.
- [ ] 5.4 `workspace-desktop`: issue entries under an "Issues" label at the top of every project's Inbox in `inbox.rs`, with Compare and the settle buttons from `workspace_notes.rs`; Assign to me and the assignee picker; Follow on the issue page.

## 6. Linear import

- [ ] 6.1 `workspace-host`: new `linear_import.rs`: the key in `keychain.rs` for the import only, teams, a dry-run summary, state and user mapping, commits of at most 100 issues, Linear identifiers in front matter so a rerun adds only new issues. Verify against recorded API responses; nothing is ever sent to Linear but reads.
- [ ] 6.2 `workspace-desktop`: Import from Linear… in `settings/trackers.rs`, with the summary, the user mapping and progress.

## 7. Docs for this change

- [x] 7.1 PRD: shared issues replace "multi-user collaboration" as out of scope, and tickets are said to stay local while issues are shared.
- [x] 7.2 Design README: link the study.
