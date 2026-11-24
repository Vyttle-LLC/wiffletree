## MODIFIED Requirements

### Requirement: A project's brain comes from its Space
A project SHALL store no brain of its own. A project's brain SHALL be its Space's brain. A project a coordinator creates with `create_project` SHALL belong to the creating project's Space and SHALL use that Space's brain. Issue threads and job runs SHALL use their own Space's brain. The Assistant, which serves every Space, SHALL use the brain of the Space it names in each `query_brain` or `log_work` call (see `project-memory` and `brain-browser`), and a project it starts with `start_work` SHALL use the brain of the Space it names there. Capture, compile, search and `query_brain` SHALL resolve the brain this way and in no other way.

#### Scenario: A project in the Sage Space
- **WHEN** a project in the Sage Space works in repositories owned by `Sage-Social` and by `Vyttle-LLC`
- **THEN** its only brain is the Sage Space's brain

#### Scenario: A project created by a coordinator
- **WHEN** a coordinator in the Vyttle Space creates a project with `create_project` that adds a repository outside Vyttle
- **THEN** the new project is in the Vyttle Space and logs only to Vyttle's folder

#### Scenario: A Space on this Mac alone
- **WHEN** a project's Space is Personal, which named no brain in an earlier release
- **THEN** the project logs to Personal's own brain, on this Mac alone, and nothing is pushed

#### Scenario: The Assistant names a Space
- **WHEN** the Assistant logs one entry naming Personal and queries the brain naming Vyttle
- **THEN** the entry goes to Personal's brain, the query reads only Vyttle's, and a work item it starts in Celadon logs to Celadon's brain
