## ADDED Requirements

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

## MODIFIED Requirements

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
