# settings-repository Specification

## Purpose
Keeps the settings the human chose to share in one folder on every machine, so a settings repository can carry them between machines.

## Requirements

### Requirement: Shared settings and settings that stay on this machine
The shared settings SHALL be: the skills, with the human's rules, rewrites, their bases and their history; the verification setting, with its verifiers, review levels, rounds per review and reviews per ticket; the model guide; the switch threshold; the two check-in thresholds; and the PR keeper's triggers, quiet period and review level. Every other setting SHALL stay on this machine, including the Who runs what table, the PR keeper's row among it, the workspace folder, the allowed owners, the settings repository chosen, the repositories and their checks commands, and appearance. Credentials, the database, conversations, projects and tickets SHALL NOT be shared, and the host SHALL NOT write them to the shared folder.

#### Scenario: What a second machine receives
- **WHEN** a machine joins a settings repository set up by another machine that allows `Sage-Social` and moves on at 90%
- **THEN** it gets the other machine's review levels, guide, switch threshold, check-in thresholds, keeper triggers and skills, and its own allowed owners, Who runs what table and workspace folder are unchanged

### Requirement: Shared settings live in one folder on every machine
The host SHALL keep the shared settings in `<data folder>/shared/`, with or without a settings repository: `wiffletree.json` naming the folder's format, `settings/review.json`, `settings/model-guide.md`, `settings/models.json`, `settings/check-ins.json`, `settings/pr-keeper.json`, and `skills/` in the skills layout. All reads and writes SHALL go through one store rooted at that folder, and writes SHALL be atomic. JSON files SHALL be written with sorted keys, and a write SHALL keep fields this release does not know. A missing file SHALL mean that setting's default. At the first start of this release, the host SHALL copy each shared setting that has no file yet from where it was saved, exactly as saved, and SHALL move `<data folder>/skills/` into the shared folder. It SHALL leave the old values in place, no longer read. Nothing SHALL create `logs/` or `tickets/`.

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

### Requirement: The human chooses the settings repository on each machine
The Settings window SHALL have a Settings repository section after Check-ins. The human SHALL name a repository on `github.com` as `owner/name` or as an HTTPS or SSH URL. Before anything is written, the host SHALL ask GitHub, read-only, for the repository's visibility, its owner and whether the owner is an organization, and an organization's base permission for members where GitHub reports it, and SHALL check with `git ls-remote` that this machine can read the repository. A public repository SHALL be refused. The section SHALL say who can read the repository: for a user's repository, only the user and the people they invite; for an organization whose base permission is Read or more, in yellow, that every member of that organization can read it; and where GitHub does not report the base permission, that members may be able to read it. Setting up a repository without `wiffletree.json` SHALL ask first, SHALL add `wiffletree.json`, `settings/` and `skills/` from this machine's shared folder in one commit on the repository's default branch, push it, and SHALL NOT change or remove any other file. The host SHALL NOT create repositories on GitHub. The choice, its push URL and its branch SHALL be saved in this machine's host settings and SHALL NOT be synced. Only the human's client SHALL change it; no agent tool SHALL.

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

#### Scenario: An agent cannot choose a repository
- **WHEN** any agent calls any workspace tool
- **THEN** no tool reads or changes the shared folder or the settings repository chosen

### Requirement: A second machine joins
Naming a repository that has `wiffletree.json` SHALL offer to join it. Before joining, the section SHALL list, for each shared setting, what the repository holds beside what this machine has, and the machine and time of the repository's last change. **Use the repository's settings** SHALL add this machine's skill history entries that the repository lacks, move this machine's shared folder to `<data folder>/backups/shared-<time>/`, and put a clone of the repository in its place. **Replace the repository's settings with this Mac's** SHALL ask first and SHALL push this machine's shared files over the repository's in one commit. After joining, the section SHALL list what this machine still needs to run the shared settings.

#### Scenario: Joining with the repository's settings
- **WHEN** a machine with stock skills joins a repository where Implementing has 3 of the human's rules
- **THEN** implementers on this machine get the 3 rules from their next turn, and the machine's previous shared folder is in `backups/`

#### Scenario: Both histories are kept
- **WHEN** both this machine and the repository have history entries for Coordinating, and the human uses the repository's settings
- **THEN** Coordinating's history holds the entries of both machines

#### Scenario: What this machine still needs
- **WHEN** the repository's keeper triggers are on and this machine has no PR keeper model
- **THEN** after joining, the section says the triggers are off on this machine until a PR keeper model is chosen in Models

### Requirement: Sync pushes only to the settings repository's branch
Choosing the settings repository on a machine SHALL be what allows the host to push to it, and the confirmation SHALL say so, naming the repository, its branch and who can read it. The allowed owners SHALL NOT grant or be needed for that push, and allowing an owner SHALL NOT allow sync to push to any repository of that owner. The host SHALL push only the recorded default branch, to the recorded push URL, with no tags and no submodules. It SHALL push only a commit that contains the remote branch's head, and SHALL NOT force a push. A commit SHALL hold changes only to `wiffletree.json`, `settings/` and `skills/`. The host SHALL push only files it would accept when reading them. Commits SHALL use the human's Git identity, and their messages SHALL name what changed and the machine.

#### Scenario: An owner allowed for pull requests
- **WHEN** `Sage-Social` is allowed and no settings repository is chosen
- **THEN** sync pushes nothing to any Sage-Social repository

#### Scenario: The remote moved first
- **WHEN** another machine pushed after this machine's last pull, and this machine pushes
- **THEN** GitHub refuses the push, nothing is overwritten, and the next pass pulls the other machine's commit before pushing again

#### Scenario: Other files in the repository
- **WHEN** the repository also has a `README.md` the human edits on GitHub
- **THEN** sync pulls the edit and never commits a change to `README.md`

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

### Requirement: A file changed outside Wiffletree is paused
A file in the shared folder whose content is neither what the host last wrote nor what it last synced SHALL be paused, with a workspace note saying it was changed outside Wiffletree on this machine. This machine SHALL use the file as it is. The note SHALL offer Sync this version, which pushes it if the host accepts it when reading it, and Put back the synced version.

#### Scenario: An agent's shell edits a skill
- **WHEN** a command in an agent's shell appends a line to `skills/implementing/additions.md`
- **THEN** the file is not pushed, and a workspace note says Implementing · your rules was changed outside Wiffletree on this machine

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

### Requirement: Sync status is shown
The Settings repository section SHALL show the repository, its branch, who can read it, the time of the last sync and what it pulled and pushed, when the next pull runs, the last error in Git's words, and the paused files, with Sync now and Stop syncing. The sidebar footer SHALL show a sync mark only while a repository is chosen: synced, syncing, waiting to reach GitHub, or paused. It SHALL be yellow while a file is paused or sync has stopped, and red after an error that has lasted more than an hour. Clicking it SHALL open the Settings window at the section.

#### Scenario: A paused file
- **WHEN** one file is paused
- **THEN** the sidebar's sync mark is yellow, and the section lists the file and why it is paused

#### Scenario: GitHub cannot be reached
- **WHEN** the last three passes could not reach GitHub
- **THEN** the section shows Git's reason, how many changes wait to be pushed, and when the next try runs

### Requirement: Stopping sync
Stop syncing SHALL ask first. It SHALL leave the shared folder as it is and keep using it, stop pulling and pushing, remove the choice from this machine's host settings, and close the workspace notes about paused files. It SHALL change nothing on GitHub.

#### Scenario: Stop syncing
- **WHEN** the human stops syncing and confirms
- **THEN** every setting keeps its current value, nothing is pulled or pushed, and the repository on GitHub is unchanged

### Requirement: The limits of the settings repository are documented
The README SHALL say which settings are shared and which stay on each machine; that choosing the repository is what allows Wiffletree to push to it; that whoever can push to the repository can change every machine's agents' instructions; that an organization's members may be able to read a private repository; and that an agent's shell can write the shared folder and push to the repository with the human's credentials, which sync does not prevent but does not spread.

#### Scenario: README
- **WHEN** a reader looks up the settings repository in the README
- **THEN** it lists what is shared, says who can change the agents' instructions through it, and says what agents' shells can still do
