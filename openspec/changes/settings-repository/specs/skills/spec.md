## MODIFIED Requirements

### Requirement: Skills are stored in a folder that can move
The host SHALL keep skills in the `skills` folder of the shared folder (see settings-repository), one folder per skill, holding the human's rules, the rewrite and its base as plain text files, and one file per history entry. All reads and writes SHALL go through one store rooted at that folder, and writes SHALL be atomic. A missing file SHALL mean no state for it. A file the host cannot read SHALL count as missing for the instructions, and the skill's page SHALL say it could not be read. Credentials, the database and conversations SHALL NOT be stored there. A skills folder from before the shared folder SHALL be moved into it unchanged at start.

#### Scenario: A file that cannot be read
- **WHEN** Implementing's rules file is not valid UTF-8
- **THEN** implementers get the stock text with no rules, and Implementing's page says its file could not be read

#### Scenario: Skills saved before the shared folder
- **WHEN** Wiffletree starts after an update with rules for Implementing in `<data folder>/skills/`
- **THEN** they are in the shared folder's `skills/implementing/`, the history is unchanged, and implementers keep getting the rules

### Requirement: Every skill has a history
Each change to a skill SHALL write a history entry holding the skill's whole state after it, the stock version, the time and what happened. At start, the host SHALL write a "New stock version" entry for each skill with a history whose recorded stock versions are all older than this release's. A machine on an older release SHALL write no such entry, and the skill's page SHALL say that another machine runs a newer stock version. The skill's page SHALL list the entries newest first, each viewable read-only and restorable. Restoring SHALL make the entry's state current and write a new entry, and SHALL delete none. Restoring a rewrite SHALL restore its base, so it may be out of date at once.

#### Scenario: Restoring earlier rules
- **WHEN** the human restores the entry from before a reset
- **THEN** those rules apply from each agent's next turn, and the history keeps the reset and gains a "Restored" entry

#### Scenario: Restoring an old rewrite
- **WHEN** the human restores a rewrite saved against v1 while this release has v2
- **THEN** the rewrite is current and marked out of date

#### Scenario: An update
- **WHEN** Wiffletree starts after an update that changed Reviewing's stock version, and the human had edited Reviewing before
- **THEN** Reviewing's history gains one "New stock version" entry naming both versions

#### Scenario: Two machines on different releases
- **WHEN** one machine has Coordinating v2 and wrote its "New stock version" entry, and another machine sharing the settings repository still has v1 and starts
- **THEN** the second machine writes no history entry, and Coordinating's page there says another machine runs v2
