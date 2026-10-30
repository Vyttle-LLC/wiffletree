## Why

The Usage page counts tokens by provider, but not which models did the work or what that work would cost. The human wants to see, by model, what the recorded work would cost at public API list prices, to compare it with what the subscriptions cost.

## What Changes

- **A built-in price table** in `workspace-core/src/pricing.rs`, keyed by model id, with the date it was checked and its sources. Each request is priced from its own counts: uncached input, cache reads, Claude cache writes by lifetime (5 minutes at 1.25× input, 1 hour at 2×), output, fast-mode rates when Claude reports `speed: fast`, and Claude Haiku 5.5's rates for prompts over 100,000 tokens.
- **The report by model.** `UsageReport` gains the period's estimated cost, each model's tokens and cost, each day's tokens and cost by model, each coordinator's cost and the count of requests whose model has no price. The report reads what it needs from each request's stored `native` usage, so history already recorded is priced too.
- **The page**, after the study the human approved on 2026-10-30: an "Estimated API cost" tile; a By model card with a share bar, a table, a total row and the cost's parts on hover; the daily chart stacked by model in each model's color, with a legend and each model's tokens and cost in the tooltip; the cost and its split by model in the day detail; an Est. cost column on By coordinator; and a footnote on list prices versus subscription billing. Scope, period and provider drive all of it.
- **Not included:** GPT-6.1 Sol's long-context tier, since Codex reports a turn's tokens and not each request's; data-residency, batch and flex pricing; editing prices in the app.

## Capabilities

### New Capabilities
- `usage-cost`: pricing recorded requests by model and showing the estimate on the Usage page.

### Modified Capabilities
<!-- None. Token accounting is unchanged. -->

## Impact

- `workspace-core`: `pricing.rs`; `Cost`, `ModelUsage`, `DailyModelUsage`; new fields on `UsageReport` and `CoordinatorUsage`, all with serde defaults.
- `workspace-host`: `usage_report` groups requests once by day, model, coordinator and price-setting fields and derives totals, daily, coordinators and costs from that one query.
- `workspace-desktop`: `usage_view.rs`.
- Docs: `docs/PRD.md`, `design/README.md`, the study in `ui/mockups.html`.
