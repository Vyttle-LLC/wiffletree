Six slices, one ticket each. Dependencies:

- (a) depends on nothing.
- (b) depends on (a).
- (c) depends on nothing, and can run beside (a) and (b).
- (d) depends on (a).
- (e) depends on (b), (c) and (d).
- (f) depends on (a) and (c); its hollow badge on child cards needs (b).

`work-progress`'s Agents panel delta is written against the text `agents-panel-finished-tickets` leaves, so that change archives before this one.

## 1. Slice (a) — Host: parent link, child creation, messaging, close guard, Move under

- [ ] 1.1 `workspace-core`: `Project.parent_id` and `Project.standing`, both defaulting to empty and left out of the store when empty; the snapshot carries both.
- [ ] 1.2 Host checks for every write of the link: same Space, one level, not onto itself, not onto a child, not for a project with children; a refusal names the rule.
- [ ] 1.3 A command to set or clear a project's parent, with `project_moved_under` and `project_moved_out` activity on the project and each parent; questions with a parent go to the human when its child is made loose.
- [ ] 1.4 Move to another Space clears the project's link and its children's; the Move dialog's wording names the children that become loose.
- [ ] 1.5 `create_project` makes a child or a sibling, with the notice to the parent for a sibling; parents' messages do not count as the human's for the briefed-project rule.
- [ ] 1.6 `send_message` between a parent's and its children's coordinators; a child coordinator's `report` delivered to its parent's coordinator; siblings refused with the reason.
- [ ] 1.7 `workspace_context`: `parent_project` and `child_projects`; `created_projects` leaves out the caller's children.
- [ ] 1.8 Archive refuses a parent with an unarchived child; restore refuses a child of an archived parent.
- [ ] 1.9 A command to set `standing`, for the human only.
- [ ] 1.10 `coordinator.md`: the naming and filing rules and the parent's escalation duty; bump its version.
- [ ] 1.11 Desktop: "Move under…" and "Standing" in the project menu, Standing in Overview, the confirmation, the refusals' words.
- [ ] 1.12 Tests: each refused link, an older store loose with `created_by` kept, a child and a sibling created, messages allowed and refused, reports reaching the parent, the archive and restore guards, a Space move clearing links.

## 2. Slice (b) — Host: questions routed through the parent

- [ ] 2.1 `Attention` gains its routing state, with the parent or with the human, defaulting to the human for older stores.
- [ ] 2.2 A child coordinator's `ask_user` creates the item with the parent and wakes the parent's coordinator with the question; approvals keep going to the human.
- [ ] 2.3 `answer_question` and `escalate_question`, offered to coordinators, refused for questions that are not their children's or not with them, and "already answered" or "already with the human" on a race.
- [ ] 2.4 The answer resumes the child as a human answer does and counts for `questions_waiting`.
- [ ] 2.5 The one-line notice to the parent when the human answers or messages a child, delivered with its next turn without starting one.
- [ ] 2.6 Inbox, needs-you counts and the snapshot count only items with the human; the snapshot carries each project's count with its parent for the hollow badge.
- [ ] 2.7 Desktop: the escalated Inbox label "<child> · via <parent>" with the note, and the hollow badge on a child's row.
- [ ] 2.8 Tests: answered by the parent, escalated, a race either way, an approval skipping the parent, a child unfiled with questions waiting, the notice not starting a turn, a restart with a question with the parent.

## 3. Slice (c) — Space repository tags and derived project tags

- [ ] 3.1 `space.yaml`: `apps`, `layers` and `repository_tags`, read as empty when missing; edits commit and sync as other Space edits.
- [ ] 3.2 The derived apps and layers on each project, "no app", "no layer" and "not started", in the snapshot and the coordinator's `workspace_context`.
- [ ] 3.3 The suggestion from the longest shared leading run of hyphen-separated words.
- [ ] 3.4 Desktop: App and Layer on the Space page's code repositories, the value menus, typing a value, removing a value in use, and the suggestion on add.
- [ ] 3.5 Tests: an older Space file, two Spaces tagging one repository differently, a project's union across ticket states, retagging, the suggestion with and without a match.

## 4. Slice (d) — Desktop: Outcome lens tree and reparent drag, groups retired

- [ ] 4.1 Remove first-word groups: the Group button, group rows, shortened names and the grouping preference, which is ignored when read.
- [ ] 4.2 The lens control with Outcome only enabled until (e) lands, saved in `desktop-preferences.json`, and ⌘1.
- [ ] 4.3 The Outcome tree: parents with their children under them, loose projects, the "standing" pill, the hollow badge.
- [ ] 4.4 The collapsed parent row with "+N" and the most urgent state; selecting a hidden child expands its parent.
- [ ] 4.5 Dragging: parents as blocks, children among siblings, filing and unfiling drops with the confirmation, no target where the host would refuse.
- [ ] 4.6 Tests: the tree from the stored order with a child above its parent, the roll-up, each drop kind, a cancelled confirmation, an older preferences file.

## 5. Slice (e) — Desktop: App, Layer and Me lenses

- [ ] 5.1 App and Layer: buckets per tag in vocabulary order, "no app" or "no layer", "not started", a project under every bucket, the parent line, folding with the first three names, no drag; ⌘2 and ⌘3.
- [ ] 5.2 Me: Questions, Merge on GitHub, Going quiet and Ready to close with their reasons, the selected Space expanded and others collapsed with counts; ⌘4.
- [ ] 5.3 The Me badge across Spaces, shown in every lens.
- [ ] 5.4 Tests: a project in two buckets, each Me bucket's rule including a standing project and a parent with an open child, the badge count, keyboard switching while a composer has focus.

## 6. Slice (f) — Desktop: Agents in the rail, breadcrumb and chips

- [ ] 6.1 Remove the header's Agents toggle; add the Agents rail button after Overview with its working badge, in every project conversation.
- [ ] 6.2 A parent's panel: "Child projects" with a folded card per child; activating one selects the child's coordinator.
- [ ] 6.3 The child's header "← Parent / Child" and its strip line.
- [ ] 6.4 Tag chips after the title, each opening its lens with the bucket expanded and in view.
- [ ] 6.5 Tests: the rail's order, the badge counting a child's agents, a child card selecting the child, the breadcrumb, a chip switching lens without changing the conversation.

## 7. Docs

- [ ] 7.1 `docs/host-protocol.md` for the new fields, commands and tools; `docs/FEATURES.md` as each slice lands.
