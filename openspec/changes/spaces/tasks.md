Five stages, each a ticket or a few, each landing on its own. This change lands before every other open change that names a Space, a tracker or a brain (see design.md, "Order of changes"). Stages 1–3 add no visible UI beyond the Space in the project Overview; stage 5 adds Settings › Spaces and New project's Space.

## 1. Spaces in the core and the store

- [x] 1.1 Core: `space.rs` (`Space`, `SpaceId`, `SpaceLocal` facts, sources, `TrackerConfig` with format and prefix), `SessionOwner` (`Project`, `Space`, `Brain`); `Project.space` replaces `Project.brain`; Space commands.
- [x] 1.2 Host: the Spaces store and the upgrade that creates Spaces from `Project.brain` folders and "Personal", with unique names and proposed prefixes. Verify two brains, two folders of one name, no brain, archived projects, export records kept, and a second start changing nothing.
- [x] 1.3 Host: every project belongs to a Space; `Host::attach_brain` sets the Space's brain; exports go to the Space's brain. Verify an upgraded project exports nothing twice and a moved project exports later entries to its new Space.
- [x] 1.4 Host: the enforced hierarchy with session owners and no hidden project records; the coordination tools refused to sessions owned by a Space or a brain. Verify a worker owned by a Space is refused.
- [x] 1.5 Host: Space commands are human-only on the host socket. Verify an uncredentialed connection is refused for every Space write and answered for reads.

## 2. The shared repository module

- [x] 2.1 Host: move `candidates`, `gh_protocol`, `gh_login`, `first_readable`, `gh_json` and the visibility and readers check from `sync.rs` into `github_access.rs`, with `sync.rs` unchanged in behavior (#162 task 1.2). Verify with the existing settings repository tests.
- [x] 2.2 Host: `repos.rs`, one engine per repository under `<data folder>/repos/`: clone, fetch at the shortest cadence of its rule sets, per-file records, push rules, backoff, one commit per rule set, files under no rule set pulled and never committed. `sync.rs` becomes the settings rule set on it. Verify the settings repository tests unchanged, and two rule sets in one repository making two commits.
- [x] 2.3 Host: move a settings repository's clone from `<data folder>/shared/` to `<data folder>/repos/<owner>-<name>/` at first start; keep `<data folder>/shared/` without one; finish an interrupted move. Set up and join make the clone under `repos/` the shared folder (reusing a storage clone) and move only the settings files of `shared/` to `backups/`; switching from another settings repository first runs Stop syncing for it, and no clone ever moves to `backups/`. Stop syncing copies the settings files back to `shared/` and stops only the settings rule set. Verify both machines, the interrupted case, set-up and join into a storage clone, a switch away from a repository that is also a Space's storage (its clone stays and its Space and issue files keep syncing), and Stop syncing with and without a Space in the same repository, each keeping every setting's value. Space storage arrives in stage 3, so here a clone already under `repos/` and the Space files in it are stand-ins; stage 3 verifies them with real Space and issue rule sets.
- [ ] 2.4 Host: one role per repository (brain, or storage and settings). Verify a brain refused as storage or settings, and storage that is also the settings repository sharing one clone. *Moved to stage 3: it needs Space storage and brains.*

## 3. Storage and the tracker

- [ ] 3.1 Host: local storage repositories (`git init` at `repos/local-<id>/`, no remote) whose first commit holds `spaces/<space>/space.yaml` and `wiffletree-issues.yaml`.
- [ ] 3.2 Host: `space.yaml` and `wiffletree-issues.yaml` read and written in YAML, unknown keys kept; prefix rules (2 to 5 capitals, unique on this Mac; a GitHub tracker's changed only with no issue; a local tracker's renamed at any time, rewriting its issue IDs and references). Verify a rename of a local tracker with linked issues, and a join refused for a clashing prefix that succeeds after the rename.
- [ ] 3.3 Host: a GitHub storage repository: the settings repository's check, the first commit with the Space's folder and, when missing, the tracker config; the Space rule set over `spaces/` with pauses, outside-edit holds and workspace notes; Set up issues for a joined repository without a tracker config.
- [ ] 3.4 Host: Add Space from a repository: list its Spaces, add the ticked ones, machine-local facts, model fallback notes, refuse a prefix already used here.

## 4. Defaults and scans

- [x] 4.1 Host: `create_project` puts the project in its creator's Space and, without `repository_ids`, uses the Space's repositories on this Mac, naming the ones not here. Verify both and that `repository_ids` still wins.
- [x] 4.2 Host: New project's default Space (last used) and default repositories, as one command the desktop calls.
- [x] 4.3 Host: Space membership of repositories by `owner/name` (several Spaces or none), changed only by human commands that edit `space.yaml`; the owner suggestion when a repository is added; the upgrade's suggestions by the projects that use each owner's repositories, held until confirmed. Verify a repository in two Spaces, No Space, a refused agent call, and that confirming nothing writes nothing. *The commands edit the Space record's `repositories`, the list `space.yaml` holds, until stage 3 stores it in YAML.*
- [ ] 4.4 Host: run `brain-gate`'s `sensitive-info` and `secret-scan` (its tasks 1–2) on changed files under `spaces/`, `issues/` and `wiffletree-issues.yaml` before a push; hold failing files and files a check could not scan, with a workspace note that never shows the match. Verify a phone number, a token, a `gate:allow` marker, a missing gitleaks and a local repository that is never scanned.

## 5. Desktop and docs

- [ ] 5.1 Desktop: Settings › Spaces (list, page, New, From a repository, edit, remove, sources), with the Storage and issues box (repository, folder, prefix, issue count, readers, Spaces sharing it, settings repository, sync status, Sync now, Set up issues), following `ui/mockups.html`.
- [ ] 5.2 Desktop: New project's Space picker and ticked repositories; the Space in the project Overview, with Move to another Space.
- [ ] 5.3 Desktop: the Repositories page grouped by Space with a No Space section, one Space chip per Space on each row to add or remove Spaces, the suggested Space when adding a repository, and the upgrade's suggestions to confirm on first open, following screen 4 of `ui/mockups.html`.
- [ ] 5.4 Docs: README on Spaces, storage repositories as trackers, who can read and change a shared Space, and the scans before a push; PRD's project model and storage.
- [ ] 5.5 Run `openspec validate --strict`, the store, sync and migration tests, then the full suite once before ready_for_testing.
