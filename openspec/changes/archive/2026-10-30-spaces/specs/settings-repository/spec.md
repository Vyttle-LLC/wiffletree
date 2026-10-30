## MODIFIED Requirements

<!-- Delta order for both requirements: 1 spaces (this) → 2 assistant-spaces-jobs-boards, which restates them with the assistant's settings. -->

### Requirement: Shared settings and settings that stay on this machine
The shared settings SHALL be: the skills, with the human's rules, rewrites, their bases and their history; the verification setting, with its verifiers, review levels, rounds per review and reviews per ticket; the model guide; the switch threshold; the two check-in thresholds; and the PR keeper's triggers, quiet period and review level. Every other setting SHALL stay on this machine, including the Who runs what table, the PR keeper's row among it, the workspace folder, the allowed owners, the settings repository chosen, the repositories and their checks commands, appearance, and the Spaces added on this machine and their machine-local facts. Space definitions and issues SHALL travel only through each Space's storage repository (see `spaces`), never as shared settings. Credentials, the database, conversations, projects and tickets SHALL NOT be shared, and the host SHALL NOT write them to the shared folder.

#### Scenario: What a second machine receives
- **WHEN** a machine joins a settings repository set up by another machine that allows `Sage-Social` and moves on at 90%
- **THEN** it gets the other machine's review levels, guide, switch threshold, check-in thresholds, keeper triggers and skills, and its own allowed owners, Who runs what table, workspace folder and Spaces are unchanged

### Requirement: Shared settings live in one folder on every machine
The host SHALL keep the shared settings in one shared folder: the settings repository's clone at `<data folder>/repos/github.com/<owner>/<name>/` when one is chosen (see `shared-repositories`), and `<data folder>/shared/` otherwise. The folder SHALL hold `wiffletree.json` naming the folder's format, `settings/review.json`, `settings/model-guide.md`, `settings/models.json`, `settings/check-ins.json`, `settings/pr-keeper.json`, and `skills/` in the skills layout. All reads and writes SHALL go through one store rooted at that folder, and writes SHALL be atomic. JSON files SHALL be written with sorted keys, and a write SHALL keep fields this release does not know. A missing file SHALL mean that setting's default. At the first start of the release that introduced the shared folder, the host SHALL copy each shared setting that has no file yet from where it was saved, exactly as saved, and SHALL move `<data folder>/skills/` into the shared folder. It SHALL leave the old values in place, no longer read. Nothing SHALL create `logs/` or `tickets/`.

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
- **THEN** its shared folder is `<data folder>/repos/github.com/MHohlios/wiffletree-storage/`, and no shared setting is read from `<data folder>/shared/`

<!-- Delta order for the three requirements below: 1 spaces (this) is the only delta. They move the shared folder between <data folder>/shared/ and the clone under repos/ so that setting up, joining and stopping keep every setting. -->

### Requirement: The human chooses the settings repository on each machine
The Settings window SHALL have a Settings repository section after Check-ins. The human SHALL name a repository on `github.com` as `owner/name` or as an HTTPS or SSH URL. Before anything is written, the host SHALL ask GitHub, read-only, for the repository's visibility, its owner and whether the owner is an organization, and an organization's base permission for members where GitHub reports it, and SHALL check with `git ls-remote` that this machine can read the repository. A public repository SHALL be refused, and so SHALL a repository that is a brain on this Mac (see `shared-repositories`). The section SHALL say who can read the repository: for a user's repository, only the user and the people they invite; for an organization whose base permission is Read or more, in yellow, that every member of that organization can read it; and where GitHub does not report the base permission, that members may be able to read it. Setting up a repository without `wiffletree.json` SHALL ask first, SHALL add `wiffletree.json`, `settings/` and `skills/` from this machine's shared folder in one commit on the repository's default branch, push it, and SHALL NOT change or remove any other file. The commit SHALL be made in the repository's clone at `<data folder>/repos/github.com/<owner>/<name>/`, reusing it when the repository is already a Space's storage; once it is pushed, that clone SHALL be the shared folder and the settings files of `<data folder>/shared/` SHALL move to `<data folder>/backups/shared-<time>/`. When another settings repository is chosen, setting up SHALL first run the Stop syncing transition for it, as joining does, and no clone SHALL be moved to `backups/`. The host SHALL NOT create repositories on GitHub. The choice, its push URL and its branch SHALL be saved in this machine's host settings and SHALL NOT be synced. Only the human's client SHALL change it; no agent tool SHALL.

#### Scenario: A user's private repository
- **WHEN** the human checks `MHohlios/wiffletree-storage`, a private repository of the user MHohlios
- **THEN** the section says only MHohlios and the people they invite can read it, and offers Set up

#### Scenario: An organization that lets members read
- **WHEN** the human checks `Sage-Social/wiffletree-storage` and GitHub reports Sage-Social's base permission as Read
- **THEN** the section says in yellow that every member of Sage-Social can read it

#### Scenario: A public repository
- **WHEN** the human checks a public repository
- **THEN** it is refused, saying the skills and settings would be public, and nothing is written

#### Scenario: Setting up beside a README
- **WHEN** the human sets up a repository whose only commit adds `README.md`
- **THEN** one commit adds `wiffletree.json`, `settings/` and `skills/`, `README.md` is unchanged, and the commit is pushed to the default branch

#### Scenario: Setting up in a Space's storage
- **WHEN** the human sets up `Sage-Social/wiffletree-storage`, already the Sage Space's storage, as the settings repository
- **THEN** the settings files are committed in the existing clone under `repos/`, that clone becomes the shared folder with every setting's value unchanged, and the old `<data folder>/shared/` is in `backups/`

#### Scenario: An agent cannot choose a repository
- **WHEN** any agent calls any workspace tool
- **THEN** no tool reads or changes the shared folder or the settings repository chosen

### Requirement: A second machine joins
Naming a repository that has `wiffletree.json` SHALL offer to join it. Before joining, the section SHALL list, for each shared setting, what the repository holds beside what this machine has, and the machine and time of the repository's last change. When another settings repository is chosen, joining SHALL first run the Stop syncing transition for it (see "Stopping sync"), so the shared folder is `<data folder>/shared/` and the other repository's clone stays at its path, still syncing its Space and issue files when it is storage. **Use the repository's settings** SHALL add this machine's skill history entries that the repository lacks, move the settings files of `<data folder>/shared/` (`wiffletree.json`, `settings/` and `skills/`) to `<data folder>/backups/shared-<time>/`, and make the repository's clone at `<data folder>/repos/github.com/<owner>/<name>/` the shared folder, cloning it there, or reusing it when the repository is already a Space's storage. **Replace the repository's settings with this Mac's** SHALL ask first, SHALL push the settings files of `<data folder>/shared/` over the repository's in one commit made in that clone, and SHALL then make that clone the shared folder and move those settings files to `backups/` in the same way. Only settings files SHALL move to `backups/`; no clone SHALL ever be moved there. After joining, the section SHALL list what this machine still needs to run the shared settings.

#### Scenario: Joining with the repository's settings
- **WHEN** a machine with stock skills joins a repository where Implementing has 3 of the human's rules
- **THEN** implementers on this machine get the 3 rules from their next turn, the shared folder is the clone under `repos/`, and the machine's previous shared folder is in `backups/`

#### Scenario: Switching from a repository that is also storage
- **WHEN** `Sage-Social/wiffletree-storage` is both this Mac's settings repository and the Sage Space's storage, and the human joins `MHohlios/wiffletree-settings` with Use the repository's settings
- **THEN** the settings files are first copied to `<data folder>/shared/` and then moved to `backups/`, the clone `<data folder>/repos/github.com/Sage-Social/wiffletree-storage/` stays where it is with Sage's Space files and issues still syncing, and the shared folder is `<data folder>/repos/github.com/MHohlios/wiffletree-settings/`

#### Scenario: Both histories are kept
- **WHEN** both this machine and the repository have history entries for Coordinating, and the human uses the repository's settings
- **THEN** Coordinating's history holds the entries of both machines

#### Scenario: What this machine still needs
- **WHEN** the repository's keeper triggers are on and this machine has no PR keeper model
- **THEN** after joining, the section says the triggers are off on this machine until a PR keeper model is chosen in Models

### Requirement: Stopping sync
Stop syncing SHALL ask first. It SHALL copy `wiffletree.json`, `settings/` and `skills/` from the settings repository's clone to `<data folder>/shared/`, exactly as they are, and use that folder as the shared folder from then on, so every setting keeps its value. It SHALL stop the settings rule set's pulls and pushes, remove the choice from this machine's host settings, and close the workspace notes about paused settings files. The clone SHALL stay where it is: when the repository is also a Space's storage, its Space and issue rule sets SHALL keep syncing, and otherwise nothing SHALL sync it. It SHALL change nothing on GitHub.

#### Scenario: Stop syncing
- **WHEN** the human stops syncing and confirms
- **THEN** every setting keeps its current value, read from `<data folder>/shared/`, no settings file is pulled or pushed, and the repository on GitHub is unchanged

#### Scenario: Stop syncing a repository that is also storage
- **WHEN** the human stops syncing `Sage-Social/wiffletree-storage`, which is also the Sage Space's storage
- **THEN** the settings keep their values from `<data folder>/shared/`, and Sage's Space files and issues in the same clone keep syncing
