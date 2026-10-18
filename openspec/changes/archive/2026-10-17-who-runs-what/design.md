## Context

The approved mockup is `ui/models.html`; its upper half (Models page) is this change. Its lower half, the Repositories page's PR automation card, is a separate change, except that the PR keeper's model becomes a row of this table.

Today `ModelSelection` holds `providers` (enabled flag and allowed model/effort pairs), `role_providers`, `guide` and `keeper`. Coordinators pass an exact profile and reason to `assign_ticket` and one per verifier to `verify_ticket`; the host checks them with `permits`. Quota readings (`QuotaReading`, `QuotaWindow`) are recorded from Claude's stream and Codex's app-server, but nothing acts on them: there is no exhaustion-based scheduling yet. The only hold at turn start is `Host::hold_reason` (no pinned model, or a coordinator-chosen provider disabled since), handled by the service's `hold_turn`.

## Goals / Non-Goals

**Goals:**
- One table decides every agent's model when it starts; a subscription past the threshold moves new agents to the next choice.
- When nothing can run, wait for the earliest reset with the input kept and other work going on.
- The coordinator asks for a job, and optionally small work; it never names a model.
- Every saved setting survives the upgrade, in the table or as a note.

**Non-Goals:**
- Switching a running agent, or a started provider conversation, to another provider.
- Paid fallback, reset credits, or any estimate of quota from token counts.
- Per-project tables, budgets, or more than two choices per line.

## Decisions

### The table

```rust
enum Job { Coordinator, Implementer, Tester, Verifier { focus }, Keeper }   // serde tag "kind"
struct Line { first: ModelProfile, then: Option<ModelProfile> }             // then None = wait
struct JobRow { job: Job, main: Line, small: Option<Line> }
struct ModelSelection { revision, jobs: Vec<JobRow>, guide, switch_at, notes, copied }
```

- `jobs` is per machine, in the existing `model_selection` row. `guide` and `switch_at` are shared files merged in on read, as the guide already is: `settings/model-guide.md` and the new `settings/models.json` (`{"switch_at": 95}`, 50–100). One Save on the Models page writes the table, the threshold and the guide, all or none.
- `validate` needs exactly one Coordinator, Implementer and Tester row, at most one PR keeper row, unique verifier focuses, valid profile strings, and a then on another provider than its first: both choices on one subscription would be spent together.
- The PR keeper row is optional. No row means no PR keeper model, which the keeper's triggers check as they checked `keeper: None`.
- The table always matches the shared verifier list on read (`ModelSelection::fit`): rows of verifiers no longer configured are left out, and a configured verifier with no row gets a copy of the first reviewer's row (else the Tester row). Its focus goes to `copied`, which the Models page shows as a note until the human saves. Nothing is written by sync or by a read, so the rule is idempotent and needs no write path in sync. Adding a verifier on the Reviews card works the same way.
- `notes` are one-time Models page notes, such as the migration's. A save keeps the saved notes whatever the client sends; `DismissModelNotes` clears them without a new revision, since they change no resolution.

Alternative considered: keep the allowlist as a hidden limit under the table. Rejected: the mockup says the allowlist is whatever the table uses, and two sources would disagree.

### Spent

`Limits { quotas, unusable, switch_at, now }` in core answers `spent(provider) -> Option<Spent>`. A window counts when its reset is unknown or later than `now`, and either its `used_percent` is at least `switch_at` or its `status` is `rejected` (Claude's report that the limit is reached; Codex reports 100%). A provider is spent while any window counts; `Spent.until` is the latest of the counting windows' resets, or unknown when any counting window has none. The reading shown in reasons is the counting window with the highest use. No reading, or a reading with no windows, is not spent. A stale window with an unknown reset counts until a newer reading of that window replaces it, which `record_quota` already does window by window.

`unusable` holds the providers whose last detection said the CLI is missing or signed out. When the service starts, it reports ready at once and then reads both CLIs' sign-in status in parallel, bounded by detection's 10-second limit, before it handles any request, so nothing is pinned before the host knows and the client is not held up; its requests wait in the queue. Sorting also waits until Claude is found signed in. The host also records each `InspectProvider` result, and the desktop runs detection at launch on every store. A provider never found signed in, as in a bare `Host` that ran no detection, is never chosen; `Host::open_set_up`, the opener for tests and scripted stores, counts both as signed in.

### Resolution

`resolve(row, small, &limits) -> Result<Pick>`, pure, in core, used by the host and by the Models page's Right now column:

1. The small line when `small` and the row has one, else the main line.
2. Its choices in order, without unusable providers. None left: refuse, naming why each cannot run.
3. The first that is not spent: `Pick::Run`.
4. Every remaining choice spent: `Pick::Wait` on the one whose `until` is earliest, unknown counting as last.

A human's model choice clears `waiting`, so it is never held. A comment read that returns after Claude became spent records nothing, so the comments are read again after the reset.

Each pick carries its reason, built in core with a time formatter the caller passes:
- "First choice", or "First choice for small work".
- "Started on Codex: Claude's weekly limit was at 96% (moves on at 95%). First choice was Opus · medium."
- "Started on Codex: Claude is not signed in on this Mac. First choice was Opus · high."
- "Waits for Claude: its weekly limit is at 96% (moves on at 95%), and no other choice can run. Starts after the reset, Mon 09:00."

Model names come from the known lineup (`Opus`, `Sonnet`, `GPT 6.1 Sol`), else the model id.

The Coordinating skill's stock line that asked the coordinator to choose a model by the guide now asks for small work only when a change is small, contained and low-risk. Its version stays v1: a changed stock text already marks a human's rewrite out of date and merges the change into it, as for any stock change without a new version.

Resolution runs only when an agent starts: `assign_ticket` (new agents only; an existing assignment is returned unchanged), each verifier `start_cycle` starts for the coordinator or the PR keeper (a queued cycle resolves again in `start_queued_cycles`, with the `small` it recorded, and replaces a verifier whose provider changed), `create_project`, and each PR keeper try in `keeper_agent`. A verifier session that already exists is reused only at the resolved profile; otherwise it is retired and a fresh one starts, as today. A PR keeper agent whose resolved provider changed is replaced as today; on the same provider it is pinned again before its run, between turns.

The pin records `Selection { chosen_by: Chooser::Wiffletree, reason, revision, at }` and logs `model_selected`.

### Waiting for a reset

A `Pick::Wait` pins the profile and sets `SessionRuntime.waiting`. `Host::hold_reason` holds a waiting session while its pinned provider is still spent, with a reason naming the window and the reset; once it is not spent, the turn starts and `launch` clears the flag, so a later spell of high use never holds a running agent. The service wakes itself at `Spent.until`, and a new quota reading wakes it while any session waits, so an unknown reset clears with the next reading below the threshold. Other readings still admit no work.

A waiting turn is held like any other (input queued, session Blocked with the reason, `turn_held` event) but the parent is told by one Wiffletree message, `wait:<session>:<first input>`, instead of a `blocked` report in the agent's name. A waiting verifier therefore gives no verdict: its cycle stays open, holding its slot, and the verifier runs after the reset. The disabled-provider hold is removed with the enabled flag.

### Comment sorting and skill merges

`ModelSelection::sorting_model()` is the PR keeper row's first Claude choice in its main line. `check_sorting_model` takes the row and refuses without a Claude choice. Sorting uses only that model; while Claude is spent, `comments_due` leaves the read for a later pass, so comments wait and are never sorted on Codex. Skill merges use the same model.

### Coordinator tools

- `assign_ticket { ticket_id, role, instruction, focus?, small?, like? }`. `profile`, `reason`, `size`, `complexity` and `provider` are refused with a message saying Wiffletree picks the model. The job is the role's row, a configured verifier's row when the focus names one, or for an ad-hoc reviewer the `like` row or the first reviewer row. An unknown `like` is refused naming the configured verifiers.
- `verify_ticket { ticket_id, level, small? }`. `verifiers` is refused the same way. `small` applies to every verifier of the level.
- `workspace_context.model_selection`: `revision`, `switch_at`, `jobs` (each row's lines and `right_now` for main and small: the profile, whether it waits and the reason), `verifiers`, `review_levels`, `max_rounds`, `max_cycles` and `notes` (the guide).

### Human choices

`ConfigureSession` and the new-agent dialog accept any model, as before. The dialog is pre-filled with what the row would use right now. `pin_human_choice` records `Default` ("From Who runs what") when the human kept the pre-fill, else `Human`.

### Migration `who-runs-what`

Runs once in `Host::open` after `model-selection`, recorded in `policy_migrations`. A store with no saved selection is left alone. Otherwise:

- Usable providers are those whose CLI is installed and signed in, read once with the CLIs' own status commands and model lists (`Host::open_detecting`; `Host::open` passes `detection::report`), or the enabled ones when none is. Available models are what those CLIs list. Detection runs only when a store has a selection to convert.
- Rows are seeded from the mockup: Coordinator Opus · high → Sol · high; Implementer Opus · medium → Sol · high, small Sonnet · high → Sol · medium; Tester Sonnet · high → Sol · medium; a verifier whose saved provider was Claude Opus · high → wait, Codex Sol · high → wait; any other reviewer Sol · medium → Sonnet · medium; a tester verifier as the Tester row; PR keeper Sonnet · medium → Sol · medium. A choice on an unusable provider is dropped and the next one moves up; a model the CLI does not list is replaced by the provider's closest listed model (same model at the nearest effort, else one whose ID contains it or is contained in it, else its first model), and kept as drawn when the CLI lists none. A saved PR keeper model is mapped the same way, and the note records the one saved.
- A saved PR keeper model becomes the keeper row's first choice, with the seeded other-provider choice as then. No saved keeper model means no keeper row, so triggers held here for want of a model stay held.
- The guide loses its `## Previous settings (migrated …)` section; the rest stays.
- The one-time note says "Set up from your previous settings. Check it." and lists what the table does not hold: every allowed model, a disabled provider's marked so, the providers per role, and the removed guide section line by line.
- Pinned agents are unchanged. Re-running changes nothing.

### Models page

Per the mockup: Subscriptions (sign-in and plan from detection, a meter per window with a tick at the threshold, reset time, the reading's age, "Unavailable" without telemetry), When a subscription runs low (the threshold, "wait for the earliest reset"), Who runs what (dropdowns built from the catalog and saved values, a Right now column), Reviews (the existing levels and verifier editor without the provider field; adding a verifier adds a row), and Notes (the guide). The Providers, Roles and PR keeper cards are removed.

## Risks / Trade-offs

- A waiting verifier keeps its cycle's verification slot until the reset, possibly for days → the coordinator is told at once and can close or re-plan; the PRD documents it.
- Detection is the host's only source for "signed out"; an undetected signed-out CLI is tried and fails at turn start as today.
- Claude reports percentages only in some rate-limit events; with none, Claude never counts as spent except through a `rejected` status. The Subscriptions card shows "Unavailable" so the human sees it.

## Migration Plan

Ships in one release with the code. The new migration id makes it run once; the legacy `model-selection` migration still runs first on older stores. Rolling back would need the previous row shape; the legacy selection is not kept, so rollback means re-saving the Models page on the older release.
