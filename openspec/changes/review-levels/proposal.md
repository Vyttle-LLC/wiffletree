## Why

Every review of a ticket starts every configured verifier, whatever changed. A rebase with one resolved conflict gets the same tester and reviewers as the ticket's first review. The human wants to choose how thorough a review is (plan of October 12, 2026): people "will want to alter quite a bit… in PR rebase/merge conflict only run 1 review etc." His principle is that the code offers choices and the text decides which to use. So the app needs named review levels the coordinator can pick from, and it must keep the one thing text cannot be trusted with: a lighter level must not become a way for a coordinator to lower the bar for accepting or publishing a ticket by itself.

## What Changes

- **Settings define named review levels.** The Review card in Models gains a Levels block above the verifier list. There is still one list of verifiers; a level is a name, a tick for each verifier it runs, one line saying when to use it, and a kind.
- **Two kinds of level: full review and re-check.** A full review can be a ticket's only review. A re-check starts only on a ticket whose latest review passed. The human sets each level's kind; the app enforces it when a review starts.
- **The coordinator names the level.** `verify_ticket` takes a required `level` and one model choice for each verifier in that level. The app starts exactly those verifiers and requires every one of them to pass. Rounds work as today inside a level: failures go back to the implementer and only the failed verifiers check again.
- **Text says when to use each level.** Each level's "Use it" line is shown to the coordinator in `model_selection.review_levels`, and the coordinator's instructions gain one sentence telling it to choose by those lines. The app does not judge the choice.
- **The app holds two lines when a review starts.** It refuses a re-check on a ticket that has not passed a review or whose latest review ended blocked, and it refuses any level that leaves out a verifier that still has an open blocking finding. With these, a passed review always rests on a full review, so `accept_ticket` and `publish_ticket` keep their rule unchanged: the latest review passed.
- **Re-checks are not counted against the review cap.** "Reviews per ticket" becomes "Full reviews per ticket". "Rounds per review" is unchanged and applies at every level.
- **The ticket shows which level ran.** Each cycle records its level's name and kind. The ticket's card names it on every cycle's line, the coordinator's outcome message names it, and the publish approval card's Review row names it. After a re-check, that card gains a "Last full review" row with the level, the commit it saw and its verifiers.
- **An existing setup is kept exactly.** Saved verifiers, their order, instructions, providers and both caps are unchanged. They are read as one level named "Full", a full review that runs all of them, so every review behaves as before. No second level is added on update. A new install's suggested setup has two: Full, with every suggested verifier, and Light, a re-check with the first suggested reviewer.
- **Not included:** the PR keeper, the Skills page, the settings repository, per-level model choices beyond what `verify_ticket` already takes, per-level round caps, narrowing a re-check's diff to what changed since the last full review, and any measure of how much changed since then.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ticket-verification`: the verification setting gains levels; `verify_ticket` names a level and starts only its verifiers; the two start rules; the cap counts full reviews; each cycle records its level; the ticket's card and the coordinator's outcome message name it; what a saved setting becomes and what a new install is offered.
- `native-workspace`: the Review card's Levels block and the renamed cap.
- `workspace-host`: `model_selection` in `workspace_context` lists the review levels with their kind, verifiers and "Use it" line.
- `ticket-publishing`: the approval card's Review row names the level, and after a re-check a second row names the last full review.

## Impact

- `workspace-core` (`lib.rs`): `ReviewLevel`, `LevelKind` and `CycleLevel` (new); `VerificationSettings` (`levels`, `Default`, `validate`, reading a setting saved without levels); `Verification` (`level`, `review_line`); a `Ticket` helper for the last passed full review. `suggestion.rs`: the suggested levels. `publishing.rs`: the "Last full review" row.
- `workspace-host`: `verification.rs` (`verify_ticket`, `verifier_choices`, `end_round`'s message), `live.rs` (`model_selection`, the tool's arguments), `mcp.rs` (the tool definition), `skills/main-coordinator.md` (v12).
- `workspace-desktop`: `verifiers.rs` (the Levels block, the cap's label), `models_view.rs` (the card's subtitle), `tree.rs` (the cycle line).
- Stored data: one new optional field in `settings.json` and one on each ticket's verification record. No table and no migration; an older build ignores both and runs every verifier.
- `accept_ticket`, `publish_ticket` and the allowed-owners rule are not changed.
- Mockup: `design/mockups/current/review-levels.html`.
