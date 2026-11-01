## ADDED Requirements

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
