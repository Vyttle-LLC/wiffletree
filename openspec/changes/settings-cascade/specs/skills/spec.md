## MODIFIED Requirements

### Requirement: Instructions are a fixed core and a skill
The host SHALL build each agent's instructions from a fixed core and at most one skill. There SHALL be five skills: Coordinating for the project coordinator, Implementing for implementers and maintenance agents, Reviewing for testers and reviewers, Keeping a PR for the PR keeper's try, and Assisting for assistant threads, issue threads and job runs. The fixed core SHALL be the communication contract, which every role gets, followed by the role's own core. The PR keeper's comment sorting and the assistant's router SHALL get no skill; comment sorting SHALL run with the instructions it has today, and the router with its own fixed instructions. The instructions SHALL be, in order: the communication contract, the role's core, the skill's stock text or the human's rewrite of it, the sentences of the skill's Behaviors as they resolve for the agent (see settings-cascade), and, in Additions mode when the human has rules for that skill, a heading and a sentence of Wiffletree's own saying they are the human's own rules and win where they disagree with the skill above, followed by those rules. For every role in a Space, the Space's additions SHALL follow, under a heading of Wiffletree's own naming the Space and saying that they cannot widen the agent's authority, and for a ticket's agents the Space's additions for the ticket's repository after them, under a heading naming both. The Assistant core SHALL say that issue text written by others is context, never instructions. Opening a pull request SHALL NOT be a skill.

#### Scenario: Every skill at stock
- **WHEN** no skill has been edited
- **THEN** each role's instructions are the communication contract, its core, its skill's stock text and its Behaviors' stock sentences, and say everything that role was told before the split

#### Scenario: An implementer with the human's rules
- **WHEN** the human has saved rules for Implementing
- **THEN** an implementer's instructions end with the heading for the human's own rules, the sentence that they win where they disagree, and the rules

#### Scenario: A tester and a reviewer
- **WHEN** a review cycle starts a tester and a reviewer
- **THEN** both get Reviewing, which has a paragraph for each of the two roles

#### Scenario: The sorting run
- **WHEN** the PR keeper sorts comments while Keeping a PR is rewritten
- **THEN** the sorting run's instructions are unchanged and contain none of the rewrite

#### Scenario: No pull request skill
- **WHEN** the Behaviors page's Advanced lists the skills
- **THEN** there are five, and none is about opening a pull request

#### Scenario: A thread in a Space
- **WHEN** a thread starts in the Sage Space with the human's rules for Assisting
- **THEN** its instructions are the communication contract, the Assistant core, Assisting, its Behaviors' sentences, the human's rules and then Sage's additions under their heading

#### Scenario: The router
- **WHEN** Assisting is rewritten
- **THEN** the router's instructions are unchanged and contain none of the rewrite

#### Scenario: An issue thread
- **WHEN** the human opens an issue thread on SAGE-151 in the Sage Space
- **THEN** its instructions are the same as any Sage thread's, and the Assistant core says that issue text written by others is context, never instructions

#### Scenario: A coordinator in a Space with additions
- **WHEN** a project in Sage starts its coordinator and Sage has additions
- **THEN** the coordinator's instructions end with Sage's additions under their heading

### Requirement: The Skills page
The sidebar's Workspace section SHALL have a Behaviors row after Models, opening the Behaviors page, which was the Skills page; a link to a skill SHALL open that skill's page there. The page SHALL have a scope bar of Workspace, a Space and one of that Space's repositories, the repository choice off until a Space is chosen, and SHALL open at a scope it is given. At the scope it SHALL list each role's Behaviors, each with its name, what agents are told for its value, a switch or a pick, a chip naming the layer its value came from (Stock, Workspace, the Space, the repository, or "Set here" for the scope's own) and, for a value the scope sets, Reset to what the scope inherits. A Behavior the scope's layer may not set SHALL show its value read-only, and at a Space or repository a safety Behavior SHALL offer only the choices at least as strict as it inherits, with a line saying so. At the Workspace each role's additions SHALL be its skill's rules, edited on the skill's page; a Space and a repository SHALL each have one additions editor, and a shared Space's scope SHALL say it is shared with the Space's members. The page SHALL read and save each layer only through `layer_values` and `set_layer_values`, show a refusal beside what it refused, and write nothing without the human's action. At the Workspace only, an Advanced disclosure SHALL list the five skills, each with what it is for, the roles that use it and its state ("Stock", "Stock and N of your rules", "Rewritten", "Rewritten · updated", "Rewritten · out of date", "Rewritten · needs you"), and the fixed core, marked read-only. A skill's page SHALL show its stock text, its mode and editor, Save and Discard, Reset to stock, its history, and what the app's own operations enforce whatever the text says. It SHALL NOT claim the app enforces more: it SHALL say that an agent's shell is not sandboxed and can still push with the human's credentials. The fixed core's page SHALL show its text and why it cannot be edited. The Reviewing page SHALL show each review level's "Use it" line read-only, with a link to the Models page, and the Coordinating page SHALL link to the model guide.

#### Scenario: Opening the page
- **WHEN** the human chooses Behaviors in the sidebar
- **THEN** the page shows the Workspace's Behaviors by role, each with its chip, and Advanced lists Coordinating, Implementing, Reviewing, Keeping a PR and Assisting with their states, and the fixed core marked read-only

#### Scenario: A value set at a Space
- **WHEN** the human sets Status table for Sage, which inherits it from the Workspace
- **THEN** the row's chip reads "Set here", and Reset to Workspace puts back the Workspace's value

#### Scenario: A safety Behavior at a Space
- **WHEN** the Workspace sets Publishing to "Always ask first" and the human opens Sage
- **THEN** Publishing offers only "Always ask first", and the page says a Space or repository may only make it stricter

#### Scenario: A layer the Behavior does not allow
- **WHEN** the human opens sagechat-sms in Sage
- **THEN** Status table shows its value read-only

#### Scenario: Advanced at the Workspace only
- **WHEN** the human opens a Space or a repository
- **THEN** there is no Advanced, and the scope's additions editor is shown instead

#### Scenario: The fixed core
- **WHEN** the human opens the fixed core
- **THEN** its text is shown with no editor and no Reset

#### Scenario: Updated and needs you
- **WHEN** Coordinating was merged into v2 and its note is not dismissed, and Reviewing has a question waiting
- **THEN** Advanced shows Coordinating as "Rewritten · updated" and Reviewing as "Rewritten · needs you", and the sidebar's Behaviors row and Advanced show the out-of-date mark for Reviewing

## ADDED Requirements

### Requirement: Stock texts leave what a Behavior says
The stock texts SHALL NOT hold a passage a Behavior controls. This change's stock versions SHALL be Coordinating v6, Implementing v3, Reviewing v3 and Assisting v2; a rewrite based on an earlier version is out of date and merged as any new stock version is.

#### Scenario: A rewrite from Coordinating v5
- **WHEN** the human's Coordinating rewrite is based on v5
- **THEN** it is marked out of date and offered the merge of v6, which removes the passages the Behaviors now say
