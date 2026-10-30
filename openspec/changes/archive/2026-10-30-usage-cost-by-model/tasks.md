## 1. Core

- [x] 1.1 `pricing.rs`: the price table with its date and sources, `estimate`, `is_long_prompt` and `model_name`, with tests for cache lifetimes, fast mode, the long-prompt tier and an unknown model.
- [x] 1.2 `Cost`, `ModelUsage`, `DailyModelUsage`; cost fields on `UsageReport` and `CoordinatorUsage` with serde defaults.

## 2. Host

- [x] 2.1 `usage_report`: one grouped query reading speed and the 1-hour write share from `native`; totals, daily, coordinators, models and daily models from it.
- [x] 2.2 Test: mixed Claude and Codex history priced per model, day and coordinator, an unknown model left out and counted, under the project and provider filters.

## 3. Desktop

- [x] 3.1 Estimated API cost tile, By model card, daily chart by model with legend and tooltip, day detail cost and split, coordinator cost column, footnote.

## 4. Docs

- [x] 4.1 `ui/mockups.html`, `design/README.md`, `docs/PRD.md`.
