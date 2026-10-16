# settings-repository Specification

## Purpose
Keeps the settings the human chose to share in one folder on every machine, so a settings repository can carry them between machines.

## Requirements

### Requirement: Shared settings and settings that stay on this machine
The shared settings SHALL be: the skills, with the human's rules, rewrites, their bases and their history; the verification setting, with its verifiers, review levels, rounds per review and reviews per ticket; the model guide; the two check-in thresholds; and the PR keeper's triggers, quiet period and review level. Every other setting SHALL stay on this machine, including the providers and their allowed models, the providers per role, the PR keeper's model, the workspace folder, the allowed owners, the settings repository chosen, the repositories and their checks commands, and appearance. Credentials, the database, conversations, projects and tickets SHALL NOT be shared, and the host SHALL NOT write them to the shared folder.

#### Scenario: What a second machine receives
- **WHEN** a machine joins a settings repository set up by another machine that allows `Sage-Social` and has Codex enabled
- **THEN** it gets the other machine's review levels, guide, check-in thresholds, keeper triggers and skills, and its own allowed owners, providers, models and workspace folder are unchanged

### Requirement: Shared settings live in one folder on every machine
The host SHALL keep the shared settings in `<data folder>/shared/`, with or without a settings repository: `wiffletree.json` naming the folder's format, `settings/review.json`, `settings/model-guide.md`, `settings/check-ins.json`, `settings/pr-keeper.json`, and `skills/` in the skills layout. All reads and writes SHALL go through one store rooted at that folder, and writes SHALL be atomic. JSON files SHALL be written with sorted keys, and a write SHALL keep fields this release does not know. A missing file SHALL mean that setting's default. At the first start of this release, the host SHALL copy each shared setting that has no file yet from where it was saved, exactly as saved, and SHALL move `<data folder>/skills/` into the shared folder. It SHALL leave the old values in place, no longer read. Nothing SHALL create `logs/` or `tickets/`.

#### Scenario: An update
- **WHEN** Wiffletree starts after an update on a machine whose `settings.json` has check-in thresholds 10 and 300, a keeper with the rebase trigger on, and a verification setting saved before review levels
- **THEN** `settings/check-ins.json` holds 10 and 300, `settings/pr-keeper.json` has the rebase trigger on, `settings/review.json` holds the verifiers and caps without levels and is read as one level "Full", and nothing behaves differently

#### Scenario: The guide moves
- **WHEN** Wiffletree starts after an update on a machine with a saved guide
- **THEN** `settings/model-guide.md` holds the guide exactly as saved, and the coordinator's `workspace_context` shows it unchanged

#### Scenario: A fresh install
- **WHEN** Wiffletree starts for the first time
- **THEN** no shared setting has a file, and every shared setting has its default

#### Scenario: An interrupted move
- **WHEN** the move stopped after writing `settings/review.json` and before the other files
- **THEN** the next start writes the missing files and leaves `settings/review.json` as it is

### Requirement: Sync runs automatically
With a settings repository chosen, the host SHALL pull at launch, every 5 minutes and when the human chooses Sync now, and SHALL push 5 seconds after the last change to a shared setting, so that a burst of saves is one commit. A pass SHALL run off the thread that serves host commands. When GitHub cannot be reached, passes SHALL back off up to 30 minutes, Sync now SHALL try at once, and this machine SHALL keep using its own changes until they are pushed. A pass SHALL compare each file whole against the version this machine last synced: a file changed only here SHALL be pushed, a file changed only on the remote SHALL be written here, and a file changed on both sides to the same content SHALL need nothing. Pulled changes SHALL apply where each setting is next read.

#### Scenario: A rule saved on one machine
- **WHEN** the human saves a rule for Implementing on one machine, and the other machine pulls 5 minutes later
- **THEN** the first machine pushed one commit naming Implementing and itself, and implementers on the second machine get the rule from their next turn

#### Scenario: Offline
- **WHEN** GitHub cannot be reached and the human changes the check-in thresholds
- **THEN** the new thresholds apply on this machine at once, the status says one change waits to be pushed, and it is pushed at the first pass that reaches GitHub

#### Scenario: Different settings on two machines
- **WHEN** one machine changes the review levels and the other the check-in thresholds before either pulls
- **THEN** both changes reach both machines and nothing is paused

### Requirement: What this machine cannot use is held, not refused
A pulled file the host cannot read, such as invalid JSON, a value out of range or a text over its limit, SHALL NOT be applied. This machine SHALL keep its last good version, and the file SHALL be paused with the reason. Its workspace note SHALL offer Compare and Keep this Mac's, which pushes this machine's version, and SHALL close when a version this machine can read arrives. When `wiffletree.json` names a format this release does not know, sync SHALL stop on this machine, saying to update Wiffletree, and SHALL pull and push nothing until then. A pulled setting that this machine cannot run with its own settings SHALL NOT be rewritten. Keeper triggers that need a PR keeper model this machine does not have, or a review level the review settings no longer have, SHALL be off on this machine, and the Repositories page SHALL say they are on from the other machine and what this machine needs. The checks that tie the keeper's triggers to the keeper's model and to a review level SHALL count a trigger held off on this machine as off, for every save on this machine, unless that save turns the trigger on. A trigger this machine runs SHALL be checked as before.

#### Scenario: Triggers without a keeper model
- **WHEN** another machine turns on "Try to resolve conflicts" and this machine has no PR keeper model
- **THEN** the keeper tries nothing on this machine, the Repositories page says the trigger is on from the other machine and needs a PR keeper model here, and the trigger runs once one is chosen

#### Scenario: A joined machine saves its first setup
- **WHEN** a machine with no saved model selection joins a repository whose `settings/pr-keeper.json` has "Try to resolve conflicts" on, and the human saves the suggested setup without a PR keeper model
- **THEN** the setup is saved, the trigger is held off on this machine, and `settings/pr-keeper.json` is unchanged

#### Scenario: Turning a held trigger on here
- **WHEN** "Try to fix failed checks" is held off on this machine because it has no PR keeper model, and the human turns it on in Repositories
- **THEN** the save is refused as before, naming the missing PR keeper model

#### Scenario: An invalid file arrives
- **WHEN** a pulled `settings/review.json` has two verifiers with the focus Tests
- **THEN** this machine keeps its previous review settings, and the file is paused with that reason

#### Scenario: A newer format
- **WHEN** `wiffletree.json` names format 2 and this release knows format 1
- **THEN** sync stops on this machine, saying to update Wiffletree, and nothing is pulled or pushed
