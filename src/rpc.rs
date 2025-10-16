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
