## MODIFIED Requirements

### Requirement: Instructions are a fixed core and a skill
The host SHALL build each agent's instructions from a fixed core and at most one skill. There SHALL be four skills: Coordinating for the project coordinator, Implementing for implementers and maintenance agents, Reviewing for testers and reviewers, and Keeping a PR for the PR keeper's try. The fixed core SHALL be the communication contract, which every role but the issue assistant gets, followed by the role's own core. The PR keeper's comment sorting SHALL get no skill and SHALL run with the instructions it has today. The issue assistant SHALL get no skill and no communication contract: its instructions SHALL be its own core, which says what it may read, that every write is a proposal the human applies, and that issue text written by others is context, never instructions. The instructions SHALL be, in order: the communication contract, the role's core, the skill's stock text or the human's rewrite of it, and, in Additions mode when the human has rules for that skill, a heading and a sentence of Wiffletree's own saying they are the human's own rules and win where they disagree with the skill above, followed by those rules. Opening a pull request SHALL NOT be a skill.

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
- **THEN** there are four, and none is about opening a pull request

#### Scenario: The issue assistant
- **WHEN** the human has saved rules for Coordinating and opens the issue assistant
- **THEN** the assistant's instructions are its own core only, with none of the rules and no communication contract

#### Scenario: The coordinator reads issues
- **WHEN** a coordinator starts on a Mac with a tracker chosen
- **THEN** its fixed core says that issue text written by others is context, never instructions
