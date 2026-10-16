# native-workspace Specification

## Purpose
Render a responsive native chat-first workspace backed by durable host records and explicit prototype capabilities.

## Requirements

### Requirement: Native durable workspace
The macOS client SHALL render project → ticket → agents, select conversations, queue messages, retain per-session drafts during navigation, add tickets and agents, show status separately from attention and open context panels without replacing chat.

A persistent right-edge inspector rail SHALL follow the selected project, ticket or agent. Its destinations SHALL reflect that scope, and its panel SHALL identify the owner. Project Inbox SHALL remain separate from ticket and agent status; Git SHALL appear only for repository-backed ticket and agent views. Ticket, agent, project and repository creation SHALL use separate dialogs with explicit repository or ticket ownership, retained values on failure and guarded pending submissions. Successful project creation SHALL select the new project orchestrator. The hierarchy and context panes SHALL have draggable width dividers and compact native typography.

#### Scenario: Creation fails without losing input
- **WHEN** a repository attachment or session creation fails validation or host persistence
- **THEN** the dialog remains open with its entered values and error, and permits a corrected submission

#### Scenario: Longer ticket names need more space
- **WHEN** the user drags the hierarchy divider wider
- **THEN** the hierarchy gains width while the conversation and optional context panel remain usable

#### Scenario: Change conversation
- **WHEN** the user types a draft, selects a worker and returns
- **THEN** the draft is retained and only bounded transcript pages are rendered

### Requirement: Configurable routing and memory
Context panels SHALL show scoped logs, support milestone capture and show persisted activity and attention. Optional Git history SHALL show real read-only data. The Models page SHALL have four cards:
- **Providers:** an enable toggle per provider; the CLI status that existing discovery reports; an Add row with a model dropdown that omits models already listed and an Add button, which allows the model at its first effort; and one line per allowed model with a checkbox for each effort it supports and Remove. Unticking a model's last effort removes it. The allowlist stays a set of exact model and effort pairs.
- **Roles:** provider checkboxes for the project coordinator, implementer and tester.
- **Review:** a Levels block, the verifier list (role, focus, provider or "Any", instruction), the round cap and the cycle cap, with "Run reviso:style with Codex" as example text. The Levels block SHALL show, for each review level, its name, its kind as a choice between "Full review" and "Re-check", one tick for each configured verifier, and its note on when to use it, labelled "Use it". It SHALL offer Add level, a control that opens a level's name and note for editing, and Remove. It SHALL say that a full review can be a ticket's only review, that a re-check starts only on a ticket whose last review passed, and that the workspace enforces this. A new level SHALL start as a re-check. While no level is a re-check, no level is named "Light" ignoring case and a reviewer is configured, the Add level form SHALL be filled in with the name "Light", the first configured reviewer ticked and the suggested Light note. Otherwise the form SHALL open empty, as a re-check. Adding a verifier SHALL tick it in every level whose kind is full review, and in no re-check level, in the same save. Removing a verifier SHALL remove it from every level in the same save. When the host refuses a change, the card SHALL show the host's reason and keep showing the saved setting. The round cap SHALL be labelled "Rounds per review", with the caption "The first review plus re-checks after the implementer's fixes. If the last round still finds blocking problems, the review stops and the coordinator decides. The same at every level." The cycle cap SHALL be labelled "Full reviews per ticket", with the caption "Reviews at a level marked Full review, with fresh reviewers. After the last one, the coordinator accepts, accepts with a waiver, closes the ticket or asks you. Re-checks are not counted." Each change saves at once. The card SHALL NOT warn about turn limits, because there are none. Settings SHALL NOT edit the verifier list.
- **Guide:** a Markdown editor.

Providers, Roles and Guide SHALL save together with one Save.

The page SHALL NOT offer routing tiers, Big/Small profiles or per-role default models.

#### Scenario: Unavailable integration
- **WHEN** a user opens provider quota, semantic compilation or PR integration
- **THEN** the UI identifies unavailable capability instead of presenting sample data as live

#### Scenario: Configure a single-subscription machine
- **WHEN** the human disables Claude, sets every role to Codex, edits the guide and saves
- **THEN** all of it survives restart and appears in the coordinator's next workspace context

#### Scenario: Role left without a provider
- **WHEN** the human disables the only provider a role uses
- **THEN** that role shows an inline error and Save stays disabled until it has an enabled provider

#### Scenario: Cycle cap on the Review card
- **WHEN** the human sets the cycle cap to 3 on the Review card
- **THEN** it saves at once, survives restart and appears in the coordinator's next workspace context

#### Scenario: Caps in plain words
- **WHEN** the human opens the Review card
- **THEN** it shows "Rounds per review" and "Full reviews per ticket", each with its caption, and no control is labelled "Round cap" or "Cycle cap"

#### Scenario: Long verifier list
- **WHEN** the human adds a seventh verifier on the Review card
- **THEN** it saves at once and the card shows no turn-limit note

#### Scenario: Levels on the Review card
- **WHEN** the human opens the Review card with the levels "Full" and "Light" and four verifiers
- **THEN** each level shows its name, whether it is a full review or a re-check, a tick for each of the four verifiers and its "Use it" line, above the verifier list

#### Scenario: A tick saves at once
- **WHEN** the human ticks Tests in the level "Light"
- **THEN** it saves at once, survives restart, and the coordinator's next workspace context shows "Light" running Tests and Correctness

#### Scenario: Adding a level
- **WHEN** the human presses Add level, names it "Docs", ticks Style, writes its "Use it" line and confirms
- **THEN** the level is saved as a re-check and appears below the others

#### Scenario: Editing a level's name and note
- **WHEN** the human opens "Light" for editing, rewrites its "Use it" line and confirms
- **THEN** the new line is saved and shown, and cancelling instead leaves the saved line

#### Scenario: Light is suggested after an update
- **WHEN** the only level is "Full" and the human presses Add level
- **THEN** the form is filled in with the name "Light", the first reviewer ticked and the suggested Light note, and nothing is saved until the human confirms

#### Scenario: A level named Light already exists
- **WHEN** the levels are "Full" and "Light", both marked "Full review", and the human presses Add level
- **THEN** the form opens empty as a re-check, with no name, no verifier ticked and no "Use it" line

#### Scenario: No reviewer to suggest
- **WHEN** the only level is "Full", every configured verifier is a tester, and the human presses Add level
- **THEN** the form opens empty as a re-check

#### Scenario: The last full review
- **WHEN** the human switches the only level marked "Full review" to "Re-check", or removes it
- **THEN** the card shows "Keep at least one level marked Full review." and the level stays as it was

#### Scenario: Removing a level's only verifier
- **WHEN** the level "Light" runs only Correctness and the human removes the verifier Correctness
- **THEN** the card shows the host's reason, and the verifier and the level stay as they were

#### Scenario: A new verifier joins the full reviews
- **WHEN** the levels are "Full" and "Docs", both marked "Full review", and "Light", a re-check, and the human adds the verifier Security
- **THEN** one save adds Security to the verifier list and ticks it in "Full" and "Docs", and "Light" is unchanged

#### Scenario: Removing a verifier that levels share
- **WHEN** the human removes the verifier Style, which "Full" runs beside three others
- **THEN** Style disappears from the verifier list and from "Full" in one save

#### Scenario: An existing setup after the update
- **WHEN** the human first opens the Review card after updating, with four saved verifiers, a round cap of 2 and a cycle cap of 3
- **THEN** it shows one level, "Full", marked "Full review" with all four verifiers ticked, the same four verifiers and the same two caps

### Requirement: Bounded rendering and background IO
The UI SHALL use virtualized transcript rows, consume canonical Tidal semantic colors, offer System/Light/Dark without losing drafts, and perform persistence and Git reads off the UI thread. Idle rendering SHALL use no animation or frame timer; the only recurring work is a one-minute quota check and an hourly update check.

#### Scenario: Large transcript
- **WHEN** a session has 100,000 historical messages
- **THEN** navigation loads a bounded indexed page instead of its full history

### Requirement: Performance evidence
The implementation SHALL ship a repeatable release host benchmark for persisted fixtures and a report distinguishing measured host overhead from unmeasured UI, provider and soak budgets.

#### Scenario: Performance report
- **WHEN** benchmark results are recorded
- **THEN** fixture, hardware, build mode, durability and p95 timings accompany the results

### Requirement: Model picker in creation dialogs
The new-agent dialog SHALL include a model picker. It SHALL be pre-filled with the role's first configured provider and that provider's first allowed model. It SHALL list the role's configured providers' allowed models first and still offer every other catalog model.

#### Scenario: Pre-fill for Reviewer
- **WHEN** the human opens the new-agent dialog for a Reviewer while Claude and Codex are enabled
- **THEN** the picker shows Claude's first allowed model, grouped above Codex's allowed models and the other models

### Requirement: Agents show how their model was chosen
An agent's chat SHALL show, above the composer's model pills, how its model was chosen: by which coordinator and its reason, by the human, or by role default. The Tickets panel SHALL show each assigned agent's model and reason under its ticket. The sidebar SHALL stay unchanged.

#### Scenario: Custom review listed with its focus
- **WHEN** a ticket has Claude, Codex and Style reviewers
- **THEN** its Tickets panel card lists all three, each with its model and reason

#### Scenario: Coordinator reason visible
- **WHEN** the human opens an implementer assigned by the project coordinator
- **THEN** the line above the composer reads "Chosen by ‹coordinator›: ‹reason›", and the ticket's card in the Tickets panel shows the same model and reason

### Requirement: Visible assignment rejection
A coordinator's rejected assignment SHALL appear as a failed step in that coordinator's work stream and as a `model_rejected` entry in Events, carrying the host's message. No agent SHALL appear.

#### Scenario: Wrong provider rejected
- **WHEN** the project coordinator assigns an implementer a Codex model while Implementer uses only Claude
- **THEN** its work stream shows the failed `assign_ticket` step saying Codex is not configured for Implementer, and neither the sidebar nor the Tickets panel shows a new implementer

### Requirement: Check-in thresholds in Settings
The Settings window SHALL have a Check-ins section under the workspace folder with two preset selectors that save at once. "Check in on an agent every" SHALL offer 10, 25, 50, 100 and 250, followed by "turns", with the caption "The coordinator hears about an agent that has worked this many turns without a message from it. Nothing pauses." "Check in with you every" SHALL offer 50, 100, 250, 500 and 1000, followed by "coordinator turns", with the caption "You get an inbox note when the coordinator has worked this many turns without hearing from you. Nothing pauses." The saved value SHALL be highlighted; a saved value outside the presets SHALL highlight none. A refused save SHALL show the host's error in the section and keep the saved values.

#### Scenario: Choose an agent threshold
- **WHEN** the human clicks 50 beside "Check in on an agent every"
- **THEN** the host saves `checkin_child_turns` 50 with `checkin_human_turns` unchanged, and 50 is highlighted

#### Scenario: Hand-edited value
- **WHEN** the shared folder's `settings/check-ins.json` holds `checkin_human_turns` 300
- **THEN** the section opens with no coordinator preset highlighted, and the host uses 300

### Requirement: Questions waiting beside an agent's status
An agent's status badge SHALL name the questions it still waits on after its status, as "Working · 1 question waiting" or "Blocked · 2 questions waiting", and its sidebar row SHALL show the short figure "1 question" or "2 questions". The badge text SHALL use the status's own color and the sidebar figure the subtle color, never yellow, because a worker's question goes to its coordinator and yellow is reserved for what needs the human. The question's text SHALL NOT be repeated beside the status. A session with no question waiting SHALL show neither.

#### Scenario: Working with a question
- **WHEN** an implementer reported `blocked` once and its turn is still running
- **THEN** its badge reads "Working · 1 question waiting" and its sidebar row shows "1 question"

#### Scenario: Turn ended unanswered
- **WHEN** that turn ends and the coordinator has not answered
- **THEN** its badge reads "Blocked · 1 question waiting"

#### Scenario: Nothing waiting
- **WHEN** a session has no unanswered `blocked` report
- **THEN** its badge and sidebar row show no question text

### Requirement: Compact action on the Context card
The Context card SHALL show, under its measurement, one row with a short hint and a Compact button, for every agent. For an agent with a provider session, Claude or Codex, the hint SHALL read "Replace the conversation with a summary." and the button "Compact…", or "Compact after this turn" while the agent is working. Activating it SHALL open a form in the card, not a dialog. For a Claude agent the form SHALL have one optional field labelled "What to keep", empty by default, and a line saying that Claude replaces the agent's conversation with a summary, that this can't be undone and that it uses one model call. For a Codex agent the form SHALL have no field, SHALL send no instructions, and its line SHALL say that Codex replaces the agent's conversation with a summary and chooses what the summary keeps, that this can't be undone and that it uses one model call. Either form SHALL have Cancel and a confirm button reading "Compact now", or "Compact after this turn" while the agent is working, when the line SHALL also say that it runs when the turn ends, before waiting messages are delivered. Nothing SHALL be sent to the host before the confirm button. For an agent with no provider session it SHALL be disabled beside "Available after this agent's first turn." A request the host refuses SHALL show the host's reason in the form and keep what was typed.

#### Scenario: Confirm on an idle agent
- **WHEN** the human presses "Compact…" on an idle Claude agent's card, types "the failing test names" and presses "Compact now"
- **THEN** the host receives one request with those instructions, and the card shows the compaction running

#### Scenario: Cancel the form
- **WHEN** the human opens the form and presses Cancel
- **THEN** the form closes and the host has received nothing

#### Scenario: Working agent
- **WHEN** the human opens the form on an agent whose turn is running
- **THEN** the confirm button reads "Compact after this turn" and the line says it runs when the turn ends, before waiting messages are delivered

#### Scenario: Codex agent
- **WHEN** the human opens the Context card of a Codex agent that has a provider session
- **THEN** the row reads "Replace the conversation with a summary." beside an enabled "Compact…" button

#### Scenario: Codex agent's form
- **WHEN** the human presses "Compact…" on an idle Codex agent's card
- **THEN** the form has no "What to keep" field, says Codex chooses what the summary keeps and that this can't be undone, and offers Cancel and "Compact now"

#### Scenario: Confirm on a Codex agent
- **WHEN** the human presses "Compact now" on that form
- **THEN** the host receives one request with no instructions, and the card shows the compaction running

#### Scenario: New agent
- **WHEN** the human opens the Context card of an agent that has not run a turn
- **THEN** the card says its measurement appears after the next turn, and the Compact button is disabled beside "Available after this agent's first turn."

### Requirement: The Context card follows a compaction
While an agent has a queued compaction, its Context card SHALL replace the Compact row with "Compacts when this turn ends", or "Compacts when this agent runs again" when the agent is not in a turn, the instructions after "Keeping:" when some were given, and a Cancel button that withdraws the request. While the compaction runs, the card SHALL show "Compacting…", an indeterminate bar and "Messages you send now are delivered when it finishes.", with no Cancel, and the card's refresh button and the conversation header's Pause button SHALL be disabled. After every outcome the card SHALL be measured again once, also when the agent's next turn has already started, and SHALL NOT be measured while a compaction runs. That measurement SHALL be the only one made during a turn; the refresh button SHALL stay disabled while the agent is working. An outcome of nothing to compact SHALL show a notice in the subtle color titled "Nothing to compact" with the provider's reason and "The context is unchanged." A failed outcome SHALL show a notice in the failure color titled "Compaction failed" with the reason and "The context is unchanged." A reason SHALL be shown as its first line, cut at 200 characters, the length at which the host cuts the reasons it builds, so that such a reason is shown whole. An interrupted outcome SHALL show a notice in the failure color titled "Compaction interrupted" saying that Wiffletree stopped while the agent's provider, named as Claude or Codex, was compacting and that the figure was measured again. Each notice SHALL have a Dismiss button, SHALL survive a restart until dismissed or until the next request, and SHALL leave the Compact row available. A compacted outcome SHALL show no notice. An interrupted compaction SHALL NOT change the agent's status badge and SHALL add no banner to its conversation.

#### Scenario: Queued behind a turn
- **WHEN** the human confirms "Compact after this turn" with a note
- **THEN** the card shows "Compacts when this turn ends", the note after "Keeping:", and Cancel

#### Scenario: Cancel a queued compaction
- **WHEN** the human presses Cancel on a queued compaction
- **THEN** the card shows the Compact row again, and no compaction runs when the turn ends

#### Scenario: Running
- **WHEN** a compaction is running
- **THEN** the card shows "Compacting…" with no Cancel, and refresh and Pause are disabled

#### Scenario: Measured again
- **WHEN** a compaction from 812k succeeds
- **THEN** the card measures once more and shows the new figure and percentage without the human pressing refresh

#### Scenario: Measured again with a message waiting
- **WHEN** a compaction queued behind a turn succeeds while a message waits, so the agent's next turn starts at once
- **THEN** the card and the chip show the new figure during that turn, and the badge reads "Working"

#### Scenario: Nothing to compact
- **WHEN** Claude reports "Not enough messages to compact."
- **THEN** the card shows "Nothing to compact" with that text and "The context is unchanged.", and the Compact button is still available

#### Scenario: Cut off by the host stopping
- **WHEN** Wiffletree quit or crashed while an agent was compacting and the human opens that agent
- **THEN** the card shows "Compaction interrupted" with Dismiss, and the Compact row is available
- **AND** its badge shows the status it had before, and its conversation has no banner

#### Scenario: Codex agent, queued
- **WHEN** the human confirms "Compact after this turn" on a working Codex agent
- **THEN** the card shows "Compacts when this turn ends" and Cancel, with no "Keeping:" line

#### Scenario: Codex agent, cut off by the host stopping
- **WHEN** Wiffletree quit while a Codex agent was compacting and the human opens that agent
- **THEN** the notice says Wiffletree stopped while Codex was compacting

#### Scenario: Codex agent, failed
- **WHEN** a Codex agent's compaction fails because Codex refused `thread/compact/start`
- **THEN** the card shows "Compaction failed" with a reason naming that request and "The context is unchanged.", and the Compact button is still available

#### Scenario: Failed
- **WHEN** a compaction fails
- **THEN** the card shows "Compaction failed" in the failure color with the reason, until the human dismisses it or compacts again

### Requirement: The header shows a queued or running compaction
The context chip in the conversation header SHALL add "compact queued" after its percentage while the agent has a queued compaction and "compacting" while one runs, and SHALL show neither otherwise. While a compaction runs, the agent's status badge SHALL read "Compacting" with the working glyph and color; before and after, it SHALL show the agent's own status. The sidebar SHALL stay unchanged. The Usage page SHALL gain no control, figure or label for compactions.

#### Scenario: Queued
- **WHEN** an agent at 81% has a compaction queued behind its running turn
- **THEN** its chip reads "81%" and "compact queued", and its badge reads "Working"

#### Scenario: Running
- **WHEN** that compaction is running
- **THEN** the chip reads "81%" and "compacting", and the badge reads "Compacting"

#### Scenario: Finished
- **WHEN** it has succeeded and the card was measured again at 4%
- **THEN** the chip reads "4%" with no note, and the badge shows the agent's status

#### Scenario: Usage page
- **WHEN** the human opens the Usage page after a Claude agent's compaction
- **THEN** its totals include the compaction's tokens and nothing on the page names compactions

#### Scenario: Usage page after a Codex compaction
- **WHEN** the human opens the Usage page after a Codex agent's compaction whose request was recorded
- **THEN** its totals include the compaction's tokens and nothing on the page names compactions

### Requirement: Projects are listed in the stored order
The sidebar SHALL list projects in the order the host returns them, and SHALL NOT sort them itself.

#### Scenario: The list follows a move
- **WHEN** the host's order changes from A, B, C to C, A, B
- **THEN** the sidebar lists C, A, B

### Requirement: Projects sharing a first word are shown as a group
A project's first word SHALL be the leading run of its name up to the first whitespace, with trailing characters that are neither letters nor digits removed, compared without regard to case. While grouping is on, two or more shown projects with the same first word SHALL appear under one group row, and every other project SHALL remain an ordinary row with its full name. Groups SHALL be computed over the projects currently shown, so archived projects count only while Archived is on. A group SHALL sit where its topmost member is in the stored order, its members SHALL keep their stored order, and its label SHALL be the first word as written in its topmost member. Groups SHALL exist only in the desktop's drawing of the list: the host stores none, and a person SHALL NOT be able to create, rename or delete one. While grouping is off the list SHALL be flat, with full names, in the same stored order.

#### Scenario: Case and a colon do not matter
- **WHEN** grouping is on and the shown projects are "Wiffletree: a" and "wiffletree b"
- **THEN** both appear under one group labelled "Wiffletree"

#### Scenario: A single project stays an ordinary row
- **WHEN** grouping is on and only "ci-runners: Autoscaling" starts with "ci-runners"
- **THEN** it is an ordinary row reading "ci-runners: Autoscaling"

#### Scenario: A group sits at its topmost member
- **WHEN** the stored order is "Trial copy", "infra: State split", "watch: Relay", "infra: Neptune"
- **THEN** the list reads "Trial copy", the group "infra" holding "State split" then "Neptune", then "watch: Relay"

#### Scenario: Archived projects join only while shown
- **WHEN** "infra: Neptune" is archived and "infra: State split" is not
- **THEN** with Archived off "infra: State split" is an ordinary row, and with Archived on both sit under the group "infra"

#### Scenario: Grouping off
- **WHEN** grouping is turned off
- **THEN** every project is an ordinary row with its full name, in the stored order

#### Scenario: Renaming moves a project between groups
- **WHEN** "infra: Neptune" is renamed "watch: Neptune" while "watch: Relay" is shown
- **THEN** it appears under the group "watch"

### Requirement: Rows inside a group drop the repeated word
A project row inside a group SHALL show its name without the first word and the separator that follows it, and SHALL show the full name when nothing else remains. Its tooltip, the conversation's title and the name being edited in an inline rename SHALL keep the full name.

#### Scenario: The word and its colon are dropped
- **WHEN** "wiffletree: Repository owners" sits in the group "wiffletree"
- **THEN** its row reads "Repository owners" and its tooltip begins "wiffletree: Repository owners"

#### Scenario: Nothing remains
- **WHEN** a project named "wiffletree" sits in the group "wiffletree"
- **THEN** its row reads "wiffletree"

### Requirement: A group row collapses and rolls up what is inside
A group row SHALL show a chevron, a folder icon, the group's label and how many projects it holds, and activating it SHALL collapse or expand the group as a project row's chevron does its project. A collapsed group SHALL show the most urgent state among its shown members: the number of items that need the human when there are any, otherwise blocked when a member's row would show blocked, otherwise working when a member's row would show working, and otherwise nothing.

#### Scenario: Needs-you outranks blocked and working
- **WHEN** a collapsed group holds one project with one item needing the human, one blocked project and one working project
- **THEN** the group row shows the needs-you count "1"

#### Scenario: Blocked outranks working
- **WHEN** a collapsed group holds a blocked project and a working project and nothing needs the human
- **THEN** the group row shows the blocked mark

#### Scenario: A quiet group
- **WHEN** every project in a collapsed group is ready, done or paused
- **THEN** the group row shows its count and no state

### Requirement: A Group button turns grouping on and off
The PROJECTS header SHALL carry a button labelled "Group" with a folder icon, to the left of Archived, drawn pressed while grouping is on, with the tooltip "Group projects by first word". Grouping SHALL be on by default. The choice SHALL be saved with the desktop's other view choices in `desktop-preferences.json`, a file without the choice SHALL read as on, and changing it SHALL NOT change the stored order.

#### Scenario: The choice survives a relaunch
- **WHEN** grouping is turned off and the app is relaunched
- **THEN** the list is flat and the button is not pressed

#### Scenario: An older preferences file
- **WHEN** `desktop-preferences.json` was saved before the choice existed
- **THEN** grouping is on and the file's other choices are kept

### Requirement: Projects are reordered by dragging
A person SHALL be able to drag project rows to reorder them, and each drop SHALL be sent to the host as one move, with the list redrawn from the order the host returns. While grouping is off, any project row SHALL be droppable at any place in the flat list. While grouping is on, a project inside a group SHALL be droppable only within that group, and a group row or an ungrouped project row SHALL be droppable only among the other groups and ungrouped projects; dragging a group SHALL move all its shown members as one block, and reordering inside a group SHALL NOT move the group. While a row is dragged, an insertion line SHALL show where it would land and the dragged row SHALL be dimmed. While a row is dragged and the pointer is near the top or bottom edge of the list, the list SHALL scroll in that direction, also while the pointer rests there, until it reaches its end; it SHALL stop scrolling when the pointer leaves that edge or the list, or the drag ends, and the insertion line SHALL keep to the row under the pointer as the list scrolls. Ticket rows and agent rows SHALL NOT be draggable, and a project row whose name is being edited SHALL NOT start a drag. A drag that left its row and is released back on it SHALL change nothing. Dragging SHALL NOT change what a click, a double-click, the chevron or the actions menu does, including when the pointer moves a little between press and release without leaving the row.

#### Scenario: A project moves within the flat list
- **WHEN** grouping is off, the list is A, B, C and C is dropped above A
- **THEN** the host's order becomes C, A, B and the list shows it

#### Scenario: A project moves within its group
- **WHEN** the group "wiffletree" holds "Reorder", "Owners", "Compact" and "Compact" is dropped above "Reorder"
- **THEN** the group holds "Compact", "Reorder", "Owners" and stays where it was among the other rows

#### Scenario: The top project of a group moves down
- **WHEN** the group "wiffletree" sits above "ci-runners: Autoscaling" and its topmost project is dropped at the bottom of the group
- **THEN** the group still sits above "ci-runners: Autoscaling"

#### Scenario: A whole group moves
- **WHEN** the group "watch", holding two projects, is dropped above the group "wiffletree"
- **THEN** both of its projects sit directly above the topmost "wiffletree" project in the host's order, in the order they had

#### Scenario: A grouped project cannot leave its group
- **WHEN** a project inside the group "wiffletree" is dragged over a row outside that group
- **THEN** no insertion line appears there and dropping it there moves nothing

#### Scenario: A row is dropped at a place that was off-screen
- **WHEN** the list is taller than the sidebar and a row is dragged to its bottom edge and held there
- **THEN** the list scrolls down to its end, and the row can be dropped beside a row that was not visible when the drag began

#### Scenario: A drag returned to its row is cancelled
- **WHEN** a group row is dragged past other rows, brought back and released on itself
- **THEN** nothing moves and the group stays expanded or collapsed as it was

#### Scenario: A click still selects
- **WHEN** a project row is clicked without moving the pointer
- **THEN** its conversation opens and nothing is moved

#### Scenario: A click that moves is still a click
- **WHEN** the pointer moves a few pixels between press and release without leaving the row
- **THEN** a project row is selected, a group row is collapsed or expanded, a project's chevron collapses or expands it, and the second click of a double-click opens rename, as each does without moving

### Requirement: The decision card can be minimized
The decision card above the project coordinator's composer SHALL have a Minimize button, tooltip "Minimize", at the right end of the row above the card, after Open inbox. Minimized, the card SHALL become one row with the card's yellow edge, a small yellow dot, the current decision's prompt on one line with an ellipsis, "N of M" only when there are several open decisions, and an Expand button, tooltip "Expand". Clicking anywhere on the row SHALL restore the full card on the same decision. Questions and approvals SHALL minimize alike. The minimized state SHALL be kept for each project, in memory only: it SHALL survive reading, typing, sending and switching to another conversation and back, and an app restart SHALL start expanded. A decision arriving SHALL NOT expand it; the row's count SHALL update. When a project has no open decision, its minimized state SHALL be cleared, so the next decision starts expanded. Answering, choices, Dismiss, Open inbox and the Inbox panel SHALL behave as before.

#### Scenario: Minimizing
- **WHEN** the human selects Minimize on a card showing "1 of 2"
- **THEN** one row shows the prompt on a single line with an ellipsis, "1 of 2" and Expand, and the conversation above it has the card's space

#### Scenario: Restoring
- **WHEN** the human clicks the minimized row or its Expand button
- **THEN** the full card returns showing the same decision

#### Scenario: Staying minimized
- **WHEN** the human types, sends a message, or selects another conversation and returns
- **THEN** the card is still minimized

#### Scenario: A new decision arrives
- **WHEN** the card is minimized and the coordinator asks another question
- **THEN** the card stays minimized and the row shows the new count

#### Scenario: The last decision is settled
- **WHEN** the human settles the last open decision while the card is minimized
- **THEN** the row disappears
- **AND** the next decision that arrives shows the full card

#### Scenario: Restart
- **WHEN** the app restarts while a decision is open
- **THEN** the card is expanded
