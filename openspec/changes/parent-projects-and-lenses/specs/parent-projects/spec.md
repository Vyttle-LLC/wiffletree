## ADDED Requirements

### Requirement: A project may have a parent project
A project SHALL have at most one parent project, stored by the host as the project's `parent_id`, and a project with no parent SHALL be loose. A project whose `parent_id` names it SHALL be that project's child, and a project with at least one child SHALL be a parent. The parent SHALL be in the same Space as the child. The tree SHALL be one level deep: a project with a parent SHALL NOT have children, and a project with children SHALL NOT have a parent. The host SHALL refuse any write that breaks these rules, change nothing, and say which rule it broke. Moving a project to another Space SHALL make it loose and SHALL make each of its children loose in their own Space, and the Move dialog SHALL say so before the human confirms; such a move SHALL still never be refused. The link SHALL survive a restart and SHALL appear on the project in every snapshot. A store from before this change SHALL open with every project loose, and a project's `created_by` SHALL NOT be turned into a parent link, then or later.

#### Scenario: A child in the snapshot
- **WHEN** "Contrast warnings" is a child of "Accessible contrast" and the host restarts
- **THEN** the snapshot gives "Contrast warnings" the parent "Accessible contrast", and "Accessible contrast" no parent

#### Scenario: No grandchildren
- **WHEN** any write would make "Contrast warnings", a child, the parent of another project
- **THEN** the host refuses it, saying a child cannot have children, and no link changes

#### Scenario: Another Space
- **WHEN** a write would make a Celadon project the parent of a Vyttle project
- **THEN** the host refuses it, saying a parent must be in the same Space, and no link changes

#### Scenario: A parent moves to another Space
- **WHEN** the human moves "Light themes", the parent of two children, from Celadon to Vyttle
- **THEN** the dialog says both children become loose and stay in Celadon, and after confirming "Light themes" is loose in Vyttle and its former children are loose in Celadon

#### Scenario: An older store
- **WHEN** a store from before this change holds "Pickers", created by the coordinator of "Accessible contrast" with `create_project`
- **THEN** "Pickers" opens loose, its `created_by` still names "Accessible contrast", and the coordinator of "Accessible contrast" still lists it under `created_projects`

### Requirement: The human files a project under a parent
The project menu SHALL offer "Move under…", which lists the other unarchived loose projects of the project's Space as possible parents and, for a child, "No parent". Dragging in the Outcome lens (see sidebar-lenses) SHALL make the same change. Either way, the desktop SHALL ask first, naming the project and its new parent, or saying the project becomes loose, and SHALL change nothing when the human cancels. The host SHALL refuse to move under a parent a project that has children, a project onto itself, and a project onto a child or a project in another Space. "Move under…" SHALL be disabled, with a tooltip saying why, for a project that has children. A project filed under a parent SHALL route its later questions through that parent; its questions already with the human SHALL stay with the human. A child that leaves its parent, made loose or filed under another parent, SHALL in the same change send every question still with its old parent to the human, each labelled with its name; the old parent SHALL no longer be able to answer them, and the new parent SHALL receive only questions asked after the move. Each change SHALL be recorded as activity on the project and on each parent it joined or left.

#### Scenario: Filing a loose project
- **WHEN** the human chooses "Move under…" on "Redo sponsor page", picks "Sponsorships" and confirms
- **THEN** "Redo sponsor page" becomes a child of "Sponsorships", and both projects' activity records the move

#### Scenario: Cancel
- **WHEN** the human picks a parent in "Move under…" and cancels the confirmation
- **THEN** no link changes

#### Scenario: A parent cannot be filed
- **WHEN** the human opens the menu of "Accessible contrast", which has children
- **THEN** "Move under…" is disabled, and its tooltip says a project with children cannot have a parent

#### Scenario: Unfiling sends waiting questions to the human
- **WHEN** "Contrast warnings" has one question with its parent and the human chooses "No parent"
- **THEN** after confirming, "Contrast warnings" is loose and that question is in the human's Inbox labelled "Contrast warnings"

#### Scenario: Refiling sends waiting questions to the human
- **WHEN** "Contrast warnings", a child of "Accessible contrast", has one question with "Accessible contrast", and the human files it under "Light themes" before "Accessible contrast" answers
- **THEN** after confirming, that question is in the human's Inbox labelled "Contrast warnings", "Accessible contrast"'s `answer_question` on it is refused, and "Light themes" is not woken with it

#### Scenario: Filing keeps the human's questions
- **WHEN** a loose project with one question in the human's Inbox is filed under a parent
- **THEN** that question stays in the human's Inbox, and the project's next `ask_user` goes to the parent

### Requirement: A coordinator's new project is a child
`create_project` SHALL put the new project under the caller's project when the caller's project is loose or a parent, and under the caller's parent when the caller's project is a child, so the new project is the caller's sibling. The new project SHALL be in that parent's Space. When a child creates a sibling, the parent's coordinator SHALL get a one-line Wiffletree notice naming both projects. Every refusal `create_project` already has SHALL still apply, including the refusal for a coordinator a brief started until the human has spoken to it; a message from a parent's coordinator SHALL NOT count as the human's.

#### Scenario: A loose coordinator creates a child
- **WHEN** the coordinator of loose project "Light themes" calls `create_project` with name "Daylight variant, softer whites"
- **THEN** the new project is a child of "Light themes" in the same Space, and "Light themes" is now a parent

#### Scenario: A child creates a sibling
- **WHEN** the coordinator of "Contrast warnings", a child of "Accessible contrast", calls `create_project` with name "Warning popup copy"
- **THEN** the new project is a child of "Accessible contrast", and the coordinator of "Accessible contrast" gets one notice naming "Contrast warnings" and "Warning popup copy"

#### Scenario: The parent's word is not the human's
- **WHEN** the coordinator of a project a brief started has received messages only from its parent's coordinator and Wiffletree, and calls `create_project`
- **THEN** it is refused, and nothing is created

### Requirement: Parent and child coordinators message each other
A parent project's coordinator and each of its children's coordinators SHALL be able to `send_message` each other, as a coordinator and its workers can. A child coordinator's `report` SHALL be delivered to its parent project's coordinator, with the same waking rules as any report. In the parent's conversation, a child's reports and messages SHALL fold into one line per run as other peer messages do (see work-stream), naming the child. A message from the parent SHALL show in the child's conversation under the parent's name. Coordinators of sibling projects SHALL NOT message each other, and a coordinator SHALL NOT message a project's coordinator it has no parent link with; such a call SHALL be refused, saying who it may message. `workspace_context` SHALL give a child coordinator `parent_project`, with the parent's id, name and coordinator id, and a parent coordinator `child_projects`, one entry per child with its id, name, coordinator id, standing flag and its tickets as the short entries accepted and closed tickets have.

#### Scenario: Parent to child
- **WHEN** the coordinator of "Accessible contrast" sends a message to the coordinator of its child "Contrast warnings"
- **THEN** it is delivered, and the child's conversation shows it as from "Accessible contrast"

#### Scenario: A child reports
- **WHEN** the coordinator of "Pickers: restore highlights" reports progress "fix ready for review"
- **THEN** the coordinator of its parent "Accessible contrast" receives it with its next turn, and its conversation shows one folded line naming "Pickers: restore highlights"

#### Scenario: Siblings
- **WHEN** the coordinator of "Contrast warnings" sends a message to the coordinator of its sibling "Pickers: restore highlights"
- **THEN** it is refused, saying it may message its parent's coordinator and its own agents, and nothing is delivered

#### Scenario: Who knows whom
- **WHEN** the coordinators of "Accessible contrast" and of its child "Contrast warnings" read `workspace_context`
- **THEN** "Accessible contrast" lists "Contrast warnings" under `child_projects` with its tickets, and "Contrast warnings" names "Accessible contrast" as `parent_project`

### Requirement: A child's questions go to its parent first
A child coordinator's `ask_user` SHALL create its Inbox item with the parent, not the human, and SHALL wake the parent's coordinator with the question, its id, the child's name and its choices. Approvals that are not questions, such as `request_permission`, publish and merge approvals and `request_repository`, SHALL reach the human directly as today. The parent's coordinator SHALL resolve each question with one of two tools, offered only for its own children's questions with the parent: `answer_question`, taking the question id and an answer, which SHALL close the item and resume the child with the answer, labelled as its parent's; or `escalate_question`, taking the question id and an optional note, which SHALL move the item to the human's Inbox labelled with the child's name and "via" the parent, with the note. A question already answered or escalated SHALL be refused with "already answered" or "already with the human". The Inbox, the needs-you counts and the Me lens SHALL count only items with the human. A child's row SHALL show, beside any filled needs-you badge, a hollow badge counting its questions with its parent, with the tooltip "<parent> is handling it". The answer that resumes a child SHALL count as answering its blocked or waiting question as a human answer does. A loose or parent project's `ask_user` SHALL reach the human as today.

#### Scenario: The parent answers
- **WHEN** the coordinator of "Contrast warnings" asks whether comments get a contrast warning when no minimum is set, and its parent "Accessible contrast" calls `answer_question` with "Yes, WCAG AA is always the floor."
- **THEN** the item closes without reaching the human's Inbox, and "Contrast warnings" resumes with that answer shown as from "Accessible contrast"

#### Scenario: The parent escalates
- **WHEN** "Accessible contrast" calls `escalate_question` on a copy question from "Contrast warnings" with the note "Product call on user-facing copy"
- **THEN** the human's Inbox shows the question labelled "Contrast warnings · via Accessible contrast" with the note, and the Me badge counts it

#### Scenario: Waiting with the parent
- **WHEN** "Contrast warnings" has three questions with its parent and none with the human
- **THEN** its row shows a hollow badge "3" and no filled badge, and the human's Inbox shows none of them

#### Scenario: An approval skips the parent
- **WHEN** a child coordinator's ticket needs the human to approve a publish
- **THEN** the approval goes straight to the human's Inbox

#### Scenario: Answered twice
- **WHEN** the human answers an escalated question and the parent then calls `answer_question` on it
- **THEN** the call is refused with "already answered", and the child receives only the human's answer

#### Scenario: Not its child
- **WHEN** a coordinator calls `answer_question` with the id of a question from a project that is not its child, or of its own question
- **THEN** it is refused and the item is unchanged

### Requirement: The parent hears when the human talks to a child
When the human answers one of a child's questions or sends a message to a child's coordinator, the parent's coordinator SHALL get one Wiffletree notice of one line, naming the child and what the human answered or the message's first line. The notice SHALL NOT start a turn by itself and SHALL be delivered with the parent's next turn.

#### Scenario: The human messages a child
- **WHEN** the human sends "For the warning popup, keep the wording short. One line, no ratio." to the coordinator of "Contrast warnings" while "Accessible contrast" is idle
- **THEN** "Accessible contrast" does not start a turn, and its next turn receives one line naming "Contrast warnings" and the message's first line

#### Scenario: The human answers an escalated question
- **WHEN** the human answers a question "Accessible contrast" escalated from "Contrast warnings"
- **THEN** "Contrast warnings" resumes with the answer, and "Accessible contrast" gets one line with the answer

### Requirement: A parent stays open while a child is open
The human SHALL close a project by archiving it, and no workspace tool SHALL archive a project. The host SHALL refuse to archive a project while any of its children is unarchived, naming those children, and SHALL refuse to restore a child whose parent is archived, naming the parent. The desktop SHALL show the refusal's words.

#### Scenario: Closing a parent with an open child
- **WHEN** the human archives "Light themes" while its child "Generated palette check" is unarchived
- **THEN** the host refuses, naming "Generated palette check", and nothing is archived

#### Scenario: Closing a parent whose children are closed
- **WHEN** every child of "Sponsorships" is archived and the human archives "Sponsorships"
- **THEN** it is archived

#### Scenario: Restoring a child first
- **WHEN** the human restores "Redo sponsor page" while its parent "Sponsorships" is archived
- **THEN** the host refuses, saying to restore "Sponsorships" first

### Requirement: Standing projects
A project SHALL carry a `standing` flag, off by default, that only the human sets, from the project menu and its Overview. A standing project SHALL show a "standing" pill on its row and SHALL never be listed under Ready to close (see sidebar-lenses). The flag SHALL change nothing else: a standing project is archived, filed and messaged like any other.

#### Scenario: Platform never finishes
- **WHEN** "Platform" is standing and every ticket of it is accepted with its PR merged
- **THEN** its row shows "standing" and the Me lens does not list it under Ready to close

#### Scenario: Only the human
- **WHEN** a coordinator looks for a way to set a project standing
- **THEN** no workspace tool offers one

### Requirement: Coordinators follow the naming and filing rules
The project coordinator's instructions SHALL say: prefer adding a ticket to your own project; create a project only for a separate outcome; a parent is named after an outcome, never an app or a layer; a child is named after one deliverable, without repeating its parent's name; a parent answers its children's questions only from decisions already made, such as the brain or the human's earlier answers, and escalates every product, security and approval question. The host SHALL NOT check project names against these rules.

#### Scenario: The instructions
- **WHEN** a project coordinator's instructions are assembled
- **THEN** they contain each of these rules, and the parent's duty to escalate product, security and approval questions

#### Scenario: No name check
- **WHEN** a coordinator calls `create_project` with the name "Backend: palette API"
- **THEN** the project is created with that name
