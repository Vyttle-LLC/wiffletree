## Context

The current memory code (`crates/workspace-host/src/memory.rs`) attaches a folder to a project (`attach_brain`), stores captured logs in `work_logs`, and exports them into `second-brain/logs/<repo>.md` under a lock file. It never commits or pushes. The desktop's `memory_view.rs` shows the logs and a capture form. `Project.brain` is null for every project created since the coordinator model.

Outside Wiffletree, the `brain-log`, `brain-compile` and `brain-query` skills read `~/.config/claude-second-brain` for one brain per machine. A launchd job (`brain-safety-net`, dotfiles) commits and pushes that checkout to `main` every 4 hours.

The gate was built in `sagehaven-docs` PR #1 (branch `wiffletree/brain-pr-gate-lint-sensitive-data-scans-log-sche`) and moves into Wiffletree as a versioned action in the `brain-gate` change. Its README defines the PR classes and the log entry contract that `brain-log-capture` writes. Sage-Social is on GitHub's Team plan, which has auto-merge and required checks but no merge queue for private repositories. The gate's path ownership rules make a merge queue unnecessary: log PRs only add files, and compile PRs never touch `logs/`.

The settings repository (`openspec/specs/settings-repository`) already pushes from the host to a GitHub repository chosen on each machine, using gh's login through a per-command credential helper. This change follows the same reach rules.

## Goals / Non-Goals

**Goals:**
- One registry of brains per machine, built from the brains its Spaces name, replacing the per-machine pointer inside Wiffletree.
- Every brain change made by Wiffletree reaches `main` only through a PR that auto-merges once the gate passes.
- One writer per checkout, so Wiffletree and the safety-net never share one.
- Failures are visible and fixable from the Inbox.

**Non-Goals:**
- Writing log entries (`brain-log-capture`), compiling (`brain-compile`), and browsing or search (`brain-browser`).
- Building or changing the gate's checks (`brain-gate`), or changing repository settings on GitHub. Michael enables auto-merge and branch protection himself.
- Teammates' machines without Wiffletree. They log through the skills (`sage-agents`), which write the same entry contract.
- Sharing the registry between machines through the settings repository.

## Decisions

### Decided by Michael (October 29, 2026)

- Brains are team-shared, and every change reaches `main` through a PR that auto-merges when its checks pass.
- There is one Brain page in the workspace, filtered by project.
- Personal and unowned repositories default to no brain. Under Spaces, this means they suggest no brain.
- The Space decides the brain, and the repository → brain owner mapping is dropped (decided after `assistant-spaces-jobs-boards` merged). The GitHub owner only suggests a brain when a Space is created. Capture, compile, search and `query_brain` resolve the brain from the project's Space. The mapping drawn in `brains.html` is superseded.
- The compiler host can be changed from the page, but it is stored in a config file in the brain repository and changed through a PR, so every install agrees.
- Amending an unmerged batch to fix a failed check is fine. Entries become append-only once merged.

### Decided in this proposal, for review

1. **The host keeps its own clone.** It lives at `<workspaces folder>/brains/<name>`, beside `projects/` and `tasks/`, and is never the human's working copy.
   - Two writers on one checkout is the failure mode the design draft names. With PR-only changes the host never needs the human's checkout: the human's own edits go through their own PRs.
   - Obsidian and manual edits in the human's clone stay legal and untouched.
   - The cost is disk space for a second clone, which is small for a Markdown repository.
   - The PRD's earlier rule about detecting external edits by hash still holds: if anything other than the host changes the host's clone, the host stops writing to that brain and raises an Inbox item. It does not overwrite the change.
2. **The registry and grants are kept on each machine**, like the allowed owners (`repository-owners`). The registry is derived from the Spaces on the machine, and shared Spaces travel through their storage repositories (`spaces`). Confirming a brain grants a push, and that grant belongs to the machine: joining a shared Space that names a brain still asks before this machine pushes to it.
3. **Confirming a brain is a separate grant from allowed owners.** It allows exactly three operations on that one repository: pushing branches under `wiffletree/brain/`, opening PRs into `main`, and turning on auto-merge for them. This follows the settings repository's rule that choosing it is what allows the push.
   - `ticket-publishing`'s "the host never approves, merges, closes or comments" stays true for ticket PRs. For a brain PR, turning on auto-merge asks GitHub to merge once the required checks pass; the host still never merges directly.
4. **The config file is `wiffletree-brain.json` at the repository root**, outside `second-brain/`, so a PR that changes it is an "other docs" PR under the gate's path ownership. Its shape (version 1):
   ```json
   {
     "version": 1,
     "brain_dir": "second-brain",
     "compiler": { "host_id": "…", "name": "Michael's Mac" },
     "compile": { "at": "02:00", "timezone": "America/New_York" },
     "gate": { "version": "brain-gate-v1.0.0", "…": "see brain-gate" }
   }
   ```
   The compiler is named by the host's stable host id (`host_identity`), with a display name. A brain without the file can still take logs. Its page shows "No compiler" and a Set up action. Set up opens one PR that adds the file with this host as the compiler, together with the gate's pinned caller, its `gate` config and `CODEOWNERS` lines (`brain-gate`). Because `CODEOWNERS` covers those files, that PR waits for the owner's review instead of auto-merging. Unknown keys are kept when the host rewrites the file. A version newer than the host understands is read for logging only, and the page asks the human to update Wiffletree before compiling.
5. **Batch branches are `wiffletree/brain/<login>/<batch-id>`**, where `<login>` is the GitHub login gh reports and `<batch-id>` is a lowercase ULID. One batch is one branch, one PR and one squash merge. The PR title names the batch's kind and source (for example "Log batch: Call relay retry (3 entries)"). Its body lists the files it adds and says it was opened by Wiffletree; it never copies entry text.
6. **The host turns on auto-merge with squash**, as `gh pr merge --auto --squash`. When GitHub refuses because auto-merge is off for the repository or `main` has no required checks, the PR stays open and one Inbox item tells the human which repository settings to apply. The `sagehaven-docs` README lists them.
7. **Brain PRs are watched like ticket PRs** (`pr-watch`): read-only `gh` calls, about every 60 seconds while a batch PR is open, with the same backoff. The watcher reads each check run's name, conclusion and annotations' file and line. It never copies check output text, so a sensitive-scan match is never shown.
8. **Inbox items go to the project that wrote the batch.** A batch with no project (an import, a ruling made on the Brains page) raises its item on the Brains page instead. Each batch raises at most one open item at a time. The item is replaced when the batch's state changes and cleared when the batch merges or is discarded.
9. **Amending an unmerged batch** offers three actions:
   - **Redact the line:** the host replaces the value of the reported field line with `[removed: <rule>]`.
   - **Not sensitive:** appends `<!-- gate:allow <rule> <reason> -->` with the human's reason.
   - **Discard the batch:** closes the PR and deletes the branch the host pushed. Its entries return to pending in `brain-log-capture`, unless the human discards them too.

   The host commits the amendment on the batch branch, pushes without force and the checks run again. Only the human chooses these actions; agents cannot.
10. **The safety-net:** the host writes `.git/wiffletree-managed` in its clone. The dotfiles ticket makes the safety-net skip any checkout with that marker and open PRs instead of pushing to `main`. The host also refuses to register a path as its checkout, so the legacy pointer can never name the host's clone. Retiring the pointer belongs to the skills tickets.
11. **Migrating `Project.brain`:** the `spaces` upgrade turns each attached folder into a Space's brain. Where the folder's `origin` names a GitHub repository, that repository becomes the Space's brain, but the host pushes nothing until the human confirms the grant in decision 3. `work_logs` and `log_exports` are migrated in `brain-log-capture`.

### Options considered

- **Write in the human's clone.** This was rejected: it means two writers, it conflicts with Obsidian and manual edits, and it could push half-edited files.
- **Share the registry through the settings repository.** This was deferred: a push grant per machine is the safer default.
- **One PR per entry.** This was rejected by Michael's design: entries are batched, one PR per session or ticket and one per compile run.

## Risks / Trade-offs

- **[Auto-merge off or `main` unprotected]** → PRs would wait open, or merge without checks if `main` has no required checks. Mitigation: the host reads the repository's auto-merge setting and `main`'s required checks with `gh` when a brain is added and on each pass. It shows a warning on the Brains page, and raises an Inbox item when one of the gate's seven checks is not required on `main`.
- **[GitHub unreachable]** → Batches wait locally as queued, with backoff up to 30 minutes and Sync now, as in the settings repository. Nothing is lost, because the entries are in the host store.
- **[Two installs as compiler]** → Only the host whose id is in the merged config compiles (`brain-compile`). A config PR that is still open changes nothing.
- **[A teammate pushes straight to `main`]** → The host's clone only fast-forwards, so it reads their commit. If the fast-forward fails, the host stops writing to the brain and raises an item; it never resets.

## Migration Plan

1. Ship with the registry empty. Existing projects lose `brain` at load, and earlier attachments are offered on the Brains page.
2. Michael adds `sagehaven-docs` and `vyttle-docs`, and sets the owner defaults.
3. Michael applies the repository settings from the gate README and merges the gate. Until then the Brains page shows the warning from Risks.
4. The dotfiles ticket makes the safety-net skip managed checkouts.

Rollback: removing a brain deletes the host's clone after confirmation and leaves GitHub untouched. Open batch PRs stay on GitHub for the human to close.

## Overlaps with other proposals

The other proposals are:
- PR #162, "Proposal: shared issues in a tracker repository" (`shared-issues`, `issue-assistant`). It is **merged on `main`** as 6194b99a and is an open change there, not yet archived. Where a spec both change, the brain changes go second.
- PR #163, "Proposal: assistant, Spaces, scheduled jobs and Boards" (`assistant-spaces-jobs-boards`). It is **merged on `main`** as 419a55ef and is an open change there, not yet archived.
- PR #164, "Coordinators create projects with create_project". It is **merged on `main`** as a8dded2d and its change is archived.
- the other changes open on `main`: `files-panel`, `processes-panel` and `quiet-findings`;
- `brain-gate`, in this proposal.

| Shared concept | Where else | Share or align | Open question |
| --- | --- | --- | --- |
| Git-backed shared repository on GitHub, reached as gh does | `settings-repository` (spec); `shared-issues` (#162, merged on `main`) reuses its reach, clone and backoff | **Share** one host module for reaching, cloning, fetching and backing off, used by settings, trackers and brains. | This change goes second. Should the shared-issues build extract the module, or this one when it reuses it? |
| How writes reach `main` | Settings and trackers push `main` directly (never forced); brains go only through PRs that auto-merge | **Align, keep different on purpose.** Settings and issues are a conversation that needs seconds-level sync; brain content needs the gate. | Should a tracker ever be PR-gated, for example for the sensitive-information scan on issue text? See `brain-gate`'s overlap notes. |
| Where the host keeps its clone | Trackers: `<data folder>/trackers/<owner>-<name>/`; brains: `<workspaces folder>/brains/<name>` (decision 1) | **Align.** Use one root and one naming scheme. The recommendation is `<data folder>/brains/<owner>-<name>/`, beside trackers and the settings clone, which also avoids name clashes between owners. | Confirm the data folder as the root for every host-owned clone. |
| Files Wiffletree did not write | Trackers hold back a file in this Mac's clone that Wiffletree did not write; brains hold all writing on any unexpected change | **Align** on "never push what Wiffletree did not write". Brains are stricter because their writes are PRs that auto-merge. | None. |
| Workspace-level registries kept on each machine | Several trackers per Mac, not shared through the settings repository (#162 decision 6); brains (decision 2); allowed owners (`repository-owners`) | **Align**: all per machine, all changed only by the human. | One Workspace page for shared repositories (settings, trackers, brains), or separate Issues and Brains pages as drawn in each mockup? |
| Suggesting from the GitHub owner | The brain mapping is dropped (Michael, 2026-10-29): a Space's code repositories' owner only suggests its brain. Issues list `repositories: owner/name` per issue, and trackers are per team with a prefix | **Align**: a Space could name its tracker the way it names its brain, with the owner suggesting both. | Should a Space name its tracker, so that New issue defaults to it? |
| Identity | Both use gh's `login`, read at set-up and when gh's account changes | **Share** one cached identity for entry authors, issue authors and assignees. | None. |
| Watching PRs | `pr-watch` watches ticket PRs; #162 adds linked PRs; this change adds batch PRs | **Share** the watcher: one pass per repository covering every kind of PR. | Does one cadence fit all three, or do brain batch PRs need their own faster loop while checks run? |
| Inbox items | Brain batch failures here; conflicts in `brain-compile`; #162 phase 5 adds issue mentions and paused fields | **Align** on one card per source with at most one open item, cleared by the source's own state. | One Inbox section per kind, or one list sorted by age? |
| Human-only write commands | #162 decision 11: every tracker write is a human command on the host socket | **Align**: brain registry changes, amendments and rulings are likewise human-only commands. | None. |
| Inspector rail | `files-panel` and `processes-panel` add rail entries; the Memory panel stays where it is | **Align** the rail order (Git, Files, Processes, Memory). This change only adds the "No brain" state. | None. |
| Ticket review | `quiet-findings` | None. | None. |
| Which brain a project uses | #163 (merged): every project belongs to a Space, a Space names a brain (a repository and a folder), and the coordinator logs to its Space's brain. Here, each repository maps to a brain by GitHub owner, and a project's brains come from its repositories | **Decided by Michael, 2026-10-29:** the Space decides the brain. The owner mapping is dropped, and the owner only suggests a brain when a Space is created. | None. |
| How brain writes reach `main` | #163 (merged) adds `project-memory` "Brain pushes from several people": fetch, rebase and push to `main` with a union merge of shared log files. That is its defaulted decision (c), "confirm at reconciliation" | **Decided by Michael, 2026-10-29:** PR-gated writes supersede (c). `brain-log-capture` removes #163's "Brain pushes from several people", and assistant entries use the per-entry contract (`logs/assistant/…`). | None. |
| Brain settings shared in a repository | #163 keeps a Space's definition in its storage repository (`spaces/<name>/space.yaml`) with the settings repository's rules; here, `wiffletree-brain.json` lives in the brain repository and changes only by PR | **Align**: shared facts live in Git and per-machine facts (clone paths, models) stay local, in both. | Should the brain's compiler and gate settings move into the Space definition, or stay with the brain because several Spaces may name one brain? |
| The Brains page and Settings › Spaces | #163 (merged): Spaces are edited on Settings › Spaces, and the assistant and Boards are pinned above Projects; here, Brains is a Workspace page | **Align** the navigation. | Is a brain configured inside a Space's settings, on its own Brains page, or both? |
| Host-owned clones | #163 (merged): storage clones live at `<data folder>/storage/<owner>-<name>/` and reuse settings sync | **Share** the repository module, as with trackers. This supports the clone-location recommendation above. | See "Where the host keeps its clone". |
| Projects created by a coordinator | #164 (merged): `create_project` takes optional `repository_ids` and defaults to every workspace repository; the project records `created_by` | **Settled by Michael's Space decision (2026-10-29):** the created project joins the creating project's Space and uses that Space's brain, whatever its repository selection. | None. |

## Open Questions

None blocking. Sharing the registry between Michael's machines can be revisited if it drifts.
