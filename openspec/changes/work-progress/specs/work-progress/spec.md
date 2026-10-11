## Purpose

Shows how far along each agent, each ticket and the whole project is, as progress bars the host derives from what it already records. Agents report nothing extra.

## ADDED Requirements

### Requirement: Progress is derived, never reported
The host SHALL derive all progress from records it already keeps: an agent's plan steps, its session status, the ticket's state and the ticket's verification record. Agents SHALL get no new tool, no new report kind or field and no new instruction for progress. Progress SHALL NOT include cost or token estimates. A percentage SHALL be shown as a whole number, rounded to the nearest percent, and SHALL be shown only where these rules define one.

#### Scenario: Agents have nothing new to call
- **WHEN** an agent lists its workspace tools after this change
- **THEN** the list is the same as before, and its role instructions do not mention progress

#### Scenario: An agent that never writes a plan
- **WHEN** an implementer works through a whole turn without writing a plan
- **THEN** its bar is indeterminate for that turn and no percentage is shown for it

### Requirement: Agent progress
An agent's progress SHALL be a checklist count, items done out of total, taken from the latest plan step of its current run. Its current step SHALL be the first unfinished item of that plan. A plan with no items SHALL count as no plan. The agent's state SHALL be the first of these that applies:

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

A ticket in state `blocked` SHALL keep the position rules 4, 5 or 6 give it and SHALL be shown as blocked. A ticket under rule 2 or 6 whose implementer is working without a plan SHALL be indeterminate: it shows its stage and no percentage, and counts as 0%. A ticket under rule 3 SHALL stay at 60% whatever its implementer is doing. Rule 2 applies to every `assigned` ticket, including one re-assigned for a fix after a cycle: it follows the checklist of the implementer's fix run. A ticket's position SHALL NOT exceed 90% before it is accepted. A position is derived from the current records only, so it MAY move backwards.

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
A project's progress SHALL be the equal-weight average of its tickets' positions, leaving out tickets in state `closed`. It SHALL be drawn as one bar with a segment of equal width per counted ticket, in the order the tickets were created, each segment filled to its ticket's position in its ticket's state. It SHALL show a phase label and the figure "N of M tickets accepted", where M is the number of counted tickets. The phase label SHALL be "Done" when every counted ticket is accepted, "Blocked" when every unaccepted ticket is blocked, and otherwise the least advanced stage among the tickets that are neither accepted nor blocked: Implementing, then Verifying, then Awaiting acceptance. When the project has no counted ticket, the label SHALL be "Planning" and the bar SHALL be the project coordinator's own agent progress.

#### Scenario: Average of four tickets
- **WHEN** a project's tickets are Accepted at 100%, Verifying at 70%, Implementing at 30% and blocked at 24%
- **THEN** the project shows "Implementing", 56%, "1 of 4 tickets accepted", and a bar of four segments, the first finished and the last blocked

#### Scenario: No tickets yet
- **WHEN** a project has no ticket and its coordinator is working with a plan at 1 of 4
- **THEN** the project shows "Planning" at 25%

#### Scenario: No tickets and no plan
- **WHEN** a project has no ticket and its coordinator is working with no plan
- **THEN** the project shows "Planning" with a striped bar and no percentage

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
The selected conversation SHALL show one progress strip pinned directly under its session title. The strip SHALL NOT scroll with the transcript and SHALL keep a fixed height, so progress changes never move the transcript. For a project coordinator, the strip SHALL show the project's phase label, its segmented bar, its percentage and "N of M tickets accepted", and a crew button. For an agent on a ticket, it SHALL show the agent's own progress, as "Step K of N" with the current step, the bar and the percentage, followed by the ticket's figure, such as "ticket 30%". K SHALL be the number of items done plus one. When every item of a working agent's plan is done, the strip SHALL read "N of N" with no step label, never "Step N+1". An agent with nothing of its own to show SHALL show only its ticket's stage and figure. A session with neither a ticket nor a project of its own SHALL show its own progress alone.

#### Scenario: Coordinator selected
- **WHEN** the human selects the coordinator of a project at 56% with one of four tickets accepted and six agents in its crew
- **THEN** the strip reads "Implementing", shows the four-segment bar, "56% · 1 of 4 tickets accepted" and a "6 agents" button

#### Scenario: Worker selected
- **WHEN** the human selects an implementer at 3 of 6 whose ticket is at 30%
- **THEN** the strip reads "Step 4 of 6 · Update the design README", shows a bar at 50%, and "50% · ticket 30%"

#### Scenario: Every item done
- **WHEN** the human selects a working implementer whose plan reads 6 of 6, on a ticket at 60%
- **THEN** the strip reads "6 of 6" with no step label, shows a full bar, and "100% · ticket 60%"

#### Scenario: Worker with no plan
- **WHEN** the human selects a working verifier with no plan, on a ticket at 70%
- **THEN** the strip reads "Working", shows a striped bar, and "ticket 70%"

#### Scenario: Steady layout
- **WHEN** an agent's plan changes while the human reads earlier messages
- **THEN** the strip updates in place and the transcript's scroll position does not change

### Requirement: Progress in the sidebar tree
Each project row, ticket row and unarchived agent row in the sidebar tree SHALL show a thin bar beneath its label and a short figure beside it: the percentage for a project; the percentage, or "blocked", for a ticket; and for an agent its checklist count such as "3 of 5", or "working", "done", "passed", "failed" or "blocked". A row with nothing to show SHALL keep an empty track and no figure, so a row's height never depends on its progress.

#### Scenario: Rows under one project
- **WHEN** a project at 56% has a ticket Verifying at 70% with a finished implementer, a tester at 3 of 5 and a reviewer working without a plan
- **THEN** the project row shows "56%", the ticket row "70%", and the agent rows "done", "3 of 5" and "working", each with its bar

#### Scenario: Blocked ticket row
- **WHEN** a ticket is blocked at 26%
- **THEN** its row shows "blocked" and a red bar filled to 26%

#### Scenario: Idle agent with no plan
- **WHEN** an agent is ready and its latest run had no plan
- **THEN** its row shows an empty track and no figure, at the same height as its neighbours

### Requirement: Crew panel
The coordinator's strip SHALL carry a crew button labelled with the number of agents the panel lists, such as "6 agents". Activating it SHALL open a crew panel beside the conversation, without replacing it. The panel SHALL list each counted ticket of the project, in the order the tickets were created, with its title, stage, percentage and bar. Under each ticket that is not accepted, it SHALL list that ticket's unarchived agents and the verifiers of its latest cycle, each once, including a verifier the host archived after it passed. Verifiers of earlier cycles and other archived agents SHALL NOT be listed. Each agent row SHALL show its role and focus, its model, its figure, its elapsed time and its bar. The elapsed time SHALL be the time since the agent's current run started while it is working, and otherwise the length of its latest run. The elapsed time SHALL refresh only while the panel is open and a listed agent is working. Selecting an agent row SHALL select that agent's conversation.

#### Scenario: Open the crew
- **WHEN** the human activates "6 agents" on the coordinator's strip
- **THEN** the panel opens beside the conversation and lists every counted ticket with its stage and percentage, and each open ticket's agents with model, figure and elapsed time

#### Scenario: Archived passed verifier
- **WHEN** round 1 ended with the tester passed and archived, and round 2 is running with one reviewer
- **THEN** the ticket's card still lists the tester as "passed" at 100%, beside the working reviewer

#### Scenario: Earlier cycle's verifiers
- **WHEN** cycle 2 is running on a ticket whose cycle 1 verifiers were archived
- **THEN** the card lists cycle 2's verifiers and none of cycle 1's

#### Scenario: Accepted ticket
- **WHEN** a ticket is accepted
- **THEN** its card shows "Accepted · 100%" and no agent rows

#### Scenario: Idle panel
- **WHEN** the panel is open and no listed agent is working
- **THEN** no elapsed-time timer runs

### Requirement: Progress states
Every bar SHALL be in exactly one of four states: in progress, drawn in the focus color; indeterminate, drawn as a striped track with no percentage; blocked, drawn in red at the position it had; or finished, drawn in green at 100%. An idle bar SHALL be drawn as in progress. No bar SHALL show a percentage that these rules do not define.

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
The host SHALL keep progress current whenever a plan step, a session's status, a ticket or a verification record changes, and SHALL rebuild the same figures from stored steps and records after a restart. Progress SHALL reach the desktop app incrementally: the host signals only when a figure actually changed, and the client then requests only the progress entries changed since its last cursor, never a full workspace snapshot. A step that is not a plan step SHALL cause no progress signal. Progress changes SHALL NOT wake, notify or message any agent, and SHALL NOT delay message receipts, permission requests or failures.

#### Scenario: A plan step arrives
- **WHEN** a worker's plan goes from 2 of 5 to 3 of 5
- **THEN** the desktop receives that agent's entry, its ticket's and its project's, and no snapshot reload occurs

#### Scenario: Ordinary steps
- **WHEN** a worker streams narration and runs commands without changing its plan
- **THEN** no progress signal is sent

#### Scenario: Coordinator stays asleep
- **WHEN** a worker's progress changes during its turn
- **THEN** its coordinator is not woken and receives no message because of it

#### Scenario: Restart
- **WHEN** the host restarts while an implementer's stored plan reads 3 of 6 and its ticket is `assigned`
- **THEN** after the restart the implementer shows 3 of 6 and the ticket 30%
