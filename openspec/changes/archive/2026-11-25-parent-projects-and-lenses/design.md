## Context

Projects are flat today. `create_project` records `created_by` on the new project and lists it under the creator's `created_projects`, read only, and the two coordinators cannot message each other. The sidebar draws one section per Space and, inside it, first-word groups that exist only in the desktop. The Agents panel opens from an "Agents · N working" toggle in every project conversation's header. Spaces list their repositories by `owner/name` in `space.yaml`; nothing describes what a repository is.

Michael approved both studies on November 23, 2026 and chose the answers to their open questions; the brief's ten decisions are what this change specifies. The studies are sanitized copies: their projects, repositories and messages are invented Celadon samples.

## Goals / Non-Goals

**Goals:**

- One level of ownership between projects, held by the host, so a parent can follow, message and answer for its children.
- Fewer questions reach the human, with no question lost: every child question is either answered by its parent or escalated.
- Names say what a project achieves; where it happens comes from repositories, not from names.
- One sidebar, four ways to look at it, with no second page for what needs the human.

**Non-Goals:**

- Deeper trees. A child never has children.
- Checking or rewriting names in code. The naming rules live in the coordinator's instructions.
- Migrating `created_by` into parent links, or converting first-word groups into parents. v1's "Make this a parent" and the Parent field on New project are not part of this change; Move under… and the drag cover filing.
- Moving projects across Spaces with their parent, or parents spanning Spaces.
- A Needs you page. v1's Part 4 is superseded by the Me lens.
- Tags on projects set by hand, or tags other than app and layer.

## Decisions

**The parent link is a project field, and depth is checked on every write.** `Project.parent_id` names another project in the same Space. The host refuses a link that would give a child a child: the parent must have no parent, and the project being filed must have no children. Same-Space is checked on every write, and moving a project to another Space clears its own link and its children's links, after the existing Move dialog says so. Keeping Move to another Space never refused matters more than keeping a tree together across Spaces. *Alternative:* moving the children along. Rejected: it changes the repositories of projects the human did not select.

**Coordinator sessions keep no parent session.** Workers still belong to their own project's coordinator. Routing between coordinators is decided from the projects' link at send time, so filing or unfiling a project needs no session rewrite, and the existing "parent or direct child" rule for `send_message` gains the parent and child projects' coordinators. A child coordinator's `report` goes to its parent project's coordinator; a loose coordinator's behaves as today.

**Reports from children reuse the quiet fold.** A child's report is a peer message, which the work-stream fold already draws as one line in the parent's chat. Nothing new is drawn; the fold names the child as sender.

**Questions are routed, not copied.** A child's `ask_user` creates one Inbox item, as today, in the state *with parent*, and wakes the parent's coordinator with the question. The parent resolves it with one of two new tools: `answer_question`, which closes the item and resumes the child with the answer from its parent, or `escalate_question`, which moves the same item to the human with an optional note. The Inbox, needs-you counts and the Me lens show only items with the human. A child's row shows a hollow badge for items with its parent. One item, two states, means an answer from either side closes it once, and a race returns "already answered". *Alternative:* the host classifies questions and forces product, security and approval questions to the human. Rejected: the host cannot tell a product question from another; the parent's instructions require escalation instead. Approvals (`request_permission`, publish and merge approvals, `request_repository`) are not questions and keep going straight to the human.

**Human contact with a child leaves a note for the parent.** When the human answers a child's question or messages a child's coordinator, the parent's coordinator gets a one-line Wiffletree notice that does not start a turn and arrives with its next one. This keeps the parent from contradicting the human without waking it.

**Closing is archiving.** Projects have no separate closed state; the human closes a project by archiving it. The host refuses to archive a parent while any child is unarchived and refuses to restore a child whose parent is archived, naming the projects to handle first. No coordinator tool archives a project.

**Standing is a project flag the human sets.** `Project.standing` is a boolean, set from the project menu and Overview. It only keeps the project out of Ready to close and draws a "standing" pill.

**Tags live with the Space's repositories, in new keys.** `space.yaml` keeps `repositories` as a list of `owner/name` strings, so older hosts read it unchanged. New keys hold the rest: `apps` and `layers`, the Space's ordered vocabularies, and `repository_tags`, a map from `owner/name` to an optional `app` and `layer`. They sync as every other `space.yaml` edit does. The suggestion for a new repository copies the tags of the Space's repository whose name shares the longest leading run of hyphen-separated words, and suggests nothing without one. The host derives a project's tags, so every client and `workspace_context` agree.

**Lenses are a desktop drawing over host facts.** The host stores the order, the parent link, the flag and the tags; the desktop groups them. The lens is a view choice in `desktop-preferences.json`, like the Group choice it replaces. The Outcome lens draws loose projects and parents in the stored order and each parent's children under it in their stored order, so the host order needs no change. A parent dragged in Outcome moves with its children as one block, as a group did.

**Agents moves to the rail for every project session.** The rail button sits right after Overview and carries the working count the toggle carried. For a parent, the count and the panel include each child's coordinator and agents. A child card is folded by default; clicking it selects the child's coordinator.

## Risks / Trade-offs

- [A parent answers what the human wanted to decide] → Product, security and approval questions must be escalated by instruction, every answer the parent gives is in the child's chat labelled with the parent, and the Inbox note trail shows what happened.
- [A busy parent delays its children's questions] → A child's row shows the hollow badge, and the check-in rules apply to the parent as to any coordinator.
- [Retiring groups changes a sidebar people rely on] → The Outcome lens keeps rows in the stored order, and loose projects stay where they were; filing is opt-in.
- [Tags are only as right as the repository table] → An untagged repository puts its projects under "no app" or "no layer", where they are easy to notice and fix once.
- [This change rewrites `Agents panel` in work-progress] → Its delta is written against the text `agents-panel-finished-tickets` leaves, so that change archives first.

## Migration Plan

Every new field defaults to empty: no parent, not standing, no question routing state (meaning with the human), no tags. Existing projects therefore appear loose in the Outcome lens, in the order they had. `created_by` and `created_projects` keep working for links made before this change. The desktop ignores the saved Group choice. Rolling back leaves the new fields unread; an older host ignores the new `space.yaml` keys.

## Open Questions

None. The studies' open questions were settled by Michael's decisions in the ticket brief: groups retire rather than staying for loose projects, the Inbox shows a child's question only once escalated, the Me lens replaces the Needs you page, and app plus layer are the only tags.
