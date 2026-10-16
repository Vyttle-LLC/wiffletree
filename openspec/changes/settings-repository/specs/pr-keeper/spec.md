## MODIFIED Requirements

### Requirement: Triggers are off until the human turns them on
The host SHALL keep the keeper's settings in the shared folder's `settings/pr-keeper.json` (see settings-repository): one switch per trigger, a quiet period in minutes and the keeper's review level. Every trigger SHALL be off until the human turns it on, on this machine or on another that shares the settings repository, also after an update and on a fresh install, and with every trigger off the host SHALL rebase, try, sort and push nothing for the keeper. The quiet period SHALL default to 2 and SHALL accept 1 to 60. The settings SHALL apply to the whole workspace; there SHALL be no per-owner or per-repository trigger. Only the human's client SHALL be able to change them; no agent tool SHALL. A save that breaks a rule SHALL be refused and the previous settings SHALL stay in effect; a trigger held off on this machine SHALL count as off in that check unless the save turns it on (see settings-repository). A trigger pulled from another machine that this machine cannot run, because it has no PR keeper model or the review level is gone, SHALL be off on this machine without the file being changed, and the Repositories page SHALL say why.

#### Scenario: After an update
- **WHEN** the app is updated, or started for the first time, and tickets in allowed owners' repositories have open PRs behind their bases
- **THEN** every trigger is as it was saved before the update, off unless the human turned it on, and with every trigger off nothing is rebased, tried, sorted or pushed

#### Scenario: An agent cannot turn a trigger on
- **WHEN** any agent calls any workspace tool
- **THEN** no tool changes the keeper's settings

#### Scenario: Quiet period out of range
- **WHEN** the human saves a quiet period of 0 or 61 minutes
- **THEN** the save is refused and the previous settings stay in effect

#### Scenario: Turned on from another machine without a keeper model here
- **WHEN** a pass pulls `settings/pr-keeper.json` with "Try to fix failed checks" on, and this machine has no PR keeper model
- **THEN** the keeper does not try failed checks on this machine, `settings/pr-keeper.json` is unchanged, and the Repositories page says the trigger is on from the other machine and needs a PR keeper model here
