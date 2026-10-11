## Context

See proposal.md for why. The approved visual contract is the static mockup the human reviewed (`progress-mockup.html` in the project home); the current design study now carries the same three placements. The code this builds on, at origin/main 6c71ab1:

- **Plan steps** (`workspace-host/src/stream.rs`). `plan()` (`:359`) builds a `StepKind::Plan` step from Claude `TodoWrite` and Codex `todo_list` (`:301`). It counts the done items, then keeps the counts only in the title, "Plan · 1 of 3 done", and the items in the detail as "✓" and "◻" lines. Nothing downstream can read the counts without parsing text.
- **Steps** (`workspace-core/src/steps.rs`, `workspace-host/src/steps.rs`). A `Step` is stored as JSON in the `steps` table when it starts and when its state changes. The desktop reads one session's steps by cursor with `Command::Steps`, and only for the selected session, so it knows nothing about the plans of the other agents in the tree.
- **Signals** (`workspace-host/src/service.rs`). `Service::changes()` signals state changes and triggers a snapshot reload. `Service::step_changes()` (`:548`) is a second bounded signal for steps, so streaming never reloads the snapshot. Both hold at most one pending signal.
- **Status** (`workspace-core/src/lib.rs`). A session is `Ready`, `Working`, `Blocked`, `Done`, `Paused` or `Disconnected`. A `blocked` or `failed` report sets `Blocked`; any other report except `progress` sets `Done` (`verification.rs:680-690`). A turn that ends without a report leaves `Ready`.
- **Tickets** (`lib.rs:625`). `Ticket.state` is a free string. The host sets `assigned` (`live.rs:248-250`, on every assignment outside a running cycle, including a fix after a cycle), `verifying`, `failed`, `passed` and `blocked` through verification, and `accepted` and `closed`. Outside a cycle it also stores the kind of any plain report from an agent on an open ticket (`verification.rs:659-661`), so `completed`, `passed`, `failed` and `blocked` can appear with or without a verification record. `Ticket.verification` is the latest cycle, with its rounds and each verifier's result; `Verification::latest_results()` (`:801`) already gives each verifier's most recent result in the cycle.
- **Verifier archiving** (`openspec/specs/agent-lifecycle`). The host archives a verifier that passed when its round ends, and every remaining verifier when the cycle ends. Archived sessions are hidden from the tree.
- **Desktop.** `conversation.rs::conversation_header` (`:266`) draws the 64 px title bar. `sidebar.rs` draws `project_row` (`:471`), `ticket_row` (`:698`) and `session_row` (`:648`). `team_view.rs` draws the Team panel's ticket cards, with each agent's model through `models::profile_label`. `activity.rs` (`:137-213`) holds the step watcher: one request in flight, plus a dirty flag.

## Goals / Non-Goals

**Goals:**
- One set of rules that turns records the host already keeps into a figure for every agent, ticket and project.
- Figures that are honest: a bar never shows a percentage the rules cannot justify.
- No stored progress. Every figure can be recomputed from the records, so a restart or a crash loses nothing.
- Delivery that costs nothing while nothing changes and never touches an agent.

**Non-Goals:**
- Cost, token or time-remaining estimates.
- Showing progress to agents, in `workspace_context` or in any message.
- Weighting tickets by size or difficulty.
- A history or chart of progress over time.
- Mapping provider plan formats other than Claude `TodoWrite` and Codex `todo_list`. Claude's `TaskCreate`/`TaskUpdate` are not mapped.

## Decisions

### 1. Plan counts become data on the step
`workspace-core/src/steps.rs` gains:

```rust
/// A plan step's checklist, as numbers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanCounts {
    pub done: u32,
    pub total: u32,
    /// The first unfinished item, bounded like a title; `None` when every item is done.
    pub current: Option<String>,
}
```

and `Step.plan: Option<PlanCounts>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`. `stream.rs::plan()` fills it, and the title and detail stay as they are. Steps are stored as JSON, so no schema change is needed. A plan step stored before this change has no counts and is treated as no plan.

*Alternative considered:* parse "Plan · 1 of 3 done" back out of the title. Rejected: the title is display text, and the current item would have to be parsed from the "◻" lines of a detail that can be truncated.

### 1a. A workspace checklist tool, because Claude has no plan tool
Task 2.1 checked which plan tool the Claude CLI emits. Claude Code 2.1.291, run with the host's flags (`-p --output-format stream-json --verbose --include-partial-messages --strict-mcp-config --no-chrome --dangerously-skip-permissions --tools default --disallowedTools EnterWorktree,ExitWorktree`), announces these plan tools in its `init` event:

| Model | Plan tools offered | With `CLAUDE_CODE_ENABLE_TASKS=0` |
|---|---|---|
| claude-opus-5-5 | none | none |
| claude-sonnet-5-5 | none | none |
| claude-haiku-4-5 | `TaskCreate`, `TaskUpdate`, `TaskGet`, `TaskList` | `TodoWrite` |

A Sonnet turn asked to record a checklist with any todo, task or plan tool searched its deferred tools, found none and said so. `--tools default,TodoWrite` does not add it. So on the models agents actually run, a Claude agent would never write a plan step, every Claude agent's bar would be striped for good, and every `assigned` ticket would count as 0%.

The human decided to add one workspace tool, `set_plan`, and a short instruction to use it. This overrides the proposal's first draft, which gave agents no new tool. What stays true: no agent reports a percentage or a stage, and ticket and project progress are derived.

- **The tool.** `set_plan { items: [{ text, done }] }` on the workspace MCP server (`mcp.rs`), for every role. Like `TodoWrite`, each call carries the whole checklist. At most 50 items, each text 1 to 200 characters (`MAX_STEP_TITLE`); anything else is refused with an error naming the limit, never truncated.
- **Same path as a provider plan.** The actor finds the caller's active run by its per-turn credential, as it does for every tool, builds the step with `stream.rs::plan()` and hands it to `record_step`. The board, the signals, the cursor, the restart rebuild and the UI therefore have one source. The tool's step has a fixed id per run, so later calls update it in place, as Codex `todo_list` updates do; a long run cannot fill its step limit with plans.
- **Plan steps are stored when their counts change.** `record_step` used to write a step only when it started or changed state. A plan step is always `Succeeded`, so an updated plan reached the store only when the turn ended. It now also writes when the counts change, which keeps the stored plan and the in-memory one equal for the restart rebuild.
- **Latest wins.** Provider mappings stay. If a run has both a provider plan and a `set_plan` step, the one recorded last is the checklist: in memory because each recorded plan step replaces the session's entry, and after a restart because `latest_plans()` orders by step revision.
- **It wakes no one.** The call writes a step and nothing else: no message, no status, no ticket state, no state-change signal. It answers with the counts.
- **The instruction.** Three sentences in `skills/communication.md` (version 6): record a checklist at the start of work, keep it current, and it is display only and never replaces `report`.

*Alternatives considered:* map `TaskCreate`/`TaskUpdate` (only Haiku emits them, and they are stateful per task id); set `CLAUDE_CODE_ENABLE_TASKS=0` (restores `TodoWrite` on Haiku only); parse narration for a plan (guesswork). An agent that ignores the instruction still shows stripes, never an invented number.

### 2. The rules are one pure function in `workspace-core`
A new `workspace-core/src/progress.rs` holds the types and `derive`:

```rust
pub enum BarState { InProgress, Indeterminate, Blocked, Finished }
pub enum Stage { Implementing, Verifying, AwaitingAcceptance, Accepted }
pub enum Phase { Planning, Implementing, Verifying, AwaitingAcceptance, Blocked, Done }
/// What an agent's figure says when it is not a checklist count.
pub enum Word { Working, Done, Passed, Failed, Blocked }

pub struct AgentProgress {
    pub session_id: String,
    pub state: BarState,
    pub plan: Option<PlanCounts>,
    pub word: Option<Word>,
    /// A turn is running now; tells a working agent's checklist from an idle one's.
    pub working: bool,
    pub run_started_at: Option<i64>,
    pub run_finished_at: Option<i64>,
}
pub struct TicketProgress {
    pub ticket_id: String,
    pub stage: Stage,
    pub state: BarState,
    /// Hundredths of a percent, 0–10000, so averages need no floats.
    pub position: u16,
    /// The agents the crew panel lists, in order.
    pub agents: Vec<String>,
}
pub struct ProjectProgress {
    pub project_id: String,
    pub phase: Phase,
    pub state: BarState,
    /// `None` while planning with no checklist to count.
    pub position: Option<u16>,
    pub accepted: u32,
    /// Counted tickets in creation order; one bar segment each.
    pub tickets: Vec<String>,
    /// The project coordinator, whose checklist is the bar while `tickets` is empty.
    pub coordinator_id: String,
}
pub struct Board { pub agents: Vec<AgentProgress>, pub tickets: Vec<TicketProgress>, pub projects: Vec<ProjectProgress> }
/// The entries changed after a cursor; `complete` when it is the whole board.
pub struct ProgressPage { pub revision: u64, pub complete: bool, pub agents: .., pub tickets: .., pub projects: .. }

pub fn derive(sessions: &[Session], runtimes: &[SessionRuntime], tickets: &[Ticket],
              plans: &HashMap<String, PlanCounts>) -> Board
```

`derive` takes plain data and returns the whole board, so every spec scenario is a table-driven unit test with no host. Positions are integers in hundredths of a percent. A ticket's position is computed exactly (60 × 3 ÷ 7 = 25.71%), a project averages the exact ticket positions, and only the display rounds. That is why the spec's four-ticket example is 56%.

*Alternative considered:* derive in the desktop from the snapshot. Rejected: the desktop does not have the other agents' plans, and a later remote client would need the same rules again.

### 3. The agent rules
In order, as the spec lists them: finished (a verifier whose latest result in the ticket's latest cycle is `passed`, or status `Done`), blocked (status `Blocked`), in progress (working with a plan), indeterminate (working, no plan), idle (anything else). The figure's word follows the state: "passed" for a passed verifier, "done" for any other finished agent, "failed" for a blocked verifier whose latest result is `failed`, "blocked" for any other blocked agent, "working" when indeterminate. Otherwise the figure is the checklist count.

`plans` holds one entry per session: the latest plan step of its latest run. The host drops a session's entry when a new run starts for it, which is what makes a fresh run indeterminate instead of showing the old run's count.

### 4. The ticket rules
| # | Condition | Stage | Position |
|---|---|---|---|
| 1 | state `accepted` | Accepted | 100% |
| 2 | state `assigned` | Implementing | 60% × implementer's fraction |
| 3 | state `ready_for_testing` | Implementing | 60% |
| 4 | has a record, latest cycle `passed` | Awaiting acceptance | 90% |
| 5 | has a record, latest cycle running or blocked | Verifying | 60% + 30% × passed ÷ verifiers |
| 6 | otherwise | Implementing | as rule 2 |

"Verifiers" in rule 5 is `Verification::latest_results()`: every verifier of the latest cycle, each with its most recent result. The ticket brief says "verifiers passed out of configured in the current round". Round 2 and later contain only the verifiers that failed, so counting inside the round alone would drop a ticket from 80% back to 60% the moment round 2 starts, although nothing was lost. Counting the cycle's verifiers gives the same number in round 1 and keeps earlier passes, which is how the verification spec treats them ("verifiers that passed SHALL keep their earlier result").

Only `accepted`, `assigned`, `ready_for_testing`, `blocked` and `closed` are read by name. Every other state, whatever report kind it holds, falls through to the verification record: rule 4 or 5 with a record, rule 6 without. So a ticket whose cycle passed reads Awaiting acceptance at 90% whether its state is `passed` or a later `completed`, never "Verifying 90%"; state `failed` with a running cycle (the implementer is fixing between rounds) is rule 5; and a ticket with no record whose state is `completed`, `passed` or `failed` is rule 6, at 60% once its implementer is `Done`. State `blocked` adds the blocked bar state on top of rule 4, 5 or 6, and it wins over indeterminate: a `blocked` ticket whose implementer starts a planless run stays red without stripes. Its position is still derived, not remembered, so under rule 6 it is 0% until that run writes a plan; under rules 4 and 5 it comes from the verification record and does not move. `closed` tickets are not derived at all.

Rule 2 covers every `assigned` ticket, with or without a record. A ticket re-assigned for a fix after a passed or blocked cycle therefore follows the fix run's checklist: striped until the run writes a plan, then 60% × its fraction, then 60% at `ready_for_testing`. It drops from 90% or 80%, which is the backwards move settled point 2 accepts, and the stage label changes to "Implementing" with it.

The implementer's fraction is 1 when its status is `Done`, its done ÷ total when it has a plan, and 0 otherwise. A ticket in rule 2 or 6 whose implementer is working with no plan is indeterminate: it shows "Implementing" and a striped bar, and counts as 0% in the project average. Rule 3 is not affected: a `ready_for_testing` ticket stays at 60% even if its implementer starts another planless run. Showing "0%" beside an agent that has worked for half an hour would be a made-up number of the kind the brief rules out for agents.

### 5. The project rules
The counted tickets are the project's tickets that are not `closed`, in creation order. The position is the mean of their exact positions. The phase is "Planning" with no counted ticket, "Done" when all are accepted, "Blocked" when every unaccepted ticket is blocked, and otherwise the least advanced stage among the tickets still moving. The project's own bar state is finished when Done, blocked when Blocked, and otherwise in progress; each segment carries its ticket's state, so one blocked ticket shows as one red segment without turning the whole project red. With no counted ticket, the project copies its coordinator's agent progress, including having no position when the coordinator has no checklist.

### 6. The host keeps a board and serves what changed
A new `workspace-host/src/progress.rs` holds:

- `plans: HashMap<String, PlanCounts>` on `Host`, keyed by session. It is set when a plan step is recorded, removed when a run starts, and filled on first use after a start by one query for the latest plan step of every session's latest run, archived sessions included. The archived verifiers of an open ticket's latest cycle are on the board (settled point 4), so leaving archived sessions out would change the board across a restart; reading them all is simpler than picking those verifiers out, and `derive` ignores the rest. `recover_steps` prunes the steps of runs finished more than 30 days ago; an agent whose latest run was pruned has no entry and shows no checklist, and retention is not changed for it.
- `Progress`, also on `Host`: the last derived board, with a revision per entry and one counter. `Host::refresh_progress` derives a new board, compares each entry with the held one, gives every changed entry the next revision, and returns whether anything changed. An entry that disappears (a closed ticket, an archived agent) needs no tombstone: `Progress` remembers the revision of the last removal, and a cursor from before it is answered with the whole board, marked `complete`, which the client swaps in. The first draft had the client drop entries nothing refers to, but not every agent is referred to (a coordinator's unassigned worker is not), so that rule would have dropped live rows. Removals are rare, and the board is small.

`Actor::process` calls `refresh` after an event that changed state and after a recorded plan step, and never for other steps. When `refresh` reports a change, the actor sends on a third bounded signal, `Service::progress_changes()`, beside the state and step signals. A separate signal keeps plan changes from triggering a snapshot reload and keeps ordinary steps from triggering a progress fetch.

`Command::Progress { after: u64 }` answers with `ProgressPage { revision, complete, agents, tickets, projects }`: the entries whose revision is greater than `after`, or the whole board when `after` is 0 or older than the last removal. The desktop keeps one request in flight and a dirty flag, as the step watcher does. No pacing delay is needed, because plans change a few times per turn, not per token.

Deriving reads the same sessions, runtimes and tickets a snapshot reads. It runs on events that already cause a snapshot reload, and on plan steps, so it adds no work to the streaming path.

*Alternatives considered:*
- Put the board in `Snapshot`. Rejected: every plan step would then reload the full snapshot, which the work-stream rules forbid.
- Send the whole board on every change. It would be small, but the brief asks for incremental delivery and per-entry revisions cost one integer each.
- Reuse the step signal. Rejected: it fires for every text delta, so the desktop would ask for progress thirty times a second to learn that nothing changed.

### 7. Progress never reaches an agent
`refresh` writes nothing to SQLite, sends no message and sets no status. It runs after the scheduler, so it cannot change which turn starts. `workspace_context` is unchanged. A `set_plan` call is handled the same way as a provider plan step: it sends no state-change signal. The spec's "coordinator stays asleep" scenario is tested by asserting that a burst of plan steps leaves every queue and the state signal empty.

### 8. Desktop: the strip
`conversation.rs` gains `progress_strip`, drawn directly under `conversation_header` and above the transcript, at a fixed 36 px, so the transcript's measurements never change. For a coordinator it shows the phase label, the segmented bar, "56% · 1 of 4 tickets accepted" and the crew button. For an agent on a ticket it shows "Step 4 of 6 · Update the design README" (ellipsized), the bar, and "50% · ticket 30%". The step number is `done + 1`; `PlanCounts` carries no item index, and the first unfinished item is the current one by definition. When a working agent's plan has every item done, the label is "6 of 6" with no step text. Only a working agent gets "Step K of N": a finished agent reads "Done" (a passed verifier "Passed"), a failed verifier "Failed", any other blocked agent "Blocked · step K of N" or just "Blocked" with no plan, and an idle agent with a past checklist "K of N" with K the items done and no current item, as the mockup shows them. The bars are small helpers in `ui.rs`: `progress_bar(position, state, p)`, `segmented_bar(segments, p)` and a striped track for indeterminate, drawn with `gpui::pattern_slash` if the pinned GPUI has it and otherwise as a row of slanted fixed divs. The stripes do not animate.

### 9. Desktop: the sidebar
`project_row`, `ticket_row` and `session_row` each gain a 3 px track under the label and a short figure before the status glyph. Every unarchived row reserves the track, filled or not, so heights are constant; that adds about 6 px per row. `tree.rs` gains the text helpers, `agent_figure`, `ticket_figure` and `strip_label`, which are unit-tested like `pr_line`. The colors come from the palette already in use: focus, green and red.

### 10. Desktop: the crew panel
A new `crew_panel.rs` draws the panel as an inspector panel, `Panel::Crew`, beside the conversation, as the approved mockup shows it. The crew button, on the coordinator's strip only, opens it. Once open it stays open whichever of the project's agents is selected, still listing the whole project, with the selected agent's row marked, until the human closes it or opens another panel. Each ticket is a card with its title, "Stage · NN%" and bar; under it, one row per agent in `TicketProgress.agents` with `Role::agent_label(focus)`, the model from `models::profile_label`, the figure, the elapsed time and the bar. A row selects the agent's conversation. An archived verifier's row is not a link target for new input; it only opens its transcript, as archived sessions do today.

Elapsed time is `now − run_started_at` while working and `run_finished_at − run_started_at` otherwise, in whole minutes. A 30-second timer runs only while the panel is open and a listed agent is working, the same rule as the activity card's timer.

*Alternative considered:* add the bars to the Team panel's ticket cards and have the crew button open that panel. It avoids a second list of tickets and agents, but those cards carry verification lines, the PR block and the decision ledger, so the crew would not fit on one screen. This is the one placement choice worth the human's second look; see Questions.

### 11. Tests
- `workspace-core/src/progress.rs`: one test per spec scenario for agents, tickets and projects, built from plain structs.
- `workspace-host/src/stream.rs`: the existing Claude and Codex fixture tests also assert `Step.plan`.
- `set_plan`: the tool list and instructions in `tests/contracts.rs`; in `service.rs`, a call yields a plan step with counts and one progress signal, a later call updates it, over-bound input is refused and records nothing, and latest wins against a provider plan.
- `workspace-host/src/progress.rs`: revisions (unchanged entries keep theirs, a changed plan bumps the agent, its ticket and its project), and the rebuild from stored steps.
- `workspace-host/src/service.rs`, beside the step-signal tests: a plan step sends one progress signal and no state signal; a command step sends none; a burst coalesces to one pending signal.
- `workspace-host/tests/restart.rs`: figures are the same before and after a restart.
- `workspace-desktop`: the `tree.rs` text helpers, and the client board's merge and its replacement by a complete page in `progress.rs`.

## Open design points, settled

**1. A closed ticket and the project average.** It is left out: no segment, and not counted in M. A closed ticket is abandoned work, not finished work. Counting it as 100% would let closing tickets raise the bar, and counting it at its last position would leave the project short of 100% forever. This also matches the tree, which hides closed tickets. If every ticket is closed, the project reads "Planning" again.

**2. A second verification cycle.** The position is derived from the current records, with no high-water mark, so it can move backwards, and a new cycle does move it: from 80% blocked to 60% when the implementer reports ready again, then up as cycle 2's verifiers pass. Inside one cycle it never moves backwards, because a failed round keeps the passes. It never exceeds 90% before acceptance, because rule 5 tops out at 90%. A high-water mark was rejected: it would need stored state, and it would show 80% for a ticket in which no current verifier has passed.

**3. An agent between turns.** It keeps showing the checklist of its latest run, unchanged and without the word "working". With no plan in that run it shows an empty track. When its next run starts, the old checklist is dropped and it is indeterminate until it writes a plan. Stripes therefore always mean "working right now, nothing to count".

**4. Archived verifiers in the crew panel.** The verifiers of the ticket's latest cycle are listed even when archived, because the host archives a verifier as soon as its round ends with it passed, and those passes are exactly what the ticket's figure counts. Verifiers of earlier cycles and other archived agents are not listed. The sidebar tree is unchanged: it keeps hiding archived sessions.

## Risks / Trade-offs

- [Agents that write no plan show only stripes] → That is the honest answer, and the ticket still advances by stage.
- [A provider may change its plan tool] → It already has: see decision 1a. `set_plan` does not depend on the provider, and the provider mappings in `stream.rs` degrade to indeterminate, never to a wrong number.
- [Agents may not keep the checklist current] → The instruction is short and the tool is optional. A stale checklist shows a stale count beside the current step's text; the ticket's stage still moves on reports and verification, which no checklist can change.
- [Checklists are a rough measure] → An agent at 5 of 6 may have its longest item left. The label shows the current step, and nothing is called an estimate of time.
- [A bar that moves backwards can look like lost work] → The stage label changes with it ("Implementing" at 60% after "Blocked" at 80%), which says why.
- [Sidebar rows grow by about 6 px] → A long tree shows fewer rows. The track is 3 px with a 3 px gap; a setting to hide bars is left out until someone asks.
- [Equal weighting] → A one-line ticket counts as much as a large one. The segmented bar makes that visible, since each ticket is one segment.

## Migration Plan

No migration. `Step.plan` and `Command::Progress` are additive, and nothing is stored. An older build reading newer steps ignores the extra field. Rolling back removes the bars and nothing else.

## Questions for the human

These were settled with a recommendation above, but each is a product choice the human may want to overrule:

1. A ticket whose implementer works without a plan shows stripes and no percentage, and counts as 0% in the project average (decision 4). The alternative is a plain "0%".
2. The crew panel is its own inspector panel, not part of the Team panel (decision 10).
3. The approved mockup's blocked agent row reads "3 of 7 · waiting on you". The spec says "blocked" and leaves "waiting on you" to the attention inbox, because the project keeps status and escalation separate: a blocked worker does not always need the human.
