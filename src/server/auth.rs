//! Token-based authentication for CassetteDB server.
//!
//! Provides a simple bearer-token authentication scheme.
//! Clients must include an `Authorization: Bearer <token>` header.

use std::collections::HashSet;
use std::sync::Mutex;

/// Trait for authentication backends.
pub trait Authenticator: Send + Sync {
    /// Validate a token. Returns true if valid.
    fn validate(&self, token: &str) -> bool;
}

/// Simple in-memory token-based authenticator.
pub struct AuthManager {
    tokens: Mutex<HashSet<String>>,
    enabled: bool,
}

impl AuthManager {
    /// Create a new auth manager with an optional master token.
    pub fn new(master_token: Option<String>) -> Self {
        let mut tokens = HashSet::new();
        let enabled = master_token.is_some();
        if let Some(token) = master_token {
            tokens.insert(token);
        }
        Self {
            tokens: Mutex::new(tokens),
            enabled,
        }
    }

    /// Add a new token.
    pub fn add_token(&self, token: String) {
        let mut tokens = self.tokens.lock().unwrap();
        tokens.insert(token);
    }

    /// Remove a token.
    pub fn remove_token(&self, token: &str) -> bool {
        let mut tokens = self.tokens.lock().unwrap();
        tokens.remove(token)
    }

    /// Check if authentication is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Extract token from an Authorization header value.
    pub fn extract_token(header: &str) -> Option<&str> {
        let header = header.trim();
        if header.len() > 7 && header[..7].eq_ignore_ascii_case("bearer ") {
            Some(&header[7..])
        } else {
            None
        }
    }
}

impl Authenticator for AuthManager {
    fn validate(&self, token: &str) -> bool {
        if !self.enabled {
            return true;
        }
        let tokens = self.tokens.lock().unwrap();
        tokens.contains(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_disabled() {
        let auth = AuthManager::new(None);
        assert!(!auth.is_enabled());
        assert!(auth.validate("anything"));
    }

    #[test]
    fn test_auth_enabled() {
        let auth = AuthManager::new(Some("secret123".to_string()));
        assert!(auth.is_enabled());
        assert!(auth.validate("secret123"));
        assert!(!auth.validate("wrong"));
    }

    #[test]
    fn test_add_remove_token() {
        let auth = AuthManager::new(Some("master".to_string()));
        auth.add_token("extra".to_string());
        assert!(auth.validate("extra"));
        assert!(auth.remove_token("extra"));
        assert!(!auth.validate("extra"));
    }

    #[test]
    fn test_extract_token() {
        assert_eq!(AuthManager::extract_token("Bearer secret123"), Some("secret123"));
        assert_eq!(AuthManager::extract_token("bearer secret123"), Some("secret123"));
        assert_eq!(AuthManager::extract_token("secret123"), None);
    }
}
