## 1. Core types and storage

Depends on `spaces`, which names each Space's brain and provides `github_access.rs` and the clone root.

- [x] 1.1 Add `Brain`, `BrainConfig` (`wiffletree-brain.json` v1) and `BrainBatch` (kind, source project or thread, branch, PR, stage, checks) to `crates/workspace-core/src/lib.rs`.
- [x] 1.2 Add `crates/workspace-host/src/brains.sql`: `brains` (with each brain's push grant), `brain_batches`, `brain_batch_checks`. At first start, turn each Space's local brain folder that the `spaces` upgrade left (an earlier `Project.brain`) with a GitHub `origin` into that repository as its Space's brain, waiting for the grant. Test the upgrade with and without a GitHub `origin`.

## 2. Registry and config

- [x] 2.1 `brains.rs`: build the registry from the brains this machine's Spaces name. A brain is confirmed (granted) by the human only, and leaves the registry with its clone when no Space names it. Find the push URL through `github_access.rs`, and clone into `<data folder>/repos/github.com/<owner>/<name>/` with the managed marker; refuse a brain that is a storage or settings repository here. Refuse a path named by the legacy pointer. Test that agents have no tool for any of these, and that two Spaces naming one brain register it once.
- [x] 2.2 Resolve a project's brain from its Space, a thread's or job run's from its own Space, and a `create_project` project's from its creator's Space. Suggest a brain from the code repositories' GitHub owner when a Space is created, pre-filled only. Test the resolution cases and that personal repositories suggest no brain.
- [x] 2.3 Read `wiffletree-brain.json` from `main` only. Validate the JSON, the folder and the version. Set up opens one PR with the config file and `brain-gate`'s caller, `gate` config and `CODEOWNERS` lines (`brain-gate` task 4.1). Compiler changes open a config-only batch, keeping unknown keys. Test the missing, invalid and newer-version cases.

## 3. Checkout and batches

- [x] 3.1 Fetch and fast-forward only. Detect changed files and a failed fast-forward, and hold writing with a Brains page item. Test both without reset or overwrite.
- [x] 3.2 Batch lifecycle: worktree from `main`, commit with the human's identity, push `wiffletree/brain/<login>/<batch>` without force, `gh pr create`, `gh pr merge --auto --squash`. Persist stages across restart. Test, with a fake `gh` and remote, that `main` is never pushed and no push is forced.
- [x] 3.3 Refused auto-merge: keep the PR open and raise an item naming the settings. Offline: keep the batch queued with backoff, and Sync now.

## 4. Watching and the Inbox

- [x] 4.1 Watch brain PRs read-only, with `pr-watch`'s cadence and backoff. Store each check's name, conclusion and annotation file and line, never text. Test that no output text reaches the store.
- [x] 4.2 Inbox items per batch: on the writing project, or on the Brains page. At most one open item, replaced on change, cleared on merge or discard.
- [x] 4.3 Amendments: redact the line, not sensitive (with reason) and discard, from the human's client only. Each is one commit pushed without force. Nothing is offered after merge. Test each action and that agents cannot call them.

## 5. Desktop

- [x] 5.1 Add Brains to the sidebar's Workspace section. The Brains page shows the brain cards with the Spaces that name each, and the PR gate list with checks per batch, following `design/mockups/current/brains.html` minus its superseded repository mapping. It offers Sync now, Set up, the compiler change, the grant confirmation and links to each Space's settings. Show the warnings for auto-merge, required checks and the legacy pointer.
- [x] 5.2 Remove the attach-brain control from `memory_view.rs`. The Memory panel shows "No brain" with a link to the Brains page; its full summary arrives in `brain-browser`.

## 6. Docs and checks

- [x] 6.1 `docs/PRD.md` already describes this change (this proposal's PR). Check it still matches the implementation and update it if not.
- [x] 6.2 Run the new `brains` tests and the `memory` and migration tests, then the full suite once before ready_for_testing.
