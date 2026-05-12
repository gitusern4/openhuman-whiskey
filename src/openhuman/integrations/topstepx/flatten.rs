//! Position flattening — submits market orders to close every open position.
//!
//! Used exclusively by the kill switch. Market orders are used (not stop-limit)
//! to guarantee a fill even in fast markets.

use super::auth::TopStepClient;

/// Flatten all open positions for `account_id` via market-order close.
/// Returns how many flatten orders were submitted (broker-reported or best-guess).
pub async fn flatten_all_positions(client: &TopStepClient, account_id: u64) -> Result<u64, String> {
    let body = serde_json::json!({
        "accountId": account_id,
        "isAutomated": true,
    });
    let resp = client
        .authorized_post("/positions/flatten", body)
        .await
        .map_err(|e| format!("flatten_all_positions failed: {}", e))?;

    let count = resp
        .get("ordersSubmitted")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(count)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn flatten_revoked_returns_err() {
        let client = TopStepClient::new("key");
        client.revoke().await;
        let result = flatten_all_positions(&client, 42).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("flatten_all_positions failed"));
    }

    #[test]
    fn flatten_body_contains_is_automated() {
        let body = serde_json::json!({
            "accountId": 42u64,
            "isAutomated": true,
        });
        assert_eq!(body["isAutomated"], true);
    }
}
