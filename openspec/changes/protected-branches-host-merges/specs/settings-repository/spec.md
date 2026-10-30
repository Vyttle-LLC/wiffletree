## MODIFIED Requirements

### Requirement: Shared settings and settings that stay on this machine
The shared settings SHALL be: the skills, with the human's rules, rewrites, their bases and their history; the verification setting, with its verifiers, review levels, rounds per review and reviews per ticket; the model guide; the switch threshold; the two check-in thresholds; and the PR keeper's triggers, quiet period and review level. Every other setting SHALL stay on this machine, including the Who runs what table, the PR keeper's row among it, the workspace folder, the allowed owners, the settings repository chosen, the repositories with their checks commands and protected branches, appearance, and the Spaces added on this machine and their machine-local facts. Space definitions and issues SHALL travel only through each Space's storage repository (see `spaces`), never as shared settings. Credentials, the database, conversations, projects and tickets SHALL NOT be shared, and the host SHALL NOT write them to the shared folder.

#### Scenario: What a second machine receives
- **WHEN** a machine joins a settings repository set up by another machine that allows `Sage-Social` and moves on at 90%
- **THEN** it gets the other machine's review levels, guide, switch threshold, check-in thresholds, keeper triggers and skills, and its own allowed owners, Who runs what table, workspace folder, Spaces and repositories' protected branches are unchanged
