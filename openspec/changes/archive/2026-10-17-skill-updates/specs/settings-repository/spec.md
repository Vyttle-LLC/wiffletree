## ADDED Requirements

### Requirement: A skill update is merged once for all machines
With a settings repository chosen, the merges the host checks for at start SHALL wait until this launch's first sync pass ends, whether it reached GitHub or not. When a pass finds that both this machine and GitHub changed a skill's files since this machine last synced them, and this machine's only change to them is its own automatic merge, the pass SHALL write GitHub's version here without pausing the skill and without a workspace note, SHALL keep both machines' history entries, and the host SHALL then check the skill again for a due merge. When this machine also changed the skill in any other way, the skill SHALL pause as before. A question about a merge SHALL reach every machine through the skill's history, and a machine whose stock text is the one the question merges into SHALL NOT run a model for that merge. Sync's commit of a merge SHALL name the skill, the stock version and the machine.

#### Scenario: One Mac merges first
- **WHEN** two Macs update to Coordinating v2, one starts and pushes its merge, and the other starts later
- **THEN** the second Mac's first pass pulls that merge, the second Mac merges nothing and runs no model, and both show the merge's note

#### Scenario: Both merge at once
- **WHEN** two Macs on v2 merge Coordinating before either has pulled, and one push reaches GitHub first
- **THEN** the other Mac takes GitHub's version at its next pass, the skill is not paused, no workspace note appears, and the history holds both merges

#### Scenario: A Mac with a newer release
- **WHEN** a Mac on v3 pulls a rewrite merged against v2 by a Mac on v2
- **THEN** the Mac on v3 merges v2 → v3 into it and pushes that, and the Mac on v2 then sends that rewrite as written without marking it out of date

#### Scenario: Offline at start
- **WHEN** a Mac cannot reach GitHub at launch and Coordinating is due a merge
- **THEN** it merges after its first pass fails, and when it reconnects after another Mac pushed its own merge, it takes that merge without a pause

#### Scenario: An edit after the merge
- **WHEN** this Mac merged Coordinating and the human then saved the rewrite before it was pushed, and another Mac pushed its own merge meanwhile
- **THEN** the skill pauses with a workspace note, as for any file changed on two machines

## MODIFIED Requirements

### Requirement: A file changed on two machines is paused
When a pass finds a file that this machine and the remote both changed, to different content, since this machine last synced it, the host SHALL pause that file: this machine SHALL keep using its own version, the host SHALL NOT push it or write the remote's version over it, and every other file SHALL keep syncing. A skill whose only change on this machine since it last synced is this machine's own automatic merge SHALL NOT pause: the pass SHALL take the remote's version, as "A skill update is merged once for all machines" says. The host SHALL raise one workspace note naming the setting and the machines that changed it. The note SHALL be shown at the top of every project's Inbox with a "Workspace" label, counted in each, and settled once for all. It SHALL offer Compare, showing both versions as a line diff; Keep this Mac's, which pushes this machine's version; and Use the other machine's, which writes the remote's version here. A skill's `additions.md`, `rewrite.md` and `base.md` SHALL be compared and paused as one unit: when any of them would pause, all three SHALL pause, and this machine SHALL keep using its own three, while the skill's history entries keep syncing. While paused, later edits here SHALL stay here, and a newer remote version SHALL replace the one compared against. The note SHALL close when the human chooses a side or when the two versions become equal.

#### Scenario: Both machines edit Implementing's rules
- **WHEN** the human saves different rules for Implementing on two machines before either syncs
- **THEN** each machine keeps using its own rules, the second to sync shows a workspace note naming Implementing · your rules and both machines, and the other shared settings keep syncing

#### Scenario: A rewrite on one machine, new rules on the other
- **WHEN** Implementing is in Additions mode on both machines, one machine starts a rewrite and syncs, and the other saves new rules before it pulls
- **THEN** the second machine keeps Implementing in Additions mode with its own new rules, does not take the rewrite or its base, and shows one workspace note for Implementing

#### Scenario: Keeping this Mac's
- **WHEN** the human chooses Keep this Mac's on that note
- **THEN** this machine's rules are pushed, the other machine gets them at its next pull, the note closes in every project's Inbox, and the other version stays in the repository's history

#### Scenario: Only this Mac's own merge
- **WHEN** both machines merged Coordinating into v2 with different results before either pulled, and the other machine pushed first
- **THEN** this machine takes the other machine's version at its next pass, and Coordinating is not paused
