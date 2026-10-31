## Context

`Project.repositories` was `None` for "every workspace repository" or `Some(ids)` for a selection. A Space's list was copied into `Some(ids)` once, at New project, `create_project` and Start project from issue. Nothing kept the copy current, and `set_project_repositories` refused to drop a repository an open ticket worked in. `Project::uses`, read by `create_ticket` and `create_session`, consulted only that field.

The approved study is `ui/mockups.html`. Its "How it behaves" list and screen 6 are part of the requirement.

## Goals / Non-Goals

**Goals:**

- A Space edit reaches its projects with no write to them, including an edit synced from another Mac.
- A repository leaving a Space, or a Move, never stops an open ticket, its agents or its reviewers.
- One upgrade, in one transaction, that changes no Space.

**Non-Goals:**

- Narrowing a project below its Space. A project cannot use fewer repositories than its Space lists on this Mac.
- Letting a coordinator change a Space's list or a project's added repositories after creation.
- Cloning a Space's repository that is not on this Mac.

## Decisions

### Three reasons, in one place

`Project::repository_use` answers why a project uses a repository: `Space`, `Added` or `OpenTicket`, in that order, or not at all. `Project::uses` is whether it has an answer. The host and `Snapshot` both call it, so the client's groups and the host's refusals cannot disagree. `Space::lists` holds the match by `Repository::is_github`, which `space_repositories` and the Repositories page share.

The alternative, resolving to a stored list on every Space edit, needs a write to every project and a hook on sync. Reading is cheap: the workspace holds tens of repositories.

### Open tickets reach the client as rows

A ticket's project is its coordinator's, and the snapshot leaves out an archived project's coordinator, so a client cannot find the third reason from `sessions`. The snapshot gains `open_ticket_repositories`: one row per open ticket with its project, repository and title. `Project::repository_use` reads the same rows on both sides. An older host omits the field, which leaves that group empty and changes nothing the host enforces.

### A repository holds the first reason shown

A repository can be listed by the Space and also added, or added with an open ticket. Overview shows it once, in the first group that applies. An added repository the Space later lists stays stored, so it stays if it leaves the Space.

Normalising the list drops a Space's repositories at each write to it: `set_added_repositories`, `create_project`, Start project from issue, Attach and Add repositories all go through `added_beyond_space`. So "repositories a Space already lists add nothing" holds in one line. Saving Add to this project can therefore drop a repository that was both added and listed, which is what the dialog shows: it is ticked as the Space's.

### The command is renamed and made the human's

`set_added_repositories` replaces `set_project_repositories` because its meaning changed: there is no `None`, and an empty list no longer means a project with no repositories. The app one release behind gets a sentence refusing the unknown command rather than a silently different result. It joins `is_human_only`, as `move_project_to_space` is, because the decided model says only the human changes it. New project and Add to this project both send it from the human's own client.

The old refusal "works in a repository you removed" goes: the third reason keeps such a repository in use until the ticket closes.

### Upgrading in place, gated by the row

The store's earlier migrations rewrite `user_version` on every open, so a crash between them and the last one leaves a lower version than the store really has. A row with no `added_repositories` key is one the upgrade has not reached, and `Project` always writes that key, so the row itself is the gate and a second run changes nothing. All rows, the `repositories` keys they lose and `user_version = 10` commit in one transaction.

The upgrade reads each row as JSON, as the Spaces upgrade does, and runs straight after it, before anything decodes a `Project`. An older migration that sets `$.repositories` runs first, so a store of any age reaches it in the shape it expects. A project with `None` names nothing and `Some([])` names nothing, so both keep only their open tickets' repositories as added.

### The Move dialog is worked out on the client

The dialog compares the two Spaces' lists against the snapshot, with no new command, using the same rows as Overview. Gains are the target's repositories here that the project does not already use. Keeps are the repositories the project holds for a reason that survives the Move and that the target does not list. Losses are the rest of the current Space's repositories here.

### `repository_uses` follows the new reasons

The upgrade's Space suggestions counted a selection, or the tickets of a project with every repository. They now count what a project uses plus every repository its tickets were in, so a project's history still informs a suggestion.

## Risks / Trade-offs

- A repository whose GitHub facts were never read matches no Space, so the upgrade keeps it as added. → That is the safe direction: nothing is lost, and the human can remove it.
- A project in a Space that lists nothing here uses nothing. → Overview says so and names the two ways to fix it.
- The app one release behind shows a project as using every repository. → Display only, until it updates; the host decides what is allowed.

## Migration Plan

Store schema 9 becomes 10 at first start. There is no way back other than a backup: an older build refuses a store at schema 10.
