//! Cost and budget bookkeeping.
//!
//! No cost here is invented. A price exists only when the provider published one
//! (see `capabilities::Price`), and a token count only when the provider reported
//! it, so `estimate` returns `None` rather than a zero when either is missing —
//! a missing number is reported as "not reported", not as a free request.
//!
//! The budget window is a rolling 24 hours rather than a calendar day, because
//! the app stores timestamps in UTC and has no user timezone to cut a day with.
//! A rolling window is the honest thing to enforce.

use crate::capabilities::Price;

/// How much a request cost and how that figure was reached.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cost {
    pub(crate) usd: f64,
    /// Always `estimated` today: providers report tokens but not money, so the
    /// figure is their price × their tokens, never a billed amount.
    pub(crate) basis: &'static str,
}

/// Money spent in a window, plus how many successful requests in it had no
/// published price — the count that makes the figure a floor rather than a
/// complete total.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Spend {
    pub(crate) usd: f64,
    pub(crate) unknown: i64,
}

/// Cost from the provider's own per-million prices and its own token counts.
/// `None` when neither count is known, because a request with no reported usage
/// has no defensible cost.
pub(crate) fn estimate(
    price: &Price,
    prompt: Option<i64>,
    completion: Option<i64>,
) -> Option<Cost> {
    let prompt = prompt.unwrap_or(0).max(0) as f64;
    let completion = completion.unwrap_or(0).max(0) as f64;
    if prompt == 0.0 && completion == 0.0 {
        return None;
    }
    let usd = prompt / 1_000_000.0 * price.input_per_million
        + completion / 1_000_000.0 * price.output_per_million;
    Some(Cost {
        usd,
        basis: "estimated",
    })
}

/// The refusal a spent budget produces, or `None` when the call may proceed.
/// A budget of zero or less means "no limit": the default, and the only value
/// that keeps a fresh install from blocking its first message.
pub(crate) fn budget_refusal(budget_usd: f64, spend: &Spend) -> Option<String> {
    if budget_usd <= 0.0 || spend.usd < budget_usd {
        return None;
    }
    let mut message = format!(
        "Today's estimated spend ${:.4} reached your ${:.2} budget (rolling 24 hours). Raise the budget in Settings, or wait for older requests to fall out of the window.",
        spend.usd, budget_usd
    );
    if spend.unknown > 0 {
        message.push_str(&format!(
            " {} request(s) in the window had no published price, so this is a floor.",
            spend.unknown
        ));
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price() -> Price {
        Price {
            input_per_million: 3.0,
            output_per_million: 15.0,
        }
    }

    #[test]
    fn cost_is_the_providers_price_times_its_reported_tokens() {
        // 1M input at $3 + 1M output at $15.
        let cost = estimate(&price(), Some(1_000_000), Some(1_000_000)).expect("cost");
        assert_eq!(cost.usd, 18.0);
        assert_eq!(cost.basis, "estimated");
        // 1k in, 500 out.
        let small = estimate(&price(), Some(1_000), Some(500)).expect("cost");
        assert!((small.usd - 0.0105).abs() < 1e-9, "{}", small.usd);
    }

    #[test]
    fn no_reported_usage_means_no_cost_rather_than_zero() {
        assert_eq!(estimate(&price(), None, None), None);
        assert_eq!(estimate(&price(), Some(0), Some(0)), None);
        // One half reported is still a cost for that half.
        assert!(estimate(&price(), Some(1_000), None).is_some());
    }

    #[test]
    fn a_budget_of_zero_never_blocks_anything() {
        let spend = Spend {
            usd: 999.0,
            unknown: 40,
        };
        assert_eq!(budget_refusal(0.0, &spend), None);
        assert_eq!(budget_refusal(-1.0, &spend), None);
    }

    #[test]
    fn a_budget_blocks_at_the_limit_and_names_the_unknown_cost_floor() {
        let spend = Spend {
            usd: 4.9999,
            unknown: 0,
        };
        assert_eq!(budget_refusal(5.0, &spend), None, "below the limit");

        let spent = Spend {
            usd: 5.0,
            unknown: 2,
        };
        let refusal = budget_refusal(5.0, &spent).expect("refused at the limit");
        assert!(refusal.contains("$5.0000"), "{refusal}");
        assert!(refusal.contains("$5.00 budget"), "{refusal}");
        assert!(refusal.contains("2 request(s)"), "{refusal}");
        assert!(refusal.contains("floor"), "{refusal}");

        let over = Spend {
            usd: 9.5,
            unknown: 0,
        };
        let refusal = budget_refusal(5.0, &over).expect("refused over the limit");
        // No unknown requests: nothing to apologise for.
        assert!(!refusal.contains("floor"), "{refusal}");
    }
}
