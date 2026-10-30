## Why

Three open changes each answer "where does this go?" in their own way. `assistant-spaces-jobs-boards` (#163) puts threads, jobs, boards and projects in a Space. The brain changes (#165, #168) let the Space decide the brain. `shared-issues` (#162), written before Spaces existed, adds a per-Mac list of trackers, a hidden `tracker` project for its own issue assistant, and a third clone root beside the settings clone and the Space storage clones. A new issue, Start project from issue and the Issues filter cannot know the Space, a new project still defaults to every workspace repository, and brain compile has no place to run.

On October 29, 2026 Michael approved one answer: the Space is the one home. Each feature reads its Space and stores nothing of its own about routing. This change carves the Space out of #163 so that it can ship first, with little UI, and the other changes build on it.

## What Changes

- **Spaces, moved from #163.** A Space has a name, a description, instructions, a brain, a storage repository, its code repositories, default models and sources. Spaces are created and edited only by the user in Settings › Spaces. Every project belongs to a Space and logs to its brain. The upgrade turns today's `Project.brain` folders into Spaces, with a "Personal" Space for the rest. Shared Spaces live in their storage repository and are joined with "Add Space → From a repository". Machine-local facts stay on the Mac.
- **The tracker is the Space's storage repository.** Issues live in `issues/` with `wiffletree-issues.yaml` at the storage repository's root, beside `spaces/`. A personal Space's local storage is a local tracker with no remote. There is no per-Mac tracker list and no Settings › Trackers. Spaces that share a storage repository share its tracker and prefix, as two Spaces may name one brain. The Space page shows the tracker: its prefix and who can read it.
- **One clone and one sync per GitHub repository, with rules per path.** A shared repository module takes #162 task 1.2's `github_access.rs` and turns `sync.rs`'s passes into one engine per repository whose rules are chosen by path. `settings/`, `skills/`, `wiffletree.json` and `spaces/` keep the settings repository's rules. `issues/` and runner-written `boards/` get their own rules in `shared-issues` and `assistant-spaces-jobs-boards`. Every clone lives under `<data folder>/repos/`: `<owner>-<name>/` for a GitHub repository and `local-<id>/` for local storage. The settings clone moves there from `<data folder>/shared/`. Brains keep their own clone and pull-request flow under the same root.
- **Files are scanned before they are pushed.** Before pushing `spaces/` or `issues/`, the host runs the brain gate's `sensitive-info` and `secret-scan` checks on this Mac and holds a failing file instead of pushing it.
- **A Space's repositories are the default for new projects.** New project starts in the Space last used, with that Space's repositories ticked. `create_project` puts the new project in its creator's Space and, without `repository_ids`, uses that Space's repositories instead of every workspace repository.
- **The Repositories page is grouped by Space.** Repositories stay a record on each Mac (clone path, base, checks); a Space lists them by `owner/name`, and a repository may be in several Spaces or none. Each row's Space chips edit the same list as the Space page. Adding a repository suggests a Space from its GitHub owner, and the upgrade suggests Spaces for existing repositories, both for the human to confirm.
- **Sessions outside projects belong to a Space or a brain.** The enforced hierarchy gains one owner type for sessions that are not in a project, with no hidden project records. This change admits no role yet: `assistant-spaces-jobs-boards` admits assistant threads, job runs and issue threads (Space), and `brain-compile` admits compile sessions (brain).
- **Space commands are the human's.** Creating, editing, removing and joining a Space, and setting up its tracker, are human-only commands on the host socket.
- **Not included:** threads, the router, jobs, Boards (`assistant-spaces-jobs-boards`); the issue model, pages and sync rules (`shared-issues`); brain checkouts and pull requests (`brain-registry-sync`).

## Capabilities

### New Capabilities
- `spaces`: Space definitions, the Spaces settings page, storage repositories and their tracker, projects in a Space and their default repositories, the upgrade of existing projects, shared Spaces and joining one, and what stays on each machine.
- `shared-repositories`: one clone and one sync engine per repository under one root, rules chosen by path, the settings clone's move, and the scans before a push.

### Modified Capabilities
- `workspace-host`: "Enforced hierarchy" requires every project to belong to a Space and gives sessions outside projects an owner, a Space or a brain; "Coordinators create projects" uses the creator's Space and its repositories.
- `project-memory`: "Safe Markdown export" exports to the project's Space's brain instead of a per-project attachment.
- `settings-repository`: Spaces are local settings that travel through their storage repositories, and the shared folder is the settings repository's clone under `repos/` when one is chosen.
- `host-process`: Space and tracker commands are human-only.

## Impact

- `workspace-core`: new `space.rs` (`Space`, `SpaceId`, local facts, sources, `TrackerConfig`), `SessionOwner` (`Project`, `Space`, `Brain`); `Command` variants for Spaces; `Project.space` replaces `Project.brain`.
- `workspace-host`: new `github_access.rs` (moved out of `sync.rs`), `repos.rs` (the clone root and one engine per repository with path rules), `spaces.rs` (store, upgrade, Space files and tracker config), `scan.rs` (running the gate's two checks before a push); `sync.rs` becomes the settings rules on top of `repos.rs`; `memory.rs` (exports through the Space); `service.rs` (`create_project` and New project defaults); `socket.rs` (human-only Space commands); store migrations.
- `workspace-desktop`: `settings/spaces.rs` (list, page, New, From a repository, Storage and issues), `creation.rs` (Space and repositories in New project), the project Overview's Space.
- Docs: `README.md`, `docs/PRD.md`.
- Mockup: `ui/mockups.html`, copied from #163's approved Space page and amended; approved by Michael on October 29, 2026, with the Repositories page he added that day.
- Depends on `brain-gate` tasks 1–2 for the scans (task 4.4 here). Every other open change listed in design.md builds on this one.
