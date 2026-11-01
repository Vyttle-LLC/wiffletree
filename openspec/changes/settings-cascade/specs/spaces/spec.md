## MODIFIED Requirements

### Requirement: A Space
A Space SHALL have a name; a description of what belongs in it; additions to every role's instructions in it, stored in `space.yaml` as `instructions:`, the key every release reads, and the Behaviors it sets (see `settings-cascade`); for each code repository it lists, optional Behaviors and additions for that repository's tickets; a brain, being a Git repository, on GitHub as `owner/name` or a local folder, and a folder in it, or none; a storage repository, which is also its tracker; the code repositories it covers, by identity (`owner/name`), never by path; default models for threads and for jobs, each optional; and a list of sources its threads may use. Two Spaces MAY name the same brain, and two Spaces MAY share one storage repository. A Space SHALL NOT be called a project anywhere in the interface.

#### Scenario: Two Spaces, one brain
- **WHEN** the Sage and Sage Watch Spaces both name `Sage-Social/sagehaven-docs` folder `second-brain/`
- **THEN** projects in both log to that one brain

#### Scenario: Instructions from an older store
- **WHEN** this host reads a `space.yaml` an older host wrote, with `instructions:`
- **THEN** the Space's additions are those instructions, and a save here writes them back under `instructions:` with no `additions:` key, so the older host keeps reading them
