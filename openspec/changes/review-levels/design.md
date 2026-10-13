## Context

See proposal.md for why. The code this builds on, at origin/main 4b29415:

- **The setting.** `workspace-core/src/lib.rs`: `VerificationSettings` (`:596`) is `verifiers`, `max_rounds` and `max_cycles`, with `Default` (`:606`) and `validate` (`:638`). `VerifierConfig` (`:680`) is a role, a focus that is unique in the list, an optional instruction and an optional provider. It lives in `HostSettings.verification` (`:539`), in the host's `settings.json`, saved by `workspace-host/src/settings.rs::set_verification` (`:110`) through `Command::SetVerification` (`workspace-core/src/lib.rs:1546`). `Host::settings` (`settings.rs:103`) falls back to every default when the file cannot be parsed, so reading an old file must never fail.
- **Starting a review.** `workspace-host/src/verification.rs::verify_ticket` (`:284`) refuses past the cycle cap (`:306`), pairs every configured verifier with the coordinator's model choice in `verifier_choices` (`:441`), and starts one `VerifierRun` per configured verifier (`:316`). The tool's arguments are read in `live.rs` (`:784`) and described in `mcp.rs` (`:41`). `VerifierChoice` (`workspace-core/src/lib.rs:692`) is `{focus, profile, reason}`.
- **Rounds.** `next_round` (`verification.rs:933`) re-runs only the cycle's failed verifiers. `start_round` (`:488`) looks up each verifier's instruction by focus in the current settings. `end_round` (`:782`) writes the coordinator's outcome message, which names "cycle N of the cap", and offers "verify again" while `cycle < max_cycles` (`:857`).
- **The ledger is keyed by focus.** A verifier's open findings are the `open` entries with its focus. When a cycle ends, `untriage_orphans` (`verification.rs:198`) turns an `open` entry whose focus had no verifier in that cycle into `untriaged`.
- **The record.** `Verification` (`workspace-core/src/lib.rs:962`) is `cycle`, `max_rounds`, `outcome` and `rounds`. `Ticket.verification` is the latest cycle and `Ticket.previous_cycles` the earlier ones. `Verification::review_line` (`:1004`) writes the publish card's Review row.
- **What a pass unlocks.** `live.rs::accept_ticket` (`:395`) needs the latest cycle to have passed, or a waiver after a blocked one. `workspace-core/src/publishing.rs::publish_ask` (`:24`) runs a publish without asking when the repository's owner is allowed and the latest cycle passed; `publish_card` (`:115`) builds the approval card's rows, with the Review row at `:157`.
- **What the coordinator reads.** `live.rs` (`:526`) puts `verifiers`, `max_rounds` and `max_cycles` into `model_selection`. `skills/main-coordinator.md` (v11) tells it to give a choice "for every verifier in model_selection.verifiers".
- **The page.** `workspace-desktop/src/verifiers.rs` is the Review card's body: the verifier list, the add row and two `cap_selector`s (`:143`), each change saved at once through `save` (`:108`). `models_view.rs::review_card` (`:370`) frames it. `tree.rs::cycle_line` (`:31`) writes each cycle's line on the ticket's card, which `team_view.rs` (`:177`) draws.
- **The first-run suggestion.** `workspace-core/src/suggestion.rs::suggest` (`:69`) starts from `VerificationSettings::default()` and drops verifiers whose provider is not usable (`:97`). Since the first-run change, saving the suggested setup writes the verifiers to `settings.json`, so a fresh install always has a saved verification setting.

## Words

- A **level** is a named set of the configured verifiers. Its name is the human's: "Full", "Light".
- A level's **kind** is **Full review** or **Re-check**. A full review can be a ticket's only review. A re-check starts only on a ticket whose latest review passed.
- The line that says when to pick a level is its **"Use it" line** on the page and `use_when` in data.
- "Cycle" stays the code's and the instructions' word for one review of a ticket. The page says "review".

## Goals / Non-Goals

**Goals:** the settings page reads at a glance; nobody's saved setup changes on update; the coordinator chooses the level and the text guides it; the app, not text, keeps a lighter level from lowering the bar for accepting or publishing; `accept_ticket` and `publish_ticket` are not touched; no table and no migration; nothing here stands in the PR keeper's way.

**Non-Goals:**
- The PR keeper (change 3), the Skills page (change 4), the settings repository (change 5).
- Model choices per level. `verify_ticket` already takes a model for each verifier it starts.
- Round caps per level.
- Telling a re-check's verifiers to look only at what changed since the last full review. After a rebase that range holds everything the base gained, so it would mislead more than help. A person who wants a narrower check adds a verifier with its own instruction and ticks only it in the level.
- Measuring how much changed since the last full review.

## Decisions

### Who decided what

**Decided by the human on October 12, 2026**, in the plan:
1. Settings define named review levels, for example Full (Tests + Claude + Codex + Style) and Light (one reviewer).
2. The coordinator names the level when it starts a review. Text, not code, says when to use each.
3. The app requires every verifier in the chosen level to pass. The ticket shows which level ran.
4. The PR keeper will run a Light review before it pushes a fix.
5. Supervision is preferred to hard limits, and thresholds are settings, not constants.

**Decided by the human on October 13, 2026**, on this proposal and its mockup, in his words "Approve with the recommended answers":
6. Authority (decision 2). There are two kinds of level, Full review and Re-check. The app refuses a re-check unless the ticket's latest review passed, and refuses any level that leaves out a verifier with an open blocking finding. `accept_ticket` and `publish_ticket` keep today's rule.
7. Starting levels (decisions 4 and 5). An existing setup becomes one level named Full and runs exactly as now. No Light is added on update; the Add level form is filled in as Light. A new install starts with Full and Light.
8. Naming a level (decision 6). `level` is required on every `verify_ticket`; there is no default.
9. The cap (decision 9). Re-checks are not counted against it, and it becomes "Full reviews per ticket".
10. The limit, which he was told and accepted: once a full review has passed, later changes of any size can pass on a re-check. The app does not measure how much changed, and in an allowed owner's repository that push goes out without a card.

**Not approved, and left out:** a one-line reason for the level choice.

**Everything else here is a design decision made in this proposal.**

### 1. One list of verifiers; a level picks from it
A level holds the focuses of the verifiers it runs, chosen from the one configured list. The alternative, each level with its own verifier list, was rejected for four reasons:

- **The page.** A verifier's role, provider and instruction are written once. A level is then a name and a row of ticks, which reads at a glance: "Light runs Correctness". With a list per level, "Tests" would be defined in every level that uses it and could drift.
- **The ledger.** Findings belong to a focus across cycles (`start_round` lists a verifier's open findings by focus, `verification.rs:532`, and a completed check clears them by focus, `:164`). "Correctness" must mean the same verifier in every level, or a finding raised in one level could not be re-checked in another.
- **The tool.** `verify_ticket` already takes its model choices keyed by focus.
- **Migration.** The saved list stays byte for byte what it was.

The cost: a level cannot give the same focus a different instruction. A person who wants that adds a second verifier, such as "Rebase check", and ticks it only where it belongs.

A verifier may be in no level; it then never runs, and the page shows it with no tick anywhere. Removing a verifier takes it out of every level, and is refused when that would leave a level with none.

### 2. Two kinds of level, checked when a review starts
In an allowed owner's repository a passed review lets the app push and open a PR without asking, and it unlocks `accept_ticket`. If any level's pass counted, a coordinator could pick the lightest level and lower that bar by itself. Three ways to prevent it were weighed.

- **A. Any level counts.** Nothing is enforced. The bar is the lightest level, and only instruction text keeps a coordinator from using it for a first review. Rejected: the plan says authority is enforced by the app, never by text.
- **B. Mark the levels that are enough, and check at the gates.** `accept_ticket` and the first publish would need a pass at a marked level somewhere in the ticket's history; a later push would take any level. This changes `accept_ticket`, `publish_ask` and the approval card, adds a new reason to ask the human, and tells the coordinator only after a light review has already run and passed that it unlocked nothing.
- **C. Mark each level's kind, and check when the review starts.** Chosen. A level is a full review or a re-check, and `verify_ticket` refuses a re-check unless the ticket's latest cycle passed.

Under C every passed cycle is either a full review, or a re-check that could start only because the cycle before it passed. So every pass rests on a passed full review with only passed re-checks since, and "the latest cycle passed" keeps meaning what `accept_ticket` and `publish_ask` need. Neither is changed. The coordinator learns at once, before any verifier runs, and the rule lives in one function.

A cycle recorded before this change has no level and counts as a full review: it ran every configured verifier.

**One more start rule is needed, under any option.** `verify_ticket` refuses a level that leaves out a configured verifier holding an `open` ledger entry. Without it, a review that ended blocked on a Regressions finding could be followed by a full-review level that does not run Regressions; `untriage_orphans` would then turn the open blocking finding into an untriaged one, the cycle would pass, and the ticket would publish without the human seeing a finding nobody re-checked. The coordinator's ways forward are the ones it has today: use a level that runs that verifier, or overrule the finding with `triage_findings`, which records its reason. A finding whose verifier was removed from the settings altogether is handled as today, by `untriage_orphans`.

**What the human controls, and its limit.** The human sets each level's kind, so marking Light as a full review deliberately gives option A. The lightest level marked as a full review is the real bar: the coordinator may name it for any ticket, and only its "Use it" line says when. The page says this in one sentence. And after a full review has passed, later changes of any size can pass on re-checks alone; the app does not measure how much changed. The ticket and the publish card name both reviews and their commits so the human can see it.

**After a blocked re-check** only a full review can start, which the cap counts, or the coordinator accepts with a waiver, closes the ticket or asks the human, as after any blocked cycle. A re-check is never a way to get more rounds out of a review that did not converge.

### 3. What is stored
All on JSON that is already stored.

```rust
// workspace-core/src/lib.rs
pub struct VerificationSettings {
    pub verifiers: Vec<VerifierConfig>,   // unchanged: the one list
    pub levels: Vec<ReviewLevel>,         // never empty once read (decision 4)
    pub max_rounds: u32,                  // unchanged
    pub max_cycles: u32,                  // now counts full reviews only (decision 9)
}
pub struct ReviewLevel {
    pub name: String,                     // 1–60 bytes, unique ignoring case, like a focus
    pub kind: LevelKind,
    pub verifiers: Vec<String>,           // focuses from `verifiers`, at least one
    pub use_when: String,                 // one line, at most 200 characters
}
pub enum LevelKind { FullReview, Recheck }            // "full_review", "recheck"
pub struct Verification { …, pub level: Option<CycleLevel> }   // None before this change
pub struct CycleLevel { pub name: String, pub kind: LevelKind }
```

- A cycle keeps the level's name and kind as they were when it started. Renaming, re-marking or removing a level later does not rewrite history, and the start rule reads the recorded kind.
- `VerificationSettings::validate` (`:638`) additionally requires: at least one level; at least one marked full review; names of 1–60 bytes, unique ignoring case; each level naming at least one verifier, each an existing focus, none twice; and a non-empty one-line `use_when` (the bound `one_line_reason` already applies to reasons, `selection.rs:326`).
- `VerificationSettings::level(&self, name) -> Result<(&ReviewLevel, Vec<&VerifierConfig>)>` resolves a name, trimmed and ignoring ASCII case as focuses are compared, to its verifiers in configured order. It is the only place a level becomes verifiers, and it knows nothing about cycles, so the PR keeper can use it for whatever it runs.
- `Verification::is_full_review(&self) -> bool`: true when `level` is `None` or its kind is a full review.
- `Ticket::last_full_review(&self) -> Option<&Verification>`: the most recent passed full review among the latest and earlier cycles, for the publish card.

### 4. What an existing setup becomes
A setting saved before this change has no `levels`. It is read as:

- `verifiers`, `max_rounds` and `max_cycles`: exactly as saved, in the same order, with every instruction and provider.
- `levels`: one level named **Full**, kind full review, running every saved verifier in order, with the "Use it" line "for every review."

So every review starts the same verifiers as before and unlocks the same things. Nothing is rewritten on disk until the human next changes the card; the level is filled in on read, by deserializing through a private `SavedVerification` struct (`#[serde(from = …)]`) whose `From` adds the level when `levels` is empty. That conversion cannot fail, which matters because `Host::settings` drops to every default on a parse error. A settings file with no `verification` key at all uses `VerificationSettings::default()`, which is today's three verifiers and the same single Full level.

No second level is added on update. *Why not add Light too:* it would change what runs after a rebase without the human having seen the page. The first time Add level is pressed while no level is a re-check, the form is filled in as Light with the first reviewer ticked and the stock "Use it" line, so adding it is two clicks.

`max_cycles` keeps its saved number. Its meaning narrows from "cycles" to "full reviews", which never refuses a review the old meaning allowed.

### 5. What a new install starts with
`suggestion.rs::suggest` (`:69`) builds, from the verifiers it keeps for the usable providers:

- **Full**, a full review, with every suggested verifier (Tests, Correctness and Regressions with both providers). "Use it": "for a ticket's first review, and after any change to what the code does."
- **Light**, a re-check, with the first suggested reviewer (Correctness, or the one reviewer a single-provider machine has). "Use it": "after a rebase, a resolved merge conflict or a small fix, on a ticket whose last review passed."

The suggested verifiers do not change. The human's example names a Style verifier; none is suggested, because it needs the person's own style tool. The card keeps "Run reviso:style with Codex" as example text.

### 6. `verify_ticket` names the level
`verify_ticket { ticket_id, level, verifiers: [{focus, profile, reason}] }`.

- **`level` is required.** Omitting it is refused with the level names. *Why no default level:* the human decided the coordinator names the level; the `verifiers` list must match the level anyway, so the coordinator has to know which it is using; a default would be one more setting to explain; and the record then always states a choice the coordinator made. The cost is one refused call for a coordinator acting from habit, and the refusal tells it what to send.
- **`verifiers` has one entry for each verifier in the level**, and none for a verifier outside it. `verifier_choices` (`:441`) is handed the level's verifiers instead of the whole list, so its existing checks (missing, duplicated, unknown, provider, allowed model) apply unchanged; its messages name the level, as in "Tests is not part of Light".
- **Checks, in order**, after the existing ones (no cycle running, ticket ready, clean worktree), each refusing with nothing started:
  1. The level exists.
  2. A re-check needs the ticket's latest cycle to have passed (decision 2). The refusal says whether the ticket has no passed review yet or its last review ended blocked, and names the full-review levels.
  3. No configured verifier outside the level has an `open` ledger entry (decision 2). The refusal names the verifier and the entry ids.
  4. A full review needs the ticket to have started fewer full reviews than `max_cycles` (decision 9).
- **A later cycle may use a different level.** That is the point: Full first, Light after a rebase. Each cycle records its own.
- **Inside a cycle nothing changes.** The cycle's verifiers are fixed when it starts. A failed round goes to the implementer and `next_round` re-runs only that cycle's failed verifiers. Verifiers outside the level are never started in it. Editing the levels while a cycle runs does not affect it.
- Cycle numbers stay one sequence across levels, so message ids, ledger sources and `PublishRequest.cycle` keep working.

### 7. What `model_selection` shows
`live.rs` (`:526`) adds `review_levels` beside the unchanged `verifiers`, `max_rounds` and `max_cycles`:

```json
"review_levels": [
  {"name": "Full", "kind": "full_review", "verifiers": ["Tests", "Correctness", "Regressions"],
   "use_when": "for a ticket's first review, and after any change to what the code does."},
  {"name": "Light", "kind": "recheck", "verifiers": ["Correctness"],
   "use_when": "after a rebase, a resolved merge conflict or a small fix, on a ticket whose last review passed."}
]
```

It is the stored `levels`, serialized as is. The coordinator reads the level names, which verifiers to choose models for, and when to use each. Each ticket's verification record in `workspace_context` carries its `level` without further code, because the record is serialized whole.

### 8. Where "when to use each level" lives until the Skills page exists
In two small places:

- **Each level's "Use it" line**, edited on the Review card beside the level it describes and sent in `model_selection`. A level added without guidance would give the coordinator nothing to go on, so the line is required and sits where the level is made.
- **One sentence in `skills/main-coordinator.md`** telling the coordinator to choose by those lines.

*Why not the Guide:* a saved guide is the human's own text about models, and an update must not write into it. *For change 4:* the sentence moves into the Reviewing skill unchanged. The "Use it" lines can stay as each level's description, which travels with the levels into the settings repository (change 5), or the Reviewing skill can absorb them and the field be dropped. Either is a small step.

### 9. The caps
- **Rounds per review stays one global setting.** It bounds every cycle at every level. Nothing argues for a different number per level.
- **The cycle cap stays one global setting but counts full reviews only.** This departs from "keep it global and unchanged", for a reason. Counted against the default of 2, one full review and one re-check would exhaust a ticket, and the cap's maximum of 5 would stop every later rebase check on a PR that stays open for days, which is the case the human named. What the cap exists for is to end a review that does not converge, and re-checks cannot be used for that: a re-check starts only after a pass, and a blocked one must be followed by a full review, which is counted. A chain of re-checks is therefore a chain of passes, each caused by a new commit. Nothing caps its length; the coordinator's and the human's check-ins supervise it, as the plan chose for the keeper.
- The card's label becomes "Full reviews per ticket" and its caption says re-checks are not counted. The setting's name in data stays `max_cycles`.
- The alternative, counting every cycle, keeps the code one line shorter and would need the human to raise the cap whenever a PR needs a third check.

### 10. The ticket shows which level ran
- **The ticket's card.** `tree.rs::cycle_line` (`:31`) names the level after the cycle number: "Verification cycle 2 · Light re-check: passed, 1 of 3 rounds", "Verification cycle 1 · Full: passed, 2 of 3 rounds". A cycle with no recorded level reads as today.
- **The coordinator's outcome message.** `end_round` (`:834`) says "cycle 3, full review 2 of 2 (Full)" or "cycle 2, a Light re-check" where it says "cycle N of M" today, and offers "verify again" after a blocked cycle only while a full review remains.
- **The publish approval card.** `Verification::review_line` (`:1004`) names the level: "Cycle 2 · Light re-check passed at this commit · Correctness". When the latest cycle is a re-check, `publishing.rs::publish_card` (`:115`) adds a row after Review, "Last full review": "Cycle 1 · Full passed at 9e03c1b · Tests, Correctness, Regressions", from `Ticket::last_full_review`. In a repository whose owner is not allowed, the human then sees how much rests on a re-check before approving. A blocked review's row gains the level in the same way.
- Nothing is added to the sidebar row.

### 11. The Review card
See `design/mockups/current/review-levels.html`.

`verifiers.rs` gains a Levels block above the verifier list. Each level is one row group: its name, a two-option kind control built with the `segment` helper the role and provider pickers use, one tick chip per configured verifier, and its "Use it" line. Ticks and the kind save at once through `save` (`:108`), as everything on the card does. A pencil opens the name and the "Use it" line as two inputs with Done and Cancel; Add level opens the same form empty, or filled in as Light when no re-check exists (decision 4). The host's refusals appear in the card's existing error line, in `validate`'s words: "Keep at least one level marked Full review.", "Light would run no verifier. Tick another verifier for Light, or remove Light." and "Two levels are named \"Light\".".

The second `cap_selector` is labelled "Full reviews per ticket". `models_view.rs::review_card` (`:370`) changes its subtitle to "who reviews a ticket, and how thoroughly". The card's closing hint says `verify_ticket` starts every verifier of the level the coordinator names.

### 12. The instructions
`skills/main-coordinator.md` becomes v12. Three sentences change:

- "verify_ticket takes the same choice for every verifier in model_selection.verifiers, keyed by its focus" becomes: "verify_ticket takes a review level by name and the same choice for every verifier in that level, keyed by its focus".
- "call verify_ticket: it starts every configured verifier on the ticket's current commit at once" becomes: "call verify_ticket with a level from model_selection.review_levels, chosen by each level's use_when line: it starts every verifier of that level on the ticket's current commit at once. A re-check level starts only on a ticket whose last review passed, and no level may leave out a verifier that still has an open finding; the workspace refuses both."
- "A ticket may start only as many cycles as the cycle cap allows" becomes: "A ticket may start only as many full reviews as the cap allows; re-checks are not counted".

The `verify_ticket` description in `mcp.rs` (`:41`) says the same, and its schema gains the required `level` string. The instruction files are sent on every turn, so a running coordinator has the new text on its next turn.

## Files and functions to change

| File | What changes |
| --- | --- |
| `crates/workspace-core/src/lib.rs` | `ReviewLevel`, `LevelKind`, `CycleLevel` (new); `VerificationSettings` (`levels`, `Default`, `validate`, new `level`, reading through `SavedVerification`); `Verification` (`level`, new `is_full_review`, `review_line`); `Ticket::last_full_review` (new) |
| `crates/workspace-core/src/suggestion.rs` | `suggest`: the Full and Light levels for the verifiers it keeps |
| `crates/workspace-core/src/publishing.rs` | `publish_card`: the "Last full review" row after a re-check |
| `crates/workspace-host/src/verification.rs` | `verify_ticket` (the `level` argument, the four checks, the recorded `CycleLevel`, only the level's verifiers started); `verifier_choices` (takes the level's verifiers; its messages name the level); `end_round` (the message's wording and the "verify again" offer) |
| `crates/workspace-host/src/live.rs` | `agent_tool`'s `verify_ticket` arm reads `level`; `model_selection` gains `review_levels` |
| `crates/workspace-host/src/mcp.rs` | `verify_ticket`'s description and schema |
| `crates/workspace-host/skills/main-coordinator.md` | v12, the three sentences in decision 12 |
| `crates/workspace-desktop/src/verifiers.rs` | `VerifierEditor` (the Levels block, the level form, removing a verifier from its levels); the cap's label and caption |
| `crates/workspace-desktop/src/models_view.rs` | `review_card`'s subtitle |
| `crates/workspace-desktop/src/tree.rs` | `cycle_line` |
| `docs/PRD.md`, `docs/DEVELOPMENT.md` | the Review card and `verify_ticket` paragraphs (`PRD.md:154`, `:157`, `:314`; `DEVELOPMENT.md:63`, `:65`, `:81`) |

Not changed: `live.rs::accept_ticket`, `publishing.rs::publish_ask`, `publish.rs`, `settings.rs::set_verification`, `next_round`, `start_round`, `record_verdict`, `untriage_orphans`, `progress.rs` (a ticket's progress already counts the latest cycle's own verifiers).

## What this closes and what it does not

**Closes:**
- A coordinator cannot make a light review a ticket's only review. The app refuses to start one.
- A coordinator cannot walk past an open blocking finding by choosing a level without its verifier.
- A coordinator cannot use re-checks to keep a review going after it ended blocked, or to get around the cap on full reviews.
- The human can always see which level each review ran at, and on the publish card which full review a re-check rests on.

**Does not close:**
- The coordinator may name any full-review level for any ticket. If the human marks a one-reviewer level as a full review, that level is the bar.
- After a full review has passed, any later change can pass on a re-check. The app does not judge its size.
- Whether a level suits a ticket is decided by text. That is the human's principle, not an oversight.
- The limit the publishing change stated remains: an agent's shell can still push.

## Risks / Trade-offs

- [A coordinator omits `level` out of habit] → one refused call that lists the levels; nothing starts.
- [The human marks every level as a full review] → their choice; the page says the lightest full review is the bar.
- [A re-check passes a large change] → accepted by the human (item 10). The ticket's card and the publish card show the re-check, the last full review and both commits. A size threshold would be a hard limit the human has said he does not want.
- [Many re-checks on one ticket] → each follows a pass and a new commit; check-ins supervise. If it proves a problem, a setting can cap them later without a migration.
- [The first-run change is not archived yet, so its "Suggested setup on first run" requirement is not in the main `model-routing` spec] → the suggested levels are specified under `ticket-verification`, beside the default verifiers, so this change does not depend on that archive.
- [`model_selection` grows] → a few hundred bytes per level.
- [A hand-edited `settings.json` names a verifier that does not exist in a level] → `verify_ticket` refuses that level and says why; the card shows the level with the missing verifier unticked and saves a valid setting on the next change.

## Migration Plan

No store migration. `levels` is filled in on read for a setting saved without it, and `Verification.level` is optional. Running sessions get the new instructions on their next turn, and a cycle already running when the host is updated finishes as it started. To roll back, revert the change: an older build ignores both new fields and starts every verifier.

## Open Questions

None. The human settled every open decision on October 13, 2026; see "Who decided what".
