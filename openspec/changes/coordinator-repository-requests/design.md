## Context

A project uses its Space's repositories, those added to it and those an open ticket works in, read on every call. `set_added_repositories` adds one to a project without touching a Space, and `attach_repository` adds a folder to the workspace as well. Publish and merge approvals show the pattern for a host decision the human answers: a host-only id prefix on `operation_id`, `is_permission`, and a card drawn from stored data rather than from text an agent wrote.

## Goals / Non-Goals

**Goals:** let a coordinator ask by name; let the human answer in one click; make the repository usable at once.

**Non-Goals:** any change to a Space's list; moving a project between Spaces; adding without approval; requests from workers.

## Decisions

### The request lives on its Inbox item

`Attention.repository_request` holds what the card shows and what approval acts on. The `attention` row already survives a restart, reaches every client in the snapshot and is what the human answers, so a separate table would add a second record to keep in step. The field defaults to none and is left out when none, so older stores load unchanged.

`request_attention` stays as it is for every other item. A request gets a fresh `operation_id` of `repository:<new id>`, so a denied request can be asked again as a new card. A repeat is found by comparing the target with the project's open items, because the operation id cannot carry it.

### The target is fixed when requested

The stored request has the repository's id when it is in the workspace, and otherwise the canonical folder path and the `owner/name` read from its `origin`. A folder already in the workspace is treated as that repository, so asking by path or by name cannot raise two cards.

Approval canonicalizes the path again and refuses if it differs, or if the `origin` no longer names what the card showed. A folder that a root check added in the meantime is used as it is, with `set_added_repositories` rather than `attach_repository`, which would overwrite its base.

### Two clones of one `owner/name`

A name that matches several unused clones is refused with their paths, so the coordinator asks by folder. Picking one would act on a clone nobody saw.

### Answering is one transaction

`Command::ResolveAttention` on a request runs `answer_attention`: the answer, the change and the result message, in one savepoint. A crash in between leaves the card waiting, so there is no start-up sweep to write and nothing can be applied twice. The publish and merge flows need one only because they run jobs off the host thread.

The change has its own savepoint inside. When it fails, it rolls back alone, and the answer and a message saying why still commit. That is how "approval can no longer succeed" reaches the coordinator.

### An archived coordinator hears on restore

Wiffletree has no archived project: archiving a project is archiving its coordinator. `queue` refuses a message to an archived session, so a new `Delivery::PastArchive` lets a request's result wait in that coordinator's queue. Nothing is added, and restoring it delivers why.

### The card is plain text, computed where it is drawn

Listed by is read from the snapshot's Spaces when drawn, so an edit while the card waits shows. The title and rows come from the stored request. The reason is a plain text child, never linked, and neither button takes anything from it.

### Space lists stay out of this path

Approval calls `add_repository` through `attach_repository`, never `add_discovered`, which is the only path that joins a followed root's Spaces. A folder under a followed root therefore joins no Space, and later scans skip it as known.

## Risks / Trade-offs

- **`Repository.github` is filled in by `refresh_github`.** A repository imported moments ago can be missed by `owner/name`. The refusal asks for the folder path, which finds it, so this heals by the route the coordinator is already told to take.
- **`refresh_github` after approving a folder** can be dropped while another read runs. It heals on the next focus or root refresh, and that is out of scope here.
