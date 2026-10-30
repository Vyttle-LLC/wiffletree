## Context

`usage_requests` stores each request's model, its normalized counts and `native`, the provider's own usage. `normalize` makes `input` include cache reads and writes for both providers, and reasoning is part of output, so uncached input is `input − cache_read − cache_write` and output is priced once. Claude's `native` holds `cache_creation.ephemeral_5m_input_tokens` / `ephemeral_1h_input_tokens` and `speed`. A Codex row is one turn, which may be several requests.

## Decisions

- **Read `native` in the query, no migration.** The report's one grouped query takes `speed` and the 1-hour share of each cache write from `native` with `json_extract`, so recorded history is priced like new requests. It groups by day, provider, model, coordinator, own-or-worker, fast and long prompt, and sums the counts and the 1-hour writes; each group is priced in Rust and folded into the totals, the provider daily series, the model and daily-model series and the coordinators. The earlier three queries become this one.
- **Cache writes without a split** (compactions, which report only a total) are priced at the 5-minute rate, the API's default lifetime.
- **Long prompts** are decided per request with a SQL function over the table's threshold, because a request's tier depends on its own size. Only Claude Haiku 5.5 has one in the table. GPT-6.1 Sol's documented tier above 272K is left out: a Codex row is a turn, so no request's size is known, and Sol is priced at its standard rates.
- **Unpriced requests.** A model not in the table, or a fast request on a model without fast rates, has no price. Its tokens still count; its cost is "—", it is left out of every cost total, and the report counts its requests.
- **Prices** were checked on 2026-10-30 against <https://platform.claude.com/docs/en/about-claude/pricing> and <https://developers.openai.com/api/docs/pricing>. Claude Sonnet 5.5's cache read is $0.10, not the $0.20 the brief listed; GPT-6.1 Sol is $2 / $0.10 / $10 (the $1 / $0.05 / $5 figure is GPT-6.1 Terra's).
- **Colors** are fixed per model id in the desktop from Tidal tokens: Opus 5.5 accent, GPT-6.1 Sol blue, Sonnet 5.5 green, Sonnet 5 magenta, Fable 5.1 focus, Haiku 5.5 yellow; any other model is subtle.
- **Day detail** keeps its per-provider blocks, which carry coverage, and adds the day's cost and its split by model beneath them.
