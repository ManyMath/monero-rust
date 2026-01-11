use monero_rust::rpc::{ConnectionConfig, ReconnectionPolicy};
use std::time::Duration;

#[test]
fn test_connection_config_new() {
    let config = ConnectionConfig::new("http://localhost:18081".to_string());
    assert_eq!(config.daemon_address, "http://localhost:18081");
    assert!(!config.trusted);
    assert!(config.credentials.is_none());
    assert_eq!(config.timeout, Duration::from_secs(30));
}

#[test]
fn test_connection_config_with_credentials() {
    let config = ConnectionConfig::new("http://localhost:18081".to_string())
        .with_credentials("user".to_string(), "pass".to_string());

    assert!(config.credentials.is_some());
    let (username, password) = config.credentials.as_ref().unwrap();
    assert_eq!(username.as_str(), "user");
    assert_eq!(password.as_str(), "pass");
}

#[test]
fn test_build_url_without_credentials() {
    let config = ConnectionConfig::new("http://localhost:18081".to_string());
    assert_eq!(config.build_url().as_str(), "http://localhost:18081");
}

#[test]
fn test_build_url_with_credentials() {
    let config = ConnectionConfig::new("http://localhost:18081".to_string())
        .with_credentials("user".to_string(), "pass".to_string());

    assert_eq!(config.build_url().as_str(), "http://user:pass@localhost:18081");
}

#[test]
fn test_build_url_no_protocol() {
    let config = ConnectionConfig::new("localhost:18081".to_string())
        .with_credentials("user".to_string(), "pass".to_string());

    assert_eq!(config.build_url().as_str(), "http://user:pass@localhost:18081");
}

#[test]
fn test_build_url_with_special_chars() {
    // Test URL encoding of special characters in credentials
    let config = ConnectionConfig::new("http://localhost:18081".to_string())
        .with_credentials("user@email".to_string(), "p@ss:word".to_string());

    // @ and : should be URL encoded
    assert_eq!(config.build_url().as_str(), "http://user%40email:p%40ss%3Aword@localhost:18081");
}

#[test]
fn test_reconnection_policy_default() {
    let policy = ReconnectionPolicy::default();
    assert_eq!(policy.max_attempts, 5);
    assert_eq!(policy.initial_delay, Duration::from_secs(1));
    assert_eq!(policy.max_delay, Duration::from_secs(16));
    assert_eq!(policy.backoff_multiplier, 2.0);
}

#[test]
fn test_delay_for_attempt() {
    let policy = ReconnectionPolicy::default();

    // Attempt 0: 1s
    assert_eq!(policy.delay_for_attempt(0), Duration::from_secs(1));

    // Attempt 1: 2s (1 * 2^1)
    assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(2));

    // Attempt 2: 4s (1 * 2^2)
    assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(4));

    // Attempt 3: 8s (1 * 2^3)
    assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(8));

    // Attempt 4: 16s (1 * 2^4), capped at max_delay
    assert_eq!(policy.delay_for_attempt(4), Duration::from_secs(16));

    // Attempt 5: Would be 32s but capped at max_delay (16s)
    assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(16));
}

#[test]
fn test_aggressive_policy() {
    let policy = ReconnectionPolicy::aggressive();
    assert_eq!(policy.max_attempts, 10);
    assert_eq!(policy.initial_delay, Duration::from_millis(500));
    assert_eq!(policy.backoff_multiplier, 1.5);
}

#[test]
fn test_conservative_policy() {
    let policy = ReconnectionPolicy::conservative();
    assert_eq!(policy.max_attempts, 3);
    assert_eq!(policy.initial_delay, Duration::from_secs(2));
    assert_eq!(policy.backoff_multiplier, 3.0);
}
