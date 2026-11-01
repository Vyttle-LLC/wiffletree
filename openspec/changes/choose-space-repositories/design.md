## Context

A Space's list of `owner/name` is the one shared fact of what it covers, and it travels in `space.yaml`. A root folder is a path on this Mac, in the `repository_roots` table. Root checks run from the app, at most once a minute, off the host thread; `add_discovered` then adds what they found on the host thread. A new repository has no GitHub facts until the next `refresh_github`.

## Goals / Non-Goals

**Goals:** choose repositories by sight; let a folder's future repositories join by themselves; keep the Space's list the only record of what it covers.

**Non-Goals:** following anything but a root folder; a rule that travels between Macs; putting existing repositories into a Space at a root check; nested folders, as root checks only read a root's immediate children.

## Decisions

### The rule is a list of root paths in `SpaceLocal`

`followed_roots: Vec<String>`, with a serde default and left out when empty, so older data loads and writes back unchanged. A path means nothing on another Mac, so it is never in `space.yaml`. On a teammate's Mac the repositories arrive as any edit does, and that Mac follows nothing until its human says so.

Each path must be a current root, so a followed root is never a path the host stopped checking. `remove_repository_root` drops it from every Space in one transaction with the root.

### A root check says which roots found a repository

`scan_roots` returns `ScannedRepository { candidate, roots }`, a host type that stays out of the protocol. Paths are canonicalized in a scan, so comparing a repository's path with a root's string could fail where the scan knows the answer. A repository reached from two roots, through a link, keeps both.

### Only a first find of the repository acts

`add_discovered` reads a new repository's `owner/name` from its `origin` on disk, because `Repository.github` is not filled in yet. It joins a Space only when no workspace repository already had that identity: a second clone of a repository the human took out of a Space is not a first find. A repository not on GitHub, or whose `origin` cannot be used, joins nothing, as it can be in no Space.

The join goes through `update_space`, so it is an ordinary commit, or an ordinary change for the next push. Each repository in each Space is one write, so one failure costs only that join. Followed roots are read when `add_discovered` runs, not when the scan started, so a root removed meanwhile adds nothing.

### What a check reports is two new fields

`RepositoryRefresh.warnings` are not shown: an unreadable root is not worth interrupting anyone for. A failed join is, so it goes in `not_joined`, shown with `toast_error`, and joins go in `joined`, shown with `toast`.

### Save sends the rule first

`SetFollowedRoots` is its own command, like `SetRepositorySpaces`, with its own row in the remote access table. Save sends it only when the draft changed it, then `UpdateSpace`. It goes first because it is the one that can be refused for a root removed meanwhile, which then leaves the form's draft as it was. If `UpdateSpace` is refused after it, Save reports that and the form keeps the draft, so saving again is the same request.

### Rows and ticks are plain data

`repository_tree.rs` holds folders, rows, the filter and the choice, with no GPUI, so its tests run without a window. `repository_chooser.rs` only draws them. The dialog works on its own copy, so Cancel has nothing to undo, and it reads the snapshot once, so rows do not move under the pointer. Ticks are `owner/name`, compared ignoring case, so two clones tick together with no extra code.

Folder order is ticked folders, other root folders, Not cloned on this Mac, then other folders, each by path, as screen 5 draws. A folder's count is its ticked rows of all its rows, as screen 5's "0 of 1" for a row that cannot be ticked, while its tick looks only at rows that can be ticked.

`~/dev/vyttle` is a substring of a shown path, like any filter. `~/dev/vyttle/*` is exactly that folder, so it does not also match `~/dev/vyttle-llc`.

## Risks / Trade-offs

- A root check reads `origin` on the host thread, once per new repository, beside the Git calls `add_repository` already makes there. A check with nothing new runs none.
- A repository removed from the workspace, then cloned afresh at a new path, is a first find. The workspace has no memory of it, and the human removed it from the workspace rather than from the Space.
- The `~` is this Mac's home, so a remote host's paths show in full. They still work as filters.

## Migration Plan

None: the field defaults to empty.
