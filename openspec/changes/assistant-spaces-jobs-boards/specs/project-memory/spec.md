## MODIFIED Requirements

### Requirement: Safe Markdown export
Maintenance SHALL export a project's entries only to its Space's brain, and an assistant thread's entries only to its own Space's brain (see spaces), preserve existing content and compiler markers, use stable entry markers and recover an interrupted export without duplication. A project whose Space has no brain SHALL export nothing. Moving a project to another Space SHALL change where later entries go and SHALL NOT move or re-export earlier ones. Unsafe repository log filenames SHALL be rejected.

#### Scenario: Write before receipt
- **WHEN** Markdown already contains an entry marker but its export receipt is missing
- **THEN** retry records the receipt without adding another copy

#### Scenario: The Space's brain
- **WHEN** a project in the Vyttle Space exports a decision
- **THEN** the entry is written to the Vyttle Space's brain folder, and to no other brain

#### Scenario: Upgraded project
- **WHEN** a project that had a brain attached before Spaces exports after the upgrade
- **THEN** it exports to the same folder through its Space, and entries already exported are not written again

## ADDED Requirements

### Requirement: Brain pushes from several people
When a Space's brain is a repository that other people also push to, the brain writer SHALL fetch and rebase before each push. A log file changed on both sides SHALL merge as the union of its entries by entry ID, in the file's newest-first order. An entry ID present on both sides with different content, or a change outside entries on both sides, SHALL be a true conflict: the push SHALL pause and a workspace note SHALL offer Compare and the settings repository's choices. The writer SHALL NOT force a push. Every entry written SHALL carry a stable ID.

#### Scenario: Two people log the same day
- **WHEN** two people's Macs each add an entry to `logs/mobile-app.md` before either pulls
- **THEN** the second push rebases, the file holds both entries once each, and nothing is forced

#### Scenario: A true conflict
- **WHEN** both sides changed the same entry ID's text
- **THEN** the push pauses with a workspace note offering Compare, and neither version is lost
