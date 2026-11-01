# Moving a Space's storage

## Context

A Space's storage is a Git repository: local at `repos/local-<id>/`, or a GitHub clone at `repos/github.com/<owner>/<name>/`. It is also the Space's tracker, so Spaces in one repository share `issues/` and a prefix. The store names a storage repository in two forms, `StorageRepository::key` (`local-<id>`, or `Owner/name` as GitHub spells it) and `repos::key` (the same in lower case), and `space.yaml` names none, as its place is the storage.

## Goals / Non-Goals

**Goals:** move every Space of one storage repository to another, in any of the three directions, with the Space never half moved and nothing lost.

**Non-Goals:** moving the settings repository's storage; splitting a repository's Spaces; carrying Git history across (the legend's "New storage starts with a commit of the current files"); merging trackers.

## Every reference to a storage repository

The list the safety rules depend on. "Rekeyed" rows change to the new key in the switch's one transaction. "Dropped" rows are facts about the old repository that mean nothing in the new one; the old repository still holds what they describe.

### Keyed by `StorageRepository::key`

| Where | Holds | At the switch |
| --- | --- | --- |
| `spaces.data` `$.storage.repository` | Each Space's storage | Rewritten for every Space moved; `$.storage.folder` is unchanged |
| `trackers.storage` | The tracker's config | Rekeyed; deleted when the target's tracker is adopted, whose row is inserted if new here |
| `issue_index.storage`, `.data` `$.tracker` | The list and search index | Rekeyed, and `$.tracker` rewritten |
| `issue_read.storage` | Read state | Rekeyed |
| `issue_temporary.storage` | Temporary IDs and their numbers | Rekeyed |
| `issue_preferred.storage` | Preferred numbers | Rekeyed |
| `issue_links.storage` | Links to projects and tickets | Rekeyed |
| `issue_derived.storage` | State derived from linked work | Rekeyed |
| `issue_pending.storage` | Events waiting on a settled file | Rekeyed |
| `issue_follows.storage` | Follow choices | Rekeyed |
| `issue_notes.storage`, `.data` `$.tracker` | Inbox news | Rekeyed, and `$.tracker` rewritten |
| `issue_collaborators`, `issue_collaborators_read` | Who the old repository's collaborators are | Dropped; read again for the new one |

When storage with no issues adopts the target's tracker, the issue tables' rows of the old key are deleted rather than rekeyed: they describe no issue.

### Keyed by `repos::key`

| Where | Holds | At the switch |
| --- | --- | --- |
| `sync_record.repository` | What this Mac last synced of the old repository | Dropped; a new record starts for the new key, unless the clone already has one, when the moved files join it |
| `sync_paused.repository`, `sync_held.repository`, `issue_parts.repository` | Pauses and holds against the old repository | Dropped. What this Mac holds is what moved, and the old repository keeps its own version |
| `storage_repositories.key` | The GitHub repository as this Mac records it | Dropped, and the target's row inserted |

### Held in memory

| Where | At the switch |
| --- | --- |
| `Host::storage_synced` | The old key is removed |
| `Actor::storage_paces` | Follows `storage_cadences`: the old key leaves and the new one starts with a pass due at once |
| `Host::storage_written`, `Host::issues_written` | A name left for the old key finds no pace and is dropped |
| `Host::linear` | A move is refused while an import runs |

### Worked out when read, so nothing to rewrite

`storage_folder` and every caller of it: `tracker.rs`, `issue_inbox.rs`, `issue_work.rs`, `issues_sync.rs`, `jobs.rs` (`last_stored_output`, `save_output`, `push_job_output`), `boards.rs` (`Shelf.repository`, `Shelf.keeping`), `proposals.rs`, `assistant.rs`, `linear_import.rs`. `IssueLink.tracker`, `IssueRecord.tracker`, `IssueTracker.tracker` and `StorageStatus` are built from the tables above or the Space. `workspace_notes` and `held_notes` read the sync tables. `storage_input`, `storage_cadences` and `issue_sync_keys` read the Spaces.

### Reads as a reference, and is not one

`issue_last_filed` names a Space. `issue_profiles` names a login. `jobs`, `job_due`, `job_runs` and `job_approvals` are keyed by Space and job, and `threads` and `projects` by Space. The private board store is under the Space's ID. `space.yaml` refuses a `storage` key. `Recipe.runner` is not a reference to storage, but see "Jobs" below.

## Decisions

### The order, and the one switch

1. **Refuse.** The refusals that need nothing from GitHub, then the target is checked and cloned to a `shared-staging-*` folder, then the rest. This is the check New Space runs, run again at the moment of Move.
2. **Copy.** `copy` compares the moved files with the staging clone's head and writes the differences. To GitHub it scans them, then commits them apart by rule set (Spaces, boards, issues), as the push's rule requires. To this Mac it commits them in a new repository.
3. **Record.** To GitHub, one `storage_moves` row names the source, the target and the commit, before the push.
4. **Push.** `repos::push`, which never forces and writes only the default branch. An error is checked against GitHub's head, as a push can land and lose its answer.
5. **Switch.** On the host thread: the refusals again, the source compared with what was copied, then one transaction rewrites every reference above and records the folder renames in `repo_moves`.
6. **Rename.** `repos::finish_moves` makes the renames: the staging clone to its place, old local storage to `backups/`.

Why one transaction and no phase column: everything before step 5 touches nothing the Space uses, and step 6 is `repo_moves`, which already finishes at the next start, before anything reads storage. So the transaction's commit is the one moment the move happens: a stop before it leaves the Space where it was, and a stop after it leaves it moved. The renames use `repo_moves` paths relative to `repos/`, hence `../backups/…` and `../shared-staging-…`. `Host::open` finishes them in `migrate_clone_folders`, before `ensure_local_storage` and before `remove_staging` sweeps what a stop left.

### A push the Space never switched to

A stop after the push leaves GitHub holding the moved files while the Space stays where it was, and a move can also stop there for a job started meanwhile or a file changed meanwhile. Retrying would be refused for the very folder and tracker it pushed. So `storage_moves` remembers what this Mac pushed: a target is its own leftover when one of its rows for the same source names a commit in the target's history and nothing under the moved paths changed since. Only then do the folder and tracker refusals stand aside, and the retry commits just the difference. A teammate's change under those paths puts the refusal back. Finding this out at the next start instead would need GitHub at startup.

### Holding everything else off

No sync pass runs during a move: `begin` refuses while a pass runs and sets a gate that `storage_pass` and `sync_pass` respect until the move ends. `Pace::running` covers a pass started but not yet at its lock. Other writes are not blocked, as copying and pushing run off the host thread and agents keep filing issues. They are caught at the switch instead: the source is compared with the staging clone's head, on the host thread with no pass running, and a difference ends the move with "changed while it was moving". The same step repeats the refusals, so a job started meanwhile refuses the switch. The retry pushes only the difference.

### Empty repositories

`start_if_empty` pushes a first commit, which would write to GitHub before the scans. A move makes that commit in the staging clone only, so a held scan leaves an empty repository empty.

### What moves

`spaces/<folder>/` of every Space moved, with its jobs and boards, plus `issues/` and `wiffletree-issues.yaml` unless the target's tracker is adopted. As a pass does, it skips hidden files and takes only regular files. A Space in the old GitHub repository that this Mac never added stays there. Local files outside those paths stay in the backup.

### The tracker

- To GitHub, a target with no tracker takes the storage's. A target with one is adopted when the storage has no issues, and refused when it has.
- To this Mac, a repository with no tracker gets one, as local storage always has one, with a prefix proposed from the name.
- A GitHub source with issue changes unpushed is refused with a sentence, because a new issue has only a temporary ID until a pass numbers it, and only a pass does that. Files held or paused are not unpushed in that sense: a held file is held again by the scan, and a paused file moves as this Mac holds it.

### Jobs

A "One runner" job in a Space on GitHub runs only on its runner's Mac, and local storage never asked for a runner, so a move to GitHub would silently stop such a job. Its recipe therefore names this Mac as it moves, as `save_job` would have. The file changes in the copy and the store changes in the switch, and failing leaves the source's recipe as it was. Job outputs on a shared Space wait for the runner's approval as any other's do, which is right: the audience changed.

### Exclusive steps, not the host thread

The service runs stage, copy and push off the host thread as jobs, each answering with an event, and holds the reply until the switch. The dialog stays busy, and Check's answer is unchanged by a passing sync. `Host` exposes each step on its own, so tests stop at each one.

### Commands

`CheckStorageMove` answers with `StorageMove`, and is a read for remote access, as `CheckStorageRepository` is. `MoveStorage` is a change, Control only, as `JoinSpaces` is. Both are in `is_human_only`, so an agent on the host's socket is refused them, and no tool names them.

## Risks

- **A busy Space may need a second Move.** A write during the move fails it; the retry is small because only the difference is pushed.
- **Board and file history restarts in the new repository.** The approved legend says so. The old history is in the backup, or still on GitHub.
- **Issue changes on GitHub this Mac has not pulled stay in the old repository.** They are not lost, and the sentence for a GitHub-to-GitHub move says teammates keep the old repository until they add the Space from the new one.
