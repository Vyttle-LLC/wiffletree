# usage-cost Specification

## Purpose
Estimates what recorded work would cost at public API list prices, by model, day and coordinator, and shows it on the Usage page beside the tokens.

## Requirements

### Requirement: Recorded requests are priced at API list prices
The host SHALL estimate each recorded request's cost from a built-in price table keyed by model id, with the date its prices were checked. It SHALL price uncached input, cache reads, cache writes and output separately, never charging cache tokens as uncached input. Claude cache writes SHALL be priced by lifetime, 5 minutes at 1.25× and 1 hour at 2× the input rate, and a write whose lifetime is not reported at the 5-minute rate. A Claude request reported with `speed` `fast` SHALL be priced at its model's fast-mode rates. A request whose prompt, cache included, is over its model's documented long-prompt threshold SHALL be priced at the long-prompt rates where the request's own size is known. History recorded before this change SHALL be priced the same way.

#### Scenario: Cache lifetimes
- **WHEN** a Claude Opus 5.5 request wrote 1M tokens to the 5-minute cache and 1M to the 1-hour cache
- **THEN** its cache write costs $5 and $8 for them

#### Scenario: Fast mode
- **WHEN** a Claude Opus 5.5 request reports `speed` `fast`
- **THEN** every kind of its tokens is priced at fast-mode rates

#### Scenario: Unknown model
- **WHEN** a request's model is `unreported`
- **THEN** it has no estimated cost and its tokens still count

### Requirement: The report gives cost by model, day and coordinator
The usage report SHALL give, for its period, project scope and provider, the estimated cost of priced requests, each model's provider, requests, tokens by kind and cost, each day's tokens and cost by model, each coordinator's cost, and the number of requests whose model has no price. Costs SHALL leave out unpriced requests, and the total SHALL equal the sum of the priced models.

#### Scenario: Mixed history
- **WHEN** the period has Claude Opus 5.5, GPT-6.1 Sol and `unreported` requests
- **THEN** the report lists all three models, the total is the sum of Opus 5.5 and Sol, and one unpriced request is counted

#### Scenario: Filters
- **WHEN** the report is asked for Codex only
- **THEN** its cost, models, days and coordinators' costs cover only Codex requests

### Requirement: The Usage page shows the estimate
The Usage page SHALL show an Estimated API cost tile; a By model card with a share bar, each model's tokens and cost, a total row and the cost's parts on hover; the daily chart stacked by model in each model's fixed color, with a legend and each model's tokens and cost in the day's tooltip; the cost and its split by model in the day detail; each coordinator's cost; and a note that prices are public API list prices as of the table's date, not subscription billing. A model with no price SHALL show "—", and the page SHALL say how many requests that leaves out.

#### Scenario: Changing filters
- **WHEN** the human changes the scope, period or provider
- **THEN** the tile, By model, chart, day detail and coordinator costs all reflect the new report
