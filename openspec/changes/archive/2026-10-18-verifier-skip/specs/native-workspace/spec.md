## MODIFIED Requirements

### Requirement: Configurable routing and memory
Context panels SHALL show scoped logs, support milestone capture and show persisted activity and attention. Optional Git history SHALL show real read-only data. The Models page SHALL follow `ui/models.html` with these cards:
- **Subscriptions:** each provider's CLI status and plan from detection, and a meter for each window of its latest quota reading, with a tick at the switch threshold, the percentage, the reset time and the reading's age. A provider without telemetry SHALL read "Unavailable", never 0%.
- **When a subscription runs low:** the switch threshold, and that when every choice is past it Wiffletree waits for the earliest reset, or skips the review where its row says skip. It SHALL say the rule applies when an agent starts and that a running agent keeps its model.
- **Who runs what:** one row per job with dropdowns for the first choice, the then choice (or wait, or on a verifier row skip) and the small-work line, built from the installed CLIs' catalogs plus saved values, and a Right now column with what each row would use now, or that it waits until a reset. A verifier row's then menu SHALL offer "Wait for the reset" and, under it, "Skip this review" with the line "The others run without it", and a cell set to skip SHALL read "skip". A row that would skip SHALL show a grey "Skips" pill with the reset time under Right now, whose tooltip gives the reason in the present tense, such as "Skipped: Claude's weekly limit is at 96% (moves on at 95%). Runs again after the reset, Mon 09:00." A PR keeper row SHALL offer No model. The page SHALL show the table's one-time notes, with Dismiss, and a note for each row copied for a verifier without one.
- **Reviews:** a Levels block, the verifier list (role, focus, instruction), the round cap and the cycle cap, with "Run reviso:style with Codex" as example text. The Levels block SHALL show, for each review level, its name, its kind as a choice between "Full review" and "Re-check", one tick for each configured verifier, and its note on when to use it, labelled "Use it". It SHALL offer Add level, a control that opens a level's name and note for editing, and Remove. It SHALL say that a full review can be a ticket's only review, that a re-check starts only on a ticket whose last review passed, and that the workspace enforces this. A new level SHALL start as a re-check. While no level is a re-check, no level is named "Light" ignoring case and a reviewer is configured, the Add level form SHALL be filled in with the name "Light", the first configured reviewer ticked and the suggested Light note. Otherwise the form SHALL open empty, as a re-check. Adding a verifier SHALL tick it in every level whose kind is full review, and in no re-check level, in the same save. Removing a verifier SHALL remove it from every level in the same save. When the host refuses a change, the card SHALL show the host's reason and keep showing the saved setting. The round cap SHALL be labelled "Rounds per review", with the caption "The first review plus re-checks after the implementer's fixes. If the last round still finds blocking problems, the review stops and the coordinator decides. The same at every level." The cycle cap SHALL be labelled "Full reviews per ticket", with the caption "Reviews at a level marked Full review, with fresh reviewers. After the last one, the coordinator accepts, accepts with a waiver, closes the ticket or asks you. Re-checks are not counted." Each change saves at once. Adding a verifier SHALL add its row to Who runs what, as a copy of the first reviewer's row until the page is saved. The card SHALL NOT warn about turn limits, because there are none. Settings SHALL NOT edit the verifier list.
- **Notes:** the guide, a Markdown editor labelled as optional notes for coordinators.

The table, the switch threshold and the notes SHALL save together with one Save.

The page SHALL NOT offer a Providers allowlist, a Roles card, a verifier provider or a separate PR keeper model.

#### Scenario: Unavailable integration
- **WHEN** a user opens provider quota, semantic compilation or PR integration
- **THEN** the UI identifies unavailable capability instead of presenting sample data as live

#### Scenario: Right now
- **WHEN** Claude's weekly window reads 96% with the threshold at 95
- **THEN** the Subscriptions card shows the weekly meter past its tick, and Right now shows Codex for the Implementer row and "Waits" with the reset for the Claude reviewer

#### Scenario: Configure a single-subscription machine
- **WHEN** on a machine where only Codex is signed in, the human sets every row to Codex models, edits the notes and saves
- **THEN** all of it survives restart and appears in the coordinator's next workspace context

#### Scenario: Role left without a provider
- **WHEN** the human sets a row's then choice to a model of the first choice's provider
- **THEN** that row shows an inline error and Save stays disabled until the then choice uses another provider or is wait

#### Scenario: Edit and save the table
- **WHEN** the human changes the Tester row to GPT 6.1 Sol · high then Sonnet · high, sets the threshold to 90, edits the notes and saves
- **THEN** all of it survives restart and appears in the coordinator's next workspace context

#### Scenario: Cycle cap on the Review card
- **WHEN** the human sets the cycle cap to 3 on the Reviews card
- **THEN** it saves at once, survives restart and appears in the coordinator's next workspace context

#### Scenario: Caps in plain words
- **WHEN** the human opens the Reviews card
- **THEN** it shows "Rounds per review" and "Full reviews per ticket", each with its caption, and no control is labelled "Round cap" or "Cycle cap"

#### Scenario: Long verifier list
- **WHEN** the human adds a seventh verifier on the Reviews card
- **THEN** it saves at once and the card shows no turn-limit note

#### Scenario: Levels on the Review card
- **WHEN** the human opens the Reviews card with the levels "Full" and "Light" and four verifiers
- **THEN** each level shows its name, whether it is a full review or a re-check, a tick for each of the four verifiers and its "Use it" line, above the verifier list

#### Scenario: A tick saves at once
- **WHEN** the human ticks Tests in the level "Light"
- **THEN** it saves at once, survives restart, and the coordinator's next workspace context shows "Light" running Tests and Correctness

#### Scenario: Adding a level
- **WHEN** the human presses Add level, names it "Docs", ticks Style, writes its "Use it" line and confirms
- **THEN** the level is saved as a re-check and appears below the others

#### Scenario: Editing a level's name and note
- **WHEN** the human opens "Light" for editing, rewrites its "Use it" line and confirms
- **THEN** the new line is saved and shown, and cancelling instead leaves the saved line

#### Scenario: Light is suggested after an update
- **WHEN** the only level is "Full" and the human presses Add level
- **THEN** the form is filled in with the name "Light", the first reviewer ticked and the suggested Light note, and nothing is saved until the human confirms

#### Scenario: A level named Light already exists
- **WHEN** the levels are "Full" and "Light", both marked "Full review", and the human presses Add level
- **THEN** the form opens empty as a re-check, with no name, no verifier ticked and no "Use it" line

#### Scenario: No reviewer to suggest
- **WHEN** the only level is "Full", every configured verifier is a tester, and the human presses Add level
- **THEN** the form opens empty as a re-check

#### Scenario: The last full review
- **WHEN** the human switches the only level marked "Full review" to "Re-check", or removes it
- **THEN** the card shows "Keep at least one level marked Full review." and the level stays as it was

#### Scenario: Removing a level's only verifier
- **WHEN** the level "Light" runs only Correctness and the human removes the verifier Correctness
- **THEN** the card shows the host's reason, and the verifier and the level stay as they were

#### Scenario: A new verifier joins the full reviews
- **WHEN** the levels are "Full" and "Docs", both marked "Full review", and "Light", a re-check, and the human adds the verifier Security
- **THEN** one save adds Security to the verifier list and ticks it in "Full" and "Docs", and "Light" is unchanged

#### Scenario: Removing a verifier that levels share
- **WHEN** the human removes the verifier Style, which "Full" runs beside three others
- **THEN** Style disappears from the verifier list and from "Full" in one save

#### Scenario: An existing setup after the update
- **WHEN** the human first opens the Reviews card after updating, with four saved verifiers, a round cap of 2 and a cycle cap of 3
- **THEN** it shows one level, "Full", marked "Full review" with all four verifiers ticked, the same four verifiers and the same two caps

#### Scenario: A reviewer set to skip
- **WHEN** the human opens the Claude reviewer's then menu, chooses "Skip this review", and Claude's weekly window reads 96% with the threshold at 95
- **THEN** the cell reads "skip" and Right now shows a grey "Skips" pill with the reset time

#### Scenario: No skip outside reviews
- **WHEN** the human opens the Coordinator row's then menu
- **THEN** it offers models on the other subscription and "Wait for the reset", and no "Skip this review"
