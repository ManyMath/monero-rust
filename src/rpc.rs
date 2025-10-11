//! RPC connection configuration for Monero daemons.

use std::time::Duration;
use zeroize::Zeroizing;

/// Daemon connection configuration.
///
/// Does not implement Debug to prevent credential leaks in logs.
#[derive(Clone)]
pub struct ConnectionConfig {
    pub daemon_address: String,
    pub trusted: bool,
    /// Credentials for HTTP digest auth, wrapped in Zeroizing for secure memory handling.
    pub credentials: Option<(Zeroizing<String>, Zeroizing<String>)>,
    pub ssl_options: Option<SslOptions>,
    pub proxy: Option<ProxyConfig>,
    pub timeout: Duration,
}

impl ConnectionConfig {
    pub fn new(daemon_address: String) -> Self {
        Self {
            daemon_address,
            trusted: false,
            credentials: None,
            ssl_options: None,
            proxy: None,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_trusted(mut self, trusted: bool) -> Self {
        self.trusted = trusted;
        self
    }

    pub fn with_credentials(mut self, username: String, password: String) -> Self {
        self.credentials = Some((Zeroizing::new(username), Zeroizing::new(password)));
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Builds URL with embedded credentials if present.
    ///
    /// Returns Zeroizing<String> since the URL may contain plaintext credentials.
    pub fn build_url(&self) -> Zeroizing<String> {
        if let Some((username, password)) = &self.credentials {
            let encoded_user = urlencoding::encode(username.as_str());
            let encoded_pass = urlencoding::encode(password.as_str());

            let url = if let Some(protocol_end) = self.daemon_address.find("://") {
                let protocol = &self.daemon_address[..protocol_end + 3];
                let rest = &self.daemon_address[protocol_end + 3..];
                format!("{}{}:{}@{}", protocol, encoded_user, encoded_pass, rest)
            } else {
                format!("http://{}:{}@{}", encoded_user, encoded_pass, self.daemon_address)
            };

            Zeroizing::new(url)
        } else {
            Zeroizing::new(self.daemon_address.clone())
        }
    }
}

#[derive(Debug, Clone)]
pub struct SslOptions {
    // TODO: cert validation, pinning, custom CA support
}

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    // TODO: proxy type, address, auth
}

/// Reconnection policy with exponential backoff.
#[derive(Debug, Clone)]
pub struct ReconnectionPolicy {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
    pub health_check_interval: Duration,
}

impl ReconnectionPolicy {
    pub fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(16),
            backoff_multiplier: 2.0,
            health_check_interval: Duration::from_secs(30),
        }
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_secs = self.initial_delay.as_secs_f64()
            * self.backoff_multiplier.powi(attempt as i32);
        let delay = Duration::from_secs_f64(delay_secs);
        delay.min(self.max_delay)
    }

    pub fn aggressive() -> Self {
        Self {
            max_attempts: 10,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
            backoff_multiplier: 1.5,
            health_check_interval: Duration::from_secs(15),
        }
    }

    pub fn conservative() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 3.0,
            health_check_interval: Duration::from_secs(60),
        }
    }
}

impl Default for ReconnectionPolicy {
    fn default() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(config.build_url().as_str(), "http://user:pass@localhost:18081");
    }

    #[test]
    fn test_build_url_no_protocol() {
        let config = ConnectionConfig::new("localhost:18081".to_string())
            .with_credentials("user".to_string(), "pass".to_string());
        assert_eq!(config.build_url().as_str(), "http://user:pass@localhost:18081");
    }

    #[test]
    fn test_build_url_encodes_special_chars() {
        let config = ConnectionConfig::new("http://localhost:18081".to_string())
            .with_credentials("user@email".to_string(), "p@ss:word".to_string());
        assert_eq!(config.build_url().as_str(), "http://user%40email:p%40ss%3Aword@localhost:18081");
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
        // Capped at max_delay
        assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(16));
    }
}
