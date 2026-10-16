# skills Specification

## Purpose
Splits every agent's instructions into a fixed core, which holds what the app depends on and cannot be edited, and a skill, a named way of working with a stock text bundled with the app.

## Requirements

### Requirement: Instructions are a fixed core and a skill
The host SHALL build each agent's instructions from a fixed core and at most one skill. There SHALL be four skills: Coordinating for the project coordinator, Implementing for implementers and maintenance agents, Reviewing for testers and reviewers, and Keeping a PR for the PR keeper's try. The fixed core SHALL be the communication contract, which every role gets, followed by the role's own core. The PR keeper's comment sorting SHALL get no skill and SHALL run with the instructions it has today. The instructions SHALL be, in order: the communication contract, the role's core, the skill's stock text or the human's rewrite of it, and, in Additions mode when the human has rules for that skill, a heading and a sentence of Wiffletree's own saying they are the human's own rules and win where they disagree with the skill above, followed by those rules. Opening a pull request SHALL NOT be a skill.

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

### Requirement: The fixed core holds what the app depends on
The fixed core SHALL hold every sentence that names a workspace tool, a report kind or the findings format, a message from Wiffletree, a folder or branch the app assigns, or something the workspace does or refuses. A skill SHALL hold only ways of working. Where another requirement says what bundled instructions say, it SHALL hold for the fixed core with every skill at stock; its statements of the first kind SHALL be in the fixed core, so they reach the agent whatever the human writes. The fixed core SHALL say that its tools and rules still apply where a skill disagrees. The fixed core SHALL NOT be editable.

#### Scenario: An empty rewrite
- **WHEN** every skill is rewritten to an empty text
- **THEN** every role's instructions still name its workspace tools, the report kinds, the findings format, the publishing rules the human confirmed on October 13, 2026 and the process rules

#### Scenario: A stock statement
- **WHEN** every skill is at stock
- **THEN** the coordinator's instructions state the stop rule, the implementer's state the objection rule, and the verifiers' define a blocking finding

#### Scenario: Rules that contradict the core
- **WHEN** the human's rules for Implementing say to push the branch with `git push`
- **THEN** the rules are sent as written, the core still says never to push from the shell, and `publish_ticket` refuses what it refused before

### Requirement: Stock skills are bundled and versioned
Each stock skill and each part of the fixed core SHALL be bundled with the app and SHALL begin with its name and a version, changed whenever its text changes. A skill with no saved state SHALL be at stock.

#### Scenario: A fresh install or an update
- **WHEN** Wiffletree starts with no skills folder
- **THEN** every skill is at stock with no rules of the human's own, and nothing is migrated
