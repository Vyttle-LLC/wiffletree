## Purpose

Spaces are configured containers, such as Sage, Vyttle or Personal, that hold the instructions, brain, storage, repositories and default models for the threads, jobs and projects in them, and that a team can share through a Git repository.

## ADDED Requirements

### Requirement: A Space
A Space SHALL have a name; a description of what belongs in it, which the router reads; instructions added to every thread and job run in it; a brain, being a repository and a folder in it, or none; a storage repository for its jobs and boards; the code repositories it covers, by identity; default models for threads and for jobs, each optional; and a list of sources its threads may use, in the format jobs use (see scheduled-jobs). Two Spaces MAY name the same brain. A Space SHALL NOT be called a project anywhere in the interface.

#### Scenario: Two Spaces, one brain
- **WHEN** the Sage and Sage Watch Spaces both name `Sage-Social/team-brain` folder `brain/`
- **THEN** threads in both read and log to that one brain

### Requirement: Only the user creates or edits a Space
Spaces SHALL be created, edited and removed only on the Spaces page of Settings, by the user, apart from the Spaces the host creates at upgrade and the default "Personal" Space. No agent, thread, job run, router or workspace tool SHALL create, change or remove a Space or read its machine-local facts. Removing a Space SHALL ask first, SHALL be refused while a non-archived project belongs to it or when it is the last Space, SHALL move its archived projects to the "Personal" Space, creating it if it was removed, and SHALL leave its storage and brain repositories unchanged.

#### Scenario: A fresh install
- **WHEN** Wiffletree starts with no Space, on a new Mac or after an upgrade with no projects
- **THEN** the host creates a Space named "Personal" with a local storage repository and no brain, so New project and the assistant's first thread have a Space

#### Scenario: The last Space
- **WHEN** the user removes the only Space left
- **THEN** the removal is refused, saying at least one Space must exist

#### Scenario: A thread asks to change instructions
- **WHEN** a thread is asked to add a rule to its Space's instructions
- **THEN** it cannot, and it suggests the user edit the Space in Settings

#### Scenario: Removing a Space with a project
- **WHEN** the user removes a Space that one open project belongs to
- **THEN** the removal is refused, naming the project

#### Scenario: Removing a Space with only archived projects
- **WHEN** the user removes the Woodworking Space, whose two projects are archived, and confirms
- **THEN** the Space is removed, both archived projects belong to Personal, and restoring one later puts it in Personal

### Requirement: The Spaces page
Settings SHALL have a Spaces section listing every Space with its name, whether it is shared, its storage and brain, and its repositories. A Space's page SHALL edit every field of the Space, SHALL show for a shared Space who can read its storage repository and the sentence "Anyone who can push to ‹repository› can change these instructions for everyone in this Space", and SHALL offer Add Space with "New" and "From a repository".

#### Scenario: A shared Space's page
- **WHEN** the user opens the Sage Space, whose storage is `Sage-Social/wiffletree-storage`
- **THEN** the page says every member of Sage-Social can read it and that anyone who can push to it can change these instructions for everyone in this Space

### Requirement: Storage is a Git repository, local by default
Each Space's storage SHALL be a Git repository holding `spaces/<space>/space.yaml`, `spaces/<space>/jobs/` and `spaces/<space>/boards/`. A new Space SHALL get a local repository in the data folder with no remote unless the user chooses a GitHub repository. Choosing a GitHub repository SHALL use the settings repository's check: refuse a public repository and say who can read it before anything is written. One storage repository MAY hold several Spaces.

#### Scenario: A new personal Space
- **WHEN** the user creates "Personal" without choosing a repository
- **THEN** its storage is a local repository with no remote, and nothing is pushed

#### Scenario: A public repository
- **WHEN** the user chooses a public repository as a Space's storage
- **THEN** it is refused and nothing is written

### Requirement: Every project belongs to a Space
Every Wiffletree project SHALL belong to exactly one Space. New project SHALL put the project in the Space last used for a new project, shown and changeable in the project's Overview, and Make this a project SHALL use the thread's Space. A project's coordinator SHALL log to its Space's brain. Moving a project to another Space SHALL ask first and SHALL change where its future entries go, leaving earlier entries where they are.

#### Scenario: A project's log
- **WHEN** a coordinator in a project of the Vyttle Space records a decision
- **THEN** the entry is exported to the Vyttle brain

### Requirement: Existing projects get Spaces on upgrade
At the first start of this release, the host SHALL create one Space for each distinct brain folder attached to a project, named after the folder, with that brain and a local storage repository, and SHALL put each such project in it. Space names SHALL be unique: when two brain folders have the same name, or a folder is named "Personal", each such Space SHALL be named "‹parent folder›/‹folder›", and when that is still not unique, a number SHALL follow it. Projects with no brain SHALL go to a Space named "Personal" with no brain. Archived projects SHALL be included. The upgrade SHALL push nothing, SHALL keep each project's export records so nothing is exported twice, and SHALL run once.

#### Scenario: Two brains
- **WHEN** three projects use `~/dev/sage/team-brain`, one uses `~/dev/vyttle/vyttle-brain` and two have none
- **THEN** the Spaces page lists "team-brain" with three projects, "vyttle-brain" with one and "Personal" with two, and the user can rename them

#### Scenario: Two brains with the same folder name
- **WHEN** one project uses `~/dev/sage/brain` and another uses `~/dev/vyttle/brain`
- **THEN** the upgrade creates the Spaces "sage/brain" and "vyttle/brain"

### Requirement: Shared Spaces live in their storage repository
A Space whose storage repository is on GitHub SHALL keep its definition in that repository's `space.yaml`, so that every Mac that adds it reads the same name, description, instructions, brain, repositories, default models and sources. Everything SHALL run on each user's own Mac; there SHALL be no cloud or headless host. A Space whose storage other people can read SHALL be shown as shared.

#### Scenario: Sage's storage
- **WHEN** the Sage Space's storage is `Sage-Social/wiffletree-storage`, which every member of Sage-Social can read
- **THEN** `spaces/sage/space.yaml` in that repository holds the Space's definition, and the Spaces page marks Sage as shared

### Requirement: Joining a shared Space
"Add Space → From a repository" SHALL take a GitHub repository, check it as for a storage repository, list the Spaces it holds and add the ones the user ticks. For each, the user SHALL say where the brain and each code repository are cloned on this Mac, or leave them missing, which threads and jobs report when they need them. A model the definition names that this Mac lacks or may not use SHALL fall back to this Mac's Who runs what choice, with a note on the Space's page naming both.

#### Scenario: A teammate joins
- **WHEN** a teammate adds `Sage-Social/wiffletree-storage` and ticks Sage
- **THEN** their Mac gets Sage's description, instructions, jobs and boards, and asks where `team-brain` and each Sage repository are cloned

#### Scenario: A model this Mac lacks
- **WHEN** the Sage definition names Opus · high for jobs and the teammate has only Codex signed in
- **THEN** Sage jobs on that Mac use its Job row, and the Space page notes "Opus · high is not available here; using GPT 6.1 Sol · medium"

### Requirement: What stays on this machine
Where each repository and brain is cloned, which models this person may use, whether the Space is enabled here, and this Mac's credentials and keys SHALL stay on this Mac and SHALL NOT be written to the storage repository. A job's runner, being a GitHub login and a machine name, SHALL be written to its recipe there (see scheduled-jobs). Threads SHALL never be written to it.

#### Scenario: Clone paths
- **WHEN** two teammates clone `mobile-app` in different folders
- **THEN** `space.yaml` names `Sage-Social/mobile-app` and no path

### Requirement: Edits to a shared Space follow the settings repository's rules
Space sync SHALL follow the settings repository's sync rules for files under `spaces/` of each GitHub storage repository: choosing the repository allows the push; it pushes only the default branch and only `spaces/`, never by force; it pulls at launch and every 5 minutes and pushes 5 seconds after a change; a file changed on two machines, or outside Wiffletree on this machine, pauses with a workspace note offering Compare and the same choices; and a file this machine cannot read is held. An output under `boards/` written by its job's runner SHALL push without comparing, re-fetching when the remote moved. When a storage repository is also this Mac's settings repository, Space sync and settings sync SHALL each commit only their own paths.

#### Scenario: Two people edit instructions
- **WHEN** two members change Sage's instructions before either pulls
- **THEN** the second to sync keeps its own, pauses `space.yaml` with a workspace note, and every other Sage file keeps syncing

#### Scenario: A shell edit
- **WHEN** an agent's shell changes `spaces/sage/space.yaml` in the storage clone
- **THEN** the change is not pushed until the user chooses Sync this version

### Requirement: Personal stays personal
A Space SHALL sync only to the storage repository the user chose for it. An output, job or entry SHALL NOT move from one Space to another unless the user chooses it with a confirmation that names the destination and who can read it. Thread outputs SHALL stay only on this Mac until the user shares them (see boards).

#### Scenario: Sharing a personal output
- **WHEN** the user chooses "Share to Space…" on a Personal output and picks Sage
- **THEN** a confirmation says every member of Sage-Social will be able to read it, and the output is copied only after the user confirms

### Requirement: Sources a Space allows
A Space's sources SHALL list the MCP servers and local CLIs its threads may use, with the tools or command prefixes each may call. Threads in the Space SHALL get only those, checked as job sources are checked (see scheduled-jobs). A source that is missing or signed out SHALL be named in the thread when it is needed, with how to fix it.

#### Scenario: Linear in a thread
- **WHEN** Sage lists the Linear MCP server with its list and read tools, and the user asks a Sage thread what is open
- **THEN** the thread can call those tools and no other Linear tool
