## MODIFIED Requirements

### Requirement: The fixed core holds what the app depends on
The fixed core SHALL hold every sentence that names a workspace tool, a report kind or the findings format, a message from Wiffletree, a folder or branch the app assigns, or something the workspace does or refuses. A skill SHALL hold only ways of working. Where another requirement says what bundled instructions say, it SHALL hold for the fixed core with every skill at stock; its statements of the first kind SHALL be in the fixed core, so they reach the agent whatever the human writes. The fixed core SHALL say that its tools and rules still apply where a skill disagrees. The fixed core SHALL NOT be editable.

#### Scenario: An empty rewrite
- **WHEN** every skill is rewritten to an empty text
- **THEN** every role's instructions still name its workspace tools, the report kinds, the findings format, the publishing rules the human confirmed on October 13, 2026 as the guard changed them on November 23, 2026, and the process rules

#### Scenario: A stock statement
- **WHEN** every skill is at stock
- **THEN** the coordinator's instructions state the stop rule, the implementer's state the objection rule, and the verifiers' define a blocking finding

#### Scenario: Rules that contradict the core
- **WHEN** the human's rules for Implementing say to push the branch with `git push origin main`
- **THEN** the rules are sent as written, the core still says an implementer pushes only its own ticket's branch, the guard refuses the push, and `publish_ticket` refuses what it refused before

### Requirement: The Skills page
The sidebar's Workspace section SHALL have a Behaviors row after Models, opening the Behaviors page, which was the Skills page; a link to a skill SHALL open that skill's page there. The page SHALL have a scope bar of Workspace, a Space and one of that Space's repositories, the repository choice off until a Space is chosen, and SHALL open at a scope it is given. At the scope it SHALL list each role's Behaviors, each with its name, what agents are told for its value, a switch or a pick, a chip naming the layer its value came from (Stock, Workspace, the Space, the repository, or "Set here" for the scope's own) and, for a value the scope sets, Reset to what the scope inherits. A Behavior the scope's layer may not set SHALL show its value read-only, and at a Space or repository a safety Behavior SHALL offer only the choices at least as strict as it inherits, with a line saying so. At the Workspace each role's additions SHALL be its skill's rules, edited on the skill's page; a Space and a repository SHALL each have one additions editor, and a shared Space's scope SHALL say it is shared with the Space's members. The page SHALL read and save each layer only through `layer_values` and `set_layer_values`, show a refusal beside what it refused, and write nothing without the human's action. At the Workspace only, an Advanced disclosure SHALL list the five skills, each with what it is for, the roles that use it and its state ("Stock", "Stock and N of your rules", "Rewritten", "Rewritten · updated", "Rewritten · out of date", "Rewritten · needs you"), and the fixed core, marked read-only. A skill's page SHALL show its stock text, its mode and editor, Save and Discard, Reset to stock, its history, and what the app's own operations enforce whatever the text says, including that Wiffletree's guard refuses agents' pull request writes and every push but an implementer's to its own ticket's branch before its pull request is open. It SHALL NOT claim the app enforces more: it SHALL say that the guard is a guardrail, not a sandbox, so an agent's shell can still reach GitHub with the human's credentials through a full path to `git` or `gh` or a direct call. The fixed core's page SHALL show its text and why it cannot be edited. The Reviewing page SHALL show each review level's "Use it" line read-only, with a link to the Models page, and the Coordinating page SHALL link to the model guide.

#### Scenario: Opening the page
- **WHEN** the human chooses Behaviors in the sidebar
- **THEN** the page shows the Workspace's Behaviors by role, each with its chip, and Advanced lists Coordinating, Implementing, Reviewing, Keeping a PR and Assisting with their states, and the fixed core marked read-only

#### Scenario: A value set at a Space
- **WHEN** the human sets Status table for Sage, which inherits it from the Workspace
- **THEN** the row's chip reads "Set here", and Reset to Workspace puts back the Workspace's value

#### Scenario: A safety Behavior at a Space
- **WHEN** the Workspace sets Publishing to "Always ask first" and the human opens Sage
- **THEN** Publishing offers only "Always ask first", and the page says a Space or repository may only make it stricter

#### Scenario: A layer the Behavior does not allow
- **WHEN** the human opens sagechat-sms in Sage
- **THEN** Status table shows its value read-only

#### Scenario: Advanced at the Workspace only
- **WHEN** the human opens a Space or a repository
- **THEN** there is no Advanced, and the scope's additions editor is shown instead

#### Scenario: The fixed core
- **WHEN** the human opens the fixed core
- **THEN** its text is shown with no editor and no Reset

#### Scenario: Updated and needs you
- **WHEN** Coordinating was merged into v2 and its note is not dismissed, and Reviewing has a question waiting
- **THEN** Advanced shows Coordinating as "Rewritten · updated" and Reviewing as "Rewritten · needs you", and the sidebar's Behaviors row and Advanced show the out-of-date mark for Reviewing

#### Scenario: What the guard holds
- **WHEN** the human opens Implementing
- **THEN** the page says the guard refuses pull request writes and every push but the implementer's own branch, and that it is a guardrail, not a sandbox
