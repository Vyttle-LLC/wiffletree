# work-progress Specification

## Purpose
Shows how far along each agent, each ticket and the whole project is, as progress bars the host derives from what it records. An agent's only part is to keep a checklist of its own steps.

## Requirements

### Requirement: Progress is derived from host records
The host SHALL derive all progress from records it keeps: an agent's plan steps, its session status, the ticket's state and the ticket's verification record. An agent SHALL never report a percentage, a stage or a ticket's or project's progress. The only thing an agent supplies is its own checklist, as a plan step: through the workspace tool `set_plan`, or through its provider's own plan tool where one exists. Agents SHALL get no new report kind or field. Progress SHALL NOT include cost or token estimates. A percentage SHALL be shown as a whole number, rounded to the nearest percent, and SHALL be shown only where these rules define one.

#### Scenario: One new tool
- **WHEN** an agent lists its workspace tools after this change
- **THEN** the only addition is `set_plan`, and `report` is unchanged

#### Scenario: An agent that never writes a plan
- **WHEN** an implementer works through a whole turn without calling `set_plan` or a provider plan tool
- **THEN** its bar is indeterminate for that turn and no percentage is shown for it

### Requirement: Checklist tool
The workspace tool server SHALL offer `set_plan` to every role. Its one argument, `items`, is the agent's whole checklist in order, each item a `text` and a `done` flag; each call replaces the checklist. The host SHALL record a call as a plan step in the caller's current run, with the same counts as a provider plan step: the first call of a run adds the step and later calls update it. The step SHALL appear in the work stream like any other plan step. When a run has plan steps from both `set_plan` and the provider's own plan tool, the most recently recorded one SHALL be the agent's checklist. The host SHALL refuse a call with more than 50 items, an item whose text is empty or longer than 200 characters, a missing `done` flag, or any other argument, with an error naming the limit, and SHALL record nothing for a refused call. A call SHALL NOT wake, notify or message any agent, SHALL NOT change a session's status or a ticket's state, and SHALL NOT write a conversation message. The communication instructions the host gives every agent SHALL ask it, in a few sentences, to record a checklist at the start of work and keep it current, and SHALL say that the checklist is display only and never replaces `report`.

#### Scenario: A call records a plan step
- **WHEN** a working agent calls `set_plan` with six items, three of them done
- **THEN** its current run has one plan step titled "Plan · 3 of 6 done" carrying 3 done, 6 total and the fourth item as the current item, and its progress is 3 of 6

#### Scenario: A later call updates the same step
- **WHEN** that agent calls `set_plan` again with four of the six items done
- **THEN** the run still has one plan step from the tool, now reading 4 of 6

#### Scenario: Over the bounds
- **WHEN** an agent calls `set_plan` with 51 items, or with an item of 201 characters, or with an empty item
- **THEN** the call fails with an error naming the limit, and its checklist is unchanged

#### Scenario: Latest plan wins
- **WHEN** an agent's provider plan tool records 1 of 2 and the agent then calls `set_plan` with 2 of 5
- **THEN** its progress is 2 of 5
- **AND** if the provider plan tool then records 2 of 2, its progress is 2 of 2

#### Scenario: A call wakes no one
- **WHEN** a worker calls `set_plan` twenty times during its turn
- **THEN** its coordinator is not woken, no message is written, and no session's status or ticket's state changes

### Requirement: Agent progress
An agent's progress SHALL be a checklist count, items done out of total, taken from the latest plan step of its current run, whether `set_plan` or the provider's own plan tool recorded it. Its current step SHALL be the first unfinished item of that plan. A plan with no items SHALL count as no plan. The agent's state SHALL be the first of these that applies:

1. **Finished, 100%**: the agent is a verifier whose latest result in the ticket's latest cycle is `passed`, or its session status is `Done`.
2. **Blocked**: its session status is `Blocked`. It keeps the checklist position of its latest run, or has no position when that run had no plan.
3. **In progress**: it is working and its current run has a plan. Its fraction is done over total.
4. **Indeterminate**: it is working and its current run has no plan yet. It shows the word "working" and no percentage.
5. **Idle**: otherwise (ready, paused or disconnected). It shows the checklist of its latest run unchanged, without the word "working", or nothing when that run had no plan.

When a new run starts, the previous run's checklist SHALL be dropped, so the agent is indeterminate until the new run's first plan step.

An agent's figure SHALL be its checklist count, such as "3 of 6", except in these cases, where it is a word: "passed" for a finished verifier whose latest result is `passed`; "done" for any other finished agent; "failed" for a blocked verifier whose latest result in the ticket's latest cycle is `failed`; "blocked" for any other blocked agent; and "working" for an indeterminate agent. An idle agent with no checklist SHALL have no figure.

#### Scenario: Checklist count
- **WHEN** a working agent's latest plan step lists six items, three of them done, and the fourth is "Update the design README"
- **THEN** its progress is 3 of 6 (50%), in progress, and its current step is "Update the design README"

#### Scenario: Working with no plan
- **WHEN** an agent is working and its current run has no plan step
- **THEN** its bar is striped, its figure is "working", and no percentage is shown

#### Scenario: The plan is rewritten
- **WHEN** an agent at 2 of 3 replaces its plan with five items, two of them done
- **THEN** its progress is 2 of 5 (40%), even though that is lower than before

#### Scenario: A new run starts
- **WHEN** an agent that finished its last run at 4 of 6 starts a new run and has not written a plan yet
- **THEN** it is indeterminate, not 4 of 6

#### Scenario: Between turns
- **WHEN** an agent's turn ended at 4 of 6 without a report and no new turn has started
- **THEN** it still shows 4 of 6, without the word "working"

#### Scenario: Verifier passed
- **WHEN** a verifier whose last plan read 3 of 5 reports `passed` and the host records its result as passed
- **THEN** it shows 100%, finished, with the figure "passed"

#### Scenario: Implementer reported ready
- **WHEN** an implementer reports `ready_for_testing`
- **THEN** it shows 100%, finished, with the figure "done"

#### Scenario: Blocked agent
- **WHEN** an agent at 3 of 7 reports `blocked`
- **THEN** its bar stays at 3 of 7 and is shown as blocked, with the figure "blocked"

#### Scenario: Verifier failed
- **WHEN** a verifier whose last plan read 4 of 5 reports `failed` with an evidenced blocking finding, and the host records its result as failed
- **THEN** its bar stays at 4 of 5 and is shown as blocked, with the figure "failed"

#### Scenario: Failed verifier starts the next round
- **WHEN** that verifier's turn for round 2 starts and it has not written a plan yet
- **THEN** it is indeterminate with the figure "working"

### Requirement: Ticket progress
A ticket's progress SHALL be a stage and a position in percent. Only the states `accepted`, `assigned`, `ready_for_testing`, `blocked` and `closed` are read by name; a ticket's state can also hold any other report kind, such as `passed`, `failed` or `completed`, and for those the verification record decides. The progress SHALL be taken from the first of these that applies:

1. State `accepted`: stage **Accepted**, 100%, finished.
2. State `assigned`: stage **Implementing**, 0–60%: 60% times the implementer's checklist fraction. The implementer is the ticket's unarchived implementer; a finished implementer counts as a full checklist, and one without a plan counts as zero.
3. State `ready_for_testing`: stage **Implementing**, 60%.
4. The ticket has a verification record and its latest cycle's outcome is `passed`: stage **Awaiting acceptance**, 90%.
5. The ticket has a verification record whose latest cycle is running or blocked: stage **Verifying**, 60–90%: 60% plus 30% times the share of the latest cycle's verifiers whose latest result in that cycle is `passed`.
6. Otherwise: stage **Implementing**, as in rule 2.

A ticket in state `blocked` SHALL keep the position rules 4, 5 or 6 give it and SHALL be shown as blocked. A ticket under rule 2 or 6 whose implementer is working without a plan SHALL be indeterminate: it shows its stage and no percentage, and counts as 0%. Blocked wins over indeterminate: a `blocked` ticket SHALL be shown as blocked, never striped, at the position its rule gives, until its state changes. Under rule 6 that position follows the implementer's current checklist, so it is 0% while a new run has written no plan. A ticket under rule 3 SHALL stay at 60% whatever its implementer is doing. Rule 2 applies to every `assigned` ticket, including one re-assigned for a fix after a cycle: it follows the checklist of the implementer's fix run. A ticket's position SHALL NOT exceed 90% before it is accepted. A position is derived from the current records only, so it MAY move backwards.

#### Scenario: Implementing
- **WHEN** a ticket is `assigned` and its implementer's checklist is 3 of 6
- **THEN** the ticket is Implementing at 30%

#### Scenario: Implementer working without a plan
- **WHEN** a ticket is `assigned` and its implementer is working with no plan
- **THEN** the ticket shows "Implementing" with a striped bar and no percentage

#### Scenario: Ready for verification
- **WHEN** the implementer reports `ready_for_testing` and no cycle has started
- **THEN** the ticket is Implementing at 60%

#### Scenario: Ready, then the implementer starts another run
- **WHEN** a ticket is `ready_for_testing` and its implementer starts a new run with no plan
- **THEN** the ticket stays Implementing at 60%, not indeterminate, while the implementer's own bar is striped

#### Scenario: Verifying
- **WHEN** a ticket is `verifying` in round 1 with three configured verifiers and one has passed
- **THEN** the ticket is Verifying at 70%

#### Scenario: A failed round keeps the passes
- **WHEN** round 1 ends with two of three verifiers passed and one failed, and the implementer is fixing the findings
- **THEN** the ticket stays Verifying at 80% through the fix and through round 2 until the third verifier reports

#### Scenario: Awaiting acceptance
- **WHEN** every verifier of the latest cycle has passed and the ticket's state is `passed`
- **THEN** the ticket is Awaiting acceptance at 90%

#### Scenario: Another report kind after a pass
- **WHEN** the latest cycle passed and the implementer then reports `completed`, so the ticket's state is `completed`
- **THEN** the ticket is still Awaiting acceptance at 90%, not Verifying

#### Scenario: Re-assigned for a fix after a pass
- **WHEN** a ticket Awaiting acceptance at 90% is assigned again for a fix, so its state is `assigned`, and the implementer's fix run has no plan yet
- **THEN** the ticket shows "Implementing" with a striped bar and no percentage
- **AND** once the fix run's plan reads 1 of 2 the ticket is Implementing at 30%

#### Scenario: Finished without verification
- **WHEN** a ticket has no verification record and its implementer reports `completed`, so the ticket's state is `completed`
- **THEN** the ticket is Implementing at 60%

#### Scenario: Accepted
- **WHEN** the coordinator accepts the ticket
- **THEN** the ticket is Accepted at 100%, finished

#### Scenario: Blocked while implementing
- **WHEN** a ticket's implementer reports `blocked` at 3 of 7 before any verification
- **THEN** the ticket is shown as blocked at 26%

#### Scenario: Blocked, then the implementer starts another run
- **WHEN** a ticket is in state `blocked`, the coordinator answers its implementer with a message, and the implementer's new run has no plan yet
- **THEN** the ticket is still shown as blocked, in red and without stripes, while the implementer's own bar is striped
- **AND** a ticket blocked at the round cap with two of three verifiers passed stays blocked at 80% whatever its implementer is doing

#### Scenario: Blocked at the round cap
- **WHEN** a cycle of three verifiers ends blocked at the round cap with two passed
- **THEN** the ticket is shown as blocked at 80%

#### Scenario: Second verification cycle
- **WHEN** cycle 1 ended blocked with two of three verifiers passed, the implementer fixes and reports `ready_for_testing`, and the coordinator starts cycle 2
- **THEN** the ticket moves from blocked at 80% to Implementing at 60%, and cycle 2 starts at Verifying 60% with all three verifiers pending
- **AND** it reaches 90% only when every verifier of cycle 2 has passed, and never exceeds 90% before acceptance

#### Scenario: Fixes after a pass
- **WHEN** a ticket Awaiting acceptance at 90% is sent back for a fix and its implementer reports `ready_for_testing`
- **THEN** the ticket is Implementing at 60%

### Requirement: Project progress
A project's progress SHALL be the equal-weight average of its tickets' positions, leaving out tickets in state `closed`. It SHALL be drawn as one bar with a segment of equal width per counted ticket, in the order the tickets were created, each segment filled to its ticket's position in its ticket's state. It SHALL show a phase label and the figure "N of M tickets accepted", where M is the number of counted tickets. The phase label SHALL be "Done" when every counted ticket is accepted, "Blocked" when every unaccepted ticket is blocked, and otherwise the least advanced stage among the tickets that are neither accepted nor blocked: Implementing, then Verifying, then Awaiting acceptance. When the project has no counted ticket, the label SHALL be "Planning" and the bar SHALL be the project coordinator's own agent progress, with a percentage only when the coordinator has a checklist.

#### Scenario: Average of four tickets
- **WHEN** a project's tickets are Accepted at 100%, Verifying at 70%, Implementing at 30% and blocked at 24%
- **THEN** the project shows "Implementing", 56%, "1 of 4 tickets accepted", and a bar of four segments, the first finished and the last blocked

#### Scenario: No tickets yet
- **WHEN** a project has no ticket and its coordinator is working with a plan at 1 of 4
- **THEN** the project shows "Planning" at 25%

#### Scenario: No tickets and no plan
- **WHEN** a project has no ticket and its coordinator is working with no plan
- **THEN** the project shows "Planning" with a striped bar and no percentage

#### Scenario: No tickets and an idle coordinator
- **WHEN** a project has no ticket and its coordinator is idle with no checklist
- **THEN** the project shows "Planning" with an empty track and no percentage

#### Scenario: Closed ticket
- **WHEN** one of a project's three tickets is closed, and the other two are at 100% and 50%
- **THEN** the project shows 75% and "1 of 2 tickets accepted", with two segments

#### Scenario: Everything left is blocked
- **WHEN** a project has one accepted ticket and two blocked tickets
- **THEN** its phase label is "Blocked"

#### Scenario: All accepted
- **WHEN** every counted ticket is accepted
- **THEN** the project shows "Done" at 100%, finished

### Requirement: Progress strip
The selected conversation SHALL show one progress strip pinned directly under its session title. The strip SHALL NOT scroll with the transcript and SHALL keep a fixed height, so progress changes never move the transcript. For a project coordinator, the strip SHALL show the project's phase label, its segmented bar, its percentage and "N of M tickets accepted". For an agent on a ticket, it SHALL show the agent's own progress, as "Step K of N" with the current step, the bar and the percentage, followed by the ticket's figure, such as "ticket 30%". K SHALL be the number of items done plus one. When every item of a working agent's plan is done, the strip SHALL read "N of N" with no step label, never "Step N+1". "Step K of N" is the label of a working agent only. A finished agent's label SHALL be "Done", or "Passed" for a verifier whose latest result is `passed`. A blocked agent's label SHALL be "Failed" for a verifier whose latest result is `failed`, and otherwise "Blocked · step K of N", or "Blocked" when its latest run had no plan. An idle agent with a checklist from its latest run SHALL read "K of N", where K is the number of items done, with no "Step" and no current item. An agent with nothing of its own to show SHALL show only its ticket's stage and figure. A session with neither a ticket nor a project of its own SHALL show its own progress alone.

#### Scenario: Coordinator selected
- **WHEN** the human selects the coordinator of a project at 56% with one of four tickets accepted
- **THEN** the strip reads "Implementing", shows the four-segment bar and "56% · 1 of 4 tickets accepted"

#### Scenario: Worker selected
- **WHEN** the human selects an implementer at 3 of 6 whose ticket is at 30%
- **THEN** the strip reads "Step 4 of 6 · Update the design README", shows a bar at 50%, and "50% · ticket 30%"

#### Scenario: Every item done
- **WHEN** the human selects a working implementer whose plan reads 6 of 6, on a ticket at 60%
- **THEN** the strip reads "6 of 6" with no step label, shows a full bar, and "100% · ticket 60%"

#### Scenario: Finished agent
- **WHEN** the human selects an implementer that reported `ready_for_testing` with its plan at 5 of 6, on a ticket at 60%
- **THEN** the strip reads "Done", not "Step 6 of 6", shows a full green bar, and "100% · ticket 60%"

#### Scenario: Passed and failed verifiers
- **WHEN** the human selects a verifier whose latest result is `passed`, and then one whose latest result is `failed` at 4 of 5
- **THEN** the first strip reads "Passed" with a full green bar, and the second reads "Failed" with a red bar at 80%

#### Scenario: Blocked agent
- **WHEN** the human selects an implementer that reported `blocked` at 3 of 7, and then one that reported `blocked` without ever writing a plan
- **THEN** the first strip reads "Blocked · step 4 of 7" with a red bar at 43%, and the second reads "Blocked" with an empty track

#### Scenario: Idle agent with a past checklist
- **WHEN** the human selects an agent whose turn ended at 4 of 6 without a report
- **THEN** the strip reads "4 of 6", with no "Step" and no current item, and shows a bar at 67%

#### Scenario: Worker with no plan
- **WHEN** the human selects a working verifier with no plan, on a ticket at 70%
- **THEN** the strip reads "Working", shows a striped bar, and "ticket 70%"

#### Scenario: Steady layout
- **WHEN** an agent's plan changes while the human reads earlier messages
- **THEN** the strip updates in place and the transcript's scroll position does not change

### Requirement: Progress in the sidebar tree
The project row in the sidebar tree SHALL show a thin bar beneath its label and the project's percentage beside it, and an archived project's row SHALL show neither bar nor percentage. Ticket rows and agent rows SHALL show neither a bar nor a progress figure, and SHALL be single-line rows whose height does not depend on progress; a ticket row with a pull request keeps its second line for the pull request. A project with nothing to show SHALL keep an empty track and no figure, so the project row's height never depends on its progress. Notes that are not progress, such as an agent's "held" or "queued input" and its questions figure, SHALL stay on the agent row.

#### Scenario: Rows under one project
- **WHEN** a project at 56% has a ticket Verifying at 70% with a finished implementer, a tester at 3 of 5 and a reviewer working without a plan
- **THEN** the project row shows "56%" and its bar, and the ticket row and the three agent rows show no figure and no bar

#### Scenario: Blocked ticket row
- **WHEN** a ticket is blocked at 26%
- **THEN** its sidebar row shows no "blocked" figure and no bar, and the strip and Agents panel still show its blocked state

#### Scenario: Project with nothing to show
- **WHEN** a project has no tickets and no progress to report
- **THEN** its row keeps an empty track and no figure, at the same height as other projects' rows

#### Scenario: Idle agent with no plan
- **WHEN** an agent is ready and its latest run had no plan
- **THEN** its row shows no figure and no track, at the same height as every other agent's and ticket's row

#### Scenario: Working agent and open ticket stay one line
- **WHEN** an agent is working at 3 of 5, or a ticket without a pull request is at 70%
- **THEN** its row has the same single-line height as an idle agent's, with no figure and no track line

### Requirement: Progress states
Every bar SHALL be in exactly one of four states: in progress, drawn in the focus color; indeterminate, drawn as a striped track with no percentage; blocked, drawn in red at the position it had; or finished, drawn in green at 100%. An idle bar SHALL be drawn as in progress. When a bar would be both blocked and indeterminate, it SHALL be blocked. No bar SHALL show a percentage that these rules do not define.

#### Scenario: Blocked keeps its position
- **WHEN** a ticket at 80% becomes blocked
- **THEN** its bar stays filled to 80% and turns red

#### Scenario: Finished
- **WHEN** a ticket is accepted
- **THEN** its bar is green at 100%

#### Scenario: Indeterminate
- **WHEN** an agent is working with no plan
- **THEN** its bar is striped across its whole width and shows no percentage

### Requirement: Progress delivery without waking agents
The host SHALL keep progress current whenever a plan step, a session's status, a ticket or a verification record changes, and SHALL rebuild the same figures from stored steps and records after a restart, for every agent the board lists, including the archived verifiers of an open ticket's latest cycle. Stored steps are kept for a limited time: an agent whose latest run's steps were pruned SHALL show no checklist. Progress SHALL reach the desktop app incrementally: the host signals only when a figure actually changed, and the client then requests only the progress entries changed since its last cursor, never a full workspace snapshot. When an entry has left the board since that cursor, such as a closed ticket or an archived agent, the host SHALL answer once with the whole board, marked complete, and the client SHALL replace what it holds. A step that is not a plan step SHALL cause no progress signal. Progress changes SHALL NOT wake, notify or message any agent, and SHALL NOT delay message receipts, permission requests or failures.

#### Scenario: A plan step arrives
- **WHEN** a worker's plan goes from 2 of 5 to 3 of 5
- **THEN** the desktop receives that agent's entry, its ticket's and its project's, and no snapshot reload occurs

#### Scenario: An entry leaves the board
- **WHEN** a ticket is closed after the desktop last read the board
- **THEN** the next page is the whole board, marked complete, without that ticket, and the desktop no longer shows a figure for it

#### Scenario: Ordinary steps
- **WHEN** a worker streams narration and runs commands without changing its plan
- **THEN** no progress signal is sent

#### Scenario: Coordinator stays asleep
- **WHEN** a worker's progress changes during its turn, by a burst of `set_plan` calls or of provider plan steps
- **THEN** its coordinator is not woken and receives no message because of it, and no session's status changes

#### Scenario: Restart
- **WHEN** the host restarts while an implementer's stored plan reads 3 of 6 and its ticket is `assigned`
- **THEN** after the restart the implementer shows 3 of 6 and the ticket 30%

#### Scenario: Restart with an archived verifier
- **WHEN** a cycle ended blocked with a verifier failed at 4 of 5, the host archived it, and the host restarts
- **THEN** the Agents panel still lists that verifier as "failed" at 4 of 5

#### Scenario: Pruned run
- **WHEN** an idle agent's latest run finished so long ago that the host pruned its steps
- **THEN** the agent shows an empty track and no figure

### Requirement: Agents panel
The header of every conversation of a project, its coordinator's and each of its agents', SHALL carry an Agents toggle labelled "Agents · N working", where N is the number of agents the panel lists that are in a turn, or "Agents" when none is. Activating it SHALL open the Agents panel beside the conversation, without replacing it, and activating it again SHALL close it. The panel SHALL list each counted ticket of the project, in the order the tickets were created, with its title, stage, percentage and bar. Under each ticket that is not accepted, it SHALL list that ticket's unarchived agents and the verifiers of its latest cycle, each once, including a verifier the host archived after it passed. Each agent row SHALL show its role and focus, its figure, its current step while it works, its model, its elapsed time and its bar. The elapsed time SHALL be the time since the agent's current run started while it is working, and otherwise the length of its latest run. The elapsed time SHALL refresh only while the panel is open and an agent listed under a ticket is working. Below the tickets, a "Previous agents (N)" section, folded until the human unfolds it, SHALL list every other agent the sidebar tree lists under the project's coordinator, archived or not: verifiers of earlier cycles and other archived agents, agents of accepted or closed tickets, workers created before tickets existed, and retired repository coordinators. Each previous agent's row SHALL show its role and focus and the ticket it worked on, or, without a ticket, its name, with "(retired)" after a retired repository coordinator's. Selecting any agent row SHALL select that agent's conversation, read-only when the agent is archived, and SHALL leave the panel open, still listing the whole project, with the selected agent's row marked, so the human can step through the agents. The strip SHALL follow the selected conversation. The panel SHALL stay open this way until the human closes it or opens another panel.

#### Scenario: Open the panel
- **WHEN** the human activates "Agents · 2 working" in the coordinator's header
- **THEN** the panel opens beside the conversation and lists every counted ticket with its stage and percentage, and each open ticket's agents with current step, model and elapsed time

#### Scenario: Toggle from an agent
- **WHEN** an agent's conversation is selected and the human activates its header's Agents toggle twice
- **THEN** the panel opens listing the agent's whole project, then closes

#### Scenario: Nothing working
- **WHEN** no agent the panel lists is in a turn
- **THEN** the toggle reads "Agents"

#### Scenario: Archived passed verifier
- **WHEN** round 1 ended with the tester passed and archived, and round 2 is running with one reviewer
- **THEN** the ticket's card still lists the tester as "passed" at 100%, beside the working reviewer

#### Scenario: Earlier cycle's verifiers
- **WHEN** cycle 2 is running on a ticket whose cycle 1 verifiers were archived
- **THEN** the card lists cycle 2's verifiers, and cycle 1's are under "Previous agents"

#### Scenario: Accepted ticket
- **WHEN** a ticket is accepted
- **THEN** its card shows "Accepted · 100%" and no agent rows, and its agents are under "Previous agents"

#### Scenario: Agents from before tickets
- **WHEN** a project has a worker created before tickets existed and a retired repository coordinator
- **THEN** "Previous agents (2)" lists the worker by name and the coordinator as "<name> (retired)", and selecting either opens its conversation

#### Scenario: Step through the agents
- **WHEN** the panel is open and the human selects the implementer's row, then a reviewer's row
- **THEN** each selection shows that agent's conversation and strip, the panel stays open listing the whole project, and the selected agent's row is marked

#### Scenario: Leave the panel
- **WHEN** an agent selected from the panel is shown and the human closes the panel
- **THEN** the panel closes, and that agent's header still offers the Agents toggle

#### Scenario: Idle panel
- **WHEN** the panel is open and no agent listed under a ticket is working
- **THEN** no elapsed-time timer runs

### Requirement: Stuck agents
An agent SHALL be stuck when it is not archived and either its status is disconnected, or its status is ready or done and its latest turn ended in an error. A blocked agent SHALL NOT be stuck, because its coordinator answers it, and neither SHALL a paused or working agent. One rule SHALL decide this for every view that shows it. In the Agents panel, a stuck agent's row SHALL show a warning icon and its reason, "Disconnected" or "Last turn failed", in the warning color, and the card of the ticket it works on SHALL have a border in the warning color.

#### Scenario: Disconnected agent
- **WHEN** a ticket's implementer is disconnected
- **THEN** its row reads "Disconnected" in the warning color and its ticket's card has a warning border

#### Scenario: Failed last turn
- **WHEN** a reviewer's latest turn ended in an error and it is ready
- **THEN** its row reads "Last turn failed" in the warning color

#### Scenario: Blocked, paused or working
- **WHEN** an agent is blocked, paused or working, even after a failed turn
- **THEN** it is not stuck and its row and card keep their ordinary colors

### Requirement: Agents card in the coordinator's conversation
While any agent the Agents panel lists is working or stuck, the conversation of the project's coordinator SHALL show one card pinned above its composer reading "N agents working on M tickets", where M counts the tickets with an agent in a turn, followed by "· K stuck" when K agents are stuck, and an "Open Agents" action that opens the panel. With no agent working, it SHALL read "No agents working" before the stuck count. The card SHALL be in the warning color while any agent is stuck. It SHALL NOT appear in an agent's conversation or an archived coordinator's, and SHALL NOT be inserted between messages.

#### Scenario: Work under way
- **WHEN** three agents are working on two tickets and none is stuck
- **THEN** the coordinator's conversation shows "3 agents working on 2 tickets" and "Open Agents" above the composer

#### Scenario: A stuck agent
- **WHEN** one agent is working on one ticket and another agent is disconnected
- **THEN** the card reads "1 agent working on 1 ticket · 1 stuck" in the warning color

#### Scenario: Quiet project
- **WHEN** no agent is working or stuck
- **THEN** no card is shown

### Requirement: Breadcrumb back to the coordinator
The header of any conversation of a project other than its coordinator's SHALL show, above its title, a link "← <project name>" followed by the agent's ticket, such as "← Calls / sagechat-sms · Fix inbound calls". Activating the link SHALL select the project's coordinator. Such an agent's title SHALL be its role and focus when it has a ticket, and otherwise its name. The coordinator's header SHALL show the project's name and its role above its title.

#### Scenario: Back from a reviewer
- **WHEN** the human has a ticket's correctness reviewer selected and activates "← Calls" in its header
- **THEN** the project's coordinator is selected
