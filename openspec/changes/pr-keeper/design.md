## Context

See proposal.md for why. The code this builds on, at origin/main e45cd30:

- **The watcher.** `workspace-host/src/pr_watch.rs`: one read-only `gh api graphql` query per repository per pass (`read`, `:66`; fields in `PULL_REQUEST_FIELDS`, `:147`), every 60 seconds while a repository has an open PR (`Watch`, `:288`). `Host::record_pull_request` (`:444`) saves a changed snapshot on `Ticket.pull_request`, and `pull_request_notices` (`:490`) wakes the coordinator once per head for failed checks or a conflict, at most 3 times per PR (`WAKE_BUDGET`, `:17`). `service.rs::refresh_pull_requests` (`:827`) starts the passes and `Event::PullRequests` (`:1181`) records them.
- **Publishing.** `workspace-host/src/publish.rs`: `Host::publish_ticket` (`:357`) checks the ticket (`check`, `:515`), freezes a `PublishRequest` and runs each step off the host thread (`run`, `:142`). `push` (`:245`) sends one refspec to the frozen URL and replaces GitHub's head, with a lease, only when it is the commit the host last pushed or was once this branch's tip (`remote_branch`, `:209`). Whether the human is asked is `workspace-core/src/publishing.rs::publish_ask` (`:24`) on top of `owners.rs::is_allowed` (`:30`). The request's one result message goes to `PublishRequest.caller` (`end_request`, `:830`).
- **The reviewed commit.** `live.rs::verified_head` (`:900`) accepts HEAD when it is the commit the latest cycle verified or has the same `worktrees::patch_id` (`worktrees.rs:548`) against the repository's base.
- **Review.** `verification.rs::verify_ticket` (`:356`) needs the ticket in `ready_for_testing`, a level, and the coordinator's exact model for each verifier; `level_may_start` (`:115`) lets a re-check start only after a passed cycle; `end_round` (`:866`) sends a failed round's findings to the implementer and wakes the coordinator once when the cycle ends. `VerificationSettings::level` (`workspace-core/src/lib.rs:765`) turns a level's name into its verifiers.
- **Worktrees and turns.** A ticket agent's working directory is the ticket's worktree (`live.rs::ticket_agent`, `:259`, `workdir` at `:298`). `service.rs::worktree_busy` (`:2245`) gives a worktree one writer or several readers. `provider.rs::configure` (`:183`) launches Claude with `--dangerously-skip-permissions --tools default` and Codex with `--dangerously-bypass-approvals-and-sandbox`.
- **The base.** `overlaps.rs::fetch_base` (`:106`) updates only the base's remote-tracking ref, and `conflicts` (`:239`) lists conflicting files with `git merge-tree --write-tree`.
- **Settings.** `HostSettings` (`workspace-core/src/lib.rs:535`) in `settings.json`; `ModelSelection` (`selection.rs:47`) saved from the Models page; `Repository` (`lib.rs:136`) as stored JSON.
- **Measured here** (Git 2.54.0, Claude Code 2.1.296, Codex 0.160.0, October 13, 2026):
  - A branch that changes one line was rebased onto a base that had changed a line two lines away. Git rebased it with no conflict, and its `git patch-id --stable` against the base changed (`134284e…` to `e339c5e…`), because the diff's context lines changed. So a rebase Git calls clean is not always the same patch as the reviewed commit.
  - `claude --help` documents `--tools ""` ("disable all tools"), `--strict-mcp-config`, `--json-schema`, `--no-session-persistence` and `--system-prompt`. `codex exec --help` offers a read-only sandbox but nothing that removes its tools.
  - One live run in an empty folder, `claude -p --model haiku --tools "" --strict-mcp-config --no-session-persistence --output-format stream-json --verbose --system-prompt <text> --json-schema <schema>`, with three sample comments on stdin, one of them telling the model to run a shell command. The run's `init` event listed one tool, `StructuredOutput`, which is how `--json-schema` returns the answer, no MCP server, and `permissionMode: default`. The model's only action was that tool, and the `result` event carried `structured_output` with one class per comment id (the hostile one as `no_action`) and its `usage`. So "no tools" here means exactly: no tool but the one that returns the answer. Slice 3's first task repeats this check before any code depends on it.

## Words

- The **PR keeper**, or keeper: the part of the host that keeps a ticket's PR current. A repository is **kept** when the keeper acts there.
- A **try**: one run of the keeper's model at a conflict or at failed checks. A try is **thrown away** when its result is not used.
- The **keeper's checkout**: a temporary Git worktree the keeper works in, never the ticket's.
- **Handed to the coordinator**: the keeper stops and sends the coordinator one message.
- "Allowed", "without asking" and "approve" keep the meanings the publishing change fixed.

## Goals / Non-Goals

**Goals:** plain code wherever a model is not needed; every push through the existing checked operation, with nothing widened; a thrown-away try leaves the ticket exactly as it was; no new table, migration or scheduler; each slice useful alone and small.

**Non-Goals:**
- Merging, approving, closing, commenting, resolving threads or reacting.
- Keeping PRs that are not a ticket's, or a ticket's PR in a repository that asks first.
- A sandbox or scoped credentials for the keeper's model (see "What this closes and what it does not").
- Detecting a broken build after a rebase that Git called clean. GitHub's checks do that; the failed-checks trigger follows.
- The "Keeping a PR" skill text and the Skills page (change 4), and syncing the triggers (change 5).

## Decisions

### Who decided what

**Decided by the human on October 12, 2026** (the plan and the planning interview): the ten points listed in the ticket's brief, numbered here H1 to H10. H1 the small model has its own setting in Models. H2 triggers are configurable; his setup is every PR rebased and conflict-free at all times. H3 a clean rebase is plain code after the base has been quiet for a couple of minutes. H4 a conflict or failed check gets one try behind a gate, then a Light review before the push; on failure the try is discarded and the coordinator told. H5 comments are sorted into three classes, read only. H6 commits Wiffletree did not make are adopted, never dropped or rewritten. H7 one try per problem per head, no caps. H8 never merge, approve, close or post. H9 no asking in allowed owners' repositories; elsewhere he is in charge before anything is public. H10 polling stays in Rust.

**Decided by the human on October 13, 2026**, on this proposal and its mockup: "Approve with the recommended answers". What he approved, as the coordinator put it to him:
- (a) "Rebased" means the app force-pushes the PR's branch with a lease, refusing if GitHub's branch is not what it last saw. He was told the cost: every commit id on the PR changes, and "changes since your last review" resets for reviewers. Merging the base in instead was the alternative he did not take.
- (b) The keeper acts only in repositories whose owner is allowed. Elsewhere it is off and raises nothing.
- (c) It keeps only the PRs of this workspace's open tickets.
- (d) Four triggers on the Repositories page, all off until ticked, and no model chosen, after an update and on a fresh install.
- (e) The triggers are set once for the workspace, with one checks command per repository.
- (f) What the code forces:
  1. A rebase is pushed without review only when its patch is the same as the reviewed one. Otherwise it is reviewed first, or the coordinator is told.
  2. The scope of a try is the files Git reports as conflicted, and for failed checks the files the PR already changes.
  3. The checks command is the human's own, per repository. No command means no tries there.
  4. The keeper's review uses the models the coordinator last chose for those verifiers on that ticket.
  5. Adopted commits are kept as they are, and nothing more is pushed until a review passes at the new head.
  6. Comment sorting runs with every tool off, needs a Claude model and covers only people who can push. Messages carry links, never comment text.
  7. One OpenSpec change, built as five PRs.

He was also told, and accepted:
- A try that fails is thrown away including its review record (decision 6).
- The model making a fix has an ordinary shell with his credentials. The app checks what it leaves behind, not everything it ran (see "What this closes and what it does not").

**Everything else in this document is a design decision made in review, not the human's.** That includes the exact conditions in decision 2, the conflicts trigger needing the rebase trigger, the record of a head that is not reviewed (decision 8), the 20-comment and 4,000-character bounds, the rule for when waiting comments stop showing, setting an adopted ticket to `ready_for_testing`, and running the checks on a rebase whose patch changed.

### 1. What the keeper is
Host code, in a new `workspace-host/src/keeper.rs`, driven by the watcher's passes. It has no session of its own and no prompt. After each recorded pass (`Event::PullRequests`, `service.rs:1181`) the actor asks `Host::keeper_due()` what to do for each kept ticket and starts at most one keeper job per ticket on its own thread, as publish steps run (`spawn_publish_step`, `service.rs:411`). A job reports back with a new `Event::Kept`.

A model appears in two places only, each an ordinary agent of the ticket with a new `Role::Keeper` (label "PR keeper"), a child of the ticket's coordinator, pinned to the model in `ModelSelection.keeper`:
- a **try session**, created for one try and archived when it ends, with the keeper's checkout as its working directory;
- a **sorting session** per ticket, which only ever runs with no tools (decision 7).

Being ordinary agents gives three things for free: their runs are `provider_runs` rows, so Usage counts them; a turn of 30 minutes checks in with the coordinator, who may stop it (H7); and the human sees and can stop them in the tree.

*Why not a keeper agent that decides what to do:* H10 keeps watching in Rust, and every rule here (where it may act, one try, the gate, what is pushed) must hold whatever a model does. A model that only edits files inside a gate needs no authority.

### 2. Where it applies
A ticket's PR is **kept** when all of these hold. Otherwise the keeper does nothing for it, and its PR is watched as today:
- the ticket is open, its coordinator is not archived and its project is not paused;
- its repository publishes without asking (`owners::is_allowed`), which already requires an allowed owner, GitHub's answer and a base on `origin`;
- its PR is open.

Sorting comments needs nothing more, since it changes nothing. The keeper **changes a kept ticket's branch** only when these also hold:
- the latest verification cycle passed and none is running;
- the worktree's HEAD is the reviewed commit or patch-equivalent to it (`live::verified_head`);
- the PR's head is the worktree's HEAD;
- the ticket records no head that is not reviewed (decision 8);
- no publish request is running or waiting.

Together these are the conditions under which `publish_ticket` would push without asking. The keeper therefore never needs an Inbox approval, and never raises one. It does one thing to a ticket's branch at a time.

- *PR head ahead of the worktree:* someone else pushed; see decision 8.
- *Worktree ahead of the PR:* an agent is working or a review is pending. The keeper waits.
- *A review that ended blocked:* publishing it always needs the human, so the keeper leaves the branch alone.

### 3. The settings
```rust
// workspace-core/src/lib.rs
pub struct HostSettings { …, pub keeper: KeeperSettings }       // serde default: everything off
pub struct KeeperSettings { pub rebase: bool, pub conflicts: bool, pub checks: bool,
    pub comments: bool, pub quiet_minutes: u32,                  // 2; accepts 1–60
    pub review_level: Option<String> }                           // a re-check level's name
pub struct Repository { …, pub checks: Option<RepositoryChecks> }
pub struct RepositoryChecks { pub command: String, pub minutes: u32 }   // 20; accepts 1–120
// workspace-core/src/selection.rs
pub struct ModelSelection { …, pub keeper: Option<ModelProfile> }
```
- The triggers and the quiet period are one value for the workspace, changed by a new `Command::SetKeeper` handled by `settings.rs::set_keeper`, beside `set_check_ins` (`:141`). Only the human's client can send it.
- Saving refuses `conflicts` or `checks` without a model and a `review_level` naming a level whose kind is re-check, and refuses `comments` without a Claude model.
- Saving refuses `conflicts` without `rebase`, and `rebase` off while `conflicts` is on. A conflict is found by the keeper's own rebase, which runs only with the rebase trigger; without it no try could start, and the watcher's held wake (decision 10) would never be sent.
- `Repository.checks` is the one per-repository value, changed by `Command::SetRepositoryChecks`. The command is the human's own text, run with `/bin/sh -c` in the keeper's checkout. A repository without one gets rebases and comment sorting, and no tries.
- `ModelSelection.keeper` is saved with the rest of the Models page and must be an allowed model of an enabled provider (`ModelSelection::validate`). It is `None` until the human chooses.

### 4. The clean rebase
**When it is due.** The watcher's query gains `baseRefOid`, GitHub's commit for the PR's base branch. It is carried on `pr_watch::Found`, not stored on the ticket, so a moving base logs no event. The actor remembers, per repository and base branch, the commit it last saw and when it first saw it. The rebase trigger is due for a kept ticket when that commit is not contained in the PR's head (`git merge-base --is-ancestor`) and the actor has seen the same commit for `quiet_minutes`. A new commit on the base restarts the wait. With passes 60 seconds apart, "quiet for 2 minutes" means between 2 and 3 minutes; after a restart the wait starts again.

*Why not GitHub's `behind`:* GitHub reports `BEHIND` only where branch protection demands an up-to-date branch. *Why not the commit's date:* that is when it was written, not when it reached the branch.

**The job** (`keeper::rebase`, off the host thread):
1. `overlaps::fetch_base`, then stop unless the base's ref is the commit the wait was measured on.
2. `git worktree add --detach <workspaces_dir>/keeper/<ticket id> <head>`, then `git rebase <base commit>` there.
3. Report one of: **same change** (the rebased commit has the reviewed commit's `patch_id`), **changed** (Git rebased it but the patch differs, as measured above), or **conflict** with the files Git named. The checkout is removed either way; the rebased commit stays in the repository under `refs/wiffletree/keeper/<ticket id>` until it is used or dropped.

**Moving the ticket's branch** (`Host::keeper_adopt`, on the host thread, the only place the keeper touches a ticket's worktree). It runs only while the worktree is on the ticket's branch, at the commit the job started from, with no uncommitted or untracked files, and no agent of the ticket is in a turn (`self.active`, as `worktree_busy` reads it). Then `git reset --hard <result>`. If an agent is in a turn, the result waits and is tried again after later passes. If HEAD has moved or the worktree is not clean, the result is dropped and the next pass decides anew. The old tip stays in the branch's reflog, which is what lets the push replace it.

**The push.** `Host::keeper_publish(ticket_id)` in `publish.rs` builds a `PublishRequest` through the same code as `publish_ticket` (`check`, the frozen request, `start_step`, `run`), with `keeper: true` and no title or body. Nothing in the path is relaxed:
- `check` still requires HEAD to be the reviewed commit or patch-equivalent to it, so only a **same change** rebase can be pushed here. The ticket's review record is not touched; equivalence is computed when asked, as today.
- `remote_branch` still replaces GitHub's head only when it is the commit the host last pushed or a former tip of this branch.
- The owner is still confirmed with GitHub before sending.
- If `publish_ask` would ask, as when the owner was unticked a moment ago, the keeper's request ends with nothing sent. A keeper request never raises an Inbox item.
- `end_request` sends a keeper request's result to `Host::keeper_published` instead of to a caller, so a rebase wakes nobody.

**What the ticket records.** `Ticket.keeper.last`: what was done, when, the commits before and after, and the base commit. An activity event `pull_request_rebased`. `Ticket.pushed` moves as for any push.

**When it is not clean.**
- *Conflict:* with the conflicts trigger off, or no checks command, or the try already used, nothing changes: the watcher's message tells the coordinator, as today. Otherwise decision 5.
- *Changed:* the commit is not pushed, because no review covers it. With the conflicts trigger on and a checks command, the host runs the checks on it and then the keeper's review (decision 6), with no model; it is not a try. A review that fails there is withdrawn like any keeper review, because the rollback record belongs to the review, not to a try. Otherwise the coordinator is told once per head and base that the rebase needs a re-check.
- A given pair of head and base is rebased once (`Ticket.keeper.last`), so a failed push is not repeated until one of them changes.

### 5. One try by the model, behind a gate
**What starts one.** A conflict from decision 4, or `Ticket.pull_request.checks == Failure` with the checks trigger on. In both cases the repository needs a checks command and the ticket must have no earlier try for this problem at this head.

**Failed checks are reproduced first.** The job runs the repository's checks in the keeper's checkout at the PR's head. If they pass, there is nothing to fix here: no model runs, and the coordinator is told the failure could not be reproduced on this machine.

**The try.** The host creates the try session with `live.rs::ticket_agent` (`:259`), which takes the keeper's checkout as `workdir` for `Role::Keeper`, pins it to `ModelSelection.keeper`, and sends it one message from Wiffletree. For a conflict the host has already started the rebase, which stopped at the conflict; the message lists the conflicted files. For failed checks it names the PR number and the checks command. The session works with the provider's ordinary tools and ends its turn. `worktree_busy` (`service.rs:2245`) ignores `Role::Keeper` sessions, since they do not use the ticket's worktree, so an implementer's turn is never held by a try. `skills/keeper.md` is its role text (`provider.rs::skill`, `:107`).

**The gate** (`keeper::gate`, off the host thread, after the session's turn ends). It reads the result; nothing the session says is believed.
1. The checkout is clean, not in the middle of a rebase, and its HEAD contains the base commit (conflict) or the PR's head (failed checks).
2. **Scope.**
   - *Conflict:* `git merge-tree --write-tree <base> <head>` gives the tree Git's own merge produces and the files it could not merge. `git diff --name-only <that tree> <result>` must name only those files. Outside the conflicted files the result is byte for byte what Git produced.
   - *Failed checks:* `git diff --name-only <head> <result>` must name only files the PR already changes against its base.
3. **Checks.** The repository's command exits 0 in the checkout within its time limit (`runtime::bounded_output`).

A gate that fails throws the try away: the checkout and the ref are removed, the try is recorded as discarded with the reason, and the coordinator is told. The ticket's worktree, branch, review and ledger were never touched.

*Why compare the result and not police the session:* the session has a shell with prompts bypassed, so no instruction limits what it runs. What it leaves behind can be checked exactly.

### 6. The review before the push
A result that passed the gate is moved onto the ticket's branch with `keeper_adopt` (decision 4) and reviewed there, because verifiers work in the ticket's worktree and the push sends the worktree's HEAD. Nothing has left the machine. If an agent is in a turn in the worktree, the result waits; if the worktree's HEAD has moved or it is not clean, the try is thrown away.

`Host::keeper_review` in `verification.rs` starts a cycle through the code `verify_ticket` uses, extracted as `start_cycle`, with these differences:
- the level is `KeeperSettings.review_level`, and `level_may_start` applies unchanged, so it starts only on a passed ticket and never leaves out a verifier with an open finding;
- no `ready_for_testing` is needed, since the host made the commit;
- each verifier's model is the one the coordinator last chose for that verifier on this ticket (the profile of the session that last ran its focus), still checked with `permits_verifier`. The host never picks a model: a verifier with no earlier choice on the ticket ends the try, and the coordinator is told;
- one round, and `Verification.keeper = true`, shown as "Light re-check by the PR keeper".

`end_round` (`:866`) treats a keeper cycle differently in two ways:
- **Passed:** the cycle stays as the ticket's latest, and `keeper_publish` pushes. The coordinator gets the ordinary cycle message only when the pass left findings to triage.
- **Not passed:** `Host::withdraw_keeper_review` throws the review away with the try. Findings are not routed to the implementer and no second round runs.

**The record of a keeper review.** When `keeper_review` starts a cycle it saves `Ticket.keeper.review`: the cycle's number, the commit the branch had before the keeper moved it, the ticket's state then, and the try it belongs to, if any. Every keeper review has one, so a rebase whose patch changed rolls back the same way a try does. A keeper review does not start while the ledger holds an `open` entry (after a passed cycle there is none), so the cycle can only add ledger entries, never change one.

**What a withdrawal does.** It applies only to the cycle `Ticket.keeper.review` names, while that cycle is the ticket's latest and marked as the keeper's. Any other cycle is never touched.
1. *At once, in one store write:* the ticket's latest cycle is the one before again, with its outcome and rounds unchanged; the ledger loses exactly the entries that cycle recorded, found by their source cycle, and no others; the ticket's state is what the record says it was. Every other entry stays as it is at that moment. The coordinator can call `triage_findings` while the review runs (`verification.rs:464` does not look at cycles), and a decision it records then on an earlier entry is kept; restoring a copy of the ledger would have erased it. The withdrawn cycle, its verdicts and its findings, with any decision recorded on them, are kept on the review's record. The activity log keeps its `verification_started` entry and gains `keeper_review_withdrawn`.
2. *The cycle's number is not reused.* The next cycle takes the number after the withdrawn one, because verifier messages are keyed by cycle number and a reused number would be mistaken for a message already sent.
3. *Then the branch is put back,* under the same rule as any move: the worktree is on the ticket's branch at the keeper's commit, clean, and no agent of the ticket is in a turn. The reporting verifier's turn is usually still winding down at that moment, so the reset waits until the worktree is free.
4. *The coordinator is told after the branch is back,* not before, so it cannot dispatch an implementer onto the discarded commit.
5. *If the branch cannot be put back,* because the worktree is not clean or its HEAD is no longer the keeper's commit, nothing is forced. The records stay restored, and the keeper's commit is recorded as a head that is not reviewed (decision 8), so `publish_ticket`, `accept_ticket` and the keeper all refuse until a later cycle ends, whatever the commit's patch id. The coordinator's message names the commit the worktree is at and the reviewed commit.

Verifier sessions are not removed: a verifier keeps its session and its conversation, as between any two cycles.

*Why withdraw instead of keeping a blocked cycle:* a blocked re-check leaves the ticket unacceptable and forces the next review to be a full one, which counts against the cycle cap. A try that failed would then cost the ticket more than not trying. "Thrown away" has to include the review.

*Why not have the coordinator run the review:* it would cost a coordinator turn per fix, and a failed one would be an ordinary blocked cycle with the cost above.

### 7. Comments
**Reading** (`keeper::read_comments`, off the host thread, read-only). Due when `Ticket.pull_request.comments` (the watcher's cursors) changed since the last sort. One `gh api graphql` query, with raw-string `-f` fields and the host named as the watcher does, reads the PR's newest comments, reviews and review-thread replies with each one's id, URL, creation time, path and line, GitHub's author association and body.

**Who is sorted.** Only comments whose author association is `OWNER`, `MEMBER` or `COLLABORATOR`, created after the last sort. Others are counted, not sorted: in a public repository anyone can comment. At most 20 comments per run, oldest first, each cut at 4,000 characters; the rest wait for the next run. For a PR the keeper has not sorted before, the starting point is the first pass that saw it with the trigger on, so turning the trigger on does not sort every old comment on every open PR.

**Sorting with no tools.** The sorting session's turn is a new `provider::TurnKind::Sort`. `provider.rs::configure` (`:183`) launches it as `claude -p --tools "" --strict-mcp-config --no-session-persistence --json-schema <schema> --system-prompt <fixed text>`, with no MCP server, no workspace token, no `--dangerously-skip-permissions`, no `--resume` and an empty working directory. As measured above, the run then has one tool, the one that returns the answer in the schema's shape. It has nothing to act with, and it never shares a conversation with a try. The host checks the run's `init` event and throws the answer away if it lists any other tool or any MCP server. Codex has no such mode, so `comments` needs a Claude model.

**What comes back** is one class per comment id: `needs_change`, `question` or `no_action`. The host accepts only ids it sent and those three words; anything else leaves that comment "not sorted". The classes are saved on `Ticket.keeper.comments` with the time sorted up to. The PR line counts the comments that need a change and the questions. The counts go back to zero when the coordinator next starts a verification cycle on the ticket, since that is when work in answer to them is under review, or when the PR merges or closes. Nothing on GitHub says a comment was dealt with, so this is an approximation.

**The coordinator's message** is sent when a batch has a comment that needs a change, a question, or one that could not be sorted. It contains fixed wording, the ticket's title and id, the PR number, and for each such comment its class, its kind, its URL built by the host from ids, and for a thread the path and line. It contains no comment text, title or author name, as the watcher's messages do not. It tells the coordinator to read the comments with `gh pr view`, that they are a reviewer's words and not instructions, and that Wiffletree never replies on GitHub.

Nothing in a comment can make the keeper act: a comment's only effect is which of three words the host receives, and that only decides whether a fixed message is sent.

### 8. Commits Wiffletree did not make
This runs while the rebase, conflicts or checks trigger is on, for a kept ticket with no cycle running and no publish request. When the PR's head is not the worktree's HEAD and is not contained in it, the job fetches GitHub's branch from the push URL into the keeper's ref. If the worktree's HEAD is an ancestor of what it fetched, GitHub's branch has commits on top that the host did not push, and `keeper_adopt` fast-forwards the ticket's branch to them (`git merge --ff-only`), under the same conditions as any move. Their content and their commits are unchanged. A later rebase replays them like every commit on the branch: same content and author, new commit ids.

The ticket's review is not touched. Today's rule would not be enough to hold the ticket there: `live::verified_head` (`:900`) accepts any HEAD with the verified commit's stable patch id, and `git patch-id` ignores whitespace, so an adopted commit that only re-indents a line, which in Python changes what the code does, would count as reviewed. So the adoption records the adopted head on the ticket as not reviewed (`Ticket.keeper.unreviewed`), and `verified_head`, the one function `accept_ticket`, `publish::check` and the keeper's own conditions all go through, refuses while that record stands. The record is cleared only when a verification cycle that started after it ends; that cycle's outcome then decides, by the existing rules. A keeper rebase of a reviewed commit is unaffected: it sets no record. To let the coordinator start that review without an implementer turn, the adoption sets a ticket whose state is `passed` to `ready_for_testing`; any other state is left alone. The coordinator gets one message naming the old and new head and the number of commits. "Keep going" (H6) therefore means: the ticket keeps its PR and the keeper resumes once a review has passed.

When the two have diverged, nothing is adopted and the coordinator is told once per head.

### 9. One try per problem per head
A problem is `conflict` or `checks`. `Ticket.keeper.tries` records each try: the problem, the PR's head, the base commit, when, and how it ended (pushed, or discarded with the reason). A try is due only when the list has none for that problem at that head. A new head, from an implementer's fix or an adopted commit, allows a new try. A clean rebase is not a try and is not limited, except once per head and base. There is no count per PR and no budget (H7). A try that was running when the host stopped is recorded as discarded at the next start and counts as that head's try, so a crash cannot repeat a model run without end.

### 10. The watcher's messages
`pull_request_notices` (`pr_watch.rs:490`) holds back its conflict or failed-checks wake for a head when the keeper will try that problem there or has already tried it (`keeper::handles`: the trigger is on, which for a conflict means the rebase trigger is on too, the repository has a checks command, the ticket's branch may be changed as decision 2 defines, or a try for that problem at that head is recorded). The coordinator then hears once, from the keeper, and only if the try is thrown away.

GitHub often reports the conflict before the quiet period has passed, so the wake is held back before any try exists. The host notes it on the ticket (`Ticket.keeper.held`). On each later pass, a held wake whose try has started is dropped; one whose head has changed or whose problem is gone is dropped; and one the keeper can no longer try, as when an implementer started working in the worktree or the trigger was unticked, is sent then, in the watcher's own words. So a held wake is never lost.

Everything else the watcher sends is unchanged, including its budget of 3, which keeper messages do not count against.

Keeper messages use the id prefix `keeper:` and the sender label "Wiffletree PR keeper" (`workspace-core/src/lib.rs::host_notice`, `:1093`). Each is sent once: `keeper:<ticket>:<what>:<head>`.

### 11. What the human sees
See `design/mockups/current/pr-keeper.html`.
- `repositories_view.rs::repositories_page` (`:13`): the "Keep pull requests up to date" block under `owners_block` (`:146`); each row's "kept" or "not kept" label and its checks command.
- `models_view.rs`: a "PR keeper" card beside `review_card` (`:370`).
- `tree.rs::pr_line` (`:122`) and `sidebar.rs::pr_line` (`:1411`): the keeper's part at the end of the PR line.
- `team_view.rs::tickets` (`:99`): the keeper's last action on the ticket's card, and "by the PR keeper" on its cycle line.
- `live.rs::agent_context` (`:490`): each ticket's `keeper` record and each repository's `kept` flag in `workspace_context`.
- `skills/main-coordinator.md`: one paragraph on what the keeper does and what its messages ask for. `skills/implementer.md`: one sentence, that the workspace may rebase the ticket's branch between its turns.

## Slices

One PR each, in this order. Each is useful alone and leaves the others off. The line counts are estimates of production code, without tests.

| # | Slice | Adds | Est. lines |
|---|---|---|---|
| 1a | Clean rebases: the host and its switch | `KeeperSettings` with the rebase trigger and quiet period, `baseRefOid`, `keeper.rs` (due, checkout, rebase), `keeper_adopt`, `keeper_publish`, the triggers block and row labels | 400; measured 722 lines of code, 830 with comments |
| 1b | Clean rebases: what the human sees | the PR line's keeper part and the card | 130 |
| 2 | Commits Wiffletree did not make | fetch and fast-forward, the message, "not reviewed" on the PR line | 120 |
| 3 | Sorting comments | `ModelSelection.keeper` and its card, `Role::Keeper`, `TurnKind::Sort`, reading, the classes, the message | 320 |
| 4 | A try at a conflict | `Repository.checks`, the try session, the gate, `keeper_review` and `withdraw_keeper_review`, tries, the watcher's held wake | 520 |
| 5 | A try at failed checks | reproduce first, the scope rule for checks | 120 |

Slices 2 and 3 depend only on slice 1. Slice 4 depends on slice 3 for the keeper's model and its role and on slice 2 for the record of a head that is not reviewed, and slice 5 on slice 4.

**One change, five PRs** (decided, (f) 7); slice 1 became two PRs, 1a and 1b, on October 14, 2026, when it measured about twice its estimate. The delta specs are written so that each requirement belongs to exactly one slice, and tasks.md lists each slice's requirements. Each slice's PR implements its tasks and syncs its own requirements into the main specs, and the last PR archives the change.

## What this closes and what it does not

**Closes:**
- A PR that only needs a rebase no longer costs a coordinator turn, an implementer turn and a review.
- A small model can fix a PR without being trusted: the app checks which files changed, runs the checks itself and has the result reviewed before anything is pushed.
- Comment text reaches a model that has no tools, and never reaches a host message.

**Does not close:**
- The try session runs with permission prompts bypassed, like every agent. Its shell can run `git push` or `gh` with the human's credentials. The gate judges what it left in the checkout, not what else it did.
- The scope gate compares files. Inside a conflicted file the model may change more than the conflict; the review is what judges that.
- A rebase Git calls clean can still break the build. Nothing local checks it (H3); GitHub's checks do.
- The coordinator still reads comments itself, with tools. A hostile comment can reach it there, as it can today. Sorting only the repository's own people narrows that; it does not remove it.
- Comments by outsiders, and by most apps and bots, are not sorted. An edited comment is not sorted again.
- The keeper acts only while Wiffletree is running.

## Risks / Trade-offs

- [Every rebase re-runs the PR's checks on GitHub, and a busy base means many rebases] → the quiet period; it is a setting.
- [A force-push changes every commit id on the PR, so reviewers lose "changes since my last review", and a repository set to dismiss stale approvals drops them] → accepted by the human, (a).
- [Checks run from a fresh checkout, so a Rust repository builds from nothing on each try] → accepted for the first version; one try per head bounds it.
- [Withdrawing a review edits the ticket's cycles and ledger, which nothing does today] → it is one function, the withdrawn cycle is kept on the try's record, it only ever removes the cycle the keeper's own review record names and the ledger entries that cycle recorded, and the human accepted it.
- [The waiting-comments counts are cleared by a rule, not by GitHub] → a question answered without a code change stays on the PR line until the next review or the merge.
- [The implementer's next turn finds its branch rebased] → the move happens only between turns on a clean worktree, and its instructions say so.
- [A checks command is the human's shell text] → only the human's client can set it; no agent tool does.
- [The model sorts a comment wrongly] → the coordinator's message links every comment it was told about and counts the rest; the PR line still shows GitHub's unresolved threads.
- [The quiet timer is in memory] → a restart waits one more quiet period.

## Migration Plan

No store migration: every new field is optional with a default. After the update and on a fresh install every trigger is off, no keeper model is chosen and no repository has a checks command, so nothing is rebased, tried, sorted or pushed until the human turns a trigger on. To roll back, revert the change; older builds ignore the extra JSON fields.

## Open decisions

None. The human answered (a) to (f) on October 13, 2026; see "Who decided what".
