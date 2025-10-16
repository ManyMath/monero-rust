use monero_rust::{ConnectionConfig, ReconnectionPolicy};
use std::time::Duration;

#[test]
fn test_connection_config_defaults() {
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
    assert_eq!(
        config.build_url().as_str(),
        "http://user:pass@localhost:18081"
    );
}

#[test]
fn test_build_url_no_protocol() {
    let config = ConnectionConfig::new("localhost:18081".to_string())
        .with_credentials("user".to_string(), "pass".to_string());
    assert_eq!(
        config.build_url().as_str(),
        "http://user:pass@localhost:18081"
    );
}

#[test]
fn test_build_url_encodes_special_chars() {
    let config = ConnectionConfig::new("http://localhost:18081".to_string())
        .with_credentials("user@email".to_string(), "p@ss:word".to_string());
    assert_eq!(
        config.build_url().as_str(),
        "http://user%40email:p%40ss%3Aword@localhost:18081"
    );
}

#[test]
fn test_reconnection_policy_defaults() {
    let policy = ReconnectionPolicy::default();
    assert_eq!(policy.max_attempts, 5);
    assert_eq!(policy.initial_delay, Duration::from_secs(1));
    assert_eq!(policy.max_delay, Duration::from_secs(16));
    assert_eq!(policy.backoff_multiplier, 2.0);
}

#[test]
fn test_exponential_backoff() {
    let policy = ReconnectionPolicy::default();
    assert_eq!(policy.delay_for_attempt(0), Duration::from_secs(1));
    assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(2));
    assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(4));
    assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(8));
    assert_eq!(policy.delay_for_attempt(4), Duration::from_secs(16));
    assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(16)); // capped
}
