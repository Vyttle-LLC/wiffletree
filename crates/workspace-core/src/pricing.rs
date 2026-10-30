//! Public API list prices, for estimating what recorded work would cost on the API. A
//! subscription is billed differently; these prices only put a dollar figure on the tokens.
//!
//! Sources, checked on the date below:
//! - Anthropic: <https://platform.claude.com/docs/en/about-claude/pricing> (model, fast mode
//!   and long context pricing). Cache writes cost 1.25× base input for 5 minutes and 2× for
//!   1 hour; fast mode rates take the same cache multipliers.
//! - OpenAI: <https://developers.openai.com/api/docs/pricing>. GPT-6.1 Sol's long-context
//!   tier (above 272K input tokens) is left out: Codex reports a turn's tokens, not each
//!   request's, so no request's size is known.
use crate::Cost;

/// The day the prices below were last checked against the sources.
pub const PRICES_AS_OF: &str = "2026-10-30";

/// US dollars per million tokens. Cache writes are priced from `input`.
struct Rates {
    input: f64,
    cache_read: f64,
    output: f64,
}
struct ModelPrice {
    id: &'static str,
    name: &'static str,
    standard: Rates,
    fast: Option<Rates>,
    /// Rates for a request whose prompt, cache included, is over this many tokens.
    long_prompt: Option<(u64, Rates)>,
}
const fn rates(input: f64, cache_read: f64, output: f64) -> Rates {
    Rates {
        input,
        cache_read,
        output,
    }
}
const PRICES: &[ModelPrice] = &[
    ModelPrice {
        id: "claude-opus-5-5",
        name: "Claude Opus 5.5",
        standard: rates(4., 0.2, 20.),
        fast: Some(rates(8., 0.4, 40.)),
        long_prompt: None,
    },
    ModelPrice {
        id: "gpt-6.1-sol",
        name: "GPT-6.1 Sol",
        standard: rates(2., 0.1, 10.),
        fast: None,
        long_prompt: None,
    },
    ModelPrice {
        id: "claude-sonnet-5-5",
        name: "Claude Sonnet 5.5",
        standard: rates(2., 0.1, 10.),
        fast: None,
        long_prompt: None,
    },
    ModelPrice {
        id: "claude-sonnet-5",
        name: "Claude Sonnet 5",
        standard: rates(2., 0.2, 10.),
        fast: None,
        long_prompt: None,
    },
    ModelPrice {
        id: "claude-fable-5-1",
        name: "Claude Fable 5.1",
        standard: rates(10., 0.25, 50.),
        fast: None,
        long_prompt: None,
    },
    ModelPrice {
        id: "claude-haiku-5-5",
        name: "Claude Haiku 5.5",
        standard: rates(0.1, 0.01, 0.5),
        fast: None,
        long_prompt: Some((100_000, rates(0.5, 0.05, 2.5))),
    },
];

/// The tokens of one or more requests that share a model, a speed and a prompt-size tier.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BilledTokens {
    pub uncached_input: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub output: u64,
    pub fast: bool,
    pub long_prompt: bool,
}

fn price(model: &str) -> Option<&'static ModelPrice> {
    PRICES.iter().find(|price| price.id == model)
}
/// A model's display name, or its id when it has no price.
pub fn model_name(model: &str) -> &str {
    price(model).map_or(model, |price| price.name)
}
/// Whether a request of `prompt` input tokens, cache included, pays `model`'s long-prompt rates.
pub fn is_long_prompt(model: &str, prompt: u64) -> bool {
    price(model)
        .and_then(|price| price.long_prompt.as_ref())
        .is_some_and(|(threshold, _)| prompt > *threshold)
}
/// What the tokens would cost on the API, or `None` when the model, or its fast mode, has no
/// known price.
pub fn estimate(model: &str, tokens: &BilledTokens) -> Option<Cost> {
    let price = price(model)?;
    let rates = match (tokens.fast, tokens.long_prompt) {
        (true, _) => price.fast.as_ref()?,
        (false, true) => &price.long_prompt.as_ref()?.1,
        (false, false) => &price.standard,
    };
    let dollars = |tokens: u64, per_million: f64| tokens as f64 * per_million / 1e6;
    Some(Cost {
        input: dollars(tokens.uncached_input, rates.input),
        cache_read: dollars(tokens.cache_read, rates.cache_read),
        cache_write: dollars(tokens.cache_write_5m, rates.input * 1.25)
            + dollars(tokens.cache_write_1h, rates.input * 2.),
        output: dollars(tokens.output, rates.output),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const MILLION: u64 = 1_000_000;

    #[test]
    fn an_hour_long_cache_write_costs_more_than_a_five_minute_one() {
        let five = BilledTokens {
            cache_write_5m: MILLION,
            ..Default::default()
        };
        let hour = BilledTokens {
            cache_write_1h: MILLION,
            ..Default::default()
        };
        let five = estimate("claude-opus-5-5", &five).unwrap().cache_write;
        let hour = estimate("claude-opus-5-5", &hour).unwrap().cache_write;
        assert_eq!(hour / five, 1.6);
    }
    #[test]
    fn fast_mode_prices_every_kind_at_its_own_rates() {
        let tokens = BilledTokens {
            uncached_input: MILLION,
            cache_read: MILLION,
            cache_write_1h: MILLION,
            output: MILLION,
            ..Default::default()
        };
        let standard = estimate("claude-opus-5-5", &tokens).unwrap();
        let fast = estimate(
            "claude-opus-5-5",
            &BilledTokens {
                fast: true,
                ..tokens.clone()
            },
        )
        .unwrap();
        assert!((fast.total() - standard.total() * 2.).abs() < 1e-9);
        // A model without a fast mode has no fast price to guess.
        assert!(
            estimate(
                "claude-sonnet-5-5",
                &BilledTokens {
                    fast: true,
                    ..tokens
                }
            )
            .is_none()
        );
    }
    #[test]
    fn only_a_prompt_over_the_threshold_pays_long_prompt_rates() {
        assert!(!is_long_prompt("claude-haiku-5-5", 100_000));
        assert!(is_long_prompt("claude-haiku-5-5", 100_001));
        assert!(!is_long_prompt("claude-opus-5-5", 900_000));
        let tokens = BilledTokens {
            output: MILLION,
            ..Default::default()
        };
        let short = estimate("claude-haiku-5-5", &tokens).unwrap();
        let long = estimate(
            "claude-haiku-5-5",
            &BilledTokens {
                long_prompt: true,
                ..tokens
            },
        )
        .unwrap();
        assert!(long.output > short.output);
    }
    #[test]
    fn an_unknown_model_has_no_price_and_keeps_its_id() {
        assert!(estimate("unreported", &BilledTokens::default()).is_none());
        assert_eq!(model_name("unreported"), "unreported");
        assert_eq!(model_name("gpt-6.1-sol"), "GPT-6.1 Sol");
    }
}
