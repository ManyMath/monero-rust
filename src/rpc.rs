//! RPC connection management for Monero daemon communication.
//!
//! This module provides structures and functions for managing connections
//! to Monero daemons, including health checks, reconnection logic, and
//! configuration management.

use std::time::Duration;
use zeroize::Zeroizing;

/// Configuration for establishing a connection to a Monero daemon.
///
/// This structure contains all parameters needed to connect to a daemon,
/// including authentication, SSL settings, and proxy configuration.
///
/// # Security Note
///
/// This type does NOT implement Debug to prevent credential leaks in logs.
/// Credentials are stored in Zeroizing wrappers to ensure they are cleared
/// from memory when dropped.
#[derive(Clone)]
pub struct ConnectionConfig {
    /// Daemon RPC address (e.g., "http://node.example.com:18081")
    pub daemon_address: String,

    /// Whether to trust the daemon as a trusted node
    ///
    /// Trusted nodes are assumed to provide accurate blockchain data.
    /// Untrusted nodes may require additional validation.
    pub trusted: bool,

    /// Optional credentials for daemon authentication
    ///
    /// Format: (username, password)
    /// These credentials are used for HTTP digest authentication.
    /// Credentials are wrapped in Zeroizing to ensure secure memory handling.
    /// Note: Credentials are stored in memory only and NOT persisted to disk.
    pub credentials: Option<(Zeroizing<String>, Zeroizing<String>)>,

    /// SSL/TLS configuration options
    ///
    /// TODO: Define SSL configuration structure once implementation is ready.
    /// This should include certificate validation, pinning, and custom CA support.
    pub ssl_options: Option<SslOptions>,

    /// Proxy configuration for routing RPC requests
    ///
    /// TODO: Define proxy configuration structure.
    /// This should support HTTP/HTTPS/SOCKS5 proxies with optional authentication.
    ///
    /// IMPORTANT: Proxy functionality is not yet implemented.
    /// Tests for proxy connections should be added when proxy support is implemented.
    pub proxy: Option<ProxyConfig>,

    /// Request timeout duration
    ///
    /// Default: 30 seconds
    pub timeout: Duration,
}

impl ConnectionConfig {
    /// Creates a new ConnectionConfig with the specified daemon address.
    ///
    /// # Arguments
    /// * `daemon_address` - The RPC endpoint URL
    ///
    /// # Returns
    /// A ConnectionConfig with default settings (untrusted, no auth, 30s timeout)
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

    /// Sets whether the daemon should be trusted.
    pub fn with_trusted(mut self, trusted: bool) -> Self {
        self.trusted = trusted;
        self
    }

    /// Sets credentials for daemon authentication.
    ///
    /// Credentials are wrapped in Zeroizing for secure memory handling.
    pub fn with_credentials(mut self, username: String, password: String) -> Self {
        self.credentials = Some((Zeroizing::new(username), Zeroizing::new(password)));
        self
    }

    /// Sets the request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Builds the daemon URL with embedded credentials if provided.
    ///
    /// If credentials are present, they are embedded in the URL format:
    /// `http://username:password@host:port`
    ///
    /// This is required for SimpleRequestRpc which handles digest authentication.
    ///
    /// # Security Note
    ///
    /// The returned URL contains credentials in plaintext and should be:
    /// - Used immediately for connection
    /// - Not logged or printed
    /// - Not stored longer than necessary
    ///
    /// Special characters in credentials are URL-encoded for safety.
    pub fn build_url(&self) -> Zeroizing<String> {
        if let Some((username, password)) = &self.credentials {
            // URL encode username and password to handle special characters
            let encoded_user = urlencoding::encode(username.as_str());
            let encoded_pass = urlencoding::encode(password.as_str());

            // Parse the URL to inject credentials
            let url = if let Some(protocol_end) = self.daemon_address.find("://") {
                let protocol = &self.daemon_address[..protocol_end + 3];
                let rest = &self.daemon_address[protocol_end + 3..];
                format!("{}{}:{}@{}", protocol, encoded_user, encoded_pass, rest)
            } else {
                // No protocol specified, assume http
                format!("http://{}:{}@{}", encoded_user, encoded_pass, self.daemon_address)
            };

            Zeroizing::new(url)
        } else {
            Zeroizing::new(self.daemon_address.clone())
        }
    }
}

/// SSL/TLS configuration options.
///
/// TODO: Implement SSL configuration structure.
/// This should include:
/// - Certificate validation settings
/// - Certificate pinning
/// - Custom CA certificate support
/// - Client certificate authentication
#[derive(Debug, Clone)]
pub struct SslOptions {
    // Placeholder for future SSL configuration
}

/// Proxy configuration for RPC requests.
///
/// TODO: Implement proxy configuration structure.
/// This should include:
/// - Proxy type (HTTP, HTTPS, SOCKS5)
/// - Proxy address and port
/// - Optional proxy authentication
/// - Proxy connection timeout
///
/// IMPORTANT: Proxy tests must be implemented when this feature is added.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    // Placeholder for future proxy configuration
}

/// Policy for automatic reconnection attempts.
///
/// This structure defines how the wallet should attempt to reconnect
/// to the daemon when a connection is lost.
#[derive(Debug, Clone)]
pub struct ReconnectionPolicy {
    /// Maximum number of reconnection attempts before giving up
    pub max_attempts: u32,

    /// Initial delay before first reconnection attempt
    pub initial_delay: Duration,

    /// Maximum delay between reconnection attempts
    pub max_delay: Duration,

    /// Backoff multiplier for exponential backoff
    ///
    /// Each retry delay is multiplied by this factor.
    /// Example: With multiplier 2.0, delays are: 1s, 2s, 4s, 8s, 16s
    pub backoff_multiplier: f64,

    /// Health check interval for background monitoring
    ///
    /// How often to check if the daemon connection is still alive.
    /// Default: 30 seconds
    pub health_check_interval: Duration,
}

impl ReconnectionPolicy {
    /// Creates a new ReconnectionPolicy with default settings.
    ///
    /// Defaults:
    /// - Max attempts: 5
    /// - Initial delay: 1 second
    /// - Max delay: 16 seconds
    /// - Backoff multiplier: 2.0 (exponential)
    /// - Health check interval: 30 seconds
    pub fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(16),
            backoff_multiplier: 2.0,
            health_check_interval: Duration::from_secs(30),
        }
    }

    /// Calculates the delay for a specific retry attempt.
    ///
    /// Uses exponential backoff: delay = initial_delay * (multiplier ^ attempt)
    /// Capped at max_delay.
    ///
    /// # Arguments
    /// * `attempt` - The attempt number (0-indexed)
    ///
    /// # Returns
    /// The duration to wait before this attempt
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_secs = self.initial_delay.as_secs_f64()
            * self.backoff_multiplier.powi(attempt as i32);

        let delay = Duration::from_secs_f64(delay_secs);

        // Cap at max_delay
        if delay > self.max_delay {
            self.max_delay
        } else {
            delay
        }
    }

    /// Creates a policy with aggressive reconnection (faster, more attempts).
    pub fn aggressive() -> Self {
        Self {
            max_attempts: 10,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
            backoff_multiplier: 1.5,
            health_check_interval: Duration::from_secs(15),
        }
    }

    /// Creates a policy with conservative reconnection (slower, fewer attempts).
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
