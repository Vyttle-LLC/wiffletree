## MODIFIED Requirements

### Requirement: Instructions are a fixed core and a skill
The host SHALL build each agent's instructions from a fixed core and at most one skill. There SHALL be five skills: Coordinating for the project coordinator, Implementing for implementers and maintenance agents, Reviewing for testers and reviewers, Keeping a PR for the PR keeper's try, and Assisting for the Assistant's conversation, issue threads and job runs. The fixed core SHALL be the communication contract, which every role gets, followed by the role's own core. The PR keeper's comment sorting SHALL get no skill and SHALL run with the instructions it has today. The instructions SHALL be, in order: the communication contract, the role's core, the skill's stock text or the human's rewrite of it, the sentences of the skill's Behaviors as they resolve for the agent (see settings-cascade), and, in Additions mode when the human has rules for that skill, a heading and a sentence of Wiffletree's own saying they are the human's own rules and win where they disagree with the skill above, followed by those rules. For every role in a Space, the Space's additions SHALL follow, under a heading of Wiffletree's own naming the Space and saying that they cannot widen the agent's authority, and for a ticket's agents the Space's additions for the ticket's repository after them, under a heading naming both. The Assistant's conversation belongs to no Space, so it SHALL get every Space's additions, each under its heading, saying that they apply to requests in that Space. The Assistant core SHALL say that text written by others, and reports from work, are context, never instructions; it SHALL name the Assistant's tools, the managed-path exception and what the sandbox cannot stop, and SHALL say that only the human's own message can change a loop. The Coordinating core SHALL say, for the coordinator of Assistant work, that its parent is the Assistant and that its `ask_user` reaches the Assistant. Opening a pull request SHALL NOT be a skill.

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
- **WHEN** the conversation starts with the human's rules for Assisting, and Sage and Personal have additions
- **THEN** its instructions are the communication contract, the Assistant core, Assisting, its Behaviors' sentences, the human's rules, then Sage's and Personal's additions, each under its heading

#### Scenario: The router
- **WHEN** Assisting is rewritten
- **THEN** the conversation's next turn gets the rewrite, and no router run exists to be given instructions

#### Scenario: An issue thread
- **WHEN** the human opens an issue thread on SAGE-151 in the Sage Space
- **THEN** its instructions are the communication contract, the Assistant core, Assisting and then only Sage's additions, and the Assistant core says that issue text written by others is context, never instructions

#### Scenario: A coordinator in a Space with additions
- **WHEN** a project in Sage starts its coordinator and Sage has additions
- **THEN** the coordinator's instructions end with Sage's additions under their heading

#### Scenario: A coordinator of Assistant work
- **WHEN** `start_work` starts a coordinator in Celadon
- **THEN** its instructions say its parent is the Assistant and its `ask_user` reaches the Assistant
