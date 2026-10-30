## Context

See proposal.md for why and `ui/mockups.html` for the look. The mockup copies #163's Space page and adds the tracker, New project and the Repositories page grouped by Space; Michael approved all four screens on October 29, 2026.

At origin/main 110bc339 none of the open changes is implemented:
- A project's brain is `Project.brain`, a local folder set by `Host::attach_brain` (`workspace-host/src/memory.rs`); exports go there and are recorded in `log_exports`.
- The settings repository (`sync.rs`, `sync.sql`) checks a GitHub repository read-only, reaches it as `gh` does, clones it into `<data folder>/shared/`, and runs model-free passes that compare whole files against what this machine last synced and pause what both sides changed. Without a settings repository, `<data folder>/shared/` is a plain folder.
- `create_project` without `repository_ids`, and the human's New project, use every workspace repository.
- There is no `space.rs`, `tracker.rs`, `brains.rs` or `github_access.rs`.

The open changes this one reconciles are `assistant-spaces-jobs-boards` (#163), `shared-issues` (#162) and the brain changes `brain-registry-sync`, `brain-log-capture`, `brain-compile`, `brain-browser` and `brain-gate` (#165, #168). The analysis is in the coordinator's `reconciliation.md`; this design records its outcome.

## Goals / Non-Goals

**Goals:** one answer to "where does this go?" for brains, threads, jobs, issues and projects; one clone and one sync engine per repository; a foundation that ships first with little UI, so that the brain track, shared issues and the assistant can then run in parallel.

**Non-Goals:** threads, the router, jobs and Boards; the issue model, its pages, merges and ID allocation; brain checkouts, batches and the gate itself; a cloud or headless host; moving a Space's storage from one repository to another.

## Michael's decisions (October 29, 2026)

Michael approved #163's mockup and the reconciliation plan. Each decision is specified here or in the change named.

| # | Decision | Specified in |
| --- | --- | --- |
| 1 | The tracker is the Space's storage repository: `issues/` and the tracker config at its root beside `spaces/`; local storage is a local tracker; no per-Mac tracker list or Settings › Trackers; Spaces sharing a storage repository share its tracker and prefix | spaces "A storage repository is also the Space's tracker"; shared-issues "The tracker is the Space's storage repository" |
| 2 | One clone and one sync engine per GitHub repository, with rules per path; clones under `<data folder>/repos/`; the brain keeps its own clone and PR flow under that root; the settings clone moves there | shared-repositories (all requirements); settings-repository "Shared settings live in one folder on every machine"; brain-registry-sync "The host keeps its own checkout" |
| 3 | One assistant: the issue assistant becomes an issue thread, with web fetch and search off; no `Role::IssueAssistant`, hidden `tracker` project or Issue assistant row | assistant-spaces-jobs-boards assistant "Issue threads"; shared-issues proposal |
| 4 | Routing follows the Space: New issue, the Issues filter, Start project from issue, New project and `create_project`, and issues proposed from a thread | spaces "Every project belongs to a Space"; workspace-host "Coordinators create projects"; shared-issues "The Issues page", "New issue…", "Starting a project from an issue links them"; assistant "Issue threads" |
| 5 | Sessions outside projects belong to a Space (threads, job runs, issue threads) or a brain (compile); compiles are a built-in, host-owned job kind on #163's scheduler with their own Brain compiler row | workspace-host "Enforced hierarchy" here, restated in assistant-spaces-jobs-boards and brain-compile; scheduled-jobs "Built-in host jobs"; brain-compile "Compile runs start on schedule, on demand and after a ruling" |
| 6 | The Repositories page is grouped by Space (Sage, Vyttle, Personal, No Space), with one clickable Space chip per Space on each row; repositories stay a record on each Mac and a Space lists them by `owner/name`; a repository may be in several Spaces; adding one suggests a Space from its GitHub owner, which only suggests; only the human changes membership; the upgrade suggests Spaces for existing repositories for the human to confirm | spaces "Repositories belong to Spaces by identity", "Existing projects get Spaces on upgrade" |

### Smaller calls — confirmed by Michael, 2026-10-29

- **YAML for the tracker config**, `wiffletree-issues.yaml`, to match `space.yaml`. `wiffletree.json` and `wiffletree-brain.json` stay JSON: they already exist in that form and are read by other tools. *Specified in spaces "A storage repository is also the Space's tracker" and shared-issues "A tracker's files".*
- **Scans before pushing `issues/` or `spaces/`.** The host runs the gate's `sensitive-info` and `secret-scan` on this Mac and holds a failing file (fail closed), because storage repositories have no PR gate. *Specified in shared-repositories "Files are scanned before they are pushed".*
- **Who runs what ends with Assistant, Router, Job and Brain compiler.** There is no Issue assistant row. *Specified in assistant-spaces-jobs-boards and brain-compile model-routing.*

## Decided while writing

Each is the smallest choice that keeps the decisions above consistent. All nine were confirmed by Michael, 2026-10-29.

1. **A repository has one role on a Mac: brain, or storage and settings.** A brain changes only through gated PRs, while storage and settings push their default branch directly; one clone cannot follow both. Setting a brain to a repository that is a storage or settings repository on this Mac, or the reverse, is refused, naming the other use. This also keeps `<owner>-<name>` unique under `repos/`. A storage repository may also be the settings repository, as #163 already allowed.
2. **Without a settings repository, the shared folder stays `<data folder>/shared/`.** It is a plain folder, not a clone. Setting up or joining a settings repository makes its clone under `repos/` the shared folder and moves the settings files of `<data folder>/shared/` to `backups/`, as joining already did. Switching from one settings repository to another first runs Stop syncing for the old one, so its clone, which may still be a Space's storage, stays and keeps syncing; no clone is ever moved to `backups/`. Stop syncing copies the settings files back to `<data folder>/shared/` and keeps using them there, while the clone keeps syncing its Space and issue files when it is also storage. `spaces` restates the settings repository's set-up, join and stop requirements to say so.
3. **The prefix is chosen with the storage.** A new storage repository, local or on GitHub, gets `wiffletree-issues.yaml` in its first commit. The Space form proposes a prefix from the Space's name (its first letters, 2 to 5 capitals) and the human may change it. Prefixes are unique among the storage repositories on this Mac. A GitHub tracker's prefix can change only while it has no issue. A local tracker's prefix can be renamed at any time, rewriting its issue IDs and references, because nothing outside this Mac refers to them; this is the way out when a shared tracker joined later uses the same prefix, and the join refusal says so. A storage repository joined without a tracker config offers Set up issues, which asks for a prefix and commits the config.
4. **One commit per rule set.** The engine commits each rule set's changes separately (settings, a Space, issues, a board), so every commit touches only one rule set's paths and the settings repository's "a commit holds changes only to `wiffletree.json`, `settings/` and `skills/`" stays true. A repository is fetched at the shortest cadence of the rule sets it holds: every minute once it holds a tracker.
5. **Sessions outside projects have an owner, not a hidden project.** `SessionOwner` is `Project`, `Space` or `Brain`. Messages and the usage ledger key on the owner, so usage reports can show "Assistant · Sage" or "Brain compile · sagehaven-docs" without a project record the sidebar must hide.
6. **A Space's repositories not cloned on this Mac are skipped.** New project ticks the Space's repositories that match a workspace repository by GitHub name and lists the rest as "not cloned here"; `create_project` does the same and says which it skipped.
7. **The scans ship with the app.** The host runs the two checks from the `gate/` folder at the version this release bundles, with the gate's default rules and its inline `gate:allow` marker. A check that cannot run holds the file too, saying why.
8. **The upgrade keeps local brain folders.** A Space created from a `Project.brain` folder names that folder as a local brain, so exports keep going where they went. `brain-registry-sync` later turns a folder whose `origin` is on GitHub into that repository, after the human confirms the grant.
9. **The upgrade's owner suggestion uses the projects.** Right after the upgrade no Space lists a repository yet, so "a Space whose repositories share that owner" has nothing to match. The upgrade instead suggests, per GitHub owner, the Space whose projects use that owner's repositories most, Personal for the human's own `gh` login, and otherwise No Space. Every suggestion waits for the human on the Repositories page's first open.

## Decisions

### 1. The Space is the one home

```text
Space (Sage | Vyttle | Personal)
  storage repository ── spaces/<space>/space.yaml, jobs/, boards/
                     └─ wiffletree-issues.yaml, issues/           the tracker
  brain repository   ── PR-gated, separate on purpose              (brain-registry-sync)
  repositories       ── default repositories for new projects
  owns: projects, assistant threads, job runs, issue threads
```

`space.yaml` holds the name, description, instructions, brain (`owner/name` or `local`, and a folder), code repositories by identity, default models, sources and format. Each machine keeps, keyed by Space: where each repository and the brain are cloned, which models it may use, and whether the Space is on here.

### 2. One engine per repository, rules by path

```text
<data folder>/repos/
  Sage-Social-wiffletree-storage/     storage + settings repository, one clone, one engine
    wiffletree.json  settings/  skills/     settings rules
    spaces/sage/  spaces/sage-watch/        settings rules (outside edits pause)
    spaces/*/boards/                        runner-written: push without comparing  (assistant-spaces-jobs-boards)
    wiffletree-issues.yaml  issues/         issue rules                            (shared-issues)
  Sage-Social-sagehaven-docs/         brain: its own clone and PR flow          (brain-registry-sync)
  local-7k2q/                         Personal's local storage, no remote
```

`repos.rs` owns, per repository: the clone, fetch with backoff, the record of what this machine last wrote and last synced per file, the push (default branch only, containing the remote head, never forced, no tags or submodules), and the workspace notes for paused files. A rule set names its paths and how a pass treats a file under them: compare whole files and pause both-sides changes (settings), merge per field (issues), or take this Mac's version when this Mac is the writer (boards). `sync.rs` becomes the settings rule set. `github_access.rs` holds the visibility and readers check and `gh`'s reach, as #162 task 1.2 planned, and is shared by storage, settings, trackers and brains.

The brain is not a rule set: brain-registry-sync's checkout fast-forwards `main` and writes only on batch branches. It uses `github_access.rs` and the clone root, nothing else.

### 3. Sessions outside projects

A session's owner is a project, a Space or a brain. A session owned by a Space or a brain has no parent session or ticket and is never offered the coordination tools. A Space-owned session has no worktree; a brain-owned one works only in a worktree of its brain's host checkout, as brain compile needs. This change only adds the owner type and its checks; it admits no role. The deltas that admit roles are ordered below.

### 4. New projects

New project shows the Space, defaulting to the last used, and ticks its repositories. Changing the Space replaces the ticked repositories with the new Space's. Other repositories are added by hand. `create_project` puts the project in the creator's Space; without `repository_ids`, it uses that Space's repositories on this Mac. Start project from issue (shared-issues) and Make this a project (assistant-spaces-jobs-boards) follow the same rule with their own source of repositories.

## Order of changes and of shared requirements

Implementation order: `spaces` first. Then in parallel: the brain track (`brain-gate` can start now; `brain-registry-sync`, `brain-log-capture` and `brain-browser` after `spaces`; `brain-compile` also after #163 stage 3, for the scheduler); `shared-issues` phases 1–3, 5 and 6; #163 stages 1–5, with issue threads (#163 stage 6) after #163 stage 1 and `shared-issues` phase 3. #163 stage 1 ships threads without brain tools, so it does not wait for the brain track: its task 1.8 adds `log_work` once `brain-log-capture` lands and `query_brain` once `brain-browser` lands. This split was chosen over ordering the brain track first because it keeps the three tracks parallel.

Archive order, which decides who restates a requirement that several changes modify: `spaces`, `shared-issues`, `assistant-spaces-jobs-boards`, `brain-registry-sync`, `brain-log-capture`, `brain-compile`, `brain-browser`; `brain-gate` modifies no main requirement and may archive at any time. Each later delta restates the requirement with every earlier addition:

| Requirement | Deltas, in order |
| --- | --- |
| workspace-host "Enforced hierarchy" | 1 `spaces` (Space per project, owner type) → 2 `assistant-spaces-jobs-boards` (threads, job runs, issue threads) → 3 `brain-compile` (compile sessions) |
| workspace-host "Coordinators create projects" | 1 `spaces` |
| project-memory "Safe Markdown export" | 1 `spaces` (Space's brain) → 2 `brain-log-capture` (removed) |
| settings-repository "Shared settings and settings that stay on this machine" | 1 `spaces` → 2 `assistant-spaces-jobs-boards` (assistant name, router threshold) |
| settings-repository "Shared settings live in one folder on every machine" | 1 `spaces` (clone root) → 2 `assistant-spaces-jobs-boards` (`settings/assistant.json`) |
| settings-repository "The human chooses the settings repository on each machine", "A second machine joins", "Stopping sync" | 1 `spaces` (the shared folder moves to and from the clone under `repos/`) |
| host-process "Human-only commands need the app's credential" | 1 `spaces` (Space commands) → 2 `shared-issues` (issue writes, proposals) |
| skills "Instructions are a fixed core and a skill" | 1 `assistant-spaces-jobs-boards` (Assisting, Space instructions, issue threads), the only delta; `shared-issues` keeps its untrusted-text rule in its own spec |
| model-routing "Who runs what" | 1 `assistant-spaces-jobs-boards` (Assistant, Router, Job) → 2 `brain-compile` (Brain compiler) |

## Risks / Trade-offs

- **Issues share a repository with Space instructions.** Anyone who can push issues can push `space.yaml`. Space files still pause edits made outside Wiffletree, and the Space page says who can change instructions.
- **A tracker shared by two Spaces has one prefix.** Sage and Sage Watch see the same issues. That is the point of sharing a storage repository; a Space that wants its own issues needs its own storage.
- **Fetching every minute** for each storage repository with a tracker. A pass is one `git fetch` of a small repository, as #162 measured.
- **Scans need the gate's tools on the Mac.** `secret-scan` needs gitleaks. Holding the file when a check cannot run is fail closed and visible; the note names the missing tool.
- **Moving the settings clone** could strand a half-moved folder. The move is a rename within the data folder, recorded before and after, and retried at the next start.

## Migration Plan

1. The store migration creates Spaces from `Project.brain` folders and "Personal", each with local storage under `repos/local-<id>/` holding `space.yaml` and `wiffletree-issues.yaml`, and sets `Project.space`. `log_exports` keeps its rows, so nothing is exported twice. Nothing is pushed.
2. A machine with a settings repository moves its clone from `<data folder>/shared/` to `<data folder>/repos/<owner>-<name>/` and records the new shared folder. A machine without one keeps `<data folder>/shared/`.
3. Rollback before any Space is put on GitHub is a database and data folder restore.

## Open Questions

None. The smaller calls and the decisions made while writing were confirmed by Michael, 2026-10-29.
