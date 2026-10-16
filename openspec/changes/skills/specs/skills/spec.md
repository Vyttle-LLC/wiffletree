## ADDED Requirements

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
A saved change to a skill SHALL apply to every agent of the skill's roles from that agent's next turn, including agents that already exist. A turn already running SHALL keep the instructions it started with. The context estimate of a session SHALL use the same instructions as its turns.

#### Scenario: A running implementer
- **WHEN** the human saves rules for Implementing while an implementer is in a turn
- **THEN** that turn is unchanged, and the implementer's next turn has the rules

### Requirement: Only the human changes a skill
Saving rules, starting and saving a rewrite, marking it up to date, resetting and restoring SHALL be commands only the human's client sends. No agent tool SHALL read or change the skills folder. The host SHALL accept any text of up to 32,000 characters for the human's rules and for a rewrite, including an empty one, and SHALL refuse a longer text with the previous state kept. The host SHALL NOT judge what the text says.

#### Scenario: An agent cannot edit a skill
- **WHEN** any agent calls any workspace tool
- **THEN** no skill changes

#### Scenario: A text too long
- **WHEN** the human saves 40,000 characters of rules
- **THEN** the save is refused with the limit named, and the previous rules stay in effect

#### Scenario: An empty rewrite
- **WHEN** the human rewrites Reviewing to an empty text and saves
- **THEN** the save is accepted, and verifiers get the communication contract and the verifier core with no skill text

### Requirement: A rewrite replaces the stock text on purpose
The human SHALL be able to start a rewrite of a skill, after a confirmation. The rewrite SHALL start as the stock text followed by the human's rules under their heading, when there are any. Starting it SHALL save the stock text as the rewrite's base and remove the human's separate rules. In Rewrite mode the rewrite SHALL be sent in place of the stock text, and no separate rules SHALL be sent. Reset to stock on a rewritten skill SHALL ask first, naming the rewrite as what stops applying, and SHALL remove the rewrite and its base.

#### Scenario: Starting a rewrite with rules
- **WHEN** Coordinating has two rules of the human's own and the human starts a rewrite
- **THEN** the editor holds the stock text followed by the two rules, the base is the current stock text, and after saving the coordinator's next turn has the rewrite in place of the stock text and no separate rules section

#### Scenario: Resetting a rewritten skill
- **WHEN** Coordinating is rewritten and out of date, and the human chooses Reset to stock and confirms the question that names the rewrite
- **THEN** the rewrite and its base are removed, Coordinating is at this release's stock text in Additions mode with no rules and no out-of-date mark, the coordinator's next turn has the stock text, and the history's previous entry still holds the rewrite

### Requirement: A rewrite is marked out of date by a new stock version
A rewrite SHALL be out of date when its base is not this release's stock text, compared as text. An out-of-date rewrite SHALL keep being sent as written. The Skills page SHALL mark it, the sidebar's Skills row SHALL show a dot while any rewrite is out of date, and its page SHALL show the base's and the stock's versions and what changed between them, line by line. The human SHALL be able to mark it up to date, which records the current stock text as its base and changes nothing else, or reset it to stock. The host SHALL NOT change a rewrite's text when the stock text changes.

#### Scenario: A release changes Coordinating
- **WHEN** Coordinating is rewritten against v1 and an update brings v2
- **THEN** the coordinator keeps getting the rewrite as written, the Skills page shows "Rewritten · out of date", and the page shows what changed from v1 to v2

#### Scenario: Marking it up to date
- **WHEN** the human edits the rewrite to take in part of v2's change and chooses Mark as up to date
- **THEN** its base becomes v2, the mark and the dot go, and the rewrite is sent as saved

#### Scenario: A stock change without a new version
- **WHEN** a release changes a stock text and forgets to change its version
- **THEN** a rewrite based on the old text is still marked out of date

### Requirement: Every skill can be reset to stock
Each skill's page SHALL offer Reset to stock, which SHALL ask first, naming what stops applying and that it stays in the history. Reset SHALL return the skill to Additions mode with no rules of the human's own, and SHALL write a history entry.

#### Scenario: Resetting a skill with rules
- **WHEN** the human chooses Reset to stock on Implementing with three rules, and confirms
- **THEN** Implementing is at stock, implementers' next turns have no rules section, and the history's previous entry still holds the three rules

#### Scenario: Cancelling
- **WHEN** the human chooses Reset to stock and then Cancel
- **THEN** nothing changes and no history entry is written

### Requirement: Every skill has a history
Each change to a skill SHALL write a history entry holding the skill's whole state after it, the stock version, the time and what happened. At start, the host SHALL write a "New stock version" entry for each skill with a history whose stock version differs from the one its latest entry recorded. The skill's page SHALL list the entries newest first, each viewable read-only and restorable. Restoring SHALL make the entry's state current and write a new entry, and SHALL delete none. Restoring a rewrite SHALL restore its base, so it may be out of date at once.

#### Scenario: Restoring earlier rules
- **WHEN** the human restores the entry from before a reset
- **THEN** those rules apply from each agent's next turn, and the history keeps the reset and gains a "Restored" entry

#### Scenario: Restoring an old rewrite
- **WHEN** the human restores a rewrite saved against v1 while this release has v2
- **THEN** the rewrite is current and marked out of date

#### Scenario: An update
- **WHEN** Wiffletree starts after an update that changed Reviewing's stock version, and the human had edited Reviewing before
- **THEN** Reviewing's history gains one "New stock version" entry naming both versions

### Requirement: Skills are stored in a folder that can move
The host SHALL keep skills in a `skills` folder in its data folder, one folder per skill, holding the human's rules, the rewrite and its base as plain text files, and one file per history entry. All reads and writes SHALL go through one store rooted at that folder, and writes SHALL be atomic. A missing file SHALL mean no state for it. A file the host cannot read SHALL count as missing for the instructions, and the skill's page SHALL say it could not be read. Credentials, the database and conversations SHALL NOT be stored there.

#### Scenario: A file that cannot be read
- **WHEN** Implementing's rules file is not valid UTF-8
- **THEN** implementers get the stock text with no rules, and Implementing's page says its file could not be read

### Requirement: The Skills page
The sidebar's Workspace section SHALL have a Skills row after Models, opening the Skills page. The page SHALL list the four skills, each with what it is for, the roles that use it and its state ("Stock", "Stock and N of your rules", "Rewritten", "Rewritten · out of date"), and the fixed core, marked read-only. A skill's page SHALL show its stock text, its mode and editor, Save and Discard, Reset to stock, its history, and what the app's own operations enforce whatever the text says. It SHALL NOT claim the app enforces more: it SHALL say that an agent's shell is not sandboxed and can still push with the human's credentials. The fixed core's page SHALL show its text and why it cannot be edited. The Reviewing page SHALL show each review level's "Use it" line read-only, with a link to the Models page, and the Coordinating page SHALL link to the model guide.

#### Scenario: Opening the page
- **WHEN** the human chooses Skills in the sidebar
- **THEN** the page lists Coordinating, Implementing, Reviewing and Keeping a PR with their states, and the fixed core marked read-only

#### Scenario: The fixed core
- **WHEN** the human opens the fixed core
- **THEN** its text is shown with no editor and no Reset

### Requirement: The limits of skills are documented
The README SHALL say that skills change how agents work but not what they may do, which the app enforces; that the human's rules win over the stock text; that a rewrite is not updated by new releases; and that an agent's shell can write the skills folder, as it can write the settings file.

#### Scenario: README
- **WHEN** a reader looks up skills in the README
- **THEN** it says where they are edited, that authority is not in them, and that agents' shells can write their folder
