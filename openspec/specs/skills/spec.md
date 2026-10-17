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

### Requirement: Only the human changes a skill
Saving rules, starting and saving a rewrite, marking it up to date, resetting, restoring, undoing a merge, merging on demand, answering a merge's question and dismissing its note SHALL be commands only the human's client sends. The host's own merge of a new stock version SHALL be the only change to a skill the host makes without a command. No agent tool SHALL read or change the skills folder. The host SHALL accept any text of up to 32,000 characters for the human's rules and for a rewrite, including an empty one, and SHALL refuse a longer text with the previous state kept. The host SHALL NOT judge what the text says.

#### Scenario: An agent cannot edit a skill
- **WHEN** any agent calls any workspace tool
- **THEN** no skill changes, and no merge starts, is undone or is answered

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
A rewrite SHALL be out of date when its base is not this release's stock text, compared as text, unless its base's version is newer than this release's stock version. A rewrite whose base is newer SHALL be sent as written, SHALL NOT be marked out of date or merged, SHALL NOT offer Mark as up to date, and its page SHALL name the newer version it is based on and say that a Mac with a newer release brought it. An out-of-date rewrite SHALL keep being sent as written until a merge applies. The Skills page SHALL mark it, the sidebar's Skills row SHALL show a dot while any rewrite is out of date, and its page SHALL show the base's and the stock's versions, what changed between them line by line, and where its merge stands: waiting for this launch's first sync, merging, failed with the reason, undone, or waiting for a model. The human SHALL be able to merge it on demand, mark it up to date, which records the current stock text as its base and changes nothing else, or reset it to stock. The host SHALL NOT change a rewrite's text when the stock text changes, except by a merge.

#### Scenario: A release changes Coordinating
- **WHEN** Coordinating is rewritten against v1, an update brings v2, and the merge has not applied yet
- **THEN** the coordinator keeps getting the rewrite as written, the Skills page shows "Rewritten · out of date", and the page shows what changed from v1 to v2 and where the merge stands

#### Scenario: Marking it up to date
- **WHEN** the human edits the rewrite to take in part of v2's change and chooses Mark as up to date
- **THEN** its base becomes v2, the mark and the dot go, and the rewrite is sent as saved

#### Scenario: A stock change without a new version
- **WHEN** a release changes a stock text and forgets to change its version
- **THEN** a rewrite based on the old text is still marked out of date, and no automatic merge runs

#### Scenario: A base from a newer release
- **WHEN** this Mac runs Coordinating v2 and pulls a rewrite merged against v3 on another Mac
- **THEN** the rewrite is sent as written, it is not marked out of date or merged, Mark as up to date is not offered, and its page says it is based on v3 from a Mac with a newer release

### Requirement: A new stock version is merged into a rewrite
The host SHALL merge this release's stock text into a rewrite that is due a merge, without waiting for the human. A rewrite SHALL be due a merge when its base's version is older than this release's stock version, its files can be read and are not paused by sync, nothing in its history stops merges into this release's stock version, as "A merge leaves a note that can be reviewed or undone" says, and no question about this stock version is waiting for the rewrite and base it has now. A rewrite whose base has the same version as this release's stock text but different text SHALL stay out of date for the human to merge or mark up to date, and SHALL NOT be merged automatically. A skill in Additions mode SHALL NOT be merged. The host SHALL check for due rewrites at start, after the "New stock version" entries are written; after a sync pass that changed a skill's files; and when the human chooses to merge it on its skill's page. The merge SHALL be three-way, paragraph by paragraph, between the base, the stock text and the rewrite. A paragraph SHALL be a run of non-blank lines together with the blank lines after it, apart from a text's first line, its title and version. The title line SHALL be merged on its own: the side that changed it SHALL be taken, and the human's when both changed it differently. It SHALL NOT be part of an overlap, and a change to it SHALL NOT make a change to the first paragraph overlap. A change that only the stock text or only the rewrite made SHALL be taken as made, and a change both made alike SHALL be taken once. Changes from the two sides SHALL overlap when their base paragraphs share a paragraph, touch with no unchanged base paragraph between them, or both insert at the same place. A blank line SHALL NOT count as an unchanged paragraph between two changes. With no overlap, the merge SHALL apply with no model. Applying a merge SHALL save the merged rewrite and this release's stock text as its base, with one "Merged" history entry, or SHALL change nothing. Until a merge applies, the rewrite SHALL be sent as written.

#### Scenario: Changes that don't overlap
- **WHEN** Coordinating is rewritten against v1, the human changed its first paragraph, and an update brings v2, which changes only its fifth paragraph
- **THEN** at start the rewrite holds the human's first paragraph and v2's fifth, its base is v2, no model runs, and the history gains one "Merged v1 → v2" entry

#### Scenario: Neighbouring paragraphs overlap
- **WHEN** the human changed Coordinating's first paragraph and v2 changes its second, with only a blank line between them in the base
- **THEN** the two changes are one overlapping passage, and the plain merge does not apply it on its own

#### Scenario: A new version line and an edited first paragraph
- **WHEN** the human changed only the first paragraph under the title, and v2 changes only the version in the title
- **THEN** nothing overlaps, and the merged rewrite has the title "# Coordinating — v2" and the human's first paragraph

#### Scenario: The human already copied the change
- **WHEN** the human copied v2's changed paragraph into the rewrite by hand before the update
- **THEN** the merge takes that paragraph once, and nothing overlaps

#### Scenario: The same version with different text
- **WHEN** a rewrite's base is Coordinating v2 and this release's stock text is also v2 but its text differs, because a release forgot to change the version
- **THEN** no automatic merge runs, at start or after any pass, and the rewrite stays out of date with Merge v2 in and Mark as up to date offered

#### Scenario: Additions need no merge
- **WHEN** an update changes Implementing's stock text while the human has rules for it
- **THEN** no merge runs, and implementers get the new stock text followed by the rules

#### Scenario: A paused skill waits
- **WHEN** sync has paused Coordinating's files and an update brings v2
- **THEN** no merge runs until the pause is settled, and the coordinator gets the rewrite as written

### Requirement: Overlapping passages are merged by a model
When a merge has overlapping passages, the host SHALL ask the PR keeper's model to merge only those passages, when that model is a Claude model. The host SHALL send it the skill's name, the base, the stock text and the rewrite, and for each passage its base, stock and human versions, and nothing else; the fixed core SHALL NOT be sent. The run SHALL be a Claude process with every tool off, no MCP server, no session persistence, hooks off, a fixed system prompt and a schema for its answer, in an empty temporary folder, with no workspace credentials, and a time limit of 5 minutes. Its answer SHALL give each passage either a merged text or a one-sentence question. The host SHALL apply nothing when the answer does not match the schema, does not answer every passage exactly once, gives a merged passage longer than that passage's stock and human versions together, gives a question longer than 300 characters, or makes a rewrite longer than 32,000 characters. When the skill's files changed while the model ran, the host SHALL drop the answer and check the skill again. Every line outside an overlapping passage SHALL be what the plain merge produced. With no PR keeper model, or one that is not a Claude model, no model SHALL run, and the skill's page SHALL offer a choice for each passage. A run that fails SHALL change nothing, SHALL NOT move to another model, and SHALL be tried again at the next start or when the human chooses Try again. The "Merged" entry SHALL record the model and the tokens it used.

#### Scenario: An overlapping passage
- **WHEN** v2 shortens "After two or three rounds" to "After two rounds" in a paragraph the human also rewrote, and the PR keeper's model is Claude · haiku · medium
- **THEN** one run merges that passage, the rest is merged as plain text, and the "Merged" entry names that model and its tokens

#### Scenario: What the model is sent
- **WHEN** a merge run starts
- **THEN** its input holds the three texts and the passages of that skill, its instructions contain none of the fixed core, and it runs with no tools and no MCP server

#### Scenario: An answer that can't be used
- **WHEN** the model's merged passage is longer than the passage's stock and human versions together
- **THEN** nothing changes, the rewrite runs as written, and the skill's page shows why with Try again

#### Scenario: The human saved while the model ran
- **WHEN** the human saves the rewrite while its merge run is in progress
- **THEN** the run's answer is dropped, and the merge is checked again from the saved rewrite

#### Scenario: No model
- **WHEN** no PR keeper model is chosen and a merge has one overlapping passage
- **THEN** no model runs, nothing goes to the Inbox, and the skill's page says the update and the rewrite both change one passage, links to Models, and offers Take v2's wording and Keep my wording

### Requirement: A merge leaves a note that can be reviewed or undone
After a merge applies, the skill's page SHALL show a note naming the versions, how many changes came from the update, how many passages the model merged and with which model, with Review changes, Undo and Dismiss, and the Skills list SHALL mark the skill "Rewritten · updated". It SHALL NOT raise an Inbox note. The note, Review changes and Undo SHALL be about the "Merged" entry whose after-state is the skill's rewrite and base now, not simply the latest entry, and there SHALL be no note when no entry's is. Review changes SHALL show the rewrite before and after the merge, line by line, each change labelled as from the update, merged by the model, or chosen by the human, with the Mac and the time. Each Mac SHALL show the note until the human chooses Review changes or Dismiss on that Mac, or the skill changes again. Undo SHALL ask first, then write back the rewrite and base from before the merge exactly, with an "Undid the merge" entry. An Undo, and a restore of a rewrite from an entry older than a "Merged" entry, SHALL stop automatic merges, on every machine, into the newer of the stock version that merge brought in and the stock version of the machine that undid or restored, and into every older stock version, and the page SHALL offer to merge it again. A newer stock version SHALL be merged as usual. The merge's history entry SHALL offer Undo this merge after later changes, and it SHALL ask first and say the later changes stay in the history.

#### Scenario: After an automatic merge
- **WHEN** Coordinating was merged into v2 at start
- **THEN** its page shows "Merged Coordinating v2 into your rewrite" with Review changes, Undo and Dismiss, and the Skills list shows "Rewritten · updated"

#### Scenario: Undo
- **WHEN** the human chooses Undo and confirms
- **THEN** `rewrite.md` and `base.md` hold exactly the bytes they held before the merge, the coordinator's next turn has that rewrite, the skill is out of date with Merge v2 in offered, and no merge runs at the next start

#### Scenario: A later entry that is not a merge
- **WHEN** Coordinating was merged into v2, and a newer "New stock version" entry from another Mac follows the "Merged" entry
- **THEN** the note, Review changes and Undo are still about that merge, because the skill still holds its rewrite and base

#### Scenario: Undo of another release's merge
- **WHEN** a Mac on v3 undoes the v1 → v2 merge a Mac on v2 made, before merging v3 in
- **THEN** neither Mac merges Coordinating automatically at its next start, and a Mac on v4 merges v4 in as usual

#### Scenario: Dismissed on one Mac
- **WHEN** the human dismisses the note on one Mac
- **THEN** the other Mac still shows it, and the merge stays in the history

### Requirement: A merge never touches the fixed core and is always recorded
No merge SHALL read, send or change the fixed core. A model SHALL NOT write a skill's files: only the host SHALL apply a merge, and the files and its history entry SHALL be written together or not at all. A merge SHALL hold a merged rewrite to the same 32,000-character limit as any rewrite. Undo SHALL restore the human's exact earlier rewrite and base from the merge's own history entry.

#### Scenario: The core after a merge
- **WHEN** every skill is rewritten and merged into a new stock version
- **THEN** every role's instructions still contain the whole fixed core

#### Scenario: A record that cannot be written
- **WHEN** the merge's history entry cannot be written
- **THEN** the rewrite and its base are as they were before the merge

### Requirement: Every skill can be reset to stock
Each skill's page SHALL offer Reset to stock, which SHALL ask first, naming what stops applying and that it stays in the history. Reset SHALL return the skill to Additions mode with no rules of the human's own, and SHALL write a history entry.

#### Scenario: Resetting a skill with rules
- **WHEN** the human chooses Reset to stock on Implementing with three rules, and confirms
- **THEN** Implementing is at stock, implementers' next turns have no rules section, and the history's previous entry still holds the three rules

#### Scenario: Cancelling
- **WHEN** the human chooses Reset to stock and then Cancel
- **THEN** nothing changes and no history entry is written

### Requirement: Every skill has a history
Each change to a skill SHALL write a history entry holding the skill's whole state after it, the stock version, the time and what happened. At start, the host SHALL write a "New stock version" entry for each skill with a history whose recorded stock versions are all older than this release's. A machine on an older release SHALL write no such entry, and the skill's page SHALL say that another machine runs a newer stock version. The skill's page SHALL list the entries newest first, each viewable read-only and restorable. Restoring SHALL make the entry's state current and write a new entry, and SHALL delete none. Restoring a rewrite SHALL restore its base, so it may be out of date at once. Restoring a rewrite from an entry older than a "Merged" entry SHALL ask first, saying that no automatic merge brings that merge's stock version in until a newer one arrives, and SHALL then stop automatic merges as an Undo does.

#### Scenario: Restoring earlier rules
- **WHEN** the human restores the entry from before a reset
- **THEN** those rules apply from each agent's next turn, and the history keeps the reset and gains a "Restored" entry

#### Scenario: Restoring an old rewrite
- **WHEN** the human restores a rewrite saved against v1 while this release has v2
- **THEN** the rewrite is current and marked out of date

#### Scenario: Restoring a rewrite from before a merge
- **WHEN** Coordinating was merged into v2, and the human restores the rewrite saved before that merge and confirms
- **THEN** the rewrite is current and out of date, and no merge runs at the next start

#### Scenario: An update
- **WHEN** Wiffletree starts after an update that changed Reviewing's stock version, and the human had edited Reviewing before
- **THEN** Reviewing's history gains one "New stock version" entry naming both versions

#### Scenario: Two machines on different releases
- **WHEN** one machine has Coordinating v2 and wrote its "New stock version" entry, and another machine sharing the settings repository still has v1 and starts
- **THEN** the second machine writes no history entry, and Coordinating's page there says another machine runs v2

### Requirement: Skills are stored in a folder that can move
The host SHALL keep skills in the `skills` folder of the shared folder (see settings-repository), one folder per skill, holding the human's rules, the rewrite and its base as plain text files, and one file per history entry. All reads and writes SHALL go through one store rooted at that folder, and writes SHALL be atomic. A missing file SHALL mean no state for it. A file the host cannot read SHALL count as missing for the instructions, and the skill's page SHALL say it could not be read. Credentials, the database and conversations SHALL NOT be stored there. A skills folder from before the shared folder SHALL be moved into it unchanged at start.

#### Scenario: A file that cannot be read
- **WHEN** Implementing's rules file is not valid UTF-8
- **THEN** implementers get the stock text with no rules, and Implementing's page says its file could not be read

#### Scenario: Skills saved before the shared folder
- **WHEN** Wiffletree starts after an update with rules for Implementing in `<data folder>/skills/`
- **THEN** they are in the shared folder's `skills/implementing/`, the history is unchanged, and implementers keep getting the rules

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
