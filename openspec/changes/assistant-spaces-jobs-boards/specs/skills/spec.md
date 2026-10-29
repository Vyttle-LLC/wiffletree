## MODIFIED Requirements

### Requirement: Instructions are a fixed core and a skill
The host SHALL build each agent's instructions from a fixed core and at most one skill. There SHALL be five skills: Coordinating for the project coordinator, Implementing for implementers and maintenance agents, Reviewing for testers and reviewers, Keeping a PR for the PR keeper's try, and Assisting for assistant threads and job runs. The fixed core SHALL be the communication contract, which every role gets, followed by the role's own core. The PR keeper's comment sorting and the assistant's router SHALL get no skill; comment sorting SHALL run with the instructions it has today, and the router with its own fixed instructions. The instructions SHALL be, in order: the communication contract, the role's core, the skill's stock text or the human's rewrite of it, and, in Additions mode when the human has rules for that skill, a heading and a sentence of Wiffletree's own saying they are the human's own rules and win where they disagree with the skill above, followed by those rules. For assistant threads and job runs, the Space's instructions SHALL follow, under a heading of Wiffletree's own naming the Space and saying that they cannot widen the thread's authority. Opening a pull request SHALL NOT be a skill.

#### Scenario: Every skill at stock
- **WHEN** no skill has been edited
- **THEN** each role's instructions are the communication contract, its core and its skill's stock text, and say everything that role was told before the split

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
- **WHEN** the Skills page lists the skills
- **THEN** there are five, and none is about opening a pull request

#### Scenario: A thread in a Space
- **WHEN** a thread starts in the Sage Space with the human's rules for Assisting
- **THEN** its instructions are the communication contract, the Assistant core, Assisting, the human's rules and then Sage's instructions under their heading

#### Scenario: The router
- **WHEN** Assisting is rewritten
- **THEN** the router's instructions are unchanged and contain none of the rewrite
