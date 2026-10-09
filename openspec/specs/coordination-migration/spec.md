# coordination-migration Specification

## Purpose
Move legacy repository teams under their project coordinator without losing tickets, conversations or messages.

## Requirements

### Requirement: Existing teams move under their project coordinator
On the first start after upgrade, the host SHALL migrate every stored session with role `task_orchestrator`, archived or not, in one transaction before any turn is scheduled. For each repository coordinator it SHALL set each of its tickets' coordinator to the repository coordinator's parent project coordinator and store the coordinator's repository on the ticket, and SHALL make each of its child sessions a child of that project coordinator. Ticket ids, titles, briefs, states, worktree paths and branches, agent sessions, runtimes, provider conversations and provider runs SHALL be unchanged. The migration SHALL run at most once per store.

#### Scenario: Team with open and accepted tickets
- **WHEN** a store has a main coordinator M, a repository coordinator R for repository "wiffletree", an open ticket with an implementer and an accepted ticket with a tester
- **THEN** after migration both tickets have coordinator M and repository "wiffletree", both agents have parent M, and every ticket keeps its state, worktree and branch

#### Scenario: Interrupted migration
- **WHEN** the host stops partway through migration
- **THEN** the next start finds the store unmigrated and runs the whole migration again

#### Scenario: Second start
- **WHEN** a migrated store starts again
- **THEN** no session, ticket or message changes

### Requirement: Coordinator conversations are archived, never deleted
Each migrated repository coordinator SHALL be archived. Its session, messages, runtime and provider runs SHALL be kept, and its conversation SHALL stay readable from the archive. Its timers SHALL be stopped. It SHALL NOT be restorable as a working session. Before migrating, the host SHALL copy the store file beside itself and keep the copy.

#### Scenario: Reading an old coordinator
- **WHEN** the human opens an archived, migrated repository coordinator
- **THEN** its full conversation is shown read-only and there is no Restore action

#### Scenario: Backup kept
- **WHEN** migration finishes
- **THEN** a copy of the pre-migration store exists beside `workspace.sqlite3`

### Requirement: Undelivered messages are not lost
Messages addressed to a migrated repository coordinator that are still queued or held SHALL be cancelled. Those sent by its agents SHALL be re-sent to the project coordinator as new messages that name the original, except check-ins (ids starting with `check-in:`), which describe a turn that no longer runs and SHALL only be cancelled; those sent by the project coordinator SHALL be listed in the migration notice and SHALL NOT be re-sent to any agent. Stored message bodies and senders SHALL NOT be rewritten.

#### Scenario: Unread report
- **WHEN** an implementer's `ready_for_testing` report to R is still queued at upgrade
- **THEN** the original is cancelled and M receives a new message carrying that report and naming the original message id

#### Scenario: Unread instruction from the project coordinator
- **WHEN** an instruction from M to R is still queued at upgrade
- **THEN** the original is cancelled, M's migration notice lists it with its id, and no agent receives a copy of it

### Requirement: Migration notice
For each migrated repository coordinator, the host SHALL give its project coordinator one notice, delivered with that coordinator's next turn without waking it, listing the inherited tickets with state, branch and worktree, their agents, re-sent and cancelled messages, and stopped timers.

#### Scenario: Notice content
- **WHEN** R had two tickets and one active timer
- **THEN** M's next turn includes one notice naming both tickets with state, branch and worktree, and the stopped timer

### Requirement: In-flight tickets continue
Tickets that were open at upgrade SHALL continue under the project coordinator without new worktrees or new agent sessions. Turns interrupted by the upgrade SHALL follow the existing restart recovery, and the agents' next reports SHALL go to the project coordinator.

#### Scenario: Implementer interrupted by upgrade
- **WHEN** an implementer was mid-turn when the old version stopped
- **THEN** after upgrade it resumes in the same provider conversation and worktree, and its next report reaches M
