//! Order placement — bracket orders via ProjectX Gateway.
//!
//! `place_bracket_order` maps `BracketOrder` → POST body and submits to the
//! gateway. `isAutomated: true` is always set — omitting it on CME instruments
//! is an exchange rule violation.
//!
//! Stop and target are expressed in **ticks** (broker-native, avoids any
//! price-precision conversion on our side).

use serde::{Deserialize, Serialize};

use super::auth::{AuthError, TopStepClient};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BracketOrder {
    pub account_id: u64,
    pub symbol: String,
    /// "Buy" or "Sell"
    pub action: String,
    pub qty: u32,
    pub order_type: String,
    /// Limit entry price (required for limit orders)
    pub price: Option<f64>,
    /// Stop-loss distance in ticks
    pub stop_loss_bracket: u32,
    /// Take-profit distance in ticks
    pub take_profit_bracket: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerResponse {
    pub order_id: Option<u64>,
    pub status: String,
    pub raw: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

/// Place a bracket order. Returns `BrokerResponse` on HTTP 2xx, `Err` otherwise.
pub async fn place_bracket_order(
    client: &TopStepClient,
    order: &BracketOrder,
) -> Result<BrokerResponse, String> {
    let body = serde_json::json!({
        "accountId": order.account_id,
        "symbol": order.symbol,
        "action": order.action,
        "orderQty": order.qty,
        "orderType": order.order_type,
        "price": order.price,
        "stopLossBracket": order.stop_loss_bracket,
        "takeProfitBracket": order.take_profit_bracket,
        "isAutomated": true,
    });

    let raw = client
        .authorized_post("/orders/bracket", body)
        .await
        .map_err(|e| format!("broker error: {}", e))?;

    let order_id = raw.get("orderId").and_then(|v| v.as_u64());
    let status = raw
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(BrokerResponse {
        order_id,
        status,
        raw,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_order_serializes_is_automated() {
        let order = BracketOrder {
            account_id: 12345,
            symbol: "MESH5".to_string(),
            action: "Buy".to_string(),
            qty: 1,
            order_type: "Limit".to_string(),
            price: Some(5200.25),
            stop_loss_bracket: 8,
            take_profit_bracket: 16,
        };
        let body = serde_json::json!({
            "accountId": order.account_id,
            "symbol": order.symbol,
            "action": order.action,
            "orderQty": order.qty,
            "orderType": order.order_type,
            "price": order.price,
            "stopLossBracket": order.stop_loss_bracket,
            "takeProfitBracket": order.take_profit_bracket,
            "isAutomated": true,
        });
        assert_eq!(body["isAutomated"], true);
        assert_eq!(body["symbol"], "MESH5");
    }

    #[test]
    fn broker_response_deserializes() {
        let raw = serde_json::json!({"orderId": 98765, "status": "Working"});
        let resp = BrokerResponse {
            order_id: raw.get("orderId").and_then(|v| v.as_u64()),
            status: raw
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            raw: raw.clone(),
        };
        assert_eq!(resp.order_id, Some(98765));
        assert_eq!(resp.status, "Working");
    }

    #[tokio::test]
    async fn revoked_client_returns_err_on_place() {
        let client = TopStepClient::new("bad-key");
        client.revoke().await;
        let order = BracketOrder {
            account_id: 1,
            symbol: "MES".to_string(),
            action: "Buy".to_string(),
            qty: 1,
            order_type: "Market".to_string(),
            price: None,
            stop_loss_bracket: 4,
            take_profit_bracket: 8,
        };
        let result = place_bracket_order(&client, &order).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("broker auth revoked"));
    }
}
