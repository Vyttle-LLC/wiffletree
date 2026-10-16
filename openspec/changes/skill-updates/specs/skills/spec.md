## ADDED Requirements

### Requirement: A new stock version is merged into a rewrite
The host SHALL merge this release's stock text into a rewrite that is due a merge, without waiting for the human. A rewrite SHALL be due a merge when its base is not this release's stock text, its base is not a newer stock version than this release's, its files can be read and are not paused by sync, its history holds no Undo of a merge into this release's stock version, and no question about this stock version is waiting for the rewrite and base it has now. A skill in Additions mode SHALL NOT be merged. The host SHALL check for due rewrites at start, after the "New stock version" entries are written; after a sync pass that changed a skill's files; and when the human chooses to merge it on its skill's page. The merge SHALL be three-way, line by line, between the base, the stock text and the rewrite. A change that only the stock text or only the rewrite made SHALL be taken as made, and a change both made alike SHALL be taken once. Changes from the two sides SHALL overlap when their base lines share a line, touch with no unchanged base line between them, or both insert at the same place. With no overlap, the merge SHALL apply with no model. Applying a merge SHALL save the merged rewrite and this release's stock text as its base, with one "Merged" history entry, or SHALL change nothing. Until a merge applies, the rewrite SHALL be sent as written.

#### Scenario: Changes that don't overlap
- **WHEN** Coordinating is rewritten against v1, the human changed its first paragraph, and an update brings v2, which changes only its fifth paragraph
- **THEN** at start the rewrite holds the human's first paragraph and v2's fifth, its base is v2, no model runs, and the history gains one "Merged v1 → v2" entry

#### Scenario: Neighbouring paragraphs overlap
- **WHEN** the human changed a paragraph and v2 adds a paragraph right after it, with no unchanged paragraph between them
- **THEN** the two changes are one overlapping passage, and the plain merge does not apply it on its own

#### Scenario: The human already copied the change
- **WHEN** the human copied v2's changed paragraph into the rewrite by hand before the update
- **THEN** the merge takes that paragraph once, and nothing overlaps

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

### Requirement: The human is asked only when there is a real question
The host SHALL ask the human about a merge only when the model answers a question for a passage. While a question waits, nothing of that merge SHALL apply, and no further merge run SHALL start for that skill and stock version while its rewrite and base are the ones the question was asked about. The host SHALL record the question in an "Asked you about" history entry holding the merge so far, with any passages the model merged, and, for each passage in question, the question and its stock and human versions, so that any Mac can show and settle it without another run. A workspace note SHALL show the question at the top of every project's Inbox, counted in each. The skill's page SHALL show it too, and the Skills list SHALL mark the skill "Rewritten · needs you". For each passage the human SHALL be able to take the update's wording or keep their own. A choice SHALL apply at once when there is one passage, and with several, Apply SHALL apply them once each passage has one. Applying SHALL save the merge with a "Merged" entry. The note SHALL close when the human chooses on any Mac, or when the skill's rewrite or base changes in any other way. A missing model, a failed run or an overlap the model merged SHALL NOT raise a note.

#### Scenario: Two wordings that disagree
- **WHEN** v2 says to run the full suite once per ticket, the human's rewrite says never to run the full suite, and the model answers a question for that passage
- **THEN** the rewrite runs as written, an Inbox note "Reviewing · v2 needs you" shows the question with both wordings, and the Skills list shows "Rewritten · needs you"

#### Scenario: Keeping my wording
- **WHEN** the human chooses Keep my wording on that note
- **THEN** the merge applies with the human's passage and v2's other changes, the base becomes v2, the note closes in every project's Inbox, and no model runs

#### Scenario: Answered on another Mac
- **WHEN** both Macs show the note and the human answers it on one
- **THEN** the other Mac pulls the merged rewrite, and its note closes

#### Scenario: No second run
- **WHEN** a question is waiting and Wiffletree starts again
- **THEN** no merge run starts for that skill

### Requirement: A merge leaves a note that can be reviewed or undone
After a merge applies, the skill's page SHALL show a note naming the versions, how many changes came from the update, how many passages the model merged and with which model, with Review changes, Undo and Dismiss, and the Skills list SHALL mark the skill "Rewritten · updated". It SHALL NOT raise an Inbox note. Review changes SHALL show the rewrite before and after the merge, line by line, each change labelled as from the update, merged by the model, or chosen by the human, with the Mac and the time. Each Mac SHALL show the note until the human chooses Review changes or Dismiss on that Mac, or the skill changes again. Undo SHALL ask first, then write back the rewrite and base from before the merge exactly, with an "Undid the merge" entry. After an Undo, no automatic merge into that stock version SHALL run, and the page SHALL offer to merge it again. The merge's history entry SHALL offer Undo this merge after later changes, and it SHALL ask first and say the later changes stay in the history.

#### Scenario: After an automatic merge
- **WHEN** Coordinating was merged into v2 at start
- **THEN** its page shows "Merged Coordinating v2 into your rewrite" with Review changes, Undo and Dismiss, and the Skills list shows "Rewritten · updated"

#### Scenario: Undo
- **WHEN** the human chooses Undo and confirms
- **THEN** `rewrite.md` and `base.md` hold exactly the bytes they held before the merge, the coordinator's next turn has that rewrite, the skill is out of date with Merge v2 in offered, and no merge runs at the next start

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

## MODIFIED Requirements

### Requirement: A rewrite is marked out of date by a new stock version
A rewrite SHALL be out of date when its base is not this release's stock text, compared as text, unless its base's version is newer than this release's stock version. A rewrite whose base is newer SHALL be sent as written, SHALL NOT be marked out of date or merged, SHALL NOT offer Mark as up to date, and its page SHALL name the newer version it is based on and say that a Mac with a newer release brought it. An out-of-date rewrite SHALL keep being sent as written until a merge applies. The Skills page SHALL mark it, the sidebar's Skills row SHALL show a dot while any rewrite is out of date or needs the human, and its page SHALL show the base's and the stock's versions, what changed between them line by line, and where its merge stands: waiting for this launch's first sync, merging, failed with the reason, undone, or waiting for a model. The human SHALL be able to merge it on demand, mark it up to date, which records the current stock text as its base and changes nothing else, or reset it to stock. The host SHALL NOT change a rewrite's text when the stock text changes, except by a merge.

#### Scenario: A release changes Coordinating
- **WHEN** Coordinating is rewritten against v1, an update brings v2, and the merge has not applied yet
- **THEN** the coordinator keeps getting the rewrite as written, the Skills page shows "Rewritten · out of date", and the page shows what changed from v1 to v2 and where the merge stands

#### Scenario: Marking it up to date
- **WHEN** the human edits the rewrite to take in part of v2's change and chooses Mark as up to date
- **THEN** its base becomes v2, the mark and the dot go, and the rewrite is sent as saved

#### Scenario: A stock change without a new version
- **WHEN** a release changes a stock text and forgets to change its version
- **THEN** a rewrite based on the old text is still marked out of date

#### Scenario: A base from a newer release
- **WHEN** this Mac runs Coordinating v2 and pulls a rewrite merged against v3 on another Mac
- **THEN** the rewrite is sent as written, it is not marked out of date or merged, Mark as up to date is not offered, and its page says it is based on v3 from a Mac with a newer release

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

### Requirement: The Skills page
The sidebar's Workspace section SHALL have a Skills row after Models, opening the Skills page. The page SHALL list the four skills, each with what it is for, the roles that use it and its state ("Stock", "Stock and N of your rules", "Rewritten", "Rewritten · updated", "Rewritten · out of date", "Rewritten · needs you"), and the fixed core, marked read-only. A skill's page SHALL show its stock text, its mode and editor, Save and Discard, Reset to stock, its history, and what the app's own operations enforce whatever the text says. It SHALL NOT claim the app enforces more: it SHALL say that an agent's shell is not sandboxed and can still push with the human's credentials. The fixed core's page SHALL show its text and why it cannot be edited. The Reviewing page SHALL show each review level's "Use it" line read-only, with a link to the Models page, and the Coordinating page SHALL link to the model guide.

#### Scenario: Opening the page
- **WHEN** the human chooses Skills in the sidebar
- **THEN** the page lists Coordinating, Implementing, Reviewing and Keeping a PR with their states, and the fixed core marked read-only

#### Scenario: The fixed core
- **WHEN** the human opens the fixed core
- **THEN** its text is shown with no editor and no Reset

#### Scenario: Updated and needs you
- **WHEN** Coordinating was merged into v2 and its note is not dismissed, and Reviewing has a question waiting
- **THEN** the list shows Coordinating as "Rewritten · updated" and Reviewing as "Rewritten · needs you", and the sidebar's Skills row shows its dot for Reviewing

### Requirement: The limits of skills are documented
The README SHALL say that skills change how agents work but not what they may do, which the app enforces; that the human's rules win over the stock text; that a new release is merged into a rewrite automatically, with a model for overlapping passages, a note that can be reviewed or undone, and a question only when the two wordings disagree; and that an agent's shell can write the skills folder, as it can write the settings file.

#### Scenario: README
- **WHEN** a reader looks up skills in the README
- **THEN** it says where they are edited, that authority is not in them, how a new release reaches a rewrite and how to undo it, and that agents' shells can write their folder
