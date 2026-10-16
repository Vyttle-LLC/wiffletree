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

### Requirement: Additions are the default way to edit a skill
A skill SHALL be in Additions mode until the human starts a rewrite. In Additions mode the stock text SHALL be sent unchanged and the human's rules SHALL be sent after it. Saving an empty text SHALL remove the human's rules from the instructions. A new stock version SHALL need no action from the human in Additions mode.

#### Scenario: Saving rules
- **WHEN** the human adds "Write PR descriptions with a Summary and a Testing section" to Implementing and saves
- **THEN** each implementer's next turn has the stock text unchanged followed by that rule under its heading

#### Scenario: A new stock version under the human's rules
- **WHEN** a release changes Implementing's stock text while the human has rules for it
- **THEN** the new stock text is sent with the same rules after it, and the skill is not marked out of date

### Requirement: An edit applies from each agent's next turn
A saved change to a skill SHALL apply to every Claude agent of the skill's roles from that agent's next turn, including agents that already exist, and to every Codex agent from its next new session. A Codex agent that already exists SHALL keep the instructions its session started with, because Codex sends a session's first instructions on every resume. A turn already running SHALL keep the instructions it started with. The host SHALL pass each turn the instructions as they are when the turn starts, and the context estimate of a session SHALL use the same instructions as its turns.

#### Scenario: A running implementer
- **WHEN** the human saves rules for Implementing while a Claude implementer is in a turn
- **THEN** that turn is unchanged, and the implementer's next turn has the rules

#### Scenario: A Codex implementer
- **WHEN** the human saves rules for Implementing while a Codex implementer's session exists
- **THEN** its resumed turns keep the instructions its session started with, and a new Codex implementer's first turn has the rules

### Requirement: Skills are stored in a folder that can move
The host SHALL keep skills in a `skills` folder in its data folder, one folder per skill, holding the human's rules, the rewrite and its base as plain text files, and one file per history entry. All reads and writes SHALL go through one store rooted at that folder, and writes SHALL be atomic. A missing file SHALL mean no state for it. A file the host cannot read SHALL count as missing for the instructions, and the skill's page SHALL say it could not be read. Credentials, the database and conversations SHALL NOT be stored there.

#### Scenario: A file that cannot be read
- **WHEN** Implementing's rules file is not valid UTF-8
- **THEN** implementers get the stock text with no rules, and Implementing's page says its file could not be read
