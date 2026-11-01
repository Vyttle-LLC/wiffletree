## Why

A Space's page asks for one typed `owner/name` at a time, with no sight of the 45 repositories already in the workspace. Filling a Space with a folder of repositories means 25 trips through a text box, and a repository cloned next month joins no Space until someone remembers.

On November 1, 2026 Michael approved `ui/mockups.html`, with one addition drawn there: a Space can follow several root folders, not just one.

## What Changes

- **A dialog replaces the text box.** Choose repositories… opens the workspace's repositories grouped by the folder that holds them, with a filter, a tri-state tick per folder and a chip for each other Space listing a repository. The `owner/name` box moves to its foot, for a repository not cloned here. Done changes the form's draft; Save writes it.
- **A Space's page stays short.** Six rows show, then "and N more", which opens the dialog.
- **A Space can follow root folders.** Each followed root has a Following line with × on the Space's page. The rule stays on this Mac in `SpaceLocal.followed_roots`, and `space.yaml` never holds a path.
- **A root check acts on the rule.** When it first adds a repository to the workspace, under a root a Space follows, its `owner/name` is appended to that Space through `update_space`, as an ordinary `space.yaml` commit. A toast names each one. A failed write leaves the repository in the workspace with a warning and no retry.
- **Only a first find acts**, so a repository the human takes out of a Space never comes back, and one added by hand gets today's suggestion and nothing else.
- **Removing a root ends following** in every Space, and its repositories stay.
- **`Command::SetFollowedRoots`** is human only, and Control only from a paired device. No agent or workspace tool can set it.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `spaces`: choosing a Space's repositories, following root folders, and the human-only rule, which now allows a rule the human set to add a repository.

## Impact

- `workspace-core`: `SpaceLocal.followed_roots`, `Command::SetFollowedRoots`, `RepositoryRefresh.joined` and `.not_joined`.
- `workspace-host`: `scan_roots` says which roots found a repository, `add_discovered` acts on the rule, `remove_repository_root` ends it.
- `workspace-desktop`: `repository_tree.rs` (rows, filter, ticks; no rendering), `repository_chooser.rs` (the dialog), `spaces_view.rs`.
- `docs/host-protocol.md`, `docs/DEVELOPMENT.md`, `docs/PRD.md`.
- Older stores and `space.yaml` files load unchanged: the new field defaults to empty and is left out when empty.
