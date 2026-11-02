## ADDED Requirements

### Requirement: Settings stay at the repository's root
The settings repository's `wiffletree.json`, `settings/` and `skills/` SHALL stay at the repository's root, whether or not the repository also holds a Wiffletree folder (see `shared-repositories`), and moving a repository into its folder SHALL NOT move them. "Anything outside the folder is left alone" describes a Space's writes: the settings repository is a separate choice, and its rule set writes only those three paths.

#### Scenario: One repository for both
- **WHEN** `Sage-Social/wiffletree-storage` is both this Mac's settings repository and Sage's repository, with folder `wiffletree/`
- **THEN** a skill edit commits under `skills/` at the root, and an issue commits under `wiffletree/issues/`

## MODIFIED Requirements

### Requirement: The human chooses the settings repository on each machine
The Settings window SHALL have a Settings repository section after Check-ins. The human SHALL name a repository on `github.com` as `owner/name` or as an HTTPS or SSH URL. Before anything is written, the host SHALL ask GitHub, read-only, for the repository's visibility, its owner and whether the owner is an organization, and an organization's base permission for members where GitHub reports it, and SHALL check with `git ls-remote` that this machine can read the repository. A public repository SHALL be refused. The section SHALL say who can read the repository: for a user's repository, only the user and the people the user invites; for an organization whose base permission is Read or more, in yellow, that every member of that organization can read it; and where GitHub does not report the base permission, that members may be able to read it. Setting up a repository without `wiffletree.json` SHALL ask first, SHALL add `wiffletree.json`, `settings/` and `skills/` from this machine's shared folder in one commit on the repository's default branch, push it, and SHALL NOT change or remove any other file. The commit SHALL be made in the repository's clone at `<data folder>/repos/github.com/<owner>/<name>/`, reusing it when the repository is already a Space's repository; once it is pushed, that clone SHALL be the shared folder and the settings files of `<data folder>/shared/` SHALL move to `<data folder>/backups/shared-<time>/`. When another settings repository is chosen, setting up SHALL first run the Stop syncing transition for it, as joining does, and no clone SHALL be moved to `backups/`. The host SHALL NOT create repositories on GitHub for this section. The choice, its push URL and its branch SHALL be saved in this machine's host settings and SHALL NOT be synced. Only the human's client SHALL change it; no agent tool SHALL.

#### Scenario: A user's private repository
- **WHEN** the human checks `MHohlios/wiffletree-storage`, a private repository of the user MHohlios
- **THEN** the section says only MHohlios and the people MHohlios invites can read it, and offers Set up

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
- **WHEN** the human sets up `Sage-Social/wiffletree-storage`, already the Sage Space's repository, as the settings repository
- **THEN** the settings files are committed in the existing clone under `repos/`, that clone becomes the shared folder with every setting's value unchanged, and the old `<data folder>/shared/` is in `backups/`

#### Scenario: A repository that was a brain
- **WHEN** the human checks `Sage-Social/sagehaven-docs`, which an older Space names as its brain
- **THEN** it is checked like any other private repository and may be used

#### Scenario: An agent cannot choose a repository
- **WHEN** any agent calls any workspace tool
- **THEN** no tool reads or changes the shared folder or the settings repository chosen
