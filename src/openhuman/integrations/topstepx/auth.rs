//! Bearer API-key auth for TopStepX / ProjectX Gateway.
//!
//! `TopStepClient::new(api_key)` stores the token and re-presents it on
//! every request. If a request returns HTTP 401 the client flags the token
//! as revoked — callers should treat `Err(AuthRevoked)` as a signal to
//! trigger the kill switch.

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::RwLock;

const BASE_URL: &str = "https://gateway.topstepx.com/api/v1";

#[derive(Debug, Clone, PartialEq)]
pub enum AuthError {
    Revoked,
    Network(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Revoked => write!(f, "broker auth revoked (401)"),
            AuthError::Network(e) => write!(f, "network error: {}", e),
        }
    }
}

/// Shared state inside `TopStepClient`.
struct Inner {
    api_key: String,
    revoked: bool,
}

/// HTTP client wrapper for TopStepX. Thread-safe via `Arc<RwLock<Inner>>`.
#[derive(Clone)]
pub struct TopStepClient {
    http: Client,
    inner: Arc<RwLock<Inner>>,
}

impl TopStepClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            inner: Arc::new(RwLock::new(Inner {
                api_key: api_key.into(),
                revoked: false,
            })),
        }
    }

    /// Revoke the stored token. All subsequent `authorized_post` calls will
    /// return `Err(AuthError::Revoked)` without touching the network.
    pub async fn revoke(&self) {
        let mut inner = self.inner.write().await;
        inner.revoked = true;
    }

    /// Execute an authenticated POST to `path` (relative to `BASE_URL`).
    /// Refreshes on 401 by marking the token revoked and returning `Err`.
    pub async fn authorized_post(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, AuthError> {
        let (api_key, revoked) = {
            let inner = self.inner.read().await;
            (inner.api_key.clone(), inner.revoked)
        };
        if revoked {
            return Err(AuthError::Revoked);
        }
        let url = format!("{}{}", BASE_URL, path);
        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", api_key))
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AuthError::Network(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.revoke().await;
            return Err(AuthError::Revoked);
        }
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| AuthError::Network(e.to_string()))
    }

    /// Execute an authenticated DELETE (for cancel).
    pub async fn authorized_delete(&self, path: &str) -> Result<serde_json::Value, AuthError> {
        let (api_key, revoked) = {
            let inner = self.inner.read().await;
            (inner.api_key.clone(), inner.revoked)
        };
        if revoked {
            return Err(AuthError::Revoked);
        }
        let url = format!("{}{}", BASE_URL, path);
        let resp = self
            .http
            .delete(&url)
            .header(AUTHORIZATION, format!("Bearer {}", api_key))
            .send()
            .await
            .map_err(|e| AuthError::Network(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.revoke().await;
            return Err(AuthError::Revoked);
        }
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| AuthError::Network(e.to_string()))
    }

    pub fn base_url() -> &'static str {
        BASE_URL
    }
}

// ---------------------------------------------------------------------------
// Tests (mocked — no live broker calls)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_client_is_not_revoked() {
        let client = TopStepClient::new("test-key");
        let inner = client.inner.read().await;
        assert!(!inner.revoked);
        assert_eq!(inner.api_key, "test-key");
    }

    #[tokio::test]
    async fn revoke_sets_flag() {
        let client = TopStepClient::new("test-key");
        client.revoke().await;
        let inner = client.inner.read().await;
        assert!(inner.revoked);
    }

    #[tokio::test]
    async fn revoked_client_returns_auth_error_without_network() {
        let client = TopStepClient::new("test-key");
        client.revoke().await;
        let result = client
            .authorized_post("/orders", serde_json::json!({}))
            .await;
        assert_eq!(result.unwrap_err(), AuthError::Revoked);
    }

    #[test]
    fn auth_error_display() {
        assert_eq!(AuthError::Revoked.to_string(), "broker auth revoked (401)");
        assert!(AuthError::Network("timeout".into())
            .to_string()
            .contains("timeout"));
    }
}
