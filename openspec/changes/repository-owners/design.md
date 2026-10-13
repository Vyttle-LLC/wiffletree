## Context

See proposal.md for why. The code this builds on, at origin/main fb9b217:

- **Repositories.** `workspace-core/src/lib.rs`: `Repository` (`:132`) is `id`, `name`, `path`, `base`, stored as JSON in the `repositories` table. `workspace-host/src/repositories.rs`: `Host::add_repository` (`:240`) writes it; `scan_roots` (`:192`) finds new ones off the host thread; the desktop asks for a refresh when the human comes back to the app, at most once a minute (`workspace-desktop/src/main.rs:417`, handled in `service.rs:628` and `:727`). `workspace-desktop/src/repositories_view.rs::repositories_page` (`:7`) draws one row per repository.
- **GitHub reads.** `workspace-host/src/pr_watch.rs` already parses a `github.com` remote URL into owner and name (`github_repository`, `:126`) and runs one read-only `gh api graphql` call per repository per pass through `runtime::gh_output` (`runtime.rs:83`). It asks only about repositories with an open ticket (`Host::pull_request_queries`, `:359`), takes the owner from the base's remote (`github_remote`, `:111`), and skips PRs from other repositories (`parse`, `:194`).
- **Review.** `live.rs::accept_ticket` (`:393`) requires the latest verification cycle to have passed, or to have ended blocked with a waiver, and calls `check_verified_head` (`:446`), which accepts HEAD when it is the verified commit or has the same patch id against the repository's base (`worktrees::patch_id`, `worktrees.rs:542`). Findings live in the ticket's ledger (`LedgerEntry`, `workspace-core/src/lib.rs:968`).
- **Tools.** `mcp.rs::tools` (`:17`) lists them. `service.rs` handles `Event::Tool` (`:881`): a few tools are handled by the actor itself (`stop_agents`, `:895`), the rest go to `Host::agent_tool` (`live.rs:692`). Ticket tools check `owned_ticket` (`live.rs:879`).
- **Approvals.** The Inbox holds `Attention` items (`workspace-core/src/lib.rs:239`), stored in the `attention` table and answered once (`Host::resolve_attention`, `lib.rs:938`). An item whose operation starts with `permission:` is an approval (`Attention::is_permission`, `:253`): the desktop draws Deny and Approve (`inspector.rs:632-662`), the human cannot dismiss it (`lib.rs:979`) and the coordinator cannot close it (`live.rs:860`).
- **Ids are single-use.** `Host::request_attention` returns the existing item, answered or not, for the same session and operation, and refuses a changed prompt (`lib.rs:892-906`). `Host::send` returns the existing message for a repeated id and refuses a different body or recipient (`lib.rs:721`).
- **Settings.** `HostSettings` (`workspace-core/src/lib.rs:486`) lives in the host's `settings.json`, written by `settings.rs::save_settings` (`:172`). It already holds one per-machine value, `workspaces_dir`.
- **Instructions.** `provider.rs::skill` (`:85`) joins `skills/communication.md` with the role's file and passes it on every turn. Line 12 of `communication.md` names "the open-pr skill" twice. `provider.rs::configure` launches Claude with `--dangerously-skip-permissions` (`:168`) and Codex with `--dangerously-bypass-approvals-and-sandbox` (`:178`).
- **Measured here** (`gh` 2.94.0, October 12, 2026): one GraphQL call with one aliased `repository(owner:, name:)` per repository answered `isFork`, `visibility` and `nameWithOwner` for this workspace's 33 distinct GitHub repositories at a cost of 1 rate-limit point. The workspace has 34 repositories, all with a GitHub `origin`: Sage-Social 29 (28 distinct; two folders are clones of the same repository), MHohlios 3, Vyttle-LLC 1, celadon-theme 1. None is a fork.

## Words

The human asked whether "automatic pull requests" meant Wiffletree could approve a PR. It cannot, so the words are fixed:

- An owner the human ticked is **allowed**. Its repositories publish **without asking**.
- **Approve** is used only for the human's answer to an Inbox card.
- Nothing in the product, the instructions or the code calls the owner list "approved" or the behavior "automatic".

## Goals / Non-Goals

**Goals:** the app, not instruction text, decides whether a push or a PR needs the human; one small operation covers push, open and update; no new table, no migration, no new scheduler; a failure to read GitHub never widens authority.

**Non-Goals:**
- A sandbox or scoped credentials for agents (see "What this closes and what it does not").
- Detecting pushes made outside the app.
- Approving, merging, closing, commenting on or labelling a PR, or editing an open PR's title and description.
- Rebasing or fixing a PR. That is the PR keeper, change 3.
- Per-repository overrides of the owner list.
- Draft PRs, PRs into another repository such as a fork's parent, GitHub Enterprise hosts, remotes other than `origin`.

## Decisions

### Who decided what

**Decided by the human on October 12, 2026.** From the plan:
1. Owners are detected and listed; the human ticks the ones allowed to have PRs opened without asking; nothing is allowed until ticked.
2. In an allowed owner's repositories, agents may push a ticket's branch, open its PR once review has passed, and keep it updated, without asking. Merging is the human's alone.
3. Everywhere else, nothing leaves the machine until the human approves each step.
4. The allowed-owners list is per machine and never synced.
5. Opening a PR is not a Wiffletree skill; the contract stops naming one.
6. The README is corrected.

And on this proposal, the same day:
7. A fork is its own entry; its owner's tick never covers it.
8. Owners only, plus fork entries. No per-repository override.
9. Merging is always the human's. The rule that let a plan's coordinator merge into the plan's feature branch is dropped.
10. The ticket's coordinator or its implementer may call `publish_ticket`; never a verifier. The implementer writes the title and description.
11. A ticket whose latest review ended blocked may be published, but only after the human approves it on an Inbox card, in every repository, allowed owners' included. The card says the review ended blocked and lists the open findings.
12. Approval is per step, literally: the push, then the PR, then every later push.
13. One `publish_ticket` tool.
14. The words: "allowed" for the owner list, "without asking" for the behavior, "approve" only for the human's Inbox answer.

**Confirmed by the human on October 13, 2026**, in so many words:
15. The contract paragraph in decision 10, exactly as written there.
16. In an allowed owner's repository, a blocked review raises one card covering the push and the PR.
17. "Open findings" on that card means every finding that is not fixed and was not settled as follow-up or won't-fix.
18. The tool's result reaches the agent as a Wiffletree message.

**Everything else in this document is a design decision made in review, not the human's.** That includes decisions 1 to 12 below wherever they go beyond the items above, and in particular the changes made after review round 1: `origin` must have one push URL (decision 1); partial GitHub answers are kept (decision 2); the request is frozen and compared before anything is sent (decision 6); the owner is confirmed with GitHub before sending, and the push sends a single ref whatever Git is configured to do (decision 7); and approval items and result messages carry a per-request id (decisions 8 and 9). The `publish:` prefix being reserved for the host (decision 8) was added after cycle 1.

### 1. The owner comes from `origin`, read on disk
A repository's GitHub owner and name are parsed from `git remote get-url --push origin` with the existing `pr_watch::github_repository`. No network is needed, so owners are listed even when `gh` is missing. A repository with no `origin`, or one not on `github.com`, has no owner and cannot be allowed.

`origin` must have exactly one push URL (`git remote get-url --push --all origin` prints one line), and its fetch URL must name the same GitHub repository. Git pushes to every push URL of a remote, so a second one would be a destination nobody checked; and the host reads GitHub's branch through the fetch URL, so the two must agree. A repository that fails either test has no owner recorded, its row says why, and `publish_ticket` refuses it.

The watcher uses the *base's* remote instead. The two differ only when the base is a local branch (`main`) or tracks another remote (`upstream/main`). A push always goes to `origin`, so `origin` is what authority must be decided on. A repository whose base does not track `origin` never publishes without asking, and `publish_ticket` refuses it, because its PR would cross repositories, which the watcher cannot follow (`pr_watch.rs:194`).

### 2. Fork status and visibility: one batched read, not the watcher's query
`repositories.rs` gains `read_github(repositories, gh)`, which runs off the host thread. For each repository it reads `origin` on disk, then asks GitHub about the distinct ones in one call per 50:

```
gh api graphql -f query=<QUERY> -f o0=<owner> -f n0=<name> -f o1=… -f n1=…
QUERY: r0: repository(owner: $o0, name: $n0) { nameWithOwner isFork visibility
         parent { nameWithOwner } owner { __typename } } … rateLimit { remaining resetAt }
```

It follows the watcher's rules: raw-string `-f` variables only, read-only.

`gh api graphql` exits non-zero when any one aliased repository cannot be resolved, while still printing the others' data, and `runtime::gh_output` discards the output of a failed call (`runtime.rs:87-103`). So `runtime.rs` gains `gh_answer(path, args)`, which returns the exit status, the output and the error line, as `git_answer` (`:49`) does for Git. `read_github` parses `data` whenever it is present. A repository whose alias came back `null`, such as one that was deleted or that this account cannot see, is recorded alone with the reason GitHub gave (`GitHubRepository.problem`); every other repository in the call gets its answer. Only an answer with no `data` at all, as when `gh` is not signed in, counts as a failed read.

- **Why not extend the watcher's per-repository query**, as the brief preferred: that query runs only for repositories with an open ticket. In this project that is 1 of 34. The human ticks owners before any ticket exists, and a list where 33 rows say "fork status unknown" is not one anyone can decide on. One extra call for the whole workspace costs 1 point; adding the fields to the watcher as well would give two writers for the same facts.
- **When it runs.** With each repository refresh (`service.rs:628`), for repositories whose facts are missing, older than a day, or whose `origin` changed. So: at launch, when repositories are added, and at most once a minute when the human returns while something is still unread.
- **The owner is the one GitHub reports.** A renamed or transferred repository still answers at its old URL. `nameWithOwner` gives where a push would really land. It is stored beside the owner and name the URL gives, the allowed list is checked against it, and it is the repository a request names; the URL's own owner and name are kept only to notice when `origin` is edited.
- **Failure.** If `gh` is missing, not signed in or rate-limited, earlier answers are kept, repositories without one stay unanswered, and the page says why. A repository with no answer, or one GitHub could not find, always asks.
- **This answer is for the page and for the first decision.** It can be a day old. What is actually sent is decided on a fresh answer (decision 7).

### 3. Where it is stored
All on JSON that is already stored; nothing new in SQL.

```rust
// workspace-core/src/lib.rs
pub struct Repository { …, pub github: Option<GitHubRepository> }      // `origin` on github.com
pub struct GitHubRepository { pub owner: String, pub name: String,      // as `origin`'s URL names it
    pub facts: Option<GitHubFacts>,                                      // None until GitHub answered
    pub problem: Option<String> }                                        // why GitHub gave no answer
pub struct GitHubFacts { pub name_with_owner: String,                    // where GitHub says it is now
    pub organization: bool, pub fork: bool, pub parent: Option<String>,
    pub public: bool, pub checked_at: i64 }
pub struct HostSettings { …, pub allowed_owners: Vec<String> }          // per machine
pub struct Ticket { …, pub publishing: Option<PublishRequest>,          // step running or waiting
    pub pushed: Option<String> }                                         // commit the host last pushed
pub struct PublishRequest { pub id: String, pub caller: String,        // one per accepted call
    pub repository: String, pub push_url: String,                        // what the card shows and
    pub branch: String, pub base: String, pub commit: String,            // what will be sent,
    pub title: Option<String>, pub body: Option<String>,                 // frozen at the request
    pub step: PublishStep,
    pub waiting: Option<String>,                                         // the Inbox item, when asking
    pub blocked_review_approved: bool }                                  // decision 8
pub enum PublishStep { Push, OpenPullRequest }
```

- `allowed_owners` holds entries: an owner's login, or `owner/name` for a fork. Logins cannot contain `/`, so the two never collide. Entries compare case-insensitively, as GitHub does.
- It goes in `settings.json` beside `workspaces_dir`, the other per-machine value, rather than in a new table. Change 5 (settings repository) must leave both out of what it syncs; the plan already lists them as per machine.
- Only the human's client can change it, through a new `Command::SetOwnerAllowed { entry, allowed }` handled by `settings.rs::set_owner_allowed`. No agent tool reads or writes the list.

### 4. One rule for "without asking", shared by the page and the host
A new `workspace-core/src/owners.rs` holds the rule and the grouping, so the desktop and the host cannot disagree:

- `owner_entry(&Repository) -> Option<String>`: the owner GitHub reports, or its `owner/name` for a fork; `None` without GitHub facts.
- `is_allowed(&Repository, allowed: &[String]) -> bool`: true only when the repository has a GitHub `origin`, GitHub has answered, its base tracks `origin`, and its entry is in the list.
- `owner_entries(&[Repository]) -> …`: the rows of the owners block: each owner with its repository count and how many are public, each fork with its parent, and the repositories with no GitHub remote.

Counts are of workspace repositories, so two clones of one GitHub repository count twice, as they do in the list below.

### 5. One tool: `publish_ticket`
`publish_ticket { ticket_id, title?, body? }`, added to `mcp.rs::tools` (`:17`).

- With no open PR on the ticket, it pushes the branch and then opens the PR; `title` and `body` are required.
- With an open PR (`Ticket.pull_request`, as the watcher last read it), it pushes; `title` and `body` are refused, because editing an open PR's text is not offered.

*Why one tool and not `push` plus `open_pull_request`:* the decided rules never allow a push without review, so "push only" is not a separate thing an agent needs before the PR exists. One call is one agent turn, and the host still asks the human about each step separately (decision 8). *Why not let agents pass a branch, remote or base:* every one of those is a way to aim the operation somewhere else. The host takes the branch from the ticket, the remote is `origin`, and the PR's base is the repository's configured base. A plan that wants a feature branch sets the repository's base to it.

### 6. What the host checks before anything leaves
A new `workspace-host/src/publish.rs` holds `Host::publish_ticket(caller, ticket_id, title, body)`. Every check here is local and fast, runs on the host thread, and refuses at once with its reason:

1. The caller is the ticket's coordinator (`owned_ticket`, `live.rs:879`) or an implementer among `ticket_agents` (`live.rs:302`). Verifiers are refused.
2. The ticket is open and no other step for it is running.
3. The ticket has a verification cycle and none is running, and `check_verified_head` (`live.rs:446`, made `pub(crate)`) accepts the worktree's HEAD: the commit the cycle's last round checked, or a patch-equivalent one. A ticket that was never reviewed is refused.
4. The worktree is on the ticket's branch (`git symbolic-ref --short HEAD` equals `Ticket.branch`). The host pushes that branch and no other.
5. `origin`, read again now, has exactly one push URL, its fetch URL names the same repository, the URL still names the owner and name recorded when GitHub was asked, and the base tracks `origin`.
6. Authority. A step runs without asking only when both hold: the cycle **passed**, and `is_allowed` is true. Otherwise the host asks the human (decision 8).

**The request is frozen.** When the checks pass, the host saves a `PublishRequest` on the ticket with a new id and everything that will be sent: GitHub's `owner/name`, the push URL, the branch, the base branch, the commit, and the title and description. The card is drawn from that record, and the steps are run from it. Nothing is read from the repository's current configuration at the moment of sending except to compare it with the record.

**Every later moment compares with the record.** When the human approves a card, and again at the start of every run on the publishing thread, the host makes checks 2 to 5 again and compares the current push URL, recorded repository, ticket branch and repository base with the frozen ones. If anything differs, or HEAD is no longer the frozen commit or patch-equivalent to it, or a new cycle has started, nothing is sent: the request is cleared, any waiting card is withdrawn, and the caller is told what changed and to call again. A new call is a new request with its own card, showing the new values. So an approval given for "into `main`" can never open a PR into `release`.

### 7. How the host pushes and opens the PR
Network work runs on its own thread, like the watcher's passes (`spawn_pull_request_check`, `service.rs:286`), and reports back with a new `Event::Published`.

- **First, GitHub's current answer** (a design decision made in review, round 1). The stored facts can be a day old, and a repository transferred to another owner keeps answering, and accepting pushes, at its old URL. So before anything is sent, the thread asks GitHub about this one repository (`nameWithOwner`, `isFork`; one read-only call) and compares it with the frozen `repository` and the fork status the decision was made on.
  - Same: go on.
  - Different: nothing is sent. The new answer is recorded, the request is cleared, and the caller is told the repository is now `<owner>/<name>` and to call again. The new call decides on the new owner, and asks the human if that owner is not allowed.
  - No answer (GitHub unreachable, `gh` not signed in, repository not found): nothing is sent and the caller is told why. An unconfirmed destination is never pushed to, with or without the human's approval.
- **Push.** `runtime.rs` gains `git_push(path, args)`, on `bounded_output` (`:112`) with a 10-minute deadline, because `git push` runs the repository's pre-push hooks. The thread reads GitHub's head for the branch with `git ls-remote origin refs/heads/<branch>` and then:
  - same commit: nothing to do;
  - branch absent, or its head is an ancestor of the commit: `git push --no-follow-tags --recurse-submodules=no origin <commit>:refs/heads/<branch>`;
  - its head is the commit the host last pushed (`Ticket.pushed`), as after a squash or a clean rebase: the same push with `--force-with-lease=refs/heads/<branch>:<that commit>`;
  - anything else: refuse. GitHub has commits the host did not put there, and overwriting them is the PR keeper's question (change 3), not this change's.
- **Only that one ref.** The explicit refspec overrides `remote.origin.push` and `push.default`. `--no-follow-tags` and `--recurse-submodules=no` override `push.followTags` and `push.recurseSubmodules`, which would otherwise send tags and submodule commits along. A remote configured as a mirror makes the command fail rather than send more. The tests set those options in the repository's and the global Git configuration and check that the destination gained exactly one ref.
- **Open.** `gh pr create --repo <owner>/<name> --head <branch> --base <base branch> --title <title> --body <body>` through `runtime::gh_output`, with every value taken from the frozen request. Arguments are passed as an argument list, never through a shell.
- **After.** `Host::record_published` saves `Ticket.pushed`, records a `ticket_pushed` or `pull_request_opened` activity event, starts or asks for the next step, and marks the repository due in the watcher (a new `Watch::check_now`, `pr_watch.rs:296`) so the ticket row shows the PR within seconds.

### 8. When the human is asked, and what the card says
When check 6 says ask, the host raises an Inbox item with `Host::request_attention` (`lib.rs:876`) under the operation `publish:<request id>:<step>` and notes it on the request.

- **Why the request id, and not the ticket and commit.** An operation can exist only once per session (`lib.rs:892-906`), and a second publish for the same step and commit is ordinary: after a denial and a corrected description, after a failed push, after a restart. Each accepted call is a new request with a new id, as provider permissions already use `permission:<new id>` (`service.rs:906`), so each gets its own card.
- **The `publish:` prefix is the host's.** Anything whose operation starts with `publish:` is drawn as an approval, so no agent may create one. `ensure_unreserved` (`lib.rs:105-112`) gains `publish:` beside the check-in prefixes, so `ask_user` (`live.rs:834`) and the client's `Command::RequestAttention` (`lib.rs:1202`) refuse such an id. And the host never reads a ticket, step or commit out of an operation id: when an approval is answered, it acts only if the answered item's id is the one a ticket's stored request names in `publishing.waiting`. An item no stored request names starts nothing.
- **It is an approval, not a question.** `Attention::is_permission` (`workspace-core/src/lib.rs:253`) becomes true for `publish:` operations too. The card then gets Deny and an approve button, the human cannot dismiss it, and the coordinator's `close_question` cannot close it. That is how "the human's approval is something the app records and checks" is met: the only thing that starts the step is the human's click on the host's own card, stored in the `attention` table.
- **The host writes the card.** The desktop draws it from the frozen request, the ticket's verification record and ledger, and the repository's facts: repository and visibility, branch, commit and its size, how the review ended, and for the PR the base branch and the full title and description. The agent's only words on it are the PR's title and description, which are what the human is approving.
- **Owner not allowed: each step asks.** The push and the PR are two cards of one request. Each later push to the same PR is a new request and asks again.
- **Review ended blocked: always asks, and the card says so** (the human's items 11, 16 and 17). It states that the review ended blocked and lists the open findings: every ledger entry that is neither fixed nor settled as follow-up or won't-fix. Showing each finding's id, severity, verifier, location and summary, naming any verifier that could not check, and the first and third points below are design decisions made in review.
  - In a repository whose owner is not allowed, that notice and list appear on each step's card.
  - In an allowed owner's repository there is one card for the commit (item 16). It names everything that would be sent (the push and, with no open PR, the PR and its title). Approving it sets `blocked_review_approved`, and the steps for that commit then run without a second card, because the owner is allowed and the review was the only reason to ask.
  - The approval covers one commit. A later push is a new commit with its own review.
- **Approve runs it.** `service.rs` handles the answer beside parked permissions (`:857`): `approved` makes the comparison in decision 6 and starts the step; anything else clears the request and tells the caller nothing was sent. The request and the Inbox item are both stored, so an approval given after a restart still works; parked provider permissions (`service.rs:398`) are in memory only, which is why this does not reuse that map.
- **Repeats.** While a card waits, the same call again (same commit, title and description) returns the waiting status. A call that differs in any of them withdraws the old card and starts a new request. Once a request has ended, by success, failure, denial or interruption, the next call is a new request.

### 9. The result arrives as a message
`publish_ticket` returns at once: a refusal, `publishing` (the steps the host is running) or `waiting_for_human` (the step and the reason). When a request ends, by success, failure, denial, withdrawal or interruption, the host sends its caller one message with id `publish:<request id>` and no sender, so it starts the caller's next turn. The id is the request's because `Host::send` refuses a repeated id with a different body or recipient (`lib.rs:721`): a second attempt at the same commit, or the coordinator retrying after the implementer, must still get its own result. `publish:` joins `HOST_NOTICE_PREFIXES` (`workspace-core/src/lib.rs:817`), so the turn input and the chat label it "Wiffletree".

*Why not hold the tool call open until the push ends:* a push with hooks can take minutes and a human's approval can take hours. Provider tool-call time limits differ, and a parked call does not survive a restart. One path for both cases is simpler than two.

A request whose step was running when the host stopped is ended at the next start, and the caller is told it was interrupted and to call again. Repeating is safe: pushing the same commit is a no-op, and `gh pr create` refuses a second PR for the same branch. Where the human must approve, the new request raises a new card.

### 10. The instructions, in Wiffletree's own words
`skills/communication.md` line 12 (version becomes v7). The two sentences that name the open-pr skill, including the coordinator's merge into a plan's feature branch, and the clause "except as the open-pr skill allows", are replaced by:

> Do not deploy, publish, change credentials, touch production, or alter main, the default branch or another shared branch without explicit human authorization. Nothing leaves this machine from your shell: never run git push, and never create, edit, comment on, review, close or merge a pull request yourself. To push a ticket's branch and open or update its pull request, call publish_ticket once the ticket's review has ended. The workspace runs the push itself: without asking when the review passed and the human allowed the repository's owner, which workspace_context shows, and otherwise only after the human approves each step in their inbox. A review that ended blocked always needs that approval. Write the pull request's title and description by the repository's and the human's own PR rules. Only the human merges.

The human confirmed this exact text on October 13, 2026 (item 15). It differs from the paragraph he approved on October 12 in two places: "approved for automatic PRs" became "allowed", and the blocked-review rule (item 11) was added.

`skills/main-coordinator.md` (v11) and `skills/implementer.md` (v5) gain one sentence each. The coordinator's: after a cycle ends, publish a ticket that ships as a PR with `publish_ticket`, or have the implementer do it; publish after a blocked cycle only where you would otherwise accept with a waiver, since the human sees the open findings and decides. The implementer's: when asked to publish, write the title and description by the repository's and the human's PR rules and call `publish_ticket`.

`workspace_context` (`live.rs::agent_context`, `:502`) shows each repository's `github` entry and a `publishes_without_asking` flag, so an agent knows beforehand whether the human will be asked.

### 11. What the human sees
See `design/mockups/current/repository-owners.html`.

- `repositories_view.rs::repositories_page` (`:7`): a block above the list, titled "Open pull requests without asking", with a tick box per owner and per fork, the repositories with no GitHub remote, the line "Wiffletree never approves, merges, closes or comments on a pull request.", and the reason when GitHub could not be read. Each repository row gains a label beside its base (`:98`): "without asking", "asks first" or "Not on GitHub".
- `inspector.rs::attention_card` (`:609`): the approval cards for a push, a PR and a blocked review.
- `sidebar.rs::ticket_row` (`:751`) with a text helper in `tree.rs`: a line under the ticket while a step waits for the human.
- `Snapshot` (`workspace-core/src/lib.rs:316`) gains `allowed_owners` and `github_error`, filled in `Host::snapshot` (`lib.rs:305`).

### 12. Documents
`README.md:51` and `:81` say what is now true: Wiffletree pushes a ticket's branch and opens its PR only through its own checked operation, without asking for owners you allow and after your approval elsewhere; it never approves, merges, closes or comments on a PR; and agents can still run any command on your Mac. `:87` drops "Integration is manual". `docs/DEVELOPMENT.md:226` and a short PRD paragraph follow.

## What this closes and what it does not

**Closes:**
- The instructions no longer point at a skill Wiffletree does not ship, and no longer leave "where may I push" to an agent's reading of text.
- An agent no longer needs its shell to publish. The path it is told to use checks the owner, the branch and the review every time.
- Through that path, an unreviewed commit, another branch, a tag, another remote or push URL, a merge and a comment are impossible, whatever any message or Git setting says. The owner is confirmed with GitHub before each publish, so a transferred repository is not pushed to on a stale answer. A commit whose review ended blocked leaves only after the human has seen the open findings.
- Where the human must approve, a coordinator's or another agent's "the human said yes" does nothing. Only the human's click on the host's card starts a step.

**Does not close:**
- An agent's shell can still run `git push`, `gh pr create`, `gh pr merge` or `gh pr comment` with the human's credentials. Agents run with permission prompts bypassed (`provider.rs:168`, `:178`). Only the instructions stand in the way.
- The same shell can edit `settings.json`, the store or a repository's remotes. The allowed list is a record the app checks, not a secret an agent cannot reach.
- Nothing here notices a push made outside the app.

Closing these needs credentials the agents do not hold, or a sandbox. Neither is part of this change.

## Risks / Trade-offs

- [A pre-push hook hangs or takes very long] → the push has a 10-minute deadline and is killed; the caller is told it failed and may call again.
- [The host's `git push` lacks credentials the human's terminal has, such as an SSH agent] → the push fails with Git's own error in the result message. Nothing is retried silently.
- [GitHub facts on the page are up to a day old] → the page can lag, but nothing is sent on them: each publish confirms the owner and fork status with GitHub first (decision 7). Visibility is shown but does not decide authority.
- [GitHub cannot be reached when a ticket is published] → nothing is sent and the caller is told; it calls again later. `gh pr create` would have failed anyway.
- [A pre-push hook sends something itself] → hooks are the repository's own code and run as they do for the human's pushes; this change does not inspect them.
- [The watcher has not yet seen a PR the host just opened, and `publish_ticket` is called again] → `gh pr create` refuses a second PR for the branch; the watcher is told to check at once.
- [An allowed owner includes a repository where opening a PR itself has effects, such as CI with cloud credentials] → accepted; the human chose owners only (item 8). The stored list already takes `owner/name` entries, so an "ask first" exception could be added later without a migration.
- [A coordinator publishes blocked reviews routinely and the human approves by habit] → the card leads with the blocked result and the open findings, and the coordinator's instructions limit it to cases it would otherwise waive.
- [`workspace_context` grows by a few fields per repository] → about 100 bytes each; 34 repositories add under 4 KB to a result that is already large.
- [Two cards per first publish feels heavy in a repository the human trusts] → allowing its owner removes both.

## Migration Plan

No store migration: every new field is optional with a default, and `allowed_owners` starts empty, so nothing publishes without asking after the update until the human ticks an owner. The instruction files are sent on every turn, so running sessions get the new text on their next turn. To roll back, revert the change; the extra JSON fields are ignored by older builds.
