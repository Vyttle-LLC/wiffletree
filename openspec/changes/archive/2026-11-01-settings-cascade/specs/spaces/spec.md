## MODIFIED Requirements

### Requirement: A Space
A Space SHALL have a name; a description of what belongs in it; additions to every role's instructions in it, stored in `space.yaml` as `instructions:`, the key every release reads, and the Behaviors it sets (see `settings-cascade`); for each code repository it lists, optional Behaviors and additions for that repository's tickets; a brain, being a Git repository, on GitHub as `owner/name` or a local folder, and a folder in it, or none; a storage repository, which is also its tracker; the code repositories it covers, by identity (`owner/name`), never by path; the rows of Who runs what it overrides and what it sets about its tickets' reviews (see `settings-cascade`); and a list of sources its threads may use. Two Spaces MAY name the same brain, and two Spaces MAY share one storage repository. A Space SHALL NOT be called a project anywhere in the interface.

#### Scenario: Two Spaces, one brain
- **WHEN** the Sage and Sage Watch Spaces both name `Sage-Social/sagehaven-docs` folder `second-brain/`
- **THEN** projects in both log to that one brain

#### Scenario: Instructions from an older store
- **WHEN** this host reads a `space.yaml` an older host wrote, with `instructions:`
- **THEN** the Space's additions are those instructions, and a save here writes them back under `instructions:` with no `additions:` key, so the older host keeps reading them

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
Where each repository and brain is cloned, whether the Space is on here, and this Mac's credentials and keys SHALL stay on this Mac and SHALL NOT be written to the storage repository.

#### Scenario: Clone paths
- **WHEN** two teammates clone `mobile-app` in different folders
- **THEN** `space.yaml` names `Sage-Social/mobile-app` and no path

#### Scenario: A list of models from an earlier release
- **WHEN** this Mac kept a list of the models it may use in Sage
- **THEN** the list no longer limits any model, a model being usable when its provider is signed in here, it stays stored, and the Models page notes once that Sage had one
