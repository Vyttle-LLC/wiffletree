## MODIFIED Requirements

### Requirement: A project's brain comes from its Space
A project SHALL store no brain of its own. A project's brain SHALL be its Space's brain, or none when its Space names none. A project a coordinator creates with `create_project` SHALL belong to the creating project's Space and SHALL use that Space's brain. Assistant threads and job runs SHALL use their own Space's brain. Capture, compile, search and `query_brain` SHALL resolve the brain this way and in no other way. A project whose Space has no brain SHALL have none, and its Memory panel SHALL say so with a link to Settings › Spaces.

#### Scenario: A project in the Sage Space
- **WHEN** a project in the Sage Space works in repositories owned by `Sage-Social` and by `Vyttle-LLC`
- **THEN** its only brain is the Sage Space's brain, `sagehaven-docs`

#### Scenario: A project created by a coordinator
- **WHEN** a coordinator in the Vyttle Space creates a project with `create_project` that adds a repository outside Vyttle
- **THEN** the new project is in the Vyttle Space and logs only to `vyttle-docs`

#### Scenario: No brain
- **WHEN** a project's Space names no brain
- **THEN** the project has no brain and its Memory panel says so, with a link to Settings › Spaces
