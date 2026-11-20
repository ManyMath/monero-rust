/// Sum atomic amounts, returning `None` on overflow.
///
/// Send and signing checks compare these sums to determine whether inputs cover
/// payments and fees. Unsigned transactions are untrusted: wrapping could make
/// excessive amounts pass those checks. Checked addition returns an error in
/// both debug and release builds.
pub fn checked_amount_sum(amounts: impl IntoIterator<Item = u64>) -> Option<u64> {
    amounts
        .into_iter()
        .try_fold(0u64, |sum, amount| sum.checked_add(amount))
}

/// Take `fee` out of the recipients' amounts as wallet2's
/// `TX::get_adjusted_dsts` does.
pub fn subtract_fee_from_recipients(
    recipients: &[(String, u64)],
    fee: u64,
) -> Result<Vec<(String, u64)>, String> {
    let total = checked_amount_sum(recipients.iter().map(|recipient| recipient.1))
        .ok_or_else(|| "recipient amounts overflow".to_string())?;
    if total < fee {
        return Err(format!(
            "the recipients receive {total} in total, less than the fee of {fee} to subtract"
        ));
    }

    let mut adjusted = recipients.to_vec();
    let count = adjusted.len() as u64;
    let mut remaining = fee;
    let mut share = 0;
    let mut next = 0;
    while remaining > 0 {
        if next == 0 {
            share = (remaining / count).max(1);
        }
        let amount = &mut adjusted[next].1;
        if *amount <= share {
            return Err(format!(
                "recipient {next} would be left with nothing after its share of the fee"
            ));
        }
        *amount -= share;
        remaining -= share;
        next = (next + 1) % adjusted.len();
    }
    Ok(adjusted)
}

/// [`subtract_fee_from_recipients`], returning no recipients where it fails.
pub fn adjust_recipients_for_fee(
    recipients: &[(String, u64)],
    estimated_fee: u64,
) -> Vec<(String, u64)> {
    subtract_fee_from_recipients(recipients, estimated_fee).unwrap_or_default()
}

/// Classify a broadcast error string into (is_double_spend, is_retryable).
pub fn classify_broadcast_error(error: &str) -> (bool, bool) {
    let lower = error.to_lowercase();
    let is_double_spend = lower.contains("double spend")
        || lower.contains("already spent")
        || lower.contains("key image already spent")
        || lower.contains("\"double_spend\":true")
        || lower.contains("\"double_spend\": true");
    let is_retryable = !is_double_spend
        && (lower.contains("connection")
            || lower.contains("timeout")
            || lower.contains("network")
            || lower.contains("failed to fetch"));
    (is_double_spend, is_retryable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_amount_sum_refuses_a_wrap() {
        assert_eq!(checked_amount_sum([1u64, 2, 3]), Some(6));
        assert_eq!(checked_amount_sum([u64::MAX, 0]), Some(u64::MAX));
        assert_eq!(checked_amount_sum([u64::MAX, 1]), None);
        assert_eq!(checked_amount_sum([u64::MAX, 2_000_000_001]), None);
        assert_eq!(checked_amount_sum(std::iter::empty()), Some(0));
    }

    #[test]
    fn classify_double_spend() {
        let (ds, retry) = classify_broadcast_error("double spend detected");
        assert!(ds);
        assert!(!retry);
    }

    #[test]
    fn classify_already_spent() {
        let (ds, retry) = classify_broadcast_error("output already spent");
        assert!(ds);
        assert!(!retry);
    }

    #[test]
    fn classify_key_image_already_spent() {
        let (ds, retry) = classify_broadcast_error("key image already spent in pool");
        assert!(ds);
        assert!(!retry);
    }

    #[test]
    fn classify_connection_error() {
        let (ds, retry) = classify_broadcast_error("connection refused");
        assert!(!ds);
        assert!(retry);
    }

    #[test]
    fn classify_timeout_error() {
        let (ds, retry) = classify_broadcast_error("request timeout");
        assert!(!ds);
        assert!(retry);
    }

    #[test]
    fn classify_network_error() {
        let (ds, retry) = classify_broadcast_error("network unreachable");
        assert!(!ds);
        assert!(retry);
    }

    #[test]
    fn classify_failed_to_fetch() {
        let (ds, retry) = classify_broadcast_error("failed to fetch from node");
        assert!(!ds);
        assert!(retry);
    }

    #[test]
    fn classify_unknown_error() {
        let (ds, retry) = classify_broadcast_error("some unknown error");
        assert!(!ds);
        assert!(!retry);
    }

    #[test]
    fn classify_case_insensitive() {
        let (ds, _) = classify_broadcast_error("DOUBLE SPEND");
        assert!(ds);
    }

    #[test]
    fn classify_json_double_spend_flag() {
        let (ds, retry) =
            classify_broadcast_error("{\"double_spend\": true, \"status\": \"Failed\"}");
        assert!(ds);
        assert!(!retry);
    }

    #[test]
    fn classify_empty_string() {
        let (ds, retry) = classify_broadcast_error("");
        assert!(!ds);
        assert!(!retry);
    }

    #[test]
    fn adjust_single_recipient() {
        let r = vec![("addr1".to_string(), 1_000_000_000_000u64)];
        let adj = adjust_recipients_for_fee(&r, 50_000_000);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].1, 1_000_000_000_000 - 50_000_000);
    }

    #[test]
    fn adjust_multiple_recipients_even() {
        let r = vec![
            ("a".to_string(), 1_000_000_000_000u64),
            ("b".to_string(), 1_000_000_000_000u64),
        ];
        let adj = adjust_recipients_for_fee(&r, 100_000_000);
        assert_eq!(adj[0].1, 1_000_000_000_000 - 50_000_000);
        assert_eq!(adj[1].1, 1_000_000_000_000 - 50_000_000);
    }

    #[test]
    fn adjust_multiple_recipients_remainder() {
        let r = vec![
            ("a".to_string(), 1_000_000_000_000u64),
            ("b".to_string(), 1_000_000_000_000u64),
            ("c".to_string(), 1_000_000_000_000u64),
        ];
        let adj = adjust_recipients_for_fee(&r, 100_000_000);
        assert_eq!(adj[0].1, 1_000_000_000_000 - 33_333_334);
        assert_eq!(adj[1].1, 1_000_000_000_000 - 33_333_333);
        assert_eq!(adj[2].1, 1_000_000_000_000 - 33_333_333);
        let total_deducted: u64 = r
            .iter()
            .zip(&adj)
            .map(|((_, orig), (_, adj))| orig - adj)
            .sum();
        assert_eq!(total_deducted, 100_000_000);
    }

    #[test]
    fn a_fee_larger_than_the_recipients_total_is_refused() {
        let r = vec![("a".to_string(), 10u64)];
        assert!(subtract_fee_from_recipients(&r, 1_000_000).is_err());
        assert!(adjust_recipients_for_fee(&r, 1_000_000).is_empty());
    }

    #[test]
    fn a_recipient_left_with_nothing_is_refused() {
        let r = vec![("a".to_string(), 500u64), ("b".to_string(), 1_000_000)];
        assert!(subtract_fee_from_recipients(&r, 1_000).is_err());
        assert!(adjust_recipients_for_fee(&r, 1_000).is_empty());
        let r = vec![("a".to_string(), 501u64), ("b".to_string(), 1_000_000)];
        assert_eq!(
            subtract_fee_from_recipients(&r, 1_000).unwrap(),
            [("a".to_string(), 1), ("b".to_string(), 999_500)]
        );
    }

    #[test]
    fn the_remainder_is_taken_one_piconero_at_a_time() {
        let r: Vec<(String, u64)> = ["a", "b", "c"]
            .iter()
            .map(|name| (name.to_string(), 100))
            .collect();
        let amounts = |fee| -> Vec<u64> {
            subtract_fee_from_recipients(&r, fee)
                .unwrap()
                .into_iter()
                .map(|recipient| recipient.1)
                .collect()
        };
        assert_eq!(amounts(11), [96, 96, 97]);
        assert_eq!(amounts(10), [96, 97, 97]);
        assert_eq!(amounts(2), [99, 99, 100]);
        assert_eq!(amounts(0), [100, 100, 100]);
    }

    /// Recorded from wallet2 22578c3f7: three destinations of 1 XMR, fee 59548931.
    #[test]
    fn a_fee_recorded_from_wallet2_is_spread_the_same() {
        const XMR: u64 = 1_000_000_000_000;
        let r: Vec<(String, u64)> = ["a", "b", "c"]
            .iter()
            .map(|name| (name.to_string(), XMR))
            .collect();
        let taken: Vec<u64> = subtract_fee_from_recipients(&r, 59_548_931)
            .unwrap()
            .into_iter()
            .map(|recipient| XMR - recipient.1)
            .collect();
        assert_eq!(taken, [19_849_644, 19_849_644, 19_849_643]);
    }

    #[test]
    fn no_recipients_can_only_bear_no_fee() {
        assert_eq!(subtract_fee_from_recipients(&[], 0), Ok(vec![]));
        assert!(subtract_fee_from_recipients(&[], 1).is_err());
    }

    #[test]
    fn adjust_empty_recipients() {
        let adj = adjust_recipients_for_fee(&[], 50_000_000);
        assert!(adj.is_empty());
    }

    #[test]
    fn adjust_zero_fee() {
        let r = vec![("a".to_string(), 500u64)];
        let adj = adjust_recipients_for_fee(&r, 0);
        assert_eq!(adj[0].1, 500);
    }

    #[test]
    fn adjust_preserves_addresses() {
        let r = vec![
            ("addr_one".to_string(), 100u64),
            ("addr_two".to_string(), 200u64),
        ];
        let adj = adjust_recipients_for_fee(&r, 10);
        assert_eq!(adj[0].0, "addr_one");
        assert_eq!(adj[1].0, "addr_two");
    }
}
