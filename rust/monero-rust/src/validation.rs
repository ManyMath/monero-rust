//! Input validation for APIs that accept strings.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("Node URL is empty")]
    EmptyNodeUrl,
    #[error("Node URL has invalid scheme: '{0}'")]
    InvalidNodeUrlScheme(String),
    #[error("Invalid network: '{0}'. Expected mainnet, testnet, or stagenet")]
    InvalidNetwork(String),
}

pub fn validate_network(value: &str) -> Result<(), ValidationError> {
    match value.to_lowercase().as_str() {
        "mainnet" | "testnet" | "stagenet" => Ok(()),
        _ => Err(ValidationError::InvalidNetwork(value.to_string())),
    }
}

pub fn validate_node_url(url: &str) -> Result<(), ValidationError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::EmptyNodeUrl);
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(ValidationError::InvalidNodeUrlScheme(trimmed.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_networks_case_insensitively() {
        assert!(validate_network("mainnet").is_ok());
        assert!(validate_network("TESTNET").is_ok());
        assert!(validate_network("Stagenet").is_ok());
    }

    #[test]
    fn rejects_unknown_networks() {
        assert_eq!(
            validate_network("devnet"),
            Err(ValidationError::InvalidNetwork("devnet".to_string()))
        );
    }

    #[test]
    fn accepts_http_and_https_node_urls() {
        assert!(validate_node_url("http://127.0.0.1:18081").is_ok());
        assert!(validate_node_url("https://node.example").is_ok());
    }

    #[test]
    fn rejects_empty_and_unsupported_node_urls() {
        assert_eq!(validate_node_url("  "), Err(ValidationError::EmptyNodeUrl));
        assert_eq!(
            validate_node_url("ws://node.example"),
            Err(ValidationError::InvalidNodeUrlScheme(
                "ws://node.example".to_string()
            ))
        );
    }
}
