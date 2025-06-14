/// Classify a broadcast error string into (is_double_spend, is_retryable).
pub fn classify_broadcast_error(error: &str) -> (bool, bool) {
    let lower = error.to_lowercase();
    let is_double_spend = lower.contains("double spend")
        || lower.contains("already spent")
        || lower.contains("key image already spent");
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
    fn classify_empty_string() {
        let (ds, retry) = classify_broadcast_error("");
        assert!(!ds);
        assert!(!retry);
    }
}
