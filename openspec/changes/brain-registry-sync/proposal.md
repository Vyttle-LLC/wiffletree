## Why

Wiffletree can attach one brain folder to a project (`attach_brain`, `Project.brain`) and export logs into it, but it never commits, pushes or reviews anything. Outside Wiffletree, agents write the brain directly through the `brain-log` skill, which finds it through one pointer per machine (`~/.config/claude-second-brain`), and a launchd safety-net commits and pushes to `main` every 4 hours. That breaks down now:

- The workspace mixes organizations, so one pointer per machine is wrong: Vyttle work logs into the Sage brain today.
- On October 29, 2026 Michael decided that brains are **team-shared**: Michael, RB and Oscar log to and read the organization's brain.
- He also decided that **every change reaches `main` through a pull request that auto-merges when its checks pass.** The gate itself (gitleaks, sensitive-information scan, markdownlint, wikilinks, log schema and size, path ownership, and the gate's own tests) was built in `sagehaven-docs` PR #1, and moves into Wiffletree as a versioned action in the `brain-gate` change.

Michael approved the Brains page in the design study `design/mockups/current/brains.html` on the same day. This change is the foundation the other three brain changes build on: `brain-log-capture`, `brain-compile` and `brain-browser`.

## What Changes

- **A workspace Brains registry on each machine.** A brain is a GitHub repository (`owner/name`). Adding one on the Brains page states what it allows: Wiffletree may push its own batch branches to that repository, open pull requests into `main` and turn on auto-merge for them. Allowing a repository's owner for ticket PRs does not grant this, and adding a brain does not allow ticket PRs.
- **Repository → brain mapping.** Each GitHub owner maps to one brain or to none, and each repository can override its owner. Personal and unowned repositories default to no brain. A project's brains come from its repositories; the project stores no brain of its own.
- **A brain config file in the brain repository.** `wiffletree-brain.json` at the repository root names the brain's folder, its compiler host, and the compile time and timezone, and holds the gate's config (`brain-gate`). Every install reads the version merged on `main`. Set up adds the file and installs the gate's pinned caller in one PR. Changing the compiler host from the Brains page opens a PR that changes this file, so every install agrees.
- **A host-owned checkout.** The host keeps its own clone of each brain under the workspaces folder, apart from any clone the human edits or opens in Obsidian. It only fast-forwards `main` there and writes only on batch branches in worktrees of that clone.
- **The batch flow.** Each batch of changes is a branch `wiffletree/brain/<login>/<batch>` that is pushed, opened as a PR into `main` and set to auto-merge. Its checks are watched without a model, and `main` is fetched again after the merge. The host never pushes `main`, never forces a push and never merges a PR itself; GitHub merges it when the required checks pass.
- **Failures reach the Inbox.** A failed check, a refused push, a merge conflict or auto-merge being off on the repository raises one Inbox item on the project that wrote the batch, saying what to do. For a failed check, the human can amend the unmerged batch: redact the reported line, or add the gate's inline allow marker with a reason. Either way the host pushes one more commit and the checks run again. Entries become append-only once merged.
- **The legacy safety-net is kept away from managed brains.** The host's clone carries a marker that the dotfiles safety-net skips (dotfiles ticket), and the host never adopts the human's own clone as its checkout.
- **Earlier attachments are migrated.** `attach_brain` and `Project.brain` are removed. A path attached before is offered on the Brains page as a brain to add.
- **The Brains page** lists the brains, the compiler host with the next and last compile, the PR gate (each batch's stage, from queued to auto-merged) and the mapping. The brain browser opened from it is specified in `brain-browser`.

## Capabilities

### New Capabilities
- `brain-registry`: the per-machine registry of brains, the repository → brain mapping, the brain config file, the host-owned checkout, the batch branch → PR → auto-merge flow, failures as Inbox items, and the registry part of the Brains page.

### Modified Capabilities
<!-- None. project-memory's export requirement is removed by brain-log-capture, which replaces it. -->

## Impact

- `crates/workspace-host/src/brains.rs` and `brains.sql` (new): registry, mapping, config reading, the checkout, batches and the PR watcher for brain PRs. It reuses the `gh` reach and credential-helper rules of the settings repository (`settings-repository`) and the polling rules of `pr_watch.rs`.
- `crates/workspace-host/src/memory.rs`: `attach_brain` is removed; `export_logs` is replaced in `brain-log-capture`.
- `crates/workspace-core/src/lib.rs`: `Project.brain` is removed with a load-time migration; new `Brain`, `BrainMapping`, `BrainBatch` and `BrainConfig` types.
- `crates/workspace-desktop`: a Brains page in the sidebar's Workspace section, beside Repositories.
- Attention items for brain batches on the project that wrote them.
- `brain-gate` defines the checks, the entry contract and the caller that Set up installs. This change does not change the gate.
- The `dotfiles` safety-net ticket reads the managed-checkout marker. That work happens in the dotfiles repository, not here.
- `docs/PRD.md`: Markdown storage and ownership, and the "Brain commit and sync" job.
