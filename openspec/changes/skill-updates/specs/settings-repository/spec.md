## ADDED Requirements

### Requirement: A skill update is merged once for all machines
With a settings repository chosen, the merges the host checks for at start SHALL wait until this launch's first sync pass ends, whether it reached GitHub or not. When a pass finds that both this machine and GitHub changed a skill's files since this machine last synced them, and this machine's only change to them is its own automatic merge, the pass SHALL write GitHub's version here without pausing the skill and without a workspace note, SHALL keep both machines' history entries, and the host SHALL then check the skill again for a due merge. When this machine also changed the skill in any other way, the skill SHALL pause as before. A question about a merge SHALL reach every machine through the skill's history, and a machine that has it SHALL NOT run a model for that merge. Sync's commit of a merge SHALL name the skill, the stock version and the machine.

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
