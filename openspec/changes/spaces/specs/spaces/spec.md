## Purpose

Spaces are configured containers, such as Sage, Vyttle or Personal, that answer "where does this go?" for everything in Wiffletree: they hold the instructions, brain, storage and tracker, repositories and default models for the projects, threads, jobs and issues in them, and a team can share one through a Git repository.

## ADDED Requirements

### Requirement: A Space
A Space SHALL have a name; a description of what belongs in it; instructions added to every thread and job run in it (see `assistant`, `scheduled-jobs`); a brain, being a Git repository, on GitHub as `owner/name` or a local folder, and a folder in it, or none; a storage repository, which is also its tracker; the code repositories it covers, by identity (`owner/name`), never by path; default models for threads and for jobs, each optional; and a list of sources its threads may use. Two Spaces MAY name the same brain, and two Spaces MAY share one storage repository. A Space SHALL NOT be called a project anywhere in the interface.

#### Scenario: Two Spaces, one brain
- **WHEN** the Sage and Sage Watch Spaces both name `Sage-Social/sagehaven-docs` folder `second-brain/`
- **THEN** projects in both log to that one brain

### Requirement: Only the user creates or edits a Space
Spaces SHALL be created, edited and removed only on the Spaces page of Settings, by the user, apart from the Spaces the host creates at upgrade and the default "Personal" Space. No agent, thread, job run, router or workspace tool SHALL create, change or remove a Space or read its machine-local facts. Removing a Space SHALL ask first, SHALL be refused while a non-archived project belongs to it or when it is the last Space, SHALL move its archived projects to the "Personal" Space, creating it if it was removed, and SHALL leave its storage and brain repositories unchanged.

#### Scenario: A fresh install
- **WHEN** Wiffletree starts with no Space, on a new Mac or after an upgrade with no projects
- **THEN** the host creates a Space named "Personal" with local storage, a local tracker and no brain, so New project has a Space

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

### Requirement: The Spaces page
Settings SHALL have a Spaces section listing every Space with its name, whether it is shared, its storage and brain, and its repositories. A Space's page SHALL edit every field of the Space and SHALL show a Storage and issues box with the storage repository, the Space's folder in it, the tracker's prefix and issue count, who can read the storage repository and therefore its issues, the other Spaces on this Mac that share the storage repository, whether it is also this Mac's settings repository, its sync status and Sync now. For a shared Space the page SHALL say "Anyone who can push to ‹repository› can change these instructions for everyone in this Space". The section SHALL offer Add Space with "New" and "From a repository".

#### Scenario: A shared Space's page
- **WHEN** the user opens the Sage Space, whose storage is `Sage-Social/wiffletree-storage` with prefix SAGE
- **THEN** the page says every member of Sage-Social can read it and its issues, shows prefix SAGE, and says that anyone who can push to it can change these instructions for everyone in this Space

#### Scenario: A personal Space's page
- **WHEN** the user opens Personal, whose storage is local
- **THEN** the Storage and issues box says the storage is local with no remote and that only the user, on this Mac, can read its issues

### Requirement: Storage is a Git repository, local by default
Each Space's storage SHALL be a Git repository holding `spaces/<space>/space.yaml`, and the folders `spaces/<space>/jobs/` and `spaces/<space>/boards/` when jobs and boards exist (see `scheduled-jobs`, `boards`). A new Space SHALL get a local repository at `<data folder>/repos/local-<id>/` with no remote unless the user chooses a GitHub repository. Choosing a GitHub repository SHALL use the settings repository's check: refuse a public repository and say who can read it before anything is written. One storage repository MAY hold several Spaces, a folder each. A repository that is a brain on this Mac SHALL NOT be chosen as storage, and the refusal SHALL name the brain's Space.

#### Scenario: A new personal Space
- **WHEN** the user creates "Personal" without choosing a repository
- **THEN** its storage is a local repository with no remote, and nothing is pushed

#### Scenario: A public repository
- **WHEN** the user chooses a public repository as a Space's storage
- **THEN** it is refused and nothing is written

#### Scenario: A brain as storage
- **WHEN** the user chooses `Sage-Social/sagehaven-docs`, the Sage Space's brain, as a Space's storage
- **THEN** it is refused, saying it is the Sage Space's brain, and nothing is written

### Requirement: A storage repository is also the Space's tracker
A storage repository SHALL be its Spaces' tracker: it SHALL hold `wiffletree-issues.yaml` and `issues/` at its root, beside `spaces/` (see `shared-issues`). Every Space stored in one repository SHALL share its issues and its prefix. A local storage repository SHALL be a local tracker that is never pushed. There SHALL be no other list of trackers and no tracker setting outside a Space. The first commit to a new storage repository, local or on GitHub, SHALL add `wiffletree-issues.yaml` with a prefix of 2 to 5 capital letters, which the Space form SHALL propose from the Space's name and the user MAY change. A prefix SHALL be unique among the storage repositories on this Mac, and SHALL change only while its tracker has no issue. A storage repository added without `wiffletree-issues.yaml` SHALL offer Set up issues on the Space's page, which SHALL ask for a prefix, ask first, and commit only that file.

#### Scenario: Two Spaces, one storage repository
- **WHEN** Sage and Sage Watch are both stored in `Sage-Social/wiffletree-storage`, whose prefix is SAGE
- **THEN** both Space pages show prefix SAGE and the same issue count, and an issue filed in either is SAGE-numbered

#### Scenario: A duplicate prefix
- **WHEN** the user creates a Space with new storage and prefix SAGE on a Mac where another storage repository uses SAGE
- **THEN** the form refuses it, asking for a prefix no other storage repository on this Mac uses

#### Scenario: Joining storage with no tracker
- **WHEN** the user adds a Space from a repository that has `spaces/` but no `wiffletree-issues.yaml`
- **THEN** the Space's page offers Set up issues, and nothing is written until the user chooses a prefix and confirms

### Requirement: Every project belongs to a Space
Every Wiffletree project SHALL belong to exactly one Space, shown and changeable in the project's Overview. New project SHALL show a Space, defaulting to the Space last used for a new project, and SHALL tick, by default, the workspace repositories whose GitHub name is one of that Space's repositories, listing the Space's other repositories as not cloned on this Mac. Changing the Space in New project SHALL replace the ticked repositories with the new Space's. The user MAY tick other workspace repositories by hand. A project's coordinator SHALL log to its Space's brain. Moving a project to another Space SHALL ask first, SHALL change where its future entries go, leaving earlier entries where they are, and SHALL NOT change its repositories.

#### Scenario: A project's log
- **WHEN** a coordinator in a project of the Vyttle Space records a decision
- **THEN** the entry goes to the Vyttle Space's brain

#### Scenario: Repositories from the Space
- **WHEN** the user opens New project with Sage last used, Sage lists `Sage-Social/mobile-app`, `Sage-Social/watch-app` and `Sage-Social/api`, and only the first two are workspace repositories here
- **THEN** mobile-app and watch-app are ticked, api is listed as not cloned here, and no personal repository is ticked

#### Scenario: Another Space
- **WHEN** the user switches New project's Space from Sage to Personal
- **THEN** the Sage repositories are unticked and Personal's are ticked

### Requirement: Existing projects get Spaces on upgrade
At the first start of this release, the host SHALL create one Space for each distinct brain folder attached to a project, named after the folder, with that folder as a local brain and local storage, and SHALL put each such project in it. Space names SHALL be unique: when two brain folders have the same name, or a folder is named "Personal", each such Space SHALL be named "‹parent folder›/‹folder›", and when that is still not unique, a number SHALL follow it. Projects with no brain SHALL go to a Space named "Personal" with no brain. Archived projects SHALL be included. Each Space created SHALL list no repositories and SHALL get a proposed prefix. The upgrade SHALL push nothing, SHALL keep each project's export records so nothing is exported twice, and SHALL run once.

#### Scenario: Two brains
- **WHEN** three projects use `~/dev/sage/team-brain`, one uses `~/dev/vyttle/vyttle-brain` and two have none
- **THEN** the Spaces page lists "team-brain" with three projects, "vyttle-brain" with one and "Personal" with two, and the user can rename them

#### Scenario: Two brains with the same folder name
- **WHEN** one project uses `~/dev/sage/brain` and another uses `~/dev/vyttle/brain`
- **THEN** the upgrade creates the Spaces "sage/brain" and "vyttle/brain"

#### Scenario: A second start
- **WHEN** the host starts again after the upgrade ran
- **THEN** no Space is created or changed

### Requirement: Shared Spaces live in their storage repository
A Space whose storage repository is on GitHub SHALL keep its definition in that repository's `space.yaml`, so that every Mac that adds it reads the same name, description, instructions, brain, repositories, default models and sources. Everything SHALL run on each user's own Mac; there SHALL be no cloud or headless host. A Space whose storage other people can read SHALL be shown as shared.

#### Scenario: Sage's storage
- **WHEN** the Sage Space's storage is `Sage-Social/wiffletree-storage`, which every member of Sage-Social can read
- **THEN** `spaces/sage/space.yaml` in that repository holds the Space's definition, and the Spaces page marks Sage as shared

### Requirement: Joining a shared Space
"Add Space → From a repository" SHALL take a GitHub repository, check it as for a storage repository, list the Spaces it holds and add the ones the user ticks. For each, the user SHALL say where the brain and each code repository are cloned on this Mac, or leave them missing. A model the definition names that this Mac lacks or may not use SHALL fall back to this Mac's Who runs what choice, with a note on the Space's page naming both. Joining SHALL be refused when the repository's prefix is used by another storage repository on this Mac, naming it.

#### Scenario: A teammate joins
- **WHEN** a teammate adds `Sage-Social/wiffletree-storage` and ticks Sage
- **THEN** their Mac gets Sage's description, instructions and the SAGE issues, and asks where `sagehaven-docs` and each Sage repository are cloned

#### Scenario: A model this Mac lacks
- **WHEN** the Sage definition names Opus · high for jobs and the teammate has only Codex signed in
- **THEN** the Space page notes "Opus · high is not available here; using GPT 6.1 Sol · medium"

### Requirement: What stays on this machine
Where each repository and brain is cloned, which models this person may use, whether the Space is on here, and this Mac's credentials and keys SHALL stay on this Mac and SHALL NOT be written to the storage repository.

#### Scenario: Clone paths
- **WHEN** two teammates clone `mobile-app` in different folders
- **THEN** `space.yaml` names `Sage-Social/mobile-app` and no path

### Requirement: Edits to a shared Space follow the settings repository's rules
Files under `spaces/` of a GitHub storage repository SHALL sync with the settings repository's rules (see `shared-repositories`): choosing the repository allows the push; a file changed on two machines, or outside Wiffletree on this machine, pauses with a workspace note offering Compare and the same choices; and a file this machine cannot read is held. Space sync SHALL commit only the files of Spaces this Mac added.

#### Scenario: Two people edit instructions
- **WHEN** two members change Sage's instructions before either pulls
- **THEN** the second to sync keeps its own, pauses `space.yaml` with a workspace note, and every other file keeps syncing

#### Scenario: A shell edit
- **WHEN** an agent's shell changes `spaces/sage/space.yaml` in the storage clone
- **THEN** the change is not pushed until the user chooses Sync this version

### Requirement: Personal stays personal
A Space SHALL sync only to the storage repository the user chose for it. An output, job, issue or entry SHALL NOT move from one Space to another unless the user chooses it with a confirmation that names the destination and who can read it.

#### Scenario: A local Space
- **WHEN** the user files issues and edits instructions in Personal, whose storage is local
- **THEN** nothing is pushed anywhere

### Requirement: Sources a Space allows
A Space's sources SHALL list the MCP servers and local CLIs its threads may use, with the tools or command prefixes each may call, in the format job recipes use (see `scheduled-jobs`). The Space's page SHALL edit them. Only threads and job runs in the Space SHALL read them (see `assistant`).

#### Scenario: Linear in Sage
- **WHEN** the user adds the Linear MCP server with its list and read tools to Sage's sources
- **THEN** `space.yaml` lists the server and those tools, and no other Space's sources change
