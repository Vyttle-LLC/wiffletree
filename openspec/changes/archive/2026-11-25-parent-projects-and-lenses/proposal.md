## Why

A coordinator that creates a project loses track of it: the two coordinators cannot talk, the new project's questions go straight to the human, and the sidebar shows both as unrelated siblings. Projects then sprawl, and their names try to carry three things at once: what the work achieves, which app and layer it touches, and who manages it. First-word groups such as "Chat" or "Watch" only sort names; they own nothing and answer nothing.

On November 23, 2026 Michael approved two studies, now in `ui/`. `ui/parent-projects.html` (v1) makes the project that creates another its parent: the parent fields its children's questions, and Agents moves to the inspector rail. `ui/mockups.html` (v2) names parents after outcomes, derives app and layer tags from repositories, and regroups the sidebar through four lenses. The Me lens in v2 replaces v1's Needs you page.

## What Changes

- **Parent projects.** A project may have one parent project in its Space, one level deep. A coordinator's `create_project` makes a child of its own project, or a sibling under its parent when it is itself a child. The human files projects with **Move under…** or by dragging in the Outcome lens, which asks first. Existing `created_by` links are not turned into parents.
- **Parent and child coordinators talk.** They can `send_message` each other, and a child's `report` reaches its parent's coordinator, folded into the parent's chat as one line. Siblings do not message each other.
- **Questions go through the parent.** A child's `ask_user` reaches its parent's coordinator first. The parent answers it, which resumes the child as a human answer would, or escalates it to the human's Inbox, labelled with the child's name. The Inbox shows only escalated questions. The parent gets a one-line note whenever the human answers or messages a child directly.
- **Closing.** A parent cannot be archived while any child is open, and restoring a child waits for its parent. Closing stays the human's decision.
- **Standing projects.** A project flag, such as on Platform, that keeps the project out of Ready to close.
- **Naming in the coordinator's instructions only.** Coordinators prefer tickets in their own project, create a child only for a separate outcome, name parents after outcomes and children after one deliverable. No code checks names.
- **Repository tags.** Each repository in a Space has an optional app and layer, kept in `space.yaml` with the Space's own list of values. A project's tags are those of the repositories its tickets work in.
- **Sidebar lenses.** A control under the Projects heading switches between Outcome, App, Layer and Me, with ⌘1–⌘4, and remembers the last one. Outcome is the parent tree and the only lens that drags. App and Layer list projects under each tag. Me lists Questions, Merge on GitHub, Going quiet and Ready to close, each with a reason, and its badge counts across Spaces.
- **BREAKING (desktop): first-word groups retire.** The Group button, group rows and the shortened names inside groups go away; the saved choice is ignored.
- **Agents in the rail.** The header's "Agents · N working" toggle goes. Agents becomes a rail button after Overview with a working count. A parent's panel adds a folded card for each child.
- **Header.** A child's header shows "← Parent / Child", and every project's header shows its tag chips, each opening its lens at that section.

## Capabilities

### New Capabilities

- `parent-projects`: the parent link, its depth and Space rules, Move under, messaging between parent and child coordinators, questions routed through the parent, the close guard, standing projects and the coordinator's naming rules.
- `sidebar-lenses`: the lens control, the Outcome tree and its drag, the App, Layer and Me lenses, the collapsed parent row and the header's tag chips.

### Modified Capabilities

- `workspace-host`: `create_project` makes a child or a sibling, the two coordinators may message each other, and `created_projects` keeps only projects that are not children.
- `spaces`: repositories carry an app and a layer, and projects derive their tags from them.
- `native-workspace`: first-word groups are removed; the sidebar draws parent and child rows; dragging and Space sections follow the lenses.
- `work-progress`: the Agents panel opens from the inspector rail and lists child projects; the breadcrumb names a child's parent.

## Impact

- `workspace-core`: `Project.parent_id` and `Project.standing`, question routing state on `Attention`, `repository_tags`, `apps` and `layers` in the Space file, derived tags on the project snapshot.
- `workspace-host`: the parent link and its checks, `create_project`, `send_message` and `report` routing between coordinators, `answer_question` and `escalate_question`, the archive and restore guards, the one-line notes, `workspace_context`'s `parent_project` and `child_projects`, and `coordinator.md`.
- `workspace-desktop`: the lens control and its four lenses, removing groups, Move under… and Standing in the project menu, the Repositories list's App and Layer columns, the Agents rail button and panel, the breadcrumb and chips.
- `docs/PRD.md`, `docs/host-protocol.md`, `design/README.md`.
- Older stores load unchanged: a project with no parent is loose, an untagged repository has no app or layer, and a Space file without the new keys reads as empty.
