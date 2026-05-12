//! Order cancellation — single order and account-wide cancel-all.

use super::auth::TopStepClient;

/// Cancel a single order by ID.
pub async fn cancel_order(client: &TopStepClient, order_id: u64) -> Result<(), String> {
    client
        .authorized_delete(&format!("/orders/{}", order_id))
        .await
        .map_err(|e| format!("cancel_order failed: {}", e))?;
    Ok(())
}

/// Cancel all open orders for a given account. Returns the number of orders
/// cancelled (best-effort from broker response; `None` if broker doesn't report).
pub async fn cancel_all_for_account(
    client: &TopStepClient,
    account_id: u64,
) -> Result<Option<u64>, String> {
    let body = serde_json::json!({ "accountId": account_id });
    let resp = client
        .authorized_post("/orders/cancelAll", body)
        .await
        .map_err(|e| format!("cancel_all_for_account failed: {}", e))?;
    let cancelled = resp.get("cancelledCount").and_then(|v| v.as_u64());
    Ok(cancelled)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_order_revoked_returns_err() {
        let client = TopStepClient::new("key");
        client.revoke().await;
        let result = cancel_order(&client, 999).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cancel_all_revoked_returns_err() {
        let client = TopStepClient::new("key");
        client.revoke().await;
        let result = cancel_all_for_account(&client, 42).await;
        assert!(result.is_err());
    }
}
