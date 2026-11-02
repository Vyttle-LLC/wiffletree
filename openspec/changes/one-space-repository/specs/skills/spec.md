## ADDED Requirements

### Requirement: The second-brain skills are retired
Wiffletree SHALL own logging, querying, compiling and reconciling, through `log_work`, the capture form, `query_brain`, the brain browser, ⌘K, the built-in compile job and the Inbox's conflict cards. The fixed core and the stock skills SHALL NOT name `brain-log`, `brain-query`, `brain-compile` or `brain-reconcile`, or tell an agent to write to a brain by hand. Log entries SHALL be described as landing straight in, and the words batch and pull request SHALL NOT be used for them. The offer of `brain-query` to save an answer as a note SHALL have no counterpart: an agent logs a finding instead. The compile procedure SHALL stay bundled, at v2 (see `brain-compile`). Each part of the fixed core whose text this changes SHALL gain a new version.

#### Scenario: An implementer's instructions
- **WHEN** the host builds an implementer's instructions with every skill at stock
- **THEN** they name `log_work` and `query_brain`, and none of the four retired skills, and they do not say entries reach the brain in a pull request

#### Scenario: Worth keeping
- **WHEN** an agent finds something worth remembering after answering from `query_brain`
- **THEN** it logs the finding with `log_work`, and the next compile folds it in
