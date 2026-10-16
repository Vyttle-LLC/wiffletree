## Context

See proposal.md for why. The code this builds on, at origin/main e4972d98:

- **Host settings.** `HostSettings` (`workspace-core/src/lib.rs:694`) is read from `settings.json` in the data folder by `Host::settings` (`settings.rs:103`) and written atomically by `save_settings` (`:245`), which keeps fields this version does not know. It holds the workspace folder, the verification setting (verifiers, review levels, rounds per review and reviews per ticket, `VerificationSettings`, `lib.rs:876`), the two check-in thresholds, the allowed owners and the PR keeper's settings (`KeeperSettings`, `lib.rs:754`). Each has its own save: `set_verification` (`:110`), `set_check_ins` (`:147`), `set_keeper` (`:159`), `set_workspaces_dir` (`:195`) and `set_owner_allowed` (`:217`). Each refuses an invalid value and keeps the saved one. `check_tries` (`lib.rs:816`) and `check_sorting_model` (`lib.rs:855`) tie the keeper's triggers to the keeper's model and to a re-check level.
- **The model selection.** `ModelSelection` (`selection.rs:47`) is one row of the database (`policies.rs:25`, saved by `set_model_selection`, `:33`). It holds the providers and their allowed models, the providers per role, the PR keeper's model and the model guide (`guide`, `selection.rs:53`), saved together at a new revision.
- **Where the human edits them.** The Settings window (`workspace-desktop/src/settings.rs`) has Appearance, Workspace folder (`:164`) and Check-ins (`:204`). The Models page has the providers, the guide, the Review card and the PR keeper's model. The Repositories page has the allowed owners and the keeper's triggers. Appearance is this client's own preference, in `desktop-preferences.json`.
- **Skills (change 4, `openspec/changes/skills`).** Being built alongside this change. Its decision 9 keeps skills in `<data folder>/skills/`, one folder per skill with `additions.md`, `rewrite.md`, `base.md` and one `history/<time>.json` per change, all through one `SkillStore` rooted at one folder, "so change 5 can move that folder into the settings repository unchanged". Its open question 10 left edits made outside Wiffletree to this change.
- **Publishing (change 1).** `publish_ticket` is the only way Wiffletree pushes. It pushes exactly one ref, with `--no-follow-tags --recurse-submodules=no`, to the push URL recorded in the request (`publish.rs:269`), and never overwrites commits it did not push. The allowed owners (`repository-owners`) decide which ticket PRs open without asking. The list stays on each machine.
- **The Inbox.** Inbox items (`Attention`, `lib.rs:387`) belong to a project and a session. Nothing today belongs to the whole workspace.
- **Background passes.** The PR watcher (`pr_watch.rs`) runs read-only passes off the host's command thread on a timer, with backoff. Git runs through `runtime::git_output` and `git_push` (`runtime.rs:93`, `:139`).
- **The repository today.** `Sage-Social/wiffletree-storage` exists, private, with one commit, cloned at `~/dev/sagehaven/wiffletree-storage`. This change does not touch it.

## Words

- **Shared settings**: the settings that follow the human from machine to machine. They are listed in decision 1.
- **The shared folder**: `<data folder>/shared/`, where shared settings live on each machine, with or without a settings repository.
- **The settings repository**: a private GitHub repository the human names on each machine. Wiffletree keeps the shared folder in sync with it. The human calls theirs `wiffletree-storage`.
- **Sync**: pulling the settings repository's changes into the shared folder and pushing this machine's changes to it.
- **Paused file**: a file sync leaves alone until the human decides, because two machines changed it, or it was changed outside Wiffletree, or this machine cannot read what arrived.
- **This Mac** and **the other machine**: how the app names the two sides. A machine is named by its computer name.

## Goals / Non-Goals

**Goals:** the settings the human chose to share follow them to every machine without a step; nothing per machine or secret leaves the machine; nothing either machine wrote is lost; a machine keeps working while offline or paused; the only new push is Wiffletree's own, to one branch of one repository the human named on that machine; each slice small and useful alone.

**Non-Goals:**
- Second-brain logs, tickets or anything else in the repository. The layout leaves room for them; nothing is built.
- Merging two machines' edits of the same file. The file pauses and the human picks.
- Sharing between people. One human, several machines.
- Creating repositories on GitHub, or changing who can read them.
- Syncing anything outside GitHub, or a public repository.
- Preventing an agent's shell from writing the shared folder or pushing to the repository.

## Decisions

### Who decided what

**Decided by the human on October 12, 2026** (the plan and the planning interview), numbered here H1 to H6:
- H1. Skills live in a local folder by default. Settings can name a settings repository (Git). It may later hold other things, such as tickets; keep the layout tidy for that, and build nothing for it now.
- H2. In the repository: edited skills, each noting its stock base version; review levels; the model guide; check-in thresholds; the keeper's triggers.
- H3. Stays per machine: providers and allowed models, the workspace folder, the approved-owners list.
- H4. Never in the repository: credentials, the database, conversations.
- H5. Sync is fully automatic: pull at launch and periodically, push after each change.
- H6. When both machines changed the same file, pause sync for that file, keep the local version working, and put a note in the Inbox.

**Raised on October 13, 2026, to settle before real content goes in:** the human sees this as one repository, `wiffletree-storage`, for everything Wiffletree keeps; the Sage-Social organization lets every member read its private repositories; and the human's machine layout keeps company and personal material apart. These are decided questions 1 and 2: a private repository under the human's own account.

**The human approved this design on October 16, 2026,** with the recommended answer to each of the proposal's decided questions.

### 1. What is shared, what stays, what never goes

| Setting | Where it lives today | With this change |
|---|---|---|
| Skills: your rules, rewrites and their bases, history | `<data folder>/skills/` (change 4) | Shared: `skills/` |
| Verifiers, review levels, rounds per review, reviews per ticket | `settings.json` → `verification` | Shared: `settings/review.json` |
| The model guide | database, `model_selection.guide` | Shared: `settings/model-guide.md` |
| Check-in thresholds | `settings.json` → `checkin_child_turns`, `checkin_human_turns` | Shared: `settings/check-ins.json` |
| The PR keeper's triggers, quiet period and review level | `settings.json` → `keeper` | Shared: `settings/pr-keeper.json` |
| Providers, allowed models, providers per role, the PR keeper's model | database, `model_selection` | This machine |
| The workspace folder | `settings.json` | This machine |
| The allowed owners | `settings.json` | This machine |
| The settings repository chosen, and sync's own record | new | This machine |
| Repositories and their checks commands | database | This machine |
| Appearance | `desktop-preferences.json` | This machine |
| Credentials, the database, conversations, projects, tickets | data folder, keychain, providers | Never shared |

- *Why the verifiers and caps go with the review levels (H2 names only the levels):* a level names its verifiers by focus, and a level without them means nothing. Rounds per review and reviews per ticket sit on the same Review card and are the same kind of choice.
- *Why the keeper's quiet period and review level go with its triggers:* they are the rest of the same setting, `KeeperSettings`, and the review level names a shared review level.
- *Why the keeper's model stays:* it is a choice among this machine's allowed models (H3).
- A verifier's provider and the keeper's model can name what another machine does not have. Decision 11 says what happens then.

### 2. The shared folder

Shared settings live in one folder on every machine, whether or not a settings repository is chosen. A settings repository turns the folder into a Git clone. Nothing else changes.

```
<data folder>/shared/
  wiffletree.json           {"format": 1}: what this folder is, for later releases
  settings/
    review.json             verifiers, review levels, rounds per review, reviews per ticket
    model-guide.md          the model guide, as the human wrote it
    check-ins.json          the two check-in thresholds
    pr-keeper.json          the keeper's triggers, quiet period and review level
  skills/                   change 4's layout, unchanged
    coordinating/ implementing/ reviewing/ keeping-a-pr/
```

- One file per setting, so a conflict pauses one setting and not all of them, and so a file is small enough to compare by eye.
- `logs/` and `tickets/` are left free for later (H1). Nothing creates them, and sync does not read them.
- JSON files are written pretty-printed with sorted keys, so two machines writing the same value write the same bytes. Like `save_settings`, a write keeps fields this release does not know, so a machine on an older release never drops a newer release's field.
- All reads and writes go through one `SharedStore` in a new `shared.rs`, rooted at the folder and written atomically. `SkillStore` is rooted at its `skills/` folder. `Host::settings` keeps returning a whole `HostSettings`: the shared fields are read from the shared files and the rest from `settings.json`. The one caller that changes is the combined save, `set_setup` (`settings.rs:127`): it checks the new review settings and the new selection together before writing anything, and when a write fails it restores `settings/review.json`, `settings/model-guide.md` and the database row as they were, instead of `settings.json`. A refused or failed combined save changes none of them.
- A missing file means that setting's default, as a missing field does today.

*Why one folder always, instead of moving settings only when a repository is chosen:* one layout and one code path. Choosing or leaving a repository then moves nothing. "Skills live in a local folder by default" (H1) holds: the local folder is the shared folder.

### 3. Moving today's settings into the shared folder

- At the first start of this release, for each shared setting whose file does not exist yet, the host copies the value as saved today into its file. `verification`, the check-in thresholds and `keeper` come from `settings.json`. The guide comes from the saved model selection. `<data folder>/skills/` is moved to `shared/skills/` with a rename.
- Values are copied exactly as saved. A verification setting saved before review levels is copied without levels, and is read as one level "Full" as today (`ticket-verification`). It is not rewritten.
- The old fields stay in `settings.json` and the database, unread, so going back to an older release starts with the values it had. The host never writes them again.
- A machine with no saved settings gets no files, and every setting is its default.
- The move happens before anything reads settings. It runs again for any file still missing, so a move interrupted halfway finishes at the next start.

### 4. Choosing a repository on the first machine

The Settings window gets a **Settings repository** section after Check-ins (mockup panel a).

- The human names a GitHub repository as `owner/name` or as an HTTPS or SSH URL. Only `github.com` is accepted, as in `repository-owners`.
- **Check** asks GitHub, read-only through `gh`, for the repository's visibility, its owner and whether the owner is an organization, and for an organization's base permission for members where GitHub answers. Then `git ls-remote` checks that this machine can read the repository. Nothing is written.
- A public repository is refused: "Choose a private repository. Your skills and settings would be public."
- The section then says who can read the repository (mockup panel b):
  - a user's repository: "Private. Only you and people you invite can read it."
  - an organization's repository with a base permission of read or more: "Private, but every member of Sage-Social can read it: the organization's base permission is Read." In yellow.
  - an organization's repository whose base permission GitHub did not report: "Private. Members of Sage-Social may be able to read it, depending on the organization's settings."
- **Set up** (for a repository without `wiffletree.json`) asks first, then adds `wiffletree.json`, `settings/` and `skills/` from this machine's shared folder in one commit on the repository's default branch, and pushes it. It never changes or removes files it did not write, such as the README GitHub created.
- **Join** (for a repository with `wiffletree.json`) is decision 5.
- Wiffletree does not create repositories on GitHub. The section says how: "Create an empty private repository on GitHub, then name it here." *Why:* Wiffletree writes nothing to GitHub apart from the pushes the human has allowed, and creating a repository under an organization is the organization's decision.
- The choice, with the push URL and the branch, is saved in this machine's `settings.json` as `settings_repository`. It is never synced, so no other machine can change where this one pushes.

### 5. A second machine joins

Joining makes this machine use the repository's shared settings (decided question 5). Mockup panel e.

1. The human names the repository and presses **Check**, as in decision 4.
2. The section lists what the repository holds next to what this machine has, setting by setting: "Review: Full and Light (this Mac: Full only)", "Implementing: 3 of your rules (this Mac: stock)". It names the machine and time of the repository's last change.
3. **Use the repository's settings** clones the repository next to the shared folder, then:
   - copies this machine's skill history entries that the repository lacks into the clone, so no history is lost, and pushes them;
   - moves this machine's shared folder to `<data folder>/backups/shared-<time>/` and the clone into its place.
4. The section then lists what this machine still needs, read from decision 11: for example, "The PR keeper's triggers are on. This Mac has no PR keeper model: choose one in Models", or "Review level Full runs Regressions on Codex, which is not enabled on this Mac."

The other way, **Replace the repository's settings with this Mac's**, is offered under the comparison and asks first. It pushes this machine's files over the repository's in one commit. The repository's previous files stay in its Git history.

*Why the repository wins by default:* the repository holds the settings the human has been using. A new machine's settings are most often the defaults.

### 6. Pushing: a new kind of push

Pushing to the settings repository is not publishing a ticket, and the allowed owners do not govern it (decided question 3).

- **Choosing the repository on this machine is the permission.** The confirmation names the repository, its branch and who can read it, and says Wiffletree will push setting changes there automatically. The choice stays on this machine, as the allowed owners do.
- **The allowed owners neither grant nor need it.** Allowing `Sage-Social` for pull requests does not let sync push to a Sage-Social repository, and a settings repository needs no allowed owner. The Repositories page and the Settings section each say so in one line.
- **What a push may do.** Sync pushes only the repository's default branch, chosen at set-up or join and recorded. It pushes only a commit that contains the remote branch's head, so a push never overwrites another machine's commit. It sends no tags and no submodules, and goes to the recorded push URL, never to a remote by name, as `publish.rs:269` does. A push the remote refuses is not retried by force: the next pass pulls first.
- **What a commit may contain.** Only files in the layout of decision 2: `wiffletree.json`, `settings/*` and `skills/**`. Other files in the repository are pulled, kept and never changed.
- **Who pushes.** Only the host's sync. No agent tool reads or changes the shared folder or the choice of repository. As with everything else, an agent's shell is not sandboxed: it can write the shared folder, and it can push to the repository with the human's credentials. Decision 10 covers the first; the README states the second.
- Pushes use the human's own Git credentials and identity, as their own commits do. Each commit's message names what changed and the machine: "Implementing: saved your rules · Mike's MacBook Air". If Git has no name and email set, sync stops with that error.

### 7. When sync runs

- **Pull** at launch, every 5 minutes, and when the human presses **Sync now** (decided question 6).
- **Push** 5 seconds after the last change to a shared setting or skill, so a burst of saves is one commit, and after each pull that left this machine's changes unpushed.
- A pass runs off the host's command thread, as the PR watcher's passes do. When GitHub cannot be reached, passes back off up to 30 minutes, and **Sync now** always tries at once. Changes wait in the folder meanwhile, and this machine keeps using them.
- Pulled changes apply where each setting is read: skills from each agent's next turn, check-in thresholds from the next turn that starts, review settings from the next `verify_ticket`, keeper triggers from the keeper's next pass, the guide from the next `workspace_context`. The client's snapshot is refreshed after a pass that changed anything.

### 8. How a pass merges, file by file

Sync's record, kept in this machine's database, holds the commit this machine last synced, and for each file the content this machine last synced and the content the host last wrote.

For each file, a pass compares three versions: the last synced one, this machine's, and the remote head's.
- Changed only here: this machine's version is committed and pushed.
- Changed only on the other side: the remote's version is written to the folder.
- Changed on both sides to the same content: nothing to do.
- Changed on both sides differently: the file is paused (decision 9).

The pass then makes one commit on top of the remote head, holding this machine's changes and, for a paused file, the remote's version. It pushes that commit, then records it as synced.

- Files are compared whole, not line by line. *Why:* a JSON setting merged line by line can be invalid or mean something neither machine chose. H6 says "the same file".
- A skill's current state, `additions.md`, `rewrite.md` and `base.md`, is compared as one unit. When any of the three would pause, all three pause together, and this machine keeps all three of its own. *Why:* starting a rewrite on one machine removes `additions.md` and adds `rewrite.md` and `base.md`. Taking the remote's new files while pausing this machine's edited rules would switch this machine to the other's rewrite and drop its own rules.
- Skill history entries are one file per entry, named by time, so two machines never change the same entry, and both histories are kept. They sync even while the skill's current state is paused.
- The record is per machine, so a machine that is offline for a week compares against what it last saw, not against the newest commit.

### 9. A file changed on both machines

- This machine keeps using its own version (H6). Sync does not push it and does not take the other one. Every other file keeps syncing.
- The host raises one **workspace note** in the Inbox, saying which setting is paused and on which machines it changed (decided question 4). It also shows in the Settings section and as a mark on the sidebar's sync status (decision 13).
- The note offers three things:
  - **Compare**: both versions, as a line diff.
  - **Keep this Mac's**: pushes this machine's version over the other.
  - **Use the other machine's**: writes the other version here.
  The version not kept stays in the repository's Git history. A skill also has the other machine's history entries, which can be restored.
- While the file is paused, further edits here stay here, and a newer version from the other machine replaces the one the note compares against.
- The note closes when the human picks a side, or when the two versions become equal, as when the human makes the same edit on both machines.

### 10. A file changed outside Wiffletree

A file in the shared folder whose content differs from what the host last wrote or pulled was changed outside Wiffletree: by the human in an editor, by a script, or by an agent's shell. This answers change 4's open question 10 (decided question 7).

- It is paused like a conflict, with a note: "Implementing · your rules was changed outside Wiffletree on this Mac."
- **Sync this version** pushes it, after the checks of decision 11 that every pushed file must pass. **Put back the synced version** restores the last synced content.
- Until the human decides, this machine uses the file as it is on disk, as today's settings file is used.

*Why pause instead of pushing:* a pushed file reaches every machine's agents. A skill is part of every turn's instructions. An edit nobody made in the app should not spread without the human seeing it.

### 11. What this machine cannot use

- **A file that cannot be read**, such as invalid JSON, a value out of range or text over the limit, is not applied when pulled. This machine keeps its last good version, and the file is paused with the reason: "settings/review.json from Mike's MacBook Air can't be used here: two verifiers have the focus Tests." Its note offers **Compare** and **Keep this Mac's**, which pushes this machine's version, and it closes by itself when a version this machine can read arrives. The host never pushes a file that it would refuse to read.
- **A newer format.** When `wiffletree.json` names a format this release does not know, sync stops on this machine with "This repository was written by a newer Wiffletree. Update to keep syncing." Nothing is pulled or pushed until then.
- **Settings this machine cannot run.** Each file is checked on its own when pulled. Across files, and against what stays on this machine, the host does not refuse what arrived; it holds the part it cannot run on this machine and says why where the setting is shown:
  - keeper triggers that need a PR keeper model this machine does not have, or a review level the review settings no longer have, are off on this machine: "On from Mike's MacBook Air. Off on this Mac: choose a PR keeper model in Models." The file is unchanged, and the triggers start once this machine can run them;
  - a verifier whose provider is not enabled here is shown on the Review card as "Needs Codex, which is not enabled on this Mac". `verify_ticket` refuses its model as it does today when a provider is disabled.
- **Saves check held triggers as off.** Today's cross-checks, `check_tries` and `check_sorting_model`, run on every save of the selection, the review settings and the keeper's settings. On this machine they count a trigger held off here as off, unless the save itself turns that trigger on. So a joined machine with no PR keeper model can still save its first model setup, the guide, the review settings or the keeper's quiet period, and the held trigger stays on in the shared file for the other machines. A trigger that runs on this machine is checked as today: removing the keeper model while it runs is still refused. Turning a trigger on that this machine cannot run is still refused.

### 12. Machines on different releases

Skills compare each rewrite's base with this machine's stock text (change 4, decision 7), so a rewrite saved on a machine with a newer release shows as out of date on an older one. Change 4 writes a "New stock version" history entry when a skill's stock version differs from its latest entry's. With a shared history, two machines on different releases would add one at every start, back and forth. With this change, the entry is written only when this release's stock version is newer than every version the skill's history records. An older release writes none, and the skill's page says "Mike's MacBook Air runs a newer Wiffletree: Coordinating v2."

### 13. Sync status

- The Settings section shows the repository, its branch, who can read it, the last sync with what it moved ("Synced at 14:32 · pulled 2 changes from Mike's MacBook Air, pushed 1"), when the next check runs, the last error in Git's own words, and the paused files. It has **Sync now** and **Stop syncing** (mockup panel c).
- The sidebar footer gets a small sync mark with a tooltip: synced, syncing, waiting to reach GitHub, or paused. It is yellow while a file is paused or sync has stopped, and red after an error that has lasted more than an hour. Clicking it opens the Settings window at the section. There is no mark before a repository is chosen.
- The snapshot carries the sync status and the paused files, with no file contents. Contents are requested when a note's **Compare** opens.

### 14. Stopping

**Stop syncing** asks first. It leaves the shared folder as it is, keeps using it, and stops pulling and pushing. It removes `settings_repository` from this machine's settings and closes the workspace notes. Nothing on GitHub changes. Joining again later is a join (decision 5).

## Risks / Trade-offs

- **Whoever can push to the repository can change every machine's agents.** Skill text is part of every agent's instructions, and the review and keeper settings decide what runs. A private repository under the human's own account limits that to the human (decided questions 1 and 2). The README says so.
- **Org members may read it.** An organization's base permission applies to every private repository. Wiffletree shows it and cannot change it.
- **Agents' shells can push.** The repository is a Git clone in the data folder, and agents' shells hold the human's credentials. Wiffletree's own push is the only one it makes, and decision 10 stops an outside edit from spreading through sync, but a shell can still push to the repository directly. This is the plan's open risk, unchanged.
- **Two machines editing the same setting pause it.** Whole-file comparison pauses some edits a line merge would have combined. It never produces a setting neither machine chose.
- **Five minutes of lag.** An edit on one machine reaches the other at its next pull. Sync now is there when it matters.
- **The old fields linger** in `settings.json` and the database, unread. Removing them later is a cleanup with no behavior.

## Mockup

See `design/mockups/current/settings-repository.html`, in Tidal Light and Tidal Dark.
