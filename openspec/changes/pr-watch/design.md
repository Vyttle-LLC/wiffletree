## Context

See proposal.md for why. The code this builds on, at origin/main be5f484:

- `workspace-host/src/service.rs`: `Actor::process` (`:574`) handles one event, then runs `schedule`, then `refresh_overlaps`, then `signal`. `refresh_overlaps` (`:587`) checks each repository with open tickets at most every 5 minutes. It runs Git on its own thread through `spawn_overlap_check` (`:255`), which catches panics and always sends `Event::Overlaps` back (`:287`, handled at `:857`). It arms no wake-up, so it runs only when some event arrives. `Actor::wake` (`:1752`) keeps the single earliest wake-up and sleeps at most `MAX_WAKE_SLEEP_MS` (`:34`). `schedule` (`:1473`) fires timers and starts turns only for live projects. `turn_prompt` (`:55`) labels a message by its sender and calls `timer:` fires "your timer".
- `workspace-host/src/overlaps.rs`: `Host::overlap_checks` gathers each repository's open tickets on the host thread. The results are cached in memory on `Host` (`lib.rs:41-43`, `branch_warnings` and `base_fetched_at`), and reach the client through `Snapshot.branch_warnings`.
- `workspace-host/src/runtime.rs`: `git_query` (`:58`) runs `git` through `bounded_output` (`:87`), which applies one deadline and an output bound. Nothing runs `gh` today.
- `workspace-host/src/live.rs`: `Host::ticket` (`:209`) and `save_ticket` (`:216`). `ticket_overview` (`:531`) serializes each `Ticket` into `workspace_context`, so a new ticket field reaches the coordinator with no extra code. `accept_ticket` (`:392`) gates on the latest cycle and calls `check_verified_head` (`:445`).
- `workspace-host/src/lib.rs`: `Host::send` (`:551`) queues a turn-starting message. A message id is unique. Re-sending the same id with the same payload returns the existing message, and a different payload is refused. A message to an archived session is refused. Host-authored messages use sender `None`, as verification's outcome does (`verification.rs:908-919`, id `verification:{ticket}:{cycle}:outcome`). `Host::event` (`:184`) appends to the `activity` table.
- `workspace-desktop`: `sidebar.rs::ticket_row` (`:698`) draws one 30 px row with a label and a state dot. `tree.rs` holds the row text helpers (`ticket_label`, `verification_lines`). `team_view.rs::tickets` (`:99`) draws ticket cards, with `warning_badge` (`:320`) for overlap warnings. `repositories_view.rs::repositories_page` (`:7`) shows "N projects · M open tickets" for each repository. `conversation.rs` (`:626`) draws every sender-`None` message as a human bubble, and `waiting_messages` (`:195`) counts them as the human's.
- `gh` 2.94.0 here. Its GraphQL API answered the query in decision 2 for this repository's PR #31 at a cost of 1 point (see open decision (c)).

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

*Alternatives:* a coordinator timer, rejected because every fire is a model turn and counts toward the 100-turn brake. A dedicated polling thread, rejected because it would be a second scheduler beside the actor's. Folding the watcher into `refresh_overlaps`, rejected because the two have different intervals and only one of them arms a wake-up.

### 2. One read-only GraphQL call per repository per pass
`pr_watch::fetch` runs:

```
gh api graphql -f query=<QUERY> -F owner=<o> -F name=<n> -f h0=<branch> -f h1=<branch> …
```

Its working directory is the repository. `QUERY` declares `$h0…$hN` and one aliased connection per branch: `tN: pullRequests(headRefName: $hN, first: 5, orderBy: {field: CREATED_AT, direction: DESC})`. Each node has `number url state isDraft isCrossRepository headRefOid baseRefName mergeStateStatus mergedAt mergeCommit{oid} commits(last:1){nodes{commit{statusCheckRollup{state}}}} reviewThreads(first:100){nodes{isResolved}} comments(last:1){nodes{databaseId}} reviews(last:1){nodes{databaseId}}`. The query also reads `rateLimit { remaining resetAt }`. Branch names are passed only as variables, never pasted into the query. A repository with more than 50 open tickets is split into several calls.

- **Owner and name** come from the base's remote. For `origin/main` that is `git remote get-url origin`, parsed for `github.com` in its HTTPS and SSH forms. A base with no remote, or a remote that isn't on `github.com`, makes no `gh` call. Its status reads "not on GitHub; PRs not watched". This is deterministic, so `gh`'s own choice among several remotes never matters.
- **Running `gh`.** `runtime.rs` gains `gh_output(path, args)`, which reuses `bounded_output` with a 30 s deadline and sets `GH_PROMPT_DISABLED=1` and `NO_COLOR=1`. A non-zero exit becomes the repository's error, using `gh`'s last error line.
- **Merged heads.** For each PR in state `MERGED`, the same thread reads the ticket worktree's HEAD with `git rev-parse HEAD`, for decision 6.

*Conditional requests:* not used, because `gh` cannot make them here. GraphQL has no ETag or `304`. `gh pr view` and `gh pr list` are GraphQL underneath. REST would need several calls per PR, and review threads aren't in REST at all. One query costs 1 point of the 5,000 per hour. Twenty repositories with active PRs, polled every 60 s, use 1,200 points an hour. Rate limits are handled in decision 7.

### 3. Picking a ticket's PR
Nodes with `isCrossRepository: true` are skipped, so a fork's branch with the same name never matches. Of the rest, an `OPEN` PR wins. Otherwise the newest wins. No PR leaves `pull_request` at `None`. Ticket branches are unique (`wiffletree/<slug>`), so in practice there is one match.

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
    pub last_comment_id: Option<u64>,
    pub merge_commit: Option<String>,
}
```

- `MergeState` mirrors GitHub's enum: `Clean`, `Behind`, `Blocked`, `Dirty`, `Unstable`, `HasHooks` and `Unknown`. It is serialized in snake_case, and `#[serde(other)]` maps anything new to `Unknown`.
- `CheckState` maps the rollup's `StatusState`. `SUCCESS` is `Success`. `FAILURE` and `ERROR` are `Failure`. `PENDING` and `EXPECTED` are `Pending`. A null rollup is `None`.
- `last_comment_id` is the larger of the last issue comment's and the last review's `databaseId`.
- `Ticket.pull_request: Option<PullRequest>` uses `#[serde(default, skip_serializing_if = "Option::is_none")]`, as `waiver` does.

The check time is **not** stored on the ticket. Storing it would rewrite every ticket on every pass. It lives in decision 8.

*Alternative:* keep snapshots only in memory, as the overlap warnings are. Rejected for three reasons. The PRD wants PR identity recorded on the assignment. The coordinator gets the snapshot through `workspace_context` for free. A restart would otherwise forget that a PR had already merged.

### 5. Change detection and events
`Host::record_pull_requests(repository, found)` handles one repository's results on the actor thread. For each ticket:

1. It reloads the ticket and skips it if the ticket is no longer open.
2. When GitHub reports `merge_state: Unknown`, it keeps the stored merge state. GitHub computes the merge state lazily, and merged PRs report `UNKNOWN`, so a passing `Unknown` reading must not flicker the row or count as a change.
3. If the snapshot differs from the stored one, it calls `save_ticket` and `Host::event(project, None, "pull_request", "<ticket id> #142: checks failed")`. A small `PullRequest::changes(&old, &new)` names what changed.
4. On a transition to `Merged`, it sends the merged message (decision 6).

It returns whether anything changed, so `process` signals the client. The 60 s cadence is the coalescing window. A burst of pushes or check updates between passes becomes one change.

### 6. The merged message
- **Id and recipient.** The id is `pr:{ticket}:merged:{number}`. It goes to `ticket.coordinator_id` with sender `None`, as a turn-starting message, like verification's outcome.
- **Text.** "PR #141 for ticket "Overlap warnings" (<id>) merged into <base> as <merge commit>. Its head <sha> is the ticket worktree's HEAD. Accept the ticket with accept_ticket." When the merged head differs from the worktree's HEAD, the second sentence becomes "Its head <sha> differs from the ticket worktree's HEAD <sha>, so the merge may hold commits verification never saw. Check what changed before you call accept_ticket." When HEAD can't be read, it says so. The text uses only the ticket's own title and Git identifiers. PR titles, bodies and comments are never copied in.
- **Once only.** The message is sent before the merged snapshot is saved. It is skipped when a message with that id already exists, so a crash between the two steps cannot send it twice or trip the reused-id check.
- **Archived coordinator.** If the coordinator is archived, the send fails. The failure is logged and the snapshot is still saved.
- **No auto-accept.** The ticket stays open. `accept_ticket` keeps its own gates.

*Alternative:* the host accepts the ticket on merge. Rejected because acceptance is the coordinator's decision, and `accept_ticket` may legitimately refuse, for example over untriaged findings.

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
- **`repositories_view.rs::repositories_page`.** The hint adds "· K open PR(s)", counting open tickets whose PR is `Open`. A second line reads "PRs checked … ago", "PR check failed …: <error> · retrying in …", or the not-on-GitHub status.
- **`assets.rs`.** New `git-pull-request` and `git-merge` drawings.

The mockup is `design/mockups/current/pr-watch.html`.

### 11. Tests, all without network
- `pr_watch.rs` unit tests. Parsing recorded GraphQL JSON: PR #31's shape, a fork PR skipped, open preferred over newer closed, a null rollup, an unknown enum value. Remote URL parsing. `Watch` cadence, backoff and rate-limit waits with explicit times.
- `Host::record_pull_requests` on a temp store:
  - an unchanged snapshot writes nothing
  - an `Unknown` merge state keeps the stored one
  - a merge sends exactly one message, including after a simulated crash between send and save
  - a closed ticket is skipped
  - an archived coordinator still gets the snapshot saved
  - the message text never includes PR text
- An actor test in `service.rs` beside the overlap tests (`:2070`): `Event::PullRequests` clears the running mark, arms the next wake and signals only on change.
- `tree.rs` tests for `pr_line` in each mockup state. A `conversation`/`turn_prompt` test for the `pr:` label.

## Risks / Trade-offs

- [`gh` missing or not logged in] → The repository row shows the error, passes back off, and agents are unaffected. Login-shell PATH adoption (`login_env.rs`) already makes `gh` visible to the host.
- [A wrong PR match] → Cross-repository PRs are skipped and ticket branch names are unique. The PR number is visible on the row, so a mismatch is obvious.
- [The thread count is capped at the first 100 review threads] → This is acceptable for ticket-sized PRs. Paging is out of scope.
- [Shared rate limit with the human's own `gh` use] → The 200-point floor and the wait for `resetAt`.
- [Merged head differs from the worktree] → The message says so, and `accept_ticket`'s verified-head check still applies to the local HEAD.
- [Verification's host messages still look like the human's] → This is pre-existing. The `host_notice` helper makes extending it to `verification:` a one-line follow-up.
- [Energy] → At most one `gh` process per repository per minute, and only while it has an open PR.

## Migration Plan

`Ticket.pull_request` defaults to `None`, so stored tickets load unchanged. Older builds ignore the unknown field. There is no SQL migration. Rollback means reverting the build. Stored snapshots are then ignored.

## Open decisions for the human

**(a) Wake the implementer on failed checks, a conflict or a trusted comment?**

*Recommendation: not the implementer. In v1, wake the coordinator, without PR text, on failed checks and on a conflict. Defer comments to their own change.*

- **Why not the implementer.** A fix after the PR opens needs a new verification cycle and someone to push, and the coordinator owns both. Waking the implementer directly would change code behind the coordinator's back, which undercuts the decision authority review convergence just gave it.
- **The checks and conflict wakes.** They would reuse decision 6's mechanism with ids `pr:{ticket}:checks:{head}` and `pr:{ticket}:conflict:{head}`. Each fires at most once per head SHA, and at most three per PR in total. A fourth gets one final "PR wake budget used; see the ticket row" message, which is the circuit breaker. They carry only the PR number, the head, and the names of failing checks, which come from the repository's own workflow files, not from PR text. That adds about 40 lines and one spec requirement.
- **Comments are the injection risk.** They need their own change with the report's safety rules:
  - act only on authors whose `authorAssociation` is OWNER, MEMBER or COLLABORATOR
  - ignore bots and the account `gh` runs as (`viewer.login`)
  - pass comment text only as quoted, untrusted data, never as instructions
  - allow about three code-change rounds per head
  - trip a circuit breaker after two agent replies in one thread with no human in between
  - never approve, merge or dismiss reviews
  - allow no replies on repositories where a comment can trigger a deploy, such as Atlantis

**(b) Bundle a PR skill and fix the README in this change?**

*Recommendation: a separate small change, before or alongside this one.*

The watcher is read-only, so "Wiffletree itself never pushes or merges" (`README.md:51`) stays true. Fixing `README.md:81` ("does not push, open pull requests…") and bundling an `open-pr` skill under `crates/workspace-host/skills/` (which `communication.md:10` already names) changes role contracts and agents' authority. That deserves its own review. Bundling it here would also pull `ticket-workspaces`' role-instruction requirement into this change.

**(c) `gh` field names: verified, no decision needed.**

These were checked with `gh pr view --help`, `gh pr view 31 --json …` and GraphQL introspection on `github.com`, all read-only:

| Field | Values |
| --- | --- |
| `mergeStateStatus` | `BEHIND`, `BLOCKED`, `CLEAN`, `DIRTY`, `HAS_HOOKS`, `UNKNOWN`, `UNSTABLE` (no `DRAFT`; drafts come from `isDraft`) |
| `mergeable` | `MERGEABLE`, `CONFLICTING`, `UNKNOWN` (not used; `DIRTY` covers conflicts) |
| Rollup `state` (`StatusState`) | `SUCCESS`, `FAILURE`, `ERROR`, `PENDING`, `EXPECTED` |
| PR `state` | `OPEN`, `CLOSED`, `MERGED` |
| `authorAssociation` | `OWNER`, `MEMBER`, `COLLABORATOR`, `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, `FIRST_TIMER`, `MANNEQUIN`, `NONE` |

`gh pr view --json` has no review-thread field, which is why the watcher uses `gh api graphql`. A merged PR (#31) reports `mergeStateStatus: UNKNOWN`, which decision 5 handles.

**(d) Tell the coordinator when a PR closes without merging?**

*Recommendation: yes.* Use one message, `pr:{ticket}:closed:{number}`: "PR #N was closed without merging; reopen it, close the ticket or ask the human." Otherwise the ticket waits silently for a merge that will never come. It reuses decision 6, adding about 10 lines and one scenario. Without it, the row shows "closed" and nothing else happens.
