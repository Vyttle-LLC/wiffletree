## Why

Wiffletree watches each ticket's pull request and tells the coordinator when it conflicts with its base or its checks fail. Everything after that costs a coordinator turn, an implementer turn and a review, even when the fix is a rebase Git can do alone. The human decided on October 12, 2026 that Wiffletree should keep open PRs current itself: "Wiffletree keeps an eye on PR's that are active and then activates the small model (configured in models) to keep it up to date and current", with his own setup being "all my PR's to be rebased and resolved conflicts at all times". This is change 3 of 6 in the plan `plan-pr-keeper-and-skills.md`; it builds on the PR watcher, on app-run publishing with allowed owners (change 1) and on review levels (change 2).

## What Changes

- **A PR keeper, mostly plain code.** The host decides when a PR needs work, rebases, runs checks, enforces limits and pushes. A model runs for three things only: one try at a conflict, one try at a failed check, and sorting new comments. Watching stays in the Rust PR watcher; no model is used to watch.
- **Four triggers, all off until ticked.** "Rebase when the base branch moves", "Try to resolve conflicts", "Try to fix failed checks" and "Sort new comments", set once for the workspace on the Repositories page under the owner list. The conflicts trigger needs the rebase trigger, since a conflict is found by the keeper's own rebase. After an update and on a fresh install every trigger is off and no model is chosen, so nothing is pushed until the human turns it on.
- **Only where publishing already needs no asking.** The keeper acts only in repositories that publish without asking today (an allowed owner, GitHub has answered, the base tracks `origin`). Everywhere else it is off: nothing is rebased, tried or sorted, and no Inbox item is raised. It keeps only the PRs of this workspace's open tickets.
- **A clean rebase is plain code.** When a PR's base has moved and has then been quiet for a couple of minutes (a setting, 2 by default), the host rebases the branch in a checkout of its own, moves the ticket's branch to the result and pushes it through the publishing path, replacing GitHub's branch with a lease. No model and no review: the push happens only when the rebased commit carries the same change as the reviewed one, by the patch-equivalence rule publishing already uses.
- **A conflict or a failed check gets one try by the small model, behind a gate.** The app, not the prompt, enforces the gate: the result must sit on the current base, change only the files Git reported as conflicted (for a failed check, only files the PR already changes), and pass the repository's checks, a command the human enters per repository. Then a review at a re-check level the human picks ("Light") runs before the push. If the gate or the review fails, the try is thrown away, nothing about the ticket has changed, and the coordinator is told so it can dispatch an implementer.
- **One try per problem per head.** A conflict, or failed checks, at a given head commit gets one try, recorded on the ticket. There is no other cap. The keeper's model runs are ordinary runs of a "PR keeper" agent, so the coordinator's check-ins cover a long one.
- **Comments are sorted, read only.** New comments from people who can push to the repository are read by the host and sorted by the model into needs a change, a question or no action, in a run with every tool turned off. The coordinator is told about the first two with fixed wording, links and paths, never the comment's text. Nothing is posted, resolved or reacted to.
- **Commits Wiffletree did not make are adopted.** When GitHub's branch gains commits on top of what the host pushed, the host fast-forwards the ticket's branch to them, unchanged, and tells the coordinator. The new head has not been reviewed, and the ticket records that: nothing is pushed or accepted until a review that started afterwards has ended, even when the adopted commits leave the patch id unchanged.
- **A new setting in Models: the PR keeper's model.** One exact provider, model and effort from the models allowed on this machine.
- **What the human sees.** The ticket's PR line ends with what the keeper is doing ("up to date", "rebased 1 min ago", "keeper resolving", "with the coordinator", "1 needs a change"); the ticket's card says what it last did and why the review stands; each repository row says whether it is kept and holds its checks command; the keeper's runs appear in Usage under their own role. Mockup: `design/mockups/current/pr-keeper.html`.
- **It never merges, approves, closes or posts.** No new operation writes to GitHub except the existing push of a ticket's branch.
- **The limit is stated.** The keeper's model session, like every agent, runs with permission prompts bypassed. The app checks everything it sends itself; it does not sandbox a model.
- **Not included:** the Skills page and a "Keeping a PR" skill text, the settings repository, replying to or resolving comments, merging, draft PRs, PRs that are not a ticket's, per-owner or per-repository triggers, asking the human for each keeper push, and any sandbox for agents' shells.

## Capabilities

### New Capabilities
- `pr-keeper`: the triggers and where they apply; the clean rebase and its quiet period; the one try at a conflict or a failed check, its gate, its review and how it is thrown away; one try per problem per head; comment sorting with no tools; adopting commits Wiffletree did not make; the keeper's messages to the coordinator; what the ticket, the repository row and Usage show; the boundary and the stated limits.

### Modified Capabilities
- `pr-watch`: each pass also reads the base branch's head commit, kept in memory; where the keeper will try a conflict or failed checks, the watcher's own wake for that head is held back, and sent after all if the keeper can no longer try; the ticket's PR line gains the keeper's part.
- `ticket-publishing`: the host may start a publish itself for the keeper, with exactly the checks `publish_ticket` makes and only where no approval would be needed; its result goes to the keeper's record, not to a caller. Every publish is refused while the ticket has adopted commits no review has checked, whatever their patch id.
- `ticket-workspaces`: `accept_ticket` is refused in the same case.
- `ticket-verification`: the host may start one review cycle for the keeper at a re-check level, with one round and the models the coordinator last chose for those verifiers on the ticket; a keeper review that does not pass is withdrawn from the ticket's cycles and ledger, its number is not reused, and the branch is put back only when the worktree is free, clean and unchanged.
- `model-routing`: the saved model selection gains the PR keeper's model, checked against the allowed models like any other choice.

## Impact

- `workspace-core`: `HostSettings.keeper` (triggers, quiet minutes, review level), `ModelSelection.keeper`, `Repository.checks`, `Ticket.keeper`, `PullRequest.base_head`, `PublishRequest.keeper`, `Verification.keeper`, `Role::Keeper`, the `keeper:` notice prefix, two commands for the settings.
- `workspace-host`: a new `keeper.rs` (what is due, the checkout, the rebase, the gate, comment reading, the messages); small additions to `pr_watch.rs`, `publish.rs`, `verification.rs`, `service.rs`, `provider.rs`, `settings.rs`, `live.rs`; a new `skills/keeper.md`.
- `workspace-desktop`: `repositories_view.rs` (triggers block, checks field, "kept" label), `models_view.rs` (the model), `tree.rs` and `sidebar.rs` (the PR line), `team_view.rs` (the card).
- Stored data: optional fields on JSON already stored. No table and no migration.
- Runtime: the same authenticated `gh` and Git the watcher and publishing use. A rebase pushed to GitHub re-runs the PR's checks there.
- Built in five slices, each its own PR and useful alone; see design.md, "Slices".
- Docs: `README.md`, `docs/DEVELOPMENT.md`, a paragraph in `docs/PRD.md`.
