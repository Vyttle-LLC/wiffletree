## ADDED Requirements

### Requirement: Every Space has issues, logs and a wiki
Every Space SHALL have issues, log entries and a wiki, in its one repository and folder. There SHALL be no Brain field and no way to turn any of the three off. A Space on this Mac alone SHALL have all three, because none needs GitHub: its compile runs on this Mac, and nothing is pushed. Spaces in one folder SHALL share one set of issues, one log and one wiki.

#### Scenario: A Space on one Mac
- **WHEN** the user creates "Woodworking" as Only this Mac
- **THEN** its repository holds `wiffletree/spaces/woodworking/`, `wiffletree/issues/`, `wiffletree/wiffletree-issues.yaml` and `wiffletree/second-brain/`, nothing is pushed, and a project in it can log and query

#### Scenario: Two Spaces, one folder
- **WHEN** Sage and Sage Watch are both in `Sage-Social/wiffletree-storage`, folder `wiffletree/`
- **THEN** a log entry from a project in either is in the same `second-brain/logs/`, and both read the same wiki and issues

### Requirement: New Space is one form
New Space SHALL ask for a name; where it lives, A private GitHub repository or Only this Mac; for GitHub, a repository, offering Create `<owner>/<name>-wiffletree-storage` with the owner taken from the Space's repositories or the `gh` login, or any private repository the user already has; the folder, `wiffletree/` by default; an issue prefix proposed from the name; and Compile the wiki, On this Mac ticked, daily at 06:00 in this Mac's timezone, saying that it uses the user's model subscription and that teammates can turn it on too. For GitHub it SHALL say, before anything is written, that choosing the repository lets Wiffletree push to its `main` only inside the folder, and that everyone who can read it can read its issues, logs and wiki. Creating a repository SHALL use `gh`, private, only when the user presses Create Space. A repository that already holds a Wiffletree folder SHALL fix the folder, the prefix and the compile time to what it holds, and the Space SHALL join them. Choosing a repository SHALL be what allows the host to push to it. An agent SHALL NOT reach this form.

#### Scenario: Vyttle's repository
- **WHEN** the user creates Vyttle on GitHub, choosing Create `Vyttle-LLC/vyttle-wiffletree-storage` with folder `wiffletree/` and prefix VYT
- **THEN** `gh` creates the repository as private, and the first pass pushes the Space's files under `wiffletree/`

#### Scenario: A second Space in a repository
- **WHEN** the user creates Sage Watch in `Sage-Social/wiffletree-storage`, which already holds Sage in `wiffletree/` with prefix SAGE
- **THEN** the form shows that folder and prefix and does not let them change, and Sage Watch joins them

#### Scenario: Compile off
- **WHEN** the user unticks On this Mac and creates the Space
- **THEN** the Space exists with no volunteer, and the Brains page says no Mac compiles its wiki

### Requirement: A repository is checked before it is used
Choosing a GitHub repository for a Space SHALL check it, read-only, before anything is written, and SHALL show each result:
- it is private, saying who can read it; a public repository SHALL be refused, saying issues and logs need a private repository;
- Wiffletree can write to its default branch, called `main` here; a repository whose rules require pull requests or status checks there SHALL be refused, naming the rule and saying to choose another repository or turn that rule off on GitHub;
- whether history is protected; history that is not protected SHALL be a warning that does not refuse, offering Protect history.

A repository SHALL be usable whatever else it holds. A repository that is a code repository of a Space, or the settings repository, SHALL NOT be refused for that.

#### Scenario: A repository that can be used
- **WHEN** the user chooses `Vyttle-LLC/vyttle-wiffletree-storage`, private, with no rule on `main`
- **THEN** it says Can be used, that 3 members of Vyttle-LLC can read it and that Wiffletree can write to `main`, with a warning that history isn't protected and Protect history

#### Scenario: Pull requests required
- **WHEN** the user chooses `Vyttle-LLC/pica-api`, whose `main` requires pull requests
- **THEN** it says Can't be used, naming that rule, and nothing is written

#### Scenario: A public repository
- **WHEN** the user chooses `Vyttle-LLC/wiffletree`, which is public
- **THEN** it says Can't be used, because issues and logs need a private repository, and nothing is written

#### Scenario: A repository with other documents
- **WHEN** the user chooses `Vyttle-LLC/vyttle-docs`, whose `main` takes direct pushes
- **THEN** it can be used, and only `wiffletree/` is ever written

### Requirement: Protect history
Protect history SHALL add a GitHub ruleset on the default branch that blocks force-pushes and deleting the branch, and SHALL NOT require pull requests or any check. It SHALL need a repository admin. When GitHub refuses, by permission or by plan, the page SHALL say why in GitHub's words and keep showing the warning. It SHALL run only when the human presses it, and no agent tool SHALL reach it.

#### Scenario: An admin protects history
- **WHEN** an admin of `Vyttle-LLC/vyttle-wiffletree-storage` presses Protect history
- **THEN** a ruleset on `main` blocks force-pushes and deletion, the warning goes, and a direct push from Wiffletree still lands

#### Scenario: A plan that does not allow it
- **WHEN** GitHub refuses the ruleset for a private repository on the organization's plan
- **THEN** the page says so in GitHub's words, and the warning stays

### Requirement: A Space from an earlier release upgrades without losing anything
At the first start of this release, every Space SHALL keep its repository and gain a folder: `wiffletree` for local storage, which moves at that start, and for GitHub storage the empty folder of a repository not yet moved (see `shared-repositories`, "Files at a repository's root move into the folder once"). A store of any earlier schema SHALL still open, and an earlier build SHALL still open this store. A Space whose `space.yaml` names a brain SHALL keep working from its folder: its new entries SHALL land in its folder, its old brain SHALL never be written to again, `space.yaml` SHALL keep the `brain` key as it was read, as a record, and Bring in a brain… SHALL be filled in from it (see `brain-registry`). `compile` and `allowed_email_domains` SHALL be carried from that brain's `wiffletree-brain.json` into `wiffletree-issues.yaml`, once, when the brain's clone is on this Mac, and a Mac the file names as compiler SHALL become a volunteer. `space.yaml` SHALL keep every other key an older host reads, under the same name and in `format` 1. The upgrade SHALL push nothing but entries that were waiting to be sent.

#### Scenario: A Space with a separate brain and storage
- **WHEN** Sage names the brain `Sage-Social/sagehaven-docs` and the storage `Sage-Social/wiffletree-storage`, and a coordinator in Sage logs a decision after the upgrade
- **THEN** the entry is in Sage's folder of `wiffletree-storage`, `sagehaven-docs` is unchanged, `space.yaml` still has its `brain` key, and Sage's page offers Bring in a brain… filled in from it

#### Scenario: An old store
- **WHEN** Wiffletree starts with a store from before Spaces
- **THEN** it opens, its upgrades run in order, and each Space it creates has a folder, issues, logs and a wiki

#### Scenario: A new Space seen by an older host
- **WHEN** an older host reads a `space.yaml` this release wrote at a repository's root, with no `brain` key
- **THEN** it reads the instructions and repositories as before, shows no brain, and refuses `log_work`, writing nowhere

## MODIFIED Requirements

### Requirement: A Space
A Space SHALL have a name; a description of what belongs in it; additions to every role's instructions in it, stored in `space.yaml` as `instructions:`, the key every release reads, and the Behaviors it sets (see `settings-cascade`); for each code repository it lists, optional Behaviors and additions for that repository's tickets; one repository, private on GitHub as `owner/name` or local to this Mac, and one folder in it; the code repositories it covers, by identity (`owner/name`), never by path; the rows of Who runs what it overrides and what it sets about its tickets' reviews (see `settings-cascade`); and a list of sources its threads may use. Two Spaces MAY name the same repository and SHALL then share its folder. A Space SHALL NOT be called a project anywhere in the interface.

#### Scenario: Two Spaces, one brain
- **WHEN** the Sage and Sage Watch Spaces both name `Sage-Social/wiffletree-storage`
- **THEN** projects in both log to the one brain in its folder

#### Scenario: Instructions from an older store
- **WHEN** this host reads a `space.yaml` an older host wrote, with `instructions:`
- **THEN** the Space's additions are those instructions, and a save here writes them back under `instructions:` with no `additions:` key, so the older host keeps reading them

### Requirement: Only the user creates or edits a Space
Spaces SHALL be created, edited and removed only on the Spaces page of Settings, by the user, apart from the Spaces the host creates at upgrade, the default "Personal" Space, and the repositories a rule the human set adds (see "A Space can follow root folders"). No agent, thread, job run, router or workspace tool SHALL create, change or remove a Space or read its machine-local facts. Removing a Space SHALL ask first, SHALL be refused while a non-archived project belongs to it or when it is the last Space, SHALL move its archived projects to the "Personal" Space, creating it if it was removed, and SHALL leave its repository unchanged. Removing "Personal" itself SHALL be refused while archived projects belong to it, because they would have nowhere to go.

#### Scenario: A fresh install
- **WHEN** Wiffletree starts with no Space, on a new Mac or after an upgrade with no projects
- **THEN** the host creates a Space named "Personal" on this Mac alone, with issues, logs and a wiki, so New project has a Space

#### Scenario: The last Space
- **WHEN** the user removes the only Space left
- **THEN** the removal is refused, saying at least one Space must exist

#### Scenario: An agent asks to change instructions
- **WHEN** a coordinator is asked to add a rule to its Space's instructions
- **THEN** no tool lets it, and it can only suggest the user edit the Space in Settings

#### Scenario: Removing a Space with a project
- **WHEN** the user removes a Space that one open project belongs to
- **THEN** the removal is refused, naming the project

#### Scenario: Removing a Space with only archived projects
- **WHEN** the user removes the Woodworking Space, whose two projects are archived, and confirms
- **THEN** the Space is removed, both archived projects belong to Personal, and restoring one later puts it in Personal

#### Scenario: Removing Personal with archived projects
- **WHEN** the user removes Personal, which holds one archived project, while Sage also exists
- **THEN** the removal is refused, saying its archived projects would have nowhere to go, and nothing changes

### Requirement: The Spaces page
Settings SHALL have a Spaces section listing every Space with its name, whether it is shared, its repository and folder, and its code repositories. A Space's page SHALL edit its name, description, instructions, code repositories and sources, and SHALL show one Repository box with where it is, the repository and folder; its sync status, such as "Synced 30 seconds ago"; who can read it and therefore its issues, logs and wiki; the issues, as prefix and count; the logs, as the count of entries and how many wait for the next compile; the wiki, as its page count and when and on which Mac it last compiled; the compile time and how many Macs compile it, with "Change on Brains →"; the other Spaces on this Mac that share the folder; and whether it is also this Mac's settings repository. The box SHALL offer Sync now, Import from Linear…, Bring in a brain… and Move…. The Brain, Folder, Brain compile and Brain gate rows SHALL NOT appear. For a shared Space the page SHALL say "Anyone who can push to ‹repository› can change these instructions for everyone in this Space". The section SHALL offer Add Space with "New" and "From a repository".

#### Scenario: A shared Space's page
- **WHEN** the user opens the Vyttle Space, whose repository is `Vyttle-LLC/vyttle-wiffletree-storage` with prefix VYT
- **THEN** the Repository box shows `Vyttle-LLC/vyttle-wiffletree-storage · wiffletree/`, that 3 members of Vyttle-LLC can read it, "Prefix VYT · 48 issues", "214 entries · 9 waiting for the next compile" and "Compiled today at 06:04 on Michael's Mac", and the page says that anyone who can push to it can change these instructions for everyone in this Space

#### Scenario: A personal Space's page
- **WHEN** the user opens Personal, whose repository is local
- **THEN** the Repository box says it is only on this Mac with no remote, and that only the user, on this Mac, can read its issues, logs and wiki

### Requirement: Storage is a Git repository, local by default
A Space's repository SHALL be a Git repository holding `spaces/<space>/space.yaml`, and the folders `spaces/<space>/jobs/` and `spaces/<space>/boards/` when jobs and boards exist (see `scheduled-jobs`, `boards`), all inside its folder (see `shared-repositories`, "A repository holds one Wiffletree folder"). A new Space SHALL get a local repository at `<data folder>/repos/local-<id>/` with no remote unless the user chooses a GitHub repository. One repository MAY hold several Spaces, a folder each under `spaces/`.

#### Scenario: A new personal Space
- **WHEN** the user creates "Personal" as Only this Mac
- **THEN** its repository is local with no remote, and nothing is pushed

#### Scenario: A public repository
- **WHEN** the user chooses a public repository as a Space's repository
- **THEN** it is refused and nothing is written

#### Scenario: A brain as storage
- **WHEN** the user chooses `Vyttle-LLC/vyttle-docs`, which an older Space names as its brain, as a Space's repository
- **THEN** it is checked like any other, and a Space may use it

### Requirement: A storage repository is also the Space's tracker
A Space's repository SHALL be its Spaces' tracker: it SHALL hold `wiffletree-issues.yaml` and `issues/` in its folder, beside `spaces/` (see `shared-issues`). Every Space stored in one repository SHALL share its issues and its prefix. A local repository SHALL be a local tracker that is never pushed. There SHALL be no other list of trackers and no tracker setting outside a Space. The first commit to a new local repository SHALL add the Space's `space.yaml` and `wiffletree-issues.yaml`, and the first push of a Space to a GitHub repository without a tracker SHALL add `wiffletree-issues.yaml` beside the Space's folder, each in a commit of its own as every commit holds one rule set's files (see `shared-repositories`), with a prefix of 2 to 5 capital letters, which the Space form SHALL propose from the Space's name and the user MAY change. `wiffletree-issues.yaml` SHALL also hold what the folder's Spaces share about their wiki: `compile`, the time and timezone, and `allowed_email_domains`; a write SHALL keep keys this release does not know. A prefix SHALL be unique among the repositories on this Mac. A GitHub tracker's prefix SHALL change only while it has no issue. A local tracker's prefix SHALL be renamable on its Space's page at any time, and renaming SHALL rewrite its issue IDs, folders and every reference to them in its own issues, `notes/` and `STATUS.md`, leaving log entries as they were, because nothing outside this Mac refers to them. A repository added without `wiffletree-issues.yaml` SHALL offer Set up issues on the Space's page, which SHALL ask for a prefix, ask first, and commit only that file.

#### Scenario: Two Spaces, one storage repository
- **WHEN** Sage and Sage Watch are both stored in `Sage-Social/wiffletree-storage`, whose prefix is SAGE
- **THEN** both Space pages show prefix SAGE and the same issue count, and an issue filed in either is SAGE-numbered

#### Scenario: A duplicate prefix
- **WHEN** the user creates a Space with a new repository and prefix SAGE on a Mac where another repository uses SAGE
- **THEN** the form refuses it, asking for a prefix no other repository on this Mac uses

#### Scenario: Joining storage with no tracker
- **WHEN** the user adds a Space from a repository that has `wiffletree/spaces/` but no `wiffletree/wiffletree-issues.yaml`
- **THEN** the Space's page offers Set up issues, and nothing is written until the user chooses a prefix and confirms

#### Scenario: Renaming a local prefix with a wiki
- **WHEN** the user renames Personal's prefix from ME to MY, a wiki page names ME-4 and a log entry names ME-4
- **THEN** the page says MY-4, the entry still says ME-4, and every issue folder is renamed

### Requirement: Shared Spaces live in their storage repository
A Space whose repository is on GitHub SHALL keep its definition in that repository's `space.yaml`, so that every Mac that adds it reads the same name, description, instructions, repositories, model rows, review settings and sources. Everything SHALL run on each user's own Mac; there SHALL be no cloud or headless host. A Space whose repository other people can read SHALL be shown as shared.

#### Scenario: Sage's storage
- **WHEN** the Sage Space's repository is `Sage-Social/wiffletree-storage`, which every member of Sage-Social can read
- **THEN** `wiffletree/spaces/sage/space.yaml` in that repository holds the Space's definition, and the Spaces page marks Sage as shared

### Requirement: Joining a shared Space
"Add Space → From a repository" SHALL take one GitHub repository, check it as for a Space's repository, find its folder (see `shared-repositories`), list the Spaces it holds with each one's prefix, issue count and wiki page count, and add the ones the user ticks. For each, the user SHALL say where each code repository is cloned on this Mac, or leave it missing. The form SHALL offer Also on this Mac for compiling, off by default, saying how many Macs compile the wiki now and that turning it on uses the user's model subscription. A row of the definition none of whose choices is signed in on this Mac SHALL fall back to this Mac's row, with a note naming both. Joining SHALL be refused when the repository's prefix is used by another repository on this Mac, naming it; when that one is a local tracker, the refusal SHALL tell the human to rename its prefix and offer to open its Space's page.

#### Scenario: A teammate joins
- **WHEN** a teammate adds `Vyttle-LLC/vyttle-wiffletree-storage` and ticks Vyttle
- **THEN** the form says "Found in `wiffletree/`" and shows Vyttle with prefix VYT, that Mac gets Vyttle's description, instructions, issues, logs and wiki, it asks only where each code repository is cloned, and compiling stays off until it is chosen there

#### Scenario: A local prefix clashes with a shared one
- **WHEN** Personal's local tracker uses SAGE with 12 issues, and the human adds `Sage-Social/wiffletree-storage`, whose prefix is also SAGE
- **THEN** joining is refused, telling the human to rename Personal's prefix; after renaming it to ME, Personal's issues are ME-1 to ME-12 with their links rewritten, and joining succeeds

#### Scenario: A model this Mac lacks
- **WHEN** the Sage definition names Opus · high for jobs and the teammate has only Codex signed in
- **THEN** the Space page and the Models page at Sage note "Sage asks for Opus · high for the Job row, but Claude is not signed in on this Mac, so it uses this Mac's row until then"

### Requirement: What stays on this machine
Where each repository is cloned, whether the Space is on here, which root folders it follows, and this Mac's credentials and keys SHALL stay on this Mac and SHALL NOT be written to the repository. Whether this Mac compiles a wiki SHALL be chosen on this Mac and published as its one file in `compilers/` (see `brain-registry`), holding only its machine name and login.

#### Scenario: Clone paths
- **WHEN** two teammates clone `mobile-app` in different folders
- **THEN** `space.yaml` names `Sage-Social/mobile-app` and no path

#### Scenario: A list of models from an earlier release
- **WHEN** this Mac kept a list of the models it may use in Sage
- **THEN** the list no longer limits any model, a model being usable when its provider is signed in here, it stays stored, and the Models page notes once that Sage had one

#### Scenario: Followed root folders
- **WHEN** a Space follows `~/dev/vyttle`
- **THEN** `space.yaml` holds no path, and an older store with no followed roots loads unchanged

#### Scenario: A volunteer's file
- **WHEN** Michael's Mac turns compiling on for Vyttle
- **THEN** `wiffletree/compilers/<host id>.yaml` holds "Michael's Mac" and Michael's login, and no path, key or token
