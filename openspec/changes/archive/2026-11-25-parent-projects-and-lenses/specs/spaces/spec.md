## ADDED Requirements

### Requirement: A Space's repositories carry an app and a layer
Each repository a Space lists SHALL have an optional app and an optional layer, each one value from that Space's own vocabulary. `space.yaml` SHALL keep `repositories` as a list of `owner/name` strings and SHALL hold the rest in new keys: `apps` and `layers`, each the Space's ordered list of values, and `repository_tags`, mapping an `owner/name` to its `app` and `layer`. A missing key SHALL read as empty, so a Space file from before this change opens with no values and no tags. Edits SHALL be commits to `space.yaml` that sync as every other Space edit does, and two Macs editing the file SHALL pause as any other whole-file conflict does. A repository's tags SHALL be the same in every Space that lists it only when each Space sets them so: each Space keeps its own. Only the human SHALL change tags or vocabularies; no workspace tool SHALL.

#### Scenario: Tags are stored with the Space
- **WHEN** the human sets `celadon-theme/nvim` to app "nvim" and layer "frontend" in Celadon
- **THEN** Celadon's `space.yaml` lists "nvim" under `apps`, "frontend" under `layers`, and `repository_tags` maps `celadon-theme/nvim` to both, and its `repositories` list is unchanged

#### Scenario: An older Space file
- **WHEN** a Space file without `apps`, `layers` or `repository_tags` is read
- **THEN** the Space has no values and every repository is untagged, and nothing is written

#### Scenario: Two Spaces, two answers
- **WHEN** `celadon-theme/website` is in Celadon with layer "frontend" and in Vyttle with no tags
- **THEN** Celadon's projects read it as "frontend" and Vyttle's as untagged

### Requirement: Tags are edited on the Space's Repositories list
The Space's page SHALL show App and Layer beside each of its code repositories. Each SHALL be a menu of the Space's values with "None" and a field to type a new value; a typed value SHALL join the Space's vocabulary and be offered for every repository in the Space. Removing a value from the vocabulary SHALL first say which repositories use it, and on confirming SHALL clear it from them. When a repository is added to the Space, by the human or a followed root, the add SHALL suggest the tags of the Space's repository whose name shares the longest leading run of hyphen-separated words with it, the alphabetically first such name when several tie, and none when no name shares a first word. A suggestion from the human's add SHALL apply only once the human confirms it; a repository a followed root adds SHALL stay untagged.

#### Scenario: A typed value becomes a choice
- **WHEN** the human types "web" as the app of `celadon-theme/website`
- **THEN** "web" is offered as an app for every other repository in Celadon

#### Scenario: A suggestion
- **WHEN** the human adds `celadon-theme/palette-cli` to Celadon, where `celadon-theme/palette-api` has layer "backend" and no app
- **THEN** the add suggests no app and layer "backend", and the repository is tagged only once the human confirms

#### Scenario: A tie
- **WHEN** the human adds `celadon-theme/palette-cli` to Celadon, where `palette-build` and `palette-api` both share "palette" and nothing shares more
- **THEN** the add suggests the tags of `palette-api`

#### Scenario: Nothing to suggest
- **WHEN** the human adds `celadon-theme/wallpapers` and no repository in Celadon shares its first word
- **THEN** no tags are suggested

#### Scenario: Removing a value in use
- **WHEN** the human removes "backend" from Celadon's layers while two repositories use it
- **THEN** the page names both before asking, and on confirming both have no layer

### Requirement: A project's tags come from its tickets' repositories
The host SHALL derive a project's apps and layers as the union of the tags, in its Space, of the repositories of all its tickets, in any state, open, accepted or closed. A project with tickets but no app among its repositories SHALL be "no app", and one with no layer "no layer"; an untagged repository SHALL otherwise add nothing. A project with no tickets SHALL have no tags and SHALL be "not started". The tags SHALL be recomputed when a ticket is created, when its Space's tags change and when the project moves to another Space, SHALL be given on the project in every snapshot and in the coordinator's `workspace_context`, and SHALL never be set by hand.

#### Scenario: Two repositories
- **WHEN** "Contrast warnings" has a ticket in `nvim` (nvim, frontend) and a closed one in `palette-api` (no app, backend)
- **THEN** its app is "nvim", and its layers are "frontend" and "backend"

#### Scenario: No app
- **WHEN** "DNS: website Terraform" has tickets only in `celadon-infra` (no app, infra)
- **THEN** it has no app, so it is "no app", and its layer is "infra"

#### Scenario: Not started
- **WHEN** a project has no tickets
- **THEN** it has no tags and is "not started"

#### Scenario: Moving to another Space
- **WHEN** a project with a ticket in `celadon-theme/website`, layer "frontend" in Celadon and untagged in Vyttle, moves from Celadon to Vyttle
- **THEN** its next snapshot no longer gives it "frontend"

#### Scenario: Retagging a repository
- **WHEN** the human changes `google-chrome`'s app from "chrome" to "browser"
- **THEN** every project with a ticket there shows "browser" instead of "chrome" in its next snapshot
