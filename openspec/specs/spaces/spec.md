# spaces Specification

## Purpose
Spaces are configured containers, such as Sage, Vyttle or Personal, that answer "where does this go?" for everything in Wiffletree: they hold the instructions, brain, storage and tracker, repositories and default models for the projects, threads, jobs and issues in them, and a team can share one through a Git repository.

## Requirements

### Requirement: A Space
A Space SHALL have a name; a description of what belongs in it; additions to every role's instructions in it, stored in `space.yaml` as `instructions:`, the key every release reads, and the Behaviors it sets (see `settings-cascade`); for each code repository it lists, optional Behaviors and additions for that repository's tickets; a brain, being a Git repository, on GitHub as `owner/name` or a local folder, and a folder in it, or none; a storage repository, which is also its tracker; the code repositories it covers, by identity (`owner/name`), never by path; the rows of Who runs what it overrides and what it sets about its tickets' reviews (see `settings-cascade`); and a list of sources its threads may use. Two Spaces MAY name the same brain, and two Spaces MAY share one storage repository. A Space SHALL NOT be called a project anywhere in the interface.

#### Scenario: Two Spaces, one brain
- **WHEN** the Sage and Sage Watch Spaces both name `Sage-Social/sagehaven-docs` folder `second-brain/`
- **THEN** projects in both log to that one brain

#### Scenario: Instructions from an older store
- **WHEN** this host reads a `space.yaml` an older host wrote, with `instructions:`
- **THEN** the Space's additions are those instructions, and a save here writes them back under `instructions:` with no `additions:` key, so the older host keeps reading them

### Requirement: Only the user creates or edits a Space
Spaces SHALL be created, edited and removed only on the Spaces page of Settings, by the user, apart from the Spaces the host creates at upgrade, the default "Personal" Space, and the repositories a rule the human set adds (see "A Space can follow root folders"). No agent, thread, job run, router or workspace tool SHALL create, change or remove a Space or read its machine-local facts. Removing a Space SHALL ask first, SHALL be refused while a non-archived project belongs to it or when it is the last Space, SHALL move its archived projects to the "Personal" Space, creating it if it was removed, and SHALL leave its storage and brain repositories unchanged. Removing "Personal" itself SHALL be refused while archived projects belong to it, because they would have nowhere to go.

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

#### Scenario: Removing Personal with archived projects
- **WHEN** the user removes Personal, which holds one archived project, while Sage also exists
- **THEN** the removal is refused, saying its archived projects would have nowhere to go, and nothing changes

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
A storage repository SHALL be its Spaces' tracker: it SHALL hold `wiffletree-issues.yaml` and `issues/` at its root, beside `spaces/` (see `shared-issues`). Every Space stored in one repository SHALL share its issues and its prefix. A local storage repository SHALL be a local tracker that is never pushed. There SHALL be no other list of trackers and no tracker setting outside a Space. The first commit to a new local storage repository SHALL add the Space's `space.yaml` and `wiffletree-issues.yaml`, and the first push of a Space to a GitHub repository without a tracker SHALL add `wiffletree-issues.yaml` beside the Space's folder, each in a commit of its own as every commit holds one rule set's files (see `shared-repositories`), with a prefix of 2 to 5 capital letters, which the Space form SHALL propose from the Space's name and the user MAY change. A prefix SHALL be unique among the storage repositories on this Mac. A GitHub tracker's prefix SHALL change only while it has no issue. A local tracker's prefix SHALL be renamable on its Space's page at any time, and renaming SHALL rewrite its issue IDs, folders and every reference to them in its own issues, because nothing outside this Mac refers to them. A storage repository added without `wiffletree-issues.yaml` SHALL offer Set up issues on the Space's page, which SHALL ask for a prefix, ask first, and commit only that file.

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
Every Wiffletree project SHALL belong to exactly one Space, shown and changeable in the project's Overview. New project SHALL show a Space, defaulting to the Space last used for a new project, and SHALL show that Space's repositories as plain rows that cannot be unticked, listing the ones not cloned on this Mac as such. Changing the Space in New project SHALL replace those rows with the new Space's and keep what the user added. The user MAY add other workspace repositories to the project with Add another repository…. A project's coordinator SHALL log to its Space's brain. Moving a project to another Space SHALL ask first, naming the repositories it gains, loses and keeps; SHALL change where its future entries go, leaving earlier entries where they are; and SHALL never be refused for its repositories or tickets.

#### Scenario: A project's log
- **WHEN** a coordinator in a project of the Vyttle Space records a decision
- **THEN** the entry goes to the Vyttle Space's brain

#### Scenario: Repositories from the Space
- **WHEN** the user opens New project with Sage last used, Sage lists `Sage-Social/mobile-app`, `Sage-Social/watch-app` and `Sage-Social/api`, and only the first two are workspace repositories here
- **THEN** mobile-app and watch-app are plain rows, api is listed as not cloned here, and no personal repository is listed

#### Scenario: Another Space
- **WHEN** the user added `simion/termic` in New project and switches its Space from Sage to Personal
- **THEN** the Sage rows give way to Personal's and termic stays, marked as added to this project

#### Scenario: Move says what it does
- **WHEN** the user moves a Personal project with one added repository and an open ticket in `pica-api` to Vyttle
- **THEN** the dialog lists what it gains from Vyttle, what it loses from Personal, and keeps the added repository and `pica-api`, naming the ticket

### Requirement: Existing projects get Spaces on upgrade
At the first start of this release, the host SHALL create one Space for each distinct brain folder attached to a project, named after the folder, with that folder as a local brain and local storage, and SHALL put each such project in it. Space names SHALL be unique: when two brain folders have the same name, or a folder is named "Personal", each such Space SHALL be named "‹parent folder›/‹folder›", and when that is still not unique, a number SHALL follow it. Projects with no brain SHALL go to a Space named "Personal" with no brain. Archived projects SHALL be included. Each Space created SHALL get a proposed prefix and SHALL list no repositories until the human confirms them. The upgrade SHALL suggest a Space for each existing workspace repository by its GitHub owner: the Space whose projects use that owner's repositories most, or Personal for the owner that is the human's own `gh` login, or No Space. The Repositories page SHALL show these suggestions on its first open after the upgrade, for the human to confirm or change; nothing SHALL be written to a Space's repositories before that. The upgrade SHALL push nothing, SHALL keep each project's export records so nothing is exported twice, and SHALL run once.

#### Scenario: Two brains
- **WHEN** three projects use `~/dev/sage/team-brain`, one uses `~/dev/vyttle/vyttle-brain` and two have none
- **THEN** the Spaces page lists "team-brain" with three projects, "vyttle-brain" with one and "Personal" with two, and the user can rename them

#### Scenario: Two brains with the same folder name
- **WHEN** one project uses `~/dev/sage/brain` and another uses `~/dev/vyttle/brain`
- **THEN** the upgrade creates the Spaces "sage/brain" and "vyttle/brain"

#### Scenario: A second start
- **WHEN** the host starts again after the upgrade ran
- **THEN** no Space is created or changed

#### Scenario: Repositories after the upgrade
- **WHEN** after the upgrade three Sage projects use `Sage-Social/mobile-app` and `Sage-Social/api`, and the human's `gh` login is `MHohlios`, which owns `MHohlios/dotfiles`
- **THEN** the first open of the Repositories page suggests Sage for both Sage-Social repositories and Personal for dotfiles, and no Space lists them until the human confirms

### Requirement: Repositories belong to Spaces by identity
Workspace repositories SHALL stay a separate record kept on this Mac, with their clone path, base and checks commands; Spaces SHALL NOT hold them. A Space SHALL list its repositories by `owner/name` only, in `space.yaml`, and that list SHALL be the one shared fact about which Spaces a repository is in. A repository MAY be in several Spaces, or in none. The Repositories page SHALL group repositories into one section per Space and a "No Space" section, listing a repository in several Spaces under each, and each row SHALL show one chip per Space it is in. Clicking a chip SHALL let the human add or remove Spaces, editing the same list as the Space page's Code repositories. Adding a repository SHALL suggest a Space from its GitHub owner, which the human SHALL confirm or change: a Space whose repositories share that owner, or No Space when none does. The owner SHALL only suggest, as it does for a Space's brain, and SHALL never decide. Only the human SHALL change which Spaces a repository is in, on either page, except that a rule the human set MAY add one (see "A Space can follow root folders"); no agent, thread or workspace tool SHALL.

#### Scenario: Grouped by Space
- **WHEN** `Sage-Social/api` is in Sage and Sage Watch, `Vyttle-LLC/site` in Vyttle, and `octo/scratch` in no Space
- **THEN** the Repositories page lists api under Sage and under Sage Watch with both chips, site under Vyttle, and scratch under No Space

#### Scenario: Adding a repository
- **WHEN** the human adds `Sage-Social/watch-app` while Sage lists other `Sage-Social` repositories
- **THEN** the add form suggests Sage, and the repository joins Sage only when the human confirms

#### Scenario: No match
- **WHEN** the human adds `octo/scratch` and no Space lists an `octo` repository
- **THEN** the suggestion is No Space

#### Scenario: A chip edits the Space
- **WHEN** the human removes the Sage chip from `Sage-Social/api` on the Repositories page
- **THEN** Sage's Code repositories no longer list it, the change is a commit to Sage's `space.yaml`, and the repository's clone path, base and checks are unchanged

#### Scenario: An agent cannot move a repository
- **WHEN** any agent calls any workspace tool
- **THEN** no tool adds a repository to a Space or removes one

### Requirement: Shared Spaces live in their storage repository
A Space whose storage repository is on GitHub SHALL keep its definition in that repository's `space.yaml`, so that every Mac that adds it reads the same name, description, instructions, brain, repositories, model rows, review settings and sources. Everything SHALL run on each user's own Mac; there SHALL be no cloud or headless host. A Space whose storage other people can read SHALL be shown as shared.

#### Scenario: Sage's storage
- **WHEN** the Sage Space's storage is `Sage-Social/wiffletree-storage`, which every member of Sage-Social can read
- **THEN** `spaces/sage/space.yaml` in that repository holds the Space's definition, and the Spaces page marks Sage as shared

### Requirement: Joining a shared Space
"Add Space → From a repository" SHALL take a GitHub repository, check it as for a storage repository, list the Spaces it holds and add the ones the user ticks. For each, the user SHALL say where the brain and each code repository are cloned on this Mac, or leave them missing. A row of the definition none of whose choices is signed in on this Mac SHALL fall back to this Mac's row, with a note naming both. Joining SHALL be refused when the repository's prefix is used by another storage repository on this Mac, naming it; when that one is a local tracker, the refusal SHALL tell the human to rename its prefix and offer to open its Space's page.

#### Scenario: A teammate joins
- **WHEN** a teammate adds `Sage-Social/wiffletree-storage` and ticks Sage
- **THEN** their Mac gets Sage's description, instructions and the SAGE issues, and asks where `sagehaven-docs` and each Sage repository are cloned

#### Scenario: A local prefix clashes with a shared one
- **WHEN** Personal's local tracker uses SAGE with 12 issues, and the human adds `Sage-Social/wiffletree-storage`, whose prefix is also SAGE
- **THEN** joining is refused, telling the human to rename Personal's prefix; after renaming it to ME, Personal's issues are ME-1 to ME-12 with their links rewritten, and joining succeeds

#### Scenario: A model this Mac lacks
- **WHEN** the Sage definition names Opus · high for jobs and the teammate has only Codex signed in
- **THEN** the Space page and the Models page at Sage note "Sage asks for Opus · high for the Job row, but Claude is not signed in on this Mac, so it uses this Mac's row until then"

### Requirement: What stays on this machine
Where each repository and brain is cloned, whether the Space is on here, which root folders it follows, and this Mac's credentials and keys SHALL stay on this Mac and SHALL NOT be written to the storage repository.

#### Scenario: Clone paths
- **WHEN** two teammates clone `mobile-app` in different folders
- **THEN** `space.yaml` names `Sage-Social/mobile-app` and no path

#### Scenario: A list of models from an earlier release
- **WHEN** this Mac kept a list of the models it may use in Sage
- **THEN** the list no longer limits any model, a model being usable when its provider is signed in here, it stays stored, and the Models page notes once that Sage had one

#### Scenario: Followed root folders
- **WHEN** a Space follows `~/dev/vyttle`
- **THEN** `space.yaml` holds no path, and an older store with no followed roots loads unchanged

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

### Requirement: A project uses its Space's repositories
A project SHALL use a repository when any of these holds: its Space lists the repository and it is in this Mac's workspace, matched by GitHub identity; the human added it to this project; or an open ticket of this project works there, whether or not the project is archived. The first SHALL be worked out when it is read, so that a Space edit, including one synced from another Mac, takes effect with no write to projects. A project SHALL NOT use fewer repositories than its Space lists on this Mac, and no setting SHALL give a project every workspace repository. Only the human SHALL change a project's added repositories or a Space's list, except that a rule the human set MAY add to a Space's list (see "A Space can follow root folders"). New tickets in a repository an open ticket holds SHALL be allowed until the last open one there closes, when it SHALL leave the project unless it was added.

#### Scenario: A repository joins a Space
- **WHEN** the human adds `Vyttle-LLC/wiffletree` to Vyttle, or that edit arrives from another Mac
- **THEN** every project in Vyttle can create a ticket there at once, and no project is written

#### Scenario: A repository leaves a Space
- **WHEN** the human removes a repository from a Space while a project in it has an open ticket there
- **THEN** that ticket's agents and reviewers still start, a new ticket there is allowed, and once the last open ticket there closes the repository leaves the project unless it was added

#### Scenario: A repository in no Space
- **WHEN** a project needs a repository that no Space lists
- **THEN** it reaches it only through Add to this project, and moving Space keeps it

#### Scenario: An addition the Space later lists
- **WHEN** the human added a repository, its Space later lists it, the human edits the project's other additions, and the Space then drops it
- **THEN** the project still uses it

#### Scenario: A repository in several Spaces
- **WHEN** two Spaces list a repository
- **THEN** a project reads only its own Space's list

#### Scenario: A Space entry not cloned here
- **WHEN** a Space lists a repository not cloned on this Mac
- **THEN** Overview shows it as not cloned here and it is unusable, until it joins the workspace, when it appears by itself

#### Scenario: A Space that lists nothing here
- **WHEN** a project's Space lists no repository on this Mac and nothing was added to the project
- **THEN** the project uses nothing, and Overview says so and says to add to the Space or the project

### Requirement: Overview shows where each repository comes from
A project's Overview SHALL group its repositories: "From <Space>" with Edit on Repositories, holding the Space's repositories here and its entries not cloned here; "Added to this project", each with Remove; and "An open ticket works here", naming the ticket, shown only when that applies. A repository SHALL appear once, in the first group that applies. The Add menu SHALL offer Add to this project…, Add repositories… and Attach one with a custom base…. Add to this project SHALL show the Space's repositories ticked and disabled and the others tickable, and SHALL NOT change the Space.

#### Scenario: Removing an added repository with an open ticket
- **WHEN** the human removes an added repository an open ticket works in
- **THEN** the repository moves to "An open ticket works here" and the ticket carries on

#### Scenario: Adding from Overview
- **WHEN** the human uses Add repositories… from a project's Overview for a repository its Space does not list
- **THEN** the repository joins the workspace and is added to that project

### Requirement: The upgrade to inherited repositories runs once
At the first start of this release, the host SHALL set each project's added repositories, archived projects included, to the repositories it named plus those of its open tickets, minus those its Space lists on this Mac. A project that used every repository named none, and an empty selection named none. The upgrade SHALL run once, in one transaction, and SHALL NOT change any Space's list. A store of any earlier schema SHALL still open.

#### Scenario: A project with every repository and open tickets elsewhere
- **WHEN** a Personal project used every repository and has open tickets in mudling, which only Vyttle lists
- **THEN** it uses Personal's repositories and has mudling added

#### Scenario: A selection inside its Space
- **WHEN** a Vyttle project named wiffletree and wiffletree-website, both of which Vyttle lists
- **THEN** nothing is added and it uses all of Vyttle's repositories on this Mac

#### Scenario: An archived project
- **WHEN** an archived Personal project has an open ticket in wiffletree
- **THEN** it has wiffletree added

### Requirement: Choosing a Space's repositories
A Space's page SHALL list the form's repositories, each with ×, showing at most six and then "and N more", which SHALL open the dialog, and SHALL offer Choose repositories… below them. The dialog SHALL list the workspace's repositories grouped by the folder that holds them, shown with `~`. Folders with a ticked repository SHALL come first and open, and the rest SHALL start closed and be able to open. Each row SHALL show a checkbox, the repository's name, its `owner/name` and a chip for each other Space listing it. A folder SHALL show a tri-state tick and a count.

The filter SHALL match name, `owner/name` and path, ignoring case, and SHALL accept `~/dev/vyttle` and `~/dev/vyttle/*`. Matching folders SHALL open. A folder's tick SHALL cover only the rows shown.

Space entries with no workspace repository SHALL be in a "Not cloned on this Mac" group, ticked, with the existing warning, and a box at the bottom SHALL add an `owner/name`, validated as on the Space's page before. A repository not on GitHub SHALL be shown and SHALL NOT be tickable, saying "Not on GitHub, so in no Space". Two clones of one identity SHALL tick together, and identities SHALL compare ignoring case. The footer SHALL count what is selected and what is new, and "Not listed? Add it on the Repositories page." SHALL link there. Cancel SHALL discard the dialog's changes. Done SHALL change the form's draft only, and Save SHALL write it, followed roots included.

#### Scenario: The dialog opens
- **WHEN** the human opens Choose repositories… on Vyttle, which lists 4 of the 25 repositories in `~/dev/vyttle`
- **THEN** `~/dev/vyttle` comes first and open with a half tick and "4 of 25", the other folders are closed, and the footer says "4 selected."

#### Scenario: A folder's tick on a filter
- **WHEN** the human filters by `pica`, which shows 7 of a folder's 25 rows, and ticks the folder
- **THEN** only those 7 are ticked

#### Scenario: A folder pattern
- **WHEN** the human filters by `~/dev/vyttle/*` and `~/dev/vyttle-llc` is also a folder
- **THEN** only the rows of `~/dev/vyttle` show

#### Scenario: Not on GitHub
- **WHEN** a workspace repository has no GitHub origin
- **THEN** its row says "Not on GitHub, so in no Space", its checkbox is disabled, and its folder's tick leaves it out

#### Scenario: Two clones
- **WHEN** two workspace repositories in different folders are clones of `Vyttle-LLC/site`, and the human ticks one
- **THEN** both show ticked and the Space lists `Vyttle-LLC/site` once

#### Scenario: Not cloned here
- **WHEN** a Space lists `Vyttle-LLC/example-web`, which is not in the workspace
- **THEN** it is ticked under "Not cloned on this Mac" with the warning, and unticking it and clicking Done takes it out of the draft

#### Scenario: Cancel
- **WHEN** the human ticks repositories and clicks Cancel
- **THEN** the form's draft is as it was

#### Scenario: Done is not Save
- **WHEN** the human clicks Done
- **THEN** the page shows the new rows and says the changes are kept on Save, and nothing is written until Save

### Requirement: A Space can follow root folders
A Space MAY follow any number of root folders, and two Spaces MAY follow the same root. The rule SHALL be a list of root paths in the Space's machine-local facts and SHALL NOT be written to `space.yaml`; the Space's `owner/name` list SHALL stay the one record of what it covers.

In the dialog, Follow SHALL show on root folders only, and only when all the folder's rows are shown. Turning it on SHALL tick every repository in the folder, and unticking one afterwards SHALL be allowed and SHALL keep it out. Turning it off SHALL leave the ticks. A Space's page SHALL show a line for each followed root, "Following ‹folder›: new repositories there join ‹Space›", with × to stop following, which SHALL keep the repositories.

Only a root check SHALL act on the rule. When it first adds a repository to the workspace, and no workspace repository already had that identity, its `owner/name` SHALL be appended to each Space following a root the check found it under, through the same write as any edit of the Space, so that it is an ordinary `space.yaml` commit. A toast SHALL name each repository that joins. A repository added by hand SHALL get the existing suggestion and nothing else. If writing to the Space fails, the repository SHALL stay in the workspace, the human SHALL see a warning, and it SHALL NOT be retried. Removing a root SHALL drop it from every Space's followed list and SHALL keep its repositories.

Only the human SHALL set a Space's followed roots. No agent, thread or workspace tool SHALL, and a paired device SHALL need Control.

#### Scenario: Follow selects a folder
- **WHEN** the human clicks Follow on `~/dev/vyttle`, then Done, then Save
- **THEN** its 25 repositories are listed in `space.yaml`, the Space follows the root, and the page shows "Following ~/dev/vyttle: new repositories there join Vyttle"

#### Scenario: Follow on a partial filter
- **WHEN** the filter `pica` shows some of a root folder's rows
- **THEN** that folder offers no Follow, and with `~/dev/vyttle/*` it does

#### Scenario: Not a root folder
- **WHEN** a workspace repository is in `~/dev/scratch`, which is not a root folder
- **THEN** that folder offers no Follow

#### Scenario: A new repository in a followed root
- **WHEN** Vyttle follows `~/dev/vyttle` and `~/dev/vyttle-llc`, and a new repository appears in either
- **THEN** the next root check adds it to the workspace and to Vyttle, with a `space.yaml` commit and the toast "pica-web joined Vyttle from ~/dev/vyttle."

#### Scenario: Two Spaces on one root
- **WHEN** Vyttle and Personal both follow `~/dev/vyttle` and a new repository appears there
- **THEN** it joins both

#### Scenario: A repository taken out stays out
- **WHEN** the human removes a repository from a following Space
- **THEN** later root checks do not add it back, and neither does another clone of it found in the root

#### Scenario: A repository added by hand
- **WHEN** the human adds a repository from a followed root by hand
- **THEN** it gets the existing suggestion and joins no Space by itself

#### Scenario: A repository not on GitHub
- **WHEN** a repository with no GitHub origin appears in a followed root
- **THEN** it joins the workspace and no Space

#### Scenario: The Space cannot be written
- **WHEN** a root check finds a repository and writing to a following Space fails
- **THEN** the repository stays in the workspace, the human sees a warning, and later root checks do not try again

#### Scenario: A repository joins while the form holds a draft
- **WHEN** the human has an unsaved change to a Space's repositories, or the dialog open, and a root check adds a repository to that Space
- **THEN** Done and Save keep the repository that joined and apply the human's own change

#### Scenario: Removing a root
- **WHEN** the human removes a root that two Spaces follow
- **THEN** neither Space follows it and their repositories stay listed

#### Scenario: Stopping following
- **WHEN** the human clicks × on a Following line and saves
- **THEN** the Space no longer follows that root and its repositories stay listed

#### Scenario: A shared Space
- **WHEN** a repository joins a shared Space on one Mac
- **THEN** it reaches teammates as any edit does, and their Macs follow nothing unless they turn it on

#### Scenario: An agent cannot set the rule
- **WHEN** a connection without the app's credential sends the command that sets a Space's followed roots
- **THEN** the host refuses it, and no workspace tool offers it
