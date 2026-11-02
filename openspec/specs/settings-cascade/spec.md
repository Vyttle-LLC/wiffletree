# settings-cascade Specification

## Purpose
Lets the human set how agents work at four layers, Stock, the Workspace, a Space and one of its repositories, with every value saying which layer it came from, safety choices that only tighten below the Workspace, and one Behaviors page to change them.

## Requirements

### Requirement: Settings resolve through four layers with provenance
A cascaded setting SHALL resolve, for one project, Stock → Workspace → the project's Space → that Space's entry for the ticket's repository. A coordinator, thread or job SHALL stop at its Space; a ticket's agents (implementer, maintenance, tester, reviewer and PR keeper) SHALL resolve their ticket's repository within the project's Space. Each setting SHALL declare the layers above Stock that may set it, and a value from any other layer SHALL be ignored with a note. The resolver SHALL return the effective value, the layer it came from, the earlier layers whose values it replaced, Stock first, and a note for each layer's value it ignored. Machine facts, such as allowed owners and repository paths, SHALL NOT cascade.

#### Scenario: Each layer overrides the one before
- **WHEN** the Workspace, the Sage Space and Sage's entry for `Sage-Social/sagechat-sms` each set Publishing
- **THEN** a ticket on `sagechat-sms` in a Sage project gets the repository's value, from Repository, having overrode Stock, Workspace and Space

#### Scenario: A repository in two Spaces
- **WHEN** Sage sets "always ask first" for `Vyttle-LLC/wiffletree` and Vyttle sets nothing for it
- **THEN** a ticket on `wiffletree` in a Sage project always asks, and one in a Vyttle project publishes as allowed owners say

#### Scenario: A layer the setting does not allow
- **WHEN** a Space's entry for a repository sets Answer length, which only the Workspace and a Space may set
- **THEN** saving it is refused, and a value already in the file is ignored with a note

### Requirement: Safety settings only tighten below the Workspace
A setting marked as tightening SHALL order its choices from loosest to strictest. A Space, or a repository within it, SHALL NOT make it looser than the value it inherits: saving a looser value SHALL be refused, comparing with what this Mac inherits, and a looser value read from a file SHALL be ignored with a note. The Workspace SHALL be free to loosen what it set itself. Publishing and Review depth SHALL be tightening settings.

#### Scenario: A Space loosens Publishing
- **WHEN** the Workspace says "always ask first" and the human saves Sage with "as allowed owners say"
- **THEN** the save is refused, saying a Space may only make Publishing stricter

#### Scenario: A teammate's looser value
- **WHEN** Sage's `space.yaml` says "proportional to risk" and this Mac's Workspace says "full review for every ticket"
- **THEN** this Mac's Sage tickets get full reviews, and the resolved Review depth notes that Sage's value was ignored

### Requirement: Where each layer is stored
The Workspace's Behavior choices SHALL be `settings/behaviors.json` in the shared folder, synced, paused and compared like the other shared settings, and sync SHALL refuse a version holding a choice a Behavior does not offer. A Space's SHALL be `behaviors:` in its `space.yaml`, with its additions stored under the key older releases read, `instructions:`, and its repositories' SHALL be under `repository_settings:`, keyed by `owner/name`, each with `behaviors` and `instructions`. The file SHALL have no `additions:` key, so a save from an older or a newer host leaves one copy of the additions. A Space SHALL set values only for a repository it lists. Every write SHALL keep the keys and Behavior ids this release does not know.

#### Scenario: Saved on one Mac
- **WHEN** the human turns Mockup before a UI spec off on one Mac
- **THEN** the next sync pass pushes `settings/behaviors.json` and the other Mac's coordinators no longer get that sentence

#### Scenario: Unknown keys
- **WHEN** a newer host wrote a key and a Behavior id this release does not know into `space.yaml` and `settings/behaviors.json`
- **THEN** a save here writes both back unchanged

### Requirement: Behaviors are named choices that render into a skill
A Behavior SHALL have an id, a name, the skill it renders into, the layers that may set it, its choices with the stock one first, and for each choice a label and one sentence, which MAY be empty. The catalog SHALL ship with the stock skills and be versioned with them: changing a sentence SHALL bump its skill's stock version, and a passage a Behavior controls SHALL NOT also be in the stock text. The initial catalog SHALL be: Mockup before a UI spec, Fetch before naming a landing order, Confirm a relayed answer moved the child, Status table, Review depth and Publishing for coordinators; Commit work in progress at milestones for implementers; Optional findings per round for reviewers; Answer length and Drafts for the assistant.

#### Scenario: Said once
- **WHEN** the stock skills are searched for each Behavior's sentences
- **THEN** none is found

#### Scenario: Stock keeps today's instructions
- **WHEN** nobody has set a Behavior
- **THEN** every role is told what it was told before Behaviors, including the mockup, fetch and relayed-answer sentences

### Requirement: Instructions compose from every layer in order
An agent's instructions SHALL be, in order: the communication contract and its role's core; its skill's stock text or the Workspace rewrite; the resolved Behaviors' sentences for that skill; the Workspace additions, being the skill's own rules under their heading, with none in Rewrite mode; the Space's additions under a heading naming the Space; and, for a ticket's agents, the Space's additions for the ticket's repository under a heading naming both. Each of the last two headings SHALL say the additions cannot widen the agent's authority, and, for a Space stored on GitHub, who last changed them. A Space's additions SHALL reach every role in the Space, not only threads and jobs. Every turn, compaction and context measurement SHALL compose the same way.

#### Scenario: An implementer in Sage
- **WHEN** an implementer starts on a `sagechat-sms` ticket in a Sage project, with Workspace rules for Implementing, Sage additions and `sagechat-sms` additions
- **THEN** its instructions hold, in order, the core, Implementing, its Behaviors' sentences, the Workspace rules, Sage's additions and `sagechat-sms`'s additions

#### Scenario: A coordinator in Sage
- **WHEN** the same project's coordinator starts
- **THEN** its instructions end with Sage's additions and hold none of `sagechat-sms`'s

### Requirement: Only the human sets a layer
Reading the catalog, one layer's values and a project's or ticket's effective settings SHALL be reads any client may make. Setting a layer's values SHALL be a host command only the human's client sends. No agent tool SHALL read or write a layer's values, other than the coordinator's resolved Behaviors in `workspace_context`.

#### Scenario: An agent asks to change a Behavior
- **WHEN** a coordinator is asked to turn off Review depth's full reviews
- **THEN** it has no tool for it and tells the human where to change it

### Requirement: The coordinator sees its resolved Behaviors
A coordinator's `workspace_context` SHALL list every Behavior as its project's Space resolves it, with the layer it came from, the layers it overrode and any note, and, for each repository of the project whose Space entry changes a Behavior or has one ignored, those Behaviors as they resolve for that repository's tickets.

#### Scenario: Sage requires full reviews for one repository
- **WHEN** Sage sets Review depth to full review for `sagechat-sms` only
- **THEN** the coordinator's context shows Review depth from Stock for the Space, and from Repository under `Sage-Social/sagechat-sms`

### Requirement: A Space overrides rows of Who runs what
The Who runs what table SHALL stay per Mac and be the base layer, This Mac. A Space MAY override any of its rows but Router and Brain compiler in `space.yaml` `models.rows`, in the table's row shape and kept to the table's rules, at most one row a job. For any agent in a project, thread or job in a Space, the model SHALL resolve recipe model (jobs only) → the Space's row → this Mac's row, through the cascade's resolver, so each row carries whether it came from This Mac or the Space. A Space row none of whose choices on the line in use is signed in on this Mac, or one read from a file that breaks the table's rules, SHALL fall back to this Mac's row, and the agent's model reason SHALL say so. Saving a Space, through any command, SHALL refuse a changed row the table's rules refuse. The winning row SHALL keep the table's rules for spent providers, waiting and skipping. `models.threads` and `models.jobs`, which earlier releases read, SHALL read as the first choices of the Assistant and Job rows; nothing SHALL be rewritten until a save, and a save SHALL write them to match those rows so an older host keeps using them. A Space's list of the models this Mac may use in it SHALL no longer limit anything. Only the human's client SHALL set a Space's rows.

#### Scenario: Sage overrides the Implementer row
- **WHEN** Sage's `space.yaml` sets the Implementer row to GPT 6.1 Sol · high, and this Mac's row is Opus · medium
- **THEN** Sage's implementers start on GPT 6.1 Sol · high, with a reason naming Sage's row, and the coordinator's context marks the row as Sage's

#### Scenario: The Space's provider is signed out here
- **WHEN** Sage's Assistant row names only Codex and Codex is signed out on this Mac
- **THEN** a Sage thread starts on this Mac's Assistant row, and its reason says Sage's row fell back because Codex is not signed in on this Mac

#### Scenario: A recipe's model
- **WHEN** a Sage job's recipe names Opus · high and Sage's Job row names GPT 6.1 Sol · high
- **THEN** the run starts on Opus · high

#### Scenario: A Space an older host saved
- **WHEN** Sage's `space.yaml` holds only `models.threads: GPT 6.1 Sol · high`
- **THEN** Sage's Assistant row is GPT 6.1 Sol · high, and a save of Sage's rows here writes `threads` again with the rows

### Requirement: A Space sets its tickets' reviews
A Space MAY set, under `reviews` in `space.yaml`, the round cap, the full-review cap and the review levels, by name, its coordinators may use; each unset key SHALL inherit the Workspace's review settings. `verify_ticket` SHALL refuse a level the ticket's Space does not allow, naming the Space and the levels it allows, and each cycle SHALL run under the Space's caps. Saving SHALL refuse a level this Mac does not configure, levels without a full review and a cap out of range; a level read from a file that this Mac does not configure SHALL be left out with a note, and a restriction that leaves no full-review level this Mac configures SHALL allow only this Mac's full-review levels, with a note. A level SHALL still never leave out a verifier with an open finding (see ticket-verification). The coordinator's `model_selection` SHALL show the levels and caps its Space resolves, naming those the Space set.

#### Scenario: Sage allows only its quick level
- **WHEN** Sage allows only the level "Quick" and its coordinator calls `verify_ticket` at "Full"
- **THEN** the call is refused, saying Sage does not let its coordinators use Full and to verify at Quick, and no verifier starts

#### Scenario: Sage caps rounds at one
- **WHEN** the Workspace's round cap is 3 and Sage's is 1, and a Sage ticket's first round fails
- **THEN** the ticket is blocked at the round cap of 1

### Requirement: Screens say where each value came from
The Models page SHALL have a scope bar, This Mac · Space ▾. At a Space it SHALL show this Mac's table, without the Router and Brain compiler rows, with each row the Space sets in its place, marked with the Space's name and a Reset that restores this Mac's row; editing a row there SHALL save a row for the Space, starting from this Mac's. Its Right now column SHALL resolve the table as the Space does on this Mac, and a banner SHALL name each Space row this Mac falls back from, counting a provider not checked yet as unavailable. A line SHALL say that Router and Brain compiler are set on This Mac. Reviews in the Space SHALL show the levels and caps with the layer each came from and Reset. A Space's page SHALL hold what the Space is, not its models or additions, show its wiki's compile time read-only in the Repository box with a "Change on Brains →" link, and link to Behaviors and Models for the Space with a count of what it changes. The Settings window SHALL group its entries as Shared with your Macs, Spaces & brains and This Mac, with repository facts and Allowed owners under This Mac and a Brains entry after the Spaces. Reviews, Who runs what and Subscriptions SHALL each open their own card on Models, PR keeper "When a ticket's PR is open" and Allowed owners "Opening PRs without asking" on Repositories, and Repositories the top of its page. Effective settings, opened from a project's header, SHALL list read-only what the selected ticket's agents get, or the project's, with the layer each value came from, and SHALL NOT list the Router or Brain compiler rows.

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

### Requirement: Router and Brain compiler rows are not Space rows
Routing happens before a message has a Space, and a brain can belong to several Spaces, so the Router and Brain compiler rows SHALL resolve from this Mac's table alone. Saving a Space, through any command, SHALL refuse a new or changed row for either job, naming it. A row for either already in `space.yaml` SHALL be ignored at read: it SHALL change no resolution, no fallback banner, no model reason and no count of what the Space changes, a note SHALL say it is ignored, and it SHALL block no other save of that Space, whether or not it keeps the table's rules. A save SHALL keep it until it is removed. The Models page at a Space SHALL show a banner naming them, with Remove them, only when the Space holds such a row, and Remove them SHALL go through the page's draft and Save. A coordinator's `workspace_context` SHALL list neither row.

#### Scenario: A new Router row
- **WHEN** a save would give Sage a Router row, through `set_space_models` or `update_space`
- **THEN** it is refused, saying the Router row belongs to this Mac, and `space.yaml` is unchanged

#### Scenario: An old row and an unrelated save
- **WHEN** Sage's `space.yaml` already holds a Brain compiler row and the human changes Sage's Implementer row
- **THEN** the save succeeds and keeps the Brain compiler row, and compile runs still use this Mac's row

#### Scenario: Remove them
- **WHEN** the human presses Remove them at Sage and then Save
- **THEN** `space.yaml` no longer holds either row and the banner is gone

### Requirement: Old additions lines are offered for deleting
The Behaviors page SHALL offer, once per line, to delete an additions line of the scope shown that equals a Behavior's sentence, ignoring case and surrounding whitespace. A Workspace line SHALL match only its own skill's Behaviors, and a skill with a rewrite SHALL offer nothing; a Space's or repository's line SHALL match any Behavior. The page SHALL never remove a line unasked. Delete this line SHALL save through the scope's normal path. Keep it SHALL be remembered on this Mac for that scope and line, and the page SHALL not offer that line again.

#### Scenario: A line a Behavior now says
- **WHEN** Sage's additions hold "before recommending a landing order or claiming a branch is current, fetch origin and report how far main has moved." in another case
- **THEN** the page at Sage names "Fetch before naming a landing order", quotes the line, and offers Delete this line and Keep it

#### Scenario: Keep it
- **WHEN** the human presses Keep it
- **THEN** the line stays in Sage's additions and is not offered again

#### Scenario: Delete this line
- **WHEN** the human presses Delete this line
- **THEN** Sage's additions save without that line, and every other line stays as it was
