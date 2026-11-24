## ADDED Requirements

### Requirement: Brain compiler rows are not Space rows
A brain can belong to several Spaces, so the Brain compiler row SHALL resolve from this Mac's table alone. Saving a Space, through any command, SHALL refuse a new or changed Brain compiler row, naming it. A Brain compiler row, or a Router row an earlier release allowed, already in `space.yaml` SHALL be ignored at read: it SHALL change no resolution, no fallback banner, no model reason and no count of what the Space changes, a note SHALL say it is ignored, and it SHALL block no other save of that Space, whether or not it keeps the table's rules. A save SHALL keep it until it is removed. The Models page at a Space SHALL show a banner naming such rows, with Remove them, only when the Space holds one, and Remove them SHALL go through the page's draft and Save. A Space's Assistant row SHALL apply to its issue threads and, without a Job row, its job runs, never to the Assistant's conversation, which belongs to no Space. A coordinator's `workspace_context` SHALL list no Brain compiler row.

#### Scenario: A new Brain compiler row
- **WHEN** a save would give Sage a Brain compiler row, through `set_space_models` or `update_space`
- **THEN** it is refused, saying the Brain compiler row belongs to this Mac, and `space.yaml` is unchanged

#### Scenario: An old Router row and an unrelated save
- **WHEN** Sage's `space.yaml` already holds a Router row and the human changes Sage's Implementer row
- **THEN** the save succeeds and keeps the Router row, which changes nothing

#### Scenario: Remove them
- **WHEN** the human presses Remove them at Sage and then Save
- **THEN** `space.yaml` no longer holds the ignored rows and the banner is gone

#### Scenario: Sage's Assistant row
- **WHEN** Sage's Assistant row is Opus · high and this Mac's is Sonnet · medium
- **THEN** a Sage issue thread starts on Opus · high, and the conversation starts on Sonnet · medium

## MODIFIED Requirements

### Requirement: A Space overrides rows of Who runs what
The Who runs what table SHALL stay per Mac and be the base layer, This Mac. A Space MAY override any of its rows but Brain compiler in `space.yaml` `models.rows`, in the table's row shape and kept to the table's rules, at most one row a job. For any agent in a project, issue thread or job in a Space, the model SHALL resolve recipe model (jobs only) → the Space's row → this Mac's row, through the cascade's resolver, so each row carries whether it came from This Mac or the Space. A Space row none of whose choices on the line in use is signed in on this Mac, or one read from a file that breaks the table's rules, SHALL fall back to this Mac's row, and the agent's model reason SHALL say so. Saving a Space, through any command, SHALL refuse a changed row the table's rules refuse. The winning row SHALL keep the table's rules for spent providers, waiting and skipping. `models.threads` and `models.jobs`, which earlier releases read, SHALL read as the first choices of the Assistant and Job rows; nothing SHALL be rewritten until a save, and a save SHALL write them to match those rows so an older host keeps using them. A Space's list of the models this Mac may use in it SHALL no longer limit anything. Only the human's client SHALL set a Space's rows.

#### Scenario: Sage overrides the Implementer row
- **WHEN** Sage's `space.yaml` sets the Implementer row to GPT 6.1 Sol · high, and this Mac's row is Opus · medium
- **THEN** Sage's implementers start on GPT 6.1 Sol · high, with a reason naming Sage's row, and the coordinator's context marks the row as Sage's

#### Scenario: The Space's provider is signed out here
- **WHEN** Sage's Assistant row names only Codex and Codex is signed out on this Mac
- **THEN** a Sage issue thread starts on this Mac's Assistant row, and its reason says Sage's row fell back because Codex is not signed in on this Mac

#### Scenario: A recipe's model
- **WHEN** a Sage job's recipe names Opus · high and Sage's Job row names GPT 6.1 Sol · high
- **THEN** the run starts on Opus · high

#### Scenario: A Space an older host saved
- **WHEN** Sage's `space.yaml` holds only `models.threads: GPT 6.1 Sol · high`
- **THEN** Sage's Assistant row is GPT 6.1 Sol · high, and a save of Sage's rows here writes `threads` again with the rows

### Requirement: Screens say where each value came from
The Models page SHALL have a scope bar, This Mac · Space ▾. At a Space it SHALL show this Mac's table, without the Brain compiler row, with each row the Space sets in its place, marked with the Space's name and a Reset that restores this Mac's row; editing a row there SHALL save a row for the Space, starting from this Mac's. Its Right now column SHALL resolve the table as the Space does on this Mac, and a banner SHALL name each Space row this Mac falls back from, counting a provider not checked yet as unavailable. A line SHALL say that Brain compiler is set on This Mac, and that the Space's Assistant row applies to its issue threads and loops, not to the conversation. Reviews in the Space SHALL show the levels and caps with the layer each came from and Reset. A Space's page SHALL hold what the Space is, not its models or additions, show its wiki's compile time read-only in the Repository box with a "Change on Brains →" link, and link to Behaviors and Models for the Space with a count of what it changes. The Settings window SHALL group its entries as Shared with your Macs, Spaces & brains and This Mac, with repository facts and Allowed owners under This Mac and a Brains entry after the Spaces. Reviews, Who runs what and Subscriptions SHALL each open their own card on Models, PR keeper "When a ticket's PR is open" and Allowed owners "Opening PRs without asking" on Repositories, and Repositories the top of its page. Effective settings, opened from a project's header, SHALL list read-only what the selected ticket's agents get, or the project's, with the layer each value came from, and SHALL NOT list the Brain compiler row.

#### Scenario: Sage overrides the Assistant row while Claude is signed out
- **WHEN** Sage's Assistant row is Opus · high then wait, and Claude is signed out on this Mac
- **THEN** the Models page at Sage shows the row with Sage's chip and Reset, Right now shows this Mac's choice marked as a fallback, and a banner says Sage asks for Opus · high and this Mac's row is used until Claude is back

#### Scenario: Reset at a Space
- **WHEN** the human presses Reset on Sage's Implementer row and saves
- **THEN** `space.yaml` no longer holds an Implementer row, and Sage's Implementers run on this Mac's row

#### Scenario: Effective settings for a ticket
- **WHEN** the human opens Effective settings on an implementer of a `sagechat-sms` ticket in Sage, whose Space entry sets Publishing to always ask first
- **THEN** Publishing reads "Always ask first" from "sagechat-sms in Sage", naming the layers it overrode

#### Scenario: From the Repository box to the Brains page
- **WHEN** the human presses "Change on Brains →" on the Compile row of Sage's Repository box
- **THEN** the Brains page opens, and nothing changes on the Space page

#### Scenario: A Settings entry opens its own card
- **WHEN** the human presses Allowed owners in the Settings sidebar
- **THEN** the Repositories page opens scrolled to "Opening PRs without asking"

## REMOVED Requirements

### Requirement: Router and Brain compiler rows are not Space rows
**Reason**: The Router row no longer exists. Replaced by "Brain compiler rows are not Space rows", which keeps the Brain compiler rule and ignores Router rows left in `space.yaml`.
**Migration**: A Router row in `space.yaml` stays, ignored, until Remove them.
