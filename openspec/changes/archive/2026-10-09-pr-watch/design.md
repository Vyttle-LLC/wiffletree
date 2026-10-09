## Context

See proposal.md for why. The code this builds on, at origin/main 6b1e7c9:

- `workspace-host/src/service.rs`: `Actor::process` (`:558`) handles one event, then runs `schedule`, then `refresh_overlaps`, then `signal`. `refresh_overlaps` (`:571`) checks each repository with open tickets at most every 5 minutes. It runs Git on its own thread through `spawn_overlap_check` (`:241`), which catches panics and always sends `Event::Overlaps` back (`:275`, handled at `:835`). It arms no wake-up, so it runs only when some event arrives. `Actor::wake` (`:1763`) keeps the single earliest wake-up and sleeps at most `MAX_WAKE_SLEEP_MS` (`:27`). `schedule` (`:1528`) fires timers and starts turns only for live projects. Since #32 there are no turn caps. Instead, `check_in_by_turns` (`:1449`) raises a human inbox item once the coordinator has run `checkin_human_turns` turns (default 100) since the human last stepped in. Stepping in is detected only from client commands, such as `Command::Send` with sender `None` (`:707-723`), so a message the host sends internally never counts as the human. `turn_prompt` (`:41`) labels a message by its sender and calls `timer:` fires "your timer".
- `workspace-host/src/overlaps.rs`: `Host::overlap_checks` gathers each repository's open tickets on the host thread. The results are cached in memory on `Host` (`lib.rs:41-43`, `branch_warnings` and `base_fetched_at`), and reach the client through `Snapshot.branch_warnings`.
- `workspace-host/src/runtime.rs`: `git_query` (`:58`) runs `git` through `bounded_output` (`:87`), which applies one deadline and an output bound. Nothing runs `gh` today.
- `workspace-host/src/live.rs`: `Host::ticket` (`:209`) and `save_ticket` (`:216`). `ticket_overview` (`:531`) serializes each `Ticket` into `workspace_context`, so a new ticket field reaches the coordinator with no extra code. `accept_ticket` (`:392`) gates on the latest cycle and calls `check_verified_head` (`:445`).
- `workspace-host/src/lib.rs`: `Host::send` (`:561`) queues a turn-starting message. A message id is unique. Re-sending the same id with the same payload returns the existing message, and a different payload is refused. A message to an archived session is refused. Host-authored messages use sender `None`, as verification's outcome does (`verification.rs:895-906`, id `verification:{ticket}:{cycle}:outcome`). `Host::event` (`:195`) appends to the `activity` table.
- `workspace-desktop`: `sidebar.rs::ticket_row` (`:698`) draws one 30 px row with a label and a state dot. `tree.rs` holds the row text helpers (`ticket_label`, `verification_lines`). `team_view.rs::tickets` (`:99`) draws ticket cards, with `warning_badge` (`:320`) for overlap warnings. `repositories_view.rs::repositories_page` (`:7`) shows "N projects · M open tickets" for each repository. `conversation.rs` (`:626`) draws every sender-`None` message as a human bubble, and `waiting_messages` (`:195`) counts them as the human's.
- `gh` 2.94.0 here. Its GraphQL API answered the query in decision 2 for this repository's PR #31 (see settled decision 3). With the review-thread comments included, it costs about 1 point per ticket branch.

## Goals / Non-Goals

**Goals:** correct, cheap PR status for every open ticket without a model; one coordinator wake per merge; no new table, no SQL migration and no new scheduler.

**Non-Goals:**
- Writing to GitHub or to branches.
- Configurable intervals. The PRD intervals are constants in v1.
- GitHub Enterprise hosts. Only `github.com` remotes are watched.
- Watching base-branch movement, which the overlap check already covers.
- Webhooks.
- Relabelling verification's host messages, which share the human-bubble problem (see Risks).

## Decisions

### 1. The watcher runs beside the overlap check and arms its own wake-up
`Actor` gains one field, `pr_watch: pr_watch::Watch`, which holds each repository's state: when its next pass is due, whether a pass is running, and how many failures in a row it has had. `Actor::process` calls a new `Actor::refresh_pull_requests(now)` after `refresh_overlaps`. That call:

1. Asks `Host::pull_request_queries()` for every repository that has an open ticket whose coordinator is not archived. Each entry carries the repository path and base, and each ticket's id, branch and worktree.
2. For each repository that is due and has no pass running, spawns a thread that calls `pr_watch::fetch(query)`. Like `spawn_overlap_check`, the thread catches panics and always sends `Event::PullRequests { repository, outcome }`.
3. Calls `self.wake(self.pr_watch.next_due())`.

Unlike the overlap check, it arms a wake-up, because a quiet host may see no other event for hours. `wake` already keeps only the earliest time, and `process` re-arms after every event, so timers and the watcher share one mechanism.

*Alternatives:* a coordinator timer, rejected because every fire is a model turn. A 5-minute poll would be 288 coordinator turns a day, and would raise a turn-count check-in with the human about every 8 hours. A dedicated polling thread, rejected because it would be a second scheduler beside the actor's. Folding the watcher into `refresh_overlaps`, rejected because the two have different intervals and only one of them arms a wake-up.

### 2. One read-only GraphQL call per repository per pass
`pr_watch::fetch` runs:

```
gh api graphql -f query=<QUERY> -f owner=<o> -f name=<n> -f h0=<branch> -f h1=<branch> …
```

Its working directory is the repository. `QUERY` declares `$h0…$hN` and one aliased connection per branch: `tN: pullRequests(headRefName: $hN, first: 2, orderBy: {field: CREATED_AT, direction: DESC})`. Each node has `number url state isDraft isCrossRepository headRefOid baseRefName mergeStateStatus mergedAt mergeCommit{oid} commits(last:1){nodes{commit{statusCheckRollup{state}}}} reviewThreads(first:50){nodes{isResolved comments(last:1){nodes{databaseId createdAt}}}} comments(last:1){nodes{databaseId}} reviews(last:1){nodes{databaseId}}`. The query also reads `rateLimit { remaining resetAt }`. Owner, name and branch names are passed only as raw-string variables (`-f`), never pasted into the query. `-F` would let `gh` coerce or expand them. Each thread's last comment is read because a reply inside a review thread changes neither the issue comments nor, necessarily, the PR's last review. Nested connections multiply GraphQL's cost, so the query asks for 2 PRs per branch and 50 threads per PR. That costs about 1 point per branch, measured with the query on this repository (2 points for 2 branches; 10 with 5 PRs and 100 threads). A repository with more than 50 open tickets is split into several calls.

- **Owner and name** come from the base's remote. For `origin/main` that is `git remote get-url origin`, parsed for `github.com` in its HTTPS and SSH forms. A base with no remote, or a remote that isn't on `github.com`, makes no `gh` call. Its status reads "not on GitHub; PRs not watched". This is deterministic, so `gh`'s own choice among several remotes never matters.
- **Running `gh`.** `runtime.rs` gains `gh_output(path, args)`, which reuses `bounded_output` with a 30 s deadline and sets `GH_PROMPT_DISABLED=1` and `NO_COLOR=1`. A non-zero exit becomes the repository's error, using `gh`'s last error line.
- **Merged heads.** For each PR in state `MERGED`, the same thread reads the ticket worktree's HEAD with `git rev-parse HEAD`, for decision 6.

*Conditional requests:* not used, because `gh` cannot make them here. GraphQL has no ETag or `304`. `gh pr view` and `gh pr list` are GraphQL underneath. REST would need several calls per PR, and review threads aren't in REST at all. The cost grows with the number of watched branches, at about 1 point per branch per pass, out of 5,000 an hour. Twenty open tickets with active PRs, polled every 60 s, use about 1,200 points an hour. Rate limits are handled in decision 7.

### 3. Picking a ticket's PR
Nodes with `isCrossRepository: true` are skipped, so a fork's branch with the same name never matches. Of the rest, which are at most the branch's 2 newest PRs, an `OPEN` PR wins. Otherwise the newest wins. No PR leaves `pull_request` at `None`. Ticket branches are unique (`wiffletree/<slug>`), so in practice there is one match.

### 4. The snapshot type, on the ticket
In `workspace-core/src/lib.rs`:

```rust
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub state: PrState,              // Open | Closed | Merged
    pub draft: bool,
    pub base: String,                // baseRefName
    pub head: String,                // headRefOid
    pub merge_state: MergeState,     // GitHub's MergeStateStatus
    pub checks: CheckState,          // None | Pending | Success | Failure
    pub unresolved_threads: u32,
    pub comments: CommentCursors,   // newest comment id of each kind
    pub merge_commit: Option<String>,
}
```

- `MergeState` mirrors GitHub's enum: `Clean`, `Behind`, `Blocked`, `Dirty`, `Unstable`, `HasHooks` and `Unknown`. It is serialized in snake_case, and `#[serde(other)]` maps anything new to `Unknown`.
- `CheckState` maps the rollup's `StatusState`. `SUCCESS` is `Success`. `FAILURE` and `ERROR` are `Failure`. `PENDING` and `EXPECTED` are `Pending`. A null rollup is `None`.
- `CommentCursors { issue, review, inline: Option<u64> }` records three ids:
  - `issue`: the last issue comment's `databaseId`
  - `review`: the last review's `databaseId`
  - `inline`: the `databaseId` of the newest review-thread comment by `createdAt`, across every thread's last comment

  Any new comment changes its own kind's cursor. The three id spaces aren't ordered against each other, so a single max can miss a new inline reply whose id is smaller than an existing issue comment's. Cursors per kind, not a digest, because they stay readable in `workspace_context` and say which kind changed.
- `Ticket.pull_request: Option<PullRequest>` uses `#[serde(default, skip_serializing_if = "Option::is_none")]`, as `waiver` does.

The check time is **not** stored on the ticket. Storing it would rewrite every ticket on every pass. It lives in decision 8.

*Alternative:* keep snapshots only in memory, as the overlap warnings are. Rejected for three reasons. The PRD wants PR identity recorded on the assignment. The coordinator gets the snapshot through `workspace_context` for free. A restart would otherwise forget that a PR had already merged.

### 5. Change detection and events
`Host::record_pull_requests(repository, found)` handles one repository's results on the actor thread. For each ticket:

1. It reloads the ticket and skips it if the ticket is no longer open.
2. When GitHub reports `merge_state: Unknown`, it keeps the stored merge state. GitHub computes the merge state lazily, and merged PRs report `UNKNOWN`, so a passing `Unknown` reading must not flicker the row or count as a change.
3. If the snapshot differs from the stored one, it calls `save_ticket` and `Host::event(project, None, "pull_request", "<ticket id> #142: checks failed")`. A small `PullRequest::changes(&old, &new)` names what changed.
4. Before saving, it sends any message the new snapshot calls for (decision 6). Sending first means a crash between the two steps resends nothing twice: the next pass sees the same change again, and each message is skipped when its id already exists.

It returns whether anything changed, so `process` signals the client. The 60 s cadence is the coalescing window. A burst of pushes or check updates between passes becomes one change.

### 6. Messages to the coordinator
Every watcher message goes to `ticket.coordinator_id` with sender `None`, as a turn-starting message like verification's outcome. Its id starts with `pr:`, and it is sent only if no message with that id exists. That rule, not the snapshot alone, keeps each message to one send, and it never trips the reused-id check. The text uses only the ticket's own title, its id, the PR number, the base name and Git SHAs. PR titles, bodies, comments, check names and author names are never copied in. A send to an archived coordinator fails, the failure is logged, and the snapshot is still saved.

- **Merged** (`pr:{ticket}:merged:{number}`, on a change to `Merged`). "PR #141 for ticket "Overlap warnings" (<id>) merged into <base> as <merge commit>. Its head <sha> is the ticket worktree's HEAD. Accept the ticket with accept_ticket." When the merged head differs from the worktree's HEAD, the second sentence becomes "Its head <sha> differs from the ticket worktree's HEAD <sha>, so the merge may hold commits verification never saw. Check what changed before you call accept_ticket." When HEAD can't be read, it says so.
- **Closed without merging** (`pr:{ticket}:closed:{number}`, on a change to `Closed`). "PR #N for ticket "X" (<id>) was closed without merging. Reopen it, close the ticket with close_ticket, or ask the human." The same PR closing again after a reopen sends nothing new.
- **Checks failed** (`pr:{ticket}:checks:{number}:{head}`, when an open PR's snapshot changes and its checks are `Failure`). "PR #143 for ticket "X" (<id>): checks failed at head <sha>. Run gh pr checks 143 in the ticket worktree for details. A fix needs a new verification cycle before it is pushed. Automatic wake 1 of 3 for this PR."
- **Conflict** (`pr:{ticket}:conflict:{number}:{head}`, when an open PR's snapshot changes and its merge state is `Dirty`). "PR #139 for ticket "X" (<id>) conflicts with <base> at head <sha>. Resolving it needs a rebase or merge in the worktree and a new verification cycle before it is pushed. Automatic wake 2 of 3 for this PR."
- **Budget.** The order is fixed. The host first checks whether the wake's message id already exists. An existing id is skipped, spends no budget and never triggers the budget message. Only a new id is checked against the budget. Checks and conflict wakes share a budget of 3 per PR number, counted from the existing `pr:{ticket}:checks:{number}:` and `pr:{ticket}:conflict:{number}:` messages. One more wake is due once 3 have been sent. Instead of sending it, the host sends `pr:{ticket}:budget:{number}` once: "PR #N for ticket "X" (<id>) has used its 3 automatic wakes. Further failed checks and conflicts show only on the ticket row and in workspace_context." After that the PR wakes nobody, apart from its merged or closed message.

Each wake is one coordinator turn. It counts toward the coordinator's turn-count check-in with the human, but never resets it, because the host's own messages are not the human stepping in. The budget keeps that cost to at most four turns per PR, plus the merged or closed message.

The wakes go to the coordinator, never to the implementer. A fix after the PR opens needs a new verification cycle and a push, and the coordinator owns both. The ticket stays open after a merge, and `accept_ticket` keeps its own gates.

*Alternatives:* the host accepts the ticket on merge. Rejected because acceptance is the coordinator's decision, and `accept_ticket` may legitimately refuse, for example over untriaged findings. Naming the failing checks in the message: check names come from workflow files the PR itself can change, so they count as PR text. `gh pr checks` gives them to whoever acts on the wake.

### 7. Cadence, backoff and rate limits
`pr_watch::Watch` holds the constants `ACTIVE_MS = 60_000`, `IDLE_MS = 300_000` and `MAX_BACKOFF_MS = 1_800_000`.

- **First pass.** A repository is due at once when the host starts, or when it first has an open ticket.
- **After a successful pass.** The next pass is due after `ACTIVE_MS` if any open ticket there has an open PR, and after `IDLE_MS` otherwise.
- **After the n-th failure in a row.** The next pass is due after `min(IDLE_MS × 2^(n-1), MAX_BACKOFF_MS)`: 5, 10, 20, then 30 minutes. A success resets the count.
- **Rate limit.** When `rateLimit.remaining` falls below 200, the next pass waits at least until `resetAt`. This leaves headroom for the human's and the agents' own `gh` use.
- **Dropped repositories.** A repository with no open tickets leaves `Watch`.

Watching continues while a project is paused, because it is read-only and costs no model. A merged message then waits in the queue, and `schedule` delivers it once the project is live. While the app is closed nothing runs. The first pass after start catches up, and a merge that happened meanwhile produces its message once.

### 8. Check status: in memory, shown on the repository row
`Host` gains `pull_request_checks: HashMap<String, PullRequestCheck>`, beside `branch_warnings`. Each `PullRequestCheck` holds the repository id, `checked_at: Option<i64>`, `error: Option<String>` and `next_at: i64`. `Snapshot.pull_request_checks` exposes it. A failed pass updates the error and keeps the tickets' last snapshots. It never clears a PR line. This state is not persisted. After a restart, a repository reads "not checked yet" for the few seconds before the first pass.

### 9. Host messages are labelled as Wiffletree
- `turn_prompt` labels a message whose id starts with `pr:` as "Wiffletree PR watcher", beside the `timer:` case.
- `conversation.rs` draws `pr:` messages as a Wiffletree notice, with the server icon and the label "Wiffletree PR watcher", instead of `human_message`.
- `waiting_messages` stops counting them as the human's.
- One shared helper `workspace_core::host_notice(id) -> Option<&'static str>` holds the prefix, so the two crates agree.

### 10. Desktop
- **`tree.rs`.** A new `pr_line(&PullRequest) -> Vec<PrPart>` gives the text and tone of each part, in this order:
  - `#142`
  - checks: `✓` (good), `✗` (bad), a running glyph, or nothing for `None`
  - `N thread(s)` when N > 0
  - one merge word: `draft`, `behind` (`Behind`), `conflict` (`Dirty`, bad) or `blocked` (`Blocked`)
  - `merged` or `closed` in place of the checks and threads for a finished PR

  `Clean`, `Unstable`, `HasHooks` and `Unknown` add no word. A `pr_sentence` gives the tooltip's full wording.
- **`sidebar.rs::ticket_row`.** When `pull_request` is set, the label becomes a two-line stack. The second line is the PR line at 11 px, muted. Good is `p.green`, bad is `p.red`, merged is `p.focus`, and the warning color stays reserved. Rows without a PR keep 30 px. The tooltip adds the PR sentence and "Checked 40 s ago" from `Snapshot.pull_request_checks`.
- **`team_view.rs::tickets`.** A PR block, like `waiver_block`, with the sentence, the head, the check time and an "Open on GitHub" link through `cx.open_url`.
- **`repositories_view.rs::repositories_page`.** The hint adds "· K open PR(s)", counting open tickets whose PR is `Open`. A second line reads "PRs checked … ago", "PR check failed …: <error> · retrying in …", or the not-on-GitHub status. A repository without open tickets is not watched and shows neither the count nor a check line.
- **`assets.rs`.** New `git-pull-request` and `git-merge` drawings.

The mockup is `design/mockups/current/pr-watch.html`.

### 11. Tests, all without network
- `pr_watch.rs` unit tests. Parsing recorded GraphQL JSON: PR #31's shape, a fork PR skipped, open preferred over newer closed, a null rollup, an unknown enum value. Remote URL parsing. `Watch` cadence, backoff and rate-limit waits with explicit times.
- `Host::record_pull_requests` on a temp store:
  - an unchanged snapshot writes nothing
  - an `Unknown` merge state keeps the stored one
  - a merge sends exactly one message, including after a simulated crash between send and save
  - a close without merging sends one message
  - failed checks and a conflict each wake the coordinator once per head, the fourth wake becomes the single budget message, and nothing follows it
  - a closed ticket is skipped
  - an archived coordinator still gets the snapshot saved
  - the message text never includes PR text
- An actor test in `service.rs` beside the overlap tests (`:2083`): `Event::PullRequests` clears the running mark, arms the next wake and signals only on change.
- `tree.rs` tests for `pr_line` in each mockup state. A `conversation`/`turn_prompt` test for the `pr:` label.

## Risks / Trade-offs

- [`gh` missing or not logged in] → The repository row shows the error, passes back off, and agents are unaffected. Login-shell PATH adoption (`login_env.rs`) already makes `gh` visible to the host.
- [A wrong PR match] → Cross-repository PRs are skipped and ticket branch names are unique. The PR number is visible on the row, so a mismatch is obvious.
- [The thread count and the reply check cover the first 50 review threads, and only the 2 newest PRs per branch are read] → Both are acceptable for ticket-sized PRs with unique branches. Paging is out of scope, and the caps keep the query at about 1 point per branch.
- [Shared rate limit with the human's own `gh` use] → The 200-point floor and the wait for `resetAt`.
- [Merged head differs from the worktree] → The message says so, and `accept_ticket`'s verified-head check still applies to the local HEAD.
- [Verification's host messages still look like the human's] → This is pre-existing. The `host_notice` helper makes extending it to `verification:` a one-line follow-up.
- [Energy] → At most one `gh` process per repository per minute, and only while it has an open PR.

## Migration Plan

`Ticket.pull_request` defaults to `None`, so stored tickets load unchanged. Older builds ignore the unknown field. There is no SQL migration. Rollback means reverting the build. Stored snapshots are then ignored.

## Settled by the human (October 9, 2026)

The human approved the mockup and proposal with these answers:

1. **Wakes on failed checks and conflicts: the coordinator only.** No PR text, at most once per head SHA and 3 times per PR, then one "budget used" message (decision 6). Comments are deferred to a later change. That change needs the report's safety rules:
   - act only on authors whose `authorAssociation` is OWNER, MEMBER or COLLABORATOR
   - ignore bots and the account `gh` runs as
   - pass comment text only as quoted, untrusted data
   - allow about three code-change rounds per head
   - trip a circuit breaker after two agent replies in one thread with no human in between
   - never approve, merge or dismiss reviews
   - allow no replies on repositories where a comment can trigger a deploy
2. **The PR skill and the README fix are a separate change.** The watcher is read-only, so `README.md:51` ("Wiffletree itself never pushes or merges") stays true. `README.md:81` and an `open-pr` skill bundled under `crates/workspace-host/skills/` go into their own change.
3. **`gh` field names are as verified.** These were checked with `gh pr view --help`, `gh pr view 31 --json …` and GraphQL introspection on `github.com`, all read-only:

   | Field | Values |
   | --- | --- |
   | `mergeStateStatus` | `BEHIND`, `BLOCKED`, `CLEAN`, `DIRTY`, `HAS_HOOKS`, `UNKNOWN`, `UNSTABLE` (no `DRAFT`; drafts come from `isDraft`) |
   | `mergeable` | `MERGEABLE`, `CONFLICTING`, `UNKNOWN` (not used; `DIRTY` covers conflicts) |
   | Rollup `state` (`StatusState`) | `SUCCESS`, `FAILURE`, `ERROR`, `PENDING`, `EXPECTED` |
   | PR `state` | `OPEN`, `CLOSED`, `MERGED` |

   `gh pr view --json` has no review-thread field, hence `gh api graphql`. A merged PR (#31) reports `mergeStateStatus: UNKNOWN`, which decision 5 handles.
4. **A PR closed without merging sends one `pr:{ticket}:closed:{number}` message** (decision 6).
