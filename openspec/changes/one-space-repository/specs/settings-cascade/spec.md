## MODIFIED Requirements

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

#### Scenario: From a Space's brain row to the Brains page
- **WHEN** the human presses "Change on Brains →" on the Compile row of Sage's Repository box
- **THEN** the Brains page opens, and nothing changes on the Space page

#### Scenario: A Settings entry opens its own card
- **WHEN** the human presses Allowed owners in the Settings sidebar
- **THEN** the Repositories page opens scrolled to "Opening PRs without asking"
