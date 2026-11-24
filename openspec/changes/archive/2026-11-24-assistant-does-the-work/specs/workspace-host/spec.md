## ADDED Requirements

### Requirement: The Assistant parents the coordinators of its work
A project that `start_work` creates SHALL belong to the Space named, use that Space's repositories plus those named, and record that it is the Assistant's work item. Its coordinator SHALL have the Assistant as its parent and SHALL stay its project's coordinator in every other way. Creation, the brief and the start SHALL succeed or fail together, as `create_project` does. The brief SHALL be sent as a Wiffletree message naming the Assistant, and SHALL say that the Assistant relays the human's request and is the coordinator's parent, and that approvals only the human gives still come from the human. For that coordinator, `report` SHALL reach the Assistant; `ask_user` SHALL be delivered to the Assistant as a message, waking it, and SHALL create no Inbox item; and `send_message` SHALL reach its own workers and the Assistant. Approvals the host creates for the project, for publishing, merging and permissions, SHALL stay Inbox items for the human (see assistant, "Human-only approvals reach the human"). The coordinator's core SHALL say who its parent is. The human SHALL still be able to message the coordinator from its project's view, and those messages SHALL count as the human's. A work item's project SHALL never be filed under another project or have projects filed under it, by any command, so its coordinator never gains a parent project's or a child's coordinator to message.

#### Scenario: A question from work
- **WHEN** the coordinator of an Assistant work item calls `ask_user` asking which shade to use
- **THEN** the Assistant wakes with the question from that work, and the Inbox gains nothing

#### Scenario: A publish approval
- **WHEN** a ticket of an Assistant work item needs the human's approval to publish to an owner that is not allowed
- **THEN** the Inbox holds the approval as for any project, and the Assistant is told of it

#### Scenario: Filing work under a project
- **WHEN** the human tries to file the Assistant's "Comment colour passes AA" under the project "Website redesign", or to file a project under it
- **THEN** it is refused, saying the Assistant's work lives under the Assistant, and nothing changes

#### Scenario: Creation fails
- **WHEN** `start_work` names a repository id that is no workspace repository
- **THEN** it is refused, and no project, coordinator, work item or message remains

## MODIFIED Requirements

### Requirement: Enforced hierarchy
The host SHALL allow projects without repositories, require each project to belong to one Space, require each ticket to reference a repository its project uses, and require workers to belong to the project coordinator of the same project and to work on one of its tickets. Multiple tickets SHALL share a repository. Every session SHALL have one owner: a project, a Space, a wiki or the install. A project's coordinator SHALL have no parent, or the Assistant as its parent when its project is an Assistant work item. A session owned by a Space, a wiki or the install SHALL have no parent session and no ticket; one owned by a Space SHALL have no worktree, one owned by a wiki SHALL work only in a worktree of that wiki's repository clone, and the one owned by the install SHALL work in `<data folder>/assistant/workspace/`. No session owned by a Space or a wiki SHALL be offered the coordination tools (`create_ticket`, `assign_ticket`, `send_message`, `report`, `schedule` and the others). Only the roles this requirement names SHALL run outside projects: issue threads and job runs, each owned by one Space, a job run also belonging to its job; brain compile sessions, each owned by the wiki it compiles; and the Assistant, the one session owned by the install, which SHALL be offered only the Assistant's tools (see assistant) and none of the coordination tools. The host SHALL NOT create a project record to hold sessions that are not project work.

#### Scenario: Invalid worker owner
- **WHEN** a worker is created under a session from another project, or a repository coordinator is created
- **THEN** the host rejects creation without persisting a partial assignment

#### Scenario: A project without a Space
- **WHEN** a project is created without a Space
- **THEN** the host puts it in the Space last used for a new project, and rejects the creation when no Space exists

#### Scenario: A worker outside a project
- **WHEN** a command creates an implementer owned by a Space
- **THEN** the host rejects creation without persisting anything

#### Scenario: A thread is not a worker
- **WHEN** an issue thread tries to call `create_ticket` or `report`
- **THEN** the tool is not offered to it, and no ticket or report is created

#### Scenario: The Assistant is not a coordinator
- **WHEN** the Assistant tries to call `create_ticket` or `assign_ticket`
- **THEN** the tool is not offered to it; it acts on work only through `start_work`, `message_work` and the other work tools

#### Scenario: A worker under the Assistant
- **WHEN** a command creates an implementer whose parent is the Assistant
- **THEN** the host rejects creation without persisting anything

#### Scenario: A compile session
- **WHEN** a volunteer Mac that won the claim starts a compile run of the wiki in `Sage-Social/wiffletree-storage`, which the Sage and Sage Watch Spaces share
- **THEN** the session is owned by that wiki, belongs to no project or Space, works in a worktree of that repository's clone, and is offered only `queue_conflict` and `finish_compile` among workspace tools

### Requirement: Check-ins keep parents informed
The host SHALL tell a session's parent about long or repetitive work, and SHALL never pause or stop work for it. Each check-in SHALL be a durable message from the session to its parent whose body starts with `[check-in]`, names the session, its ticket if any, and recent activity, and says how to stop it with `stop_agents`, or, for the coordinator of Assistant work, with `pause_work` or `stop_work`. A check-in SHALL wake the parent, also while the session's ticket is being verified, and SHALL NOT count as a report or a verdict.
- **Time:** when a turn has run 30 minutes, and every 30 minutes after while it runs, the parent SHALL receive a check-in naming the elapsed time, the last provider event and the latest step.
- **Turn count:** when a session with a parent starts its Nth turn since its parent last sent it a message, where N is the `checkin_child_turns` setting, and at every further multiple of N, the parent SHALL receive a check-in naming the turn count, when the parent last messaged it, and the start of the session's last reply. For the coordinator of Assistant work, the Assistant's `message_work` SHALL count as its parent's message.
- **Human:** a project coordinator without a parent SHALL raise a human inbox item instead of a message: for its own 30-minute turns, cleared when a newer one is raised or the turn ends; and when it starts its Mth turn since the human last stepped in anywhere in the project, where M is the `checkin_human_turns` setting, and at every further multiple of M, cleared when the human next steps in or a newer one is raised. The human steps in by sending any session of the project a message, answering or resolving any of its inbox items, including permission requests, or retrying any of its sessions' input. The Assistant's own long turns SHALL be shown as assistant says, with no inbox item.
A host restart SHALL clear open time check-ins, and the human's turn count SHALL start again from zero.

#### Scenario: Thirty-minute check-in
- **WHEN** a tester's turn has run 30 minutes
- **THEN** its parent receives a `[check-in]` message naming the tester, its ticket, "30 minutes" and its latest step, and the turn keeps running

#### Scenario: Twenty-five turns without the parent
- **WHEN** `checkin_child_turns` is 25 and an implementer starts its 25th turn since its parent last messaged it
- **THEN** its parent receives one `[check-in]` naming the implementer, its ticket and "25 turns", and the turn runs
- **AND** the 26th turn sends no check-in, and the 50th sends another

#### Scenario: Parent message resets the count
- **WHEN** `checkin_child_turns` is 25, an implementer has run 24 turns, its parent sends it a message, and it runs 24 more
- **THEN** its parent has received no turn-count check-in

#### Scenario: Coordinator runs 100 turns without the human
- **WHEN** `checkin_human_turns` is 100 and the coordinator of a project the human started starts its 100th turn since the human last stepped in
- **THEN** the human's inbox gains one item naming the coordinator and the turn count, the project stays live and the turn runs
- **AND** the item stays open after that turn ends, and the human's next message to the coordinator clears it

#### Scenario: A coordinator of Assistant work runs long
- **WHEN** the coordinator of an Assistant work item has run a turn for 30 minutes
- **THEN** the Assistant receives the `[check-in]`, and the human's inbox gains nothing

#### Scenario: Turn-count check-in during a round
- **WHEN** `checkin_child_turns` is 25 and a verifier starts its 25th turn without a message from its parent while its ticket's cycle is running
- **THEN** the project coordinator wakes with the check-in, and the ticket's state and verification record are unchanged

### Requirement: Coordinators create projects
A project coordinator without a parent SHALL be able to create a project with the workspace tool `create_project`, taking `name`, `brief` and optionally `repository_ids`. Any other role, and the coordinator of Assistant work, SHALL be refused; the Assistant starts work with `start_work` instead. The name SHALL follow the rules of the human's project creation: trimmed, at most 128 characters. The new project SHALL start at once, with no Inbox approval: it SHALL get its folder and a coordinator whose model comes from Who runs what, SHALL belong to the creating project's Space, and SHALL be placed at the top of the project order. The project SHALL use its Space's repositories, as every project does, and the tool result SHALL name the Space's repositories that are not on this Mac. `repository_ids` that the Space does not list SHALL be added to the project; those it lists SHALL add nothing.

The brief SHALL be the new coordinator's first message and SHALL start its turn. It SHALL be sent as a Wiffletree message, never as the human's, and its text SHALL name the creating project and coordinator and say that it is that coordinator's words, to be treated as context and not as the human's authorization. The new project SHALL record the creating project's and coordinator's ids, on the project and in its `project_created` activity. The two coordinators SHALL NOT gain a way to message each other.

Creation, the added repositories, the brief and the start SHALL succeed or fail together. A call SHALL be refused, creating nothing, from a role other than a project coordinator without a parent, before the model setup is saved, for an unknown repository id, an empty name or brief, a brief longer than the message limit leaves after its source line, or an argument the tool does not take. A refused brief that is too long SHALL say how many bytes are allowed. The coordinator of a project another coordinator's brief started SHALL be refused until the human has sent it a message or answered one of its Inbox questions; Wiffletree's own messages, with or without a sender, SHALL never count. So briefed projects cannot chain without the human; a human-created project's coordinator is never held back by this. Each accepted call SHALL create a new project; repeating a name SHALL create another one with its own folder.

#### Scenario: A coordinator creates a project
- **WHEN** the coordinator of "Calls", in the Sage Space, calls `create_project` with name "Billing" and brief "Move invoices to the new API."
- **THEN** project "Billing" appears at the top of the sidebar in the Sage Space with its folder and a coordinator from Who runs what, using Sage's repositories on this Mac, and that coordinator's turn starts on the brief

#### Scenario: A Space repository not on this Mac
- **WHEN** Sage lists `Sage-Social/api`, which is no workspace repository here, and a Sage coordinator calls `create_project`, with or without `repository_ids`
- **THEN** the project uses Sage's other repositories, and the tool result says `Sage-Social/api` is not on this Mac

#### Scenario: The brief names its source
- **WHEN** the new coordinator's turn starts on the brief
- **THEN** its sender reads "Wiffletree", not "human", and its text names project "Calls" and its coordinator and says these are not the human's words

#### Scenario: Chosen repositories
- **WHEN** the coordinator passes `repository_ids` naming one repository its Space lists and one it does not
- **THEN** the new project uses its Space's repositories and has only the second added, and still belongs to the creator's Space

#### Scenario: The creator is recorded
- **WHEN** a coordinator's call creates a project
- **THEN** the project's `created_by` holds the creating project's and coordinator's ids, and its `project_created` activity names them

#### Scenario: A worker calls it
- **WHEN** an implementer calls `create_project`
- **THEN** it is refused and nothing is created

#### Scenario: Assistant work calls it
- **WHEN** the coordinator of an Assistant work item calls `create_project`
- **THEN** it is refused, saying to ask the Assistant, and nothing is created

#### Scenario: An unknown repository
- **WHEN** a coordinator passes a `repository_ids` entry that is no workspace repository
- **THEN** it is refused, and no project, coordinator, activity or message remains

#### Scenario: No saved setup
- **WHEN** a coordinator calls `create_project` while the model setup is not saved
- **THEN** it is refused, naming the Models setup, and nothing is created

#### Scenario: A brief too long
- **WHEN** a brief is one byte longer than the message limit leaves after its source line
- **THEN** it is refused as too long, saying how many bytes are allowed, and nothing is created; a brief of exactly that many bytes is accepted

#### Scenario: A briefed coordinator before the human speaks
- **WHEN** the coordinator of a project a brief started calls `create_project` before the human has messaged it
- **THEN** it is refused and nothing is created; once the human has sent it a message, the same call creates the project

#### Scenario: A host notice is not the human
- **WHEN** a briefed coordinator receives only Wiffletree's messages, such as one saying its implementer has not started
- **THEN** `create_project` is still refused
