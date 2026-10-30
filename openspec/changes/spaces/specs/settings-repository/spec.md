## MODIFIED Requirements

<!-- Delta order for both requirements: 1 spaces (this) → 2 assistant-spaces-jobs-boards, which restates them with the assistant's settings. -->

### Requirement: Shared settings and settings that stay on this machine
The shared settings SHALL be: the skills, with the human's rules, rewrites, their bases and their history; the verification setting, with its verifiers, review levels, rounds per review and reviews per ticket; the model guide; the switch threshold; the two check-in thresholds; and the PR keeper's triggers, quiet period and review level. Every other setting SHALL stay on this machine, including the Who runs what table, the PR keeper's row among it, the workspace folder, the allowed owners, the settings repository chosen, the repositories and their checks commands, appearance, and the Spaces added on this machine and their machine-local facts. Space definitions and issues SHALL travel only through each Space's storage repository (see `spaces`), never as shared settings. Credentials, the database, conversations, projects and tickets SHALL NOT be shared, and the host SHALL NOT write them to the shared folder.

#### Scenario: What a second machine receives
- **WHEN** a machine joins a settings repository set up by another machine that allows `Sage-Social` and moves on at 90%
- **THEN** it gets the other machine's review levels, guide, switch threshold, check-in thresholds, keeper triggers and skills, and its own allowed owners, Who runs what table, workspace folder and Spaces are unchanged

### Requirement: Shared settings live in one folder on every machine
The host SHALL keep the shared settings in one shared folder: the settings repository's clone at `<data folder>/repos/<owner>-<name>/` when one is chosen (see `shared-repositories`), and `<data folder>/shared/` otherwise. The folder SHALL hold `wiffletree.json` naming the folder's format, `settings/review.json`, `settings/model-guide.md`, `settings/models.json`, `settings/check-ins.json`, `settings/pr-keeper.json`, and `skills/` in the skills layout. All reads and writes SHALL go through one store rooted at that folder, and writes SHALL be atomic. JSON files SHALL be written with sorted keys, and a write SHALL keep fields this release does not know. A missing file SHALL mean that setting's default. At the first start of the release that introduced the shared folder, the host SHALL copy each shared setting that has no file yet from where it was saved, exactly as saved, and SHALL move `<data folder>/skills/` into the shared folder. It SHALL leave the old values in place, no longer read. Nothing SHALL create `logs/` or `tickets/`.

#### Scenario: An update
- **WHEN** Wiffletree starts after an update on a machine whose `settings.json` has check-in thresholds 10 and 300, a keeper with the rebase trigger on, and a verification setting saved before review levels
- **THEN** `settings/check-ins.json` holds 10 and 300, `settings/pr-keeper.json` has the rebase trigger on, `settings/review.json` holds the verifiers and caps without levels and is read as one level "Full", and nothing behaves differently

#### Scenario: The guide moves
- **WHEN** Wiffletree starts after an update on a machine with a saved guide
- **THEN** `settings/model-guide.md` holds the guide exactly as saved, and the coordinator's `workspace_context` shows it unchanged

#### Scenario: A fresh install
- **WHEN** Wiffletree starts for the first time
- **THEN** no shared setting has a file, and every shared setting has its default, a switch threshold of 95 among them

#### Scenario: An interrupted move
- **WHEN** the move stopped after writing `settings/review.json` and before the other files
- **THEN** the next start writes the missing files and leaves `settings/review.json` as it is

#### Scenario: A settings repository
- **WHEN** a machine uses `MHohlios/wiffletree-storage` as its settings repository
- **THEN** its shared folder is `<data folder>/repos/MHohlios-wiffletree-storage/`, and no shared setting is read from `<data folder>/shared/`
