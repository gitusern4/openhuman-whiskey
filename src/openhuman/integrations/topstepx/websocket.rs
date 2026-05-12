//! TopStepX WebSocket client — fills + position streaming.
//!
//! Connects to the TopStepX WS endpoint, subscribes to fills and position
//! updates, and emits Tauri events on each message so the React frontend can
//! update in real time.
//!
//! The connection runs in a background `tokio::task`. A `ShutdownToken` allows
//! the kill switch to terminate the WS loop cleanly.

use futures_util::{SinkExt, StreamExt};
use tokio::sync::watch;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const WS_URL: &str = "wss://gateway.topstepx.com/ws/v1";

/// Events emitted on the Tauri event bus.
pub const EVENT_FILL: &str = "topstepx://fill";
pub const EVENT_POSITION: &str = "topstepx://position";

pub type ShutdownToken = watch::Sender<bool>;
pub type ShutdownReceiver = watch::Receiver<bool>;

/// Spawn the WebSocket listener in the background.
/// Returns a `ShutdownToken` — call `send(true)` to terminate the loop.
/// Tauri event emission is modelled as a callback to avoid a hard Tauri dep
/// in this module (easier to test).
pub async fn spawn_ws_listener<F>(
    api_key: String,
    account_id: u64,
    event_cb: F,
) -> Result<ShutdownToken, String>
where
    F: Fn(&str, serde_json::Value) + Send + 'static,
{
    let (tx, mut rx) = watch::channel(false);
    tokio::spawn(async move {
        let url = format!("{}?accountId={}", WS_URL, account_id);
        let ws_stream = match connect_async(&url).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                log::error!("topstepx WS connect failed: {}", e);
                return;
            }
        };
        let (mut write, mut read) = ws_stream.split();

        // Auth handshake
        let auth_msg = serde_json::json!({ "type": "auth", "token": api_key });
        if let Err(e) = write.send(Message::Text(auth_msg.to_string().into())).await {
            log::error!("topstepx WS auth send failed: {}", e);
            return;
        }

        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(txt))) => {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                                let msg_type = v
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                match msg_type.as_str() {
                                    "fill" => event_cb(EVENT_FILL, v),
                                    "position" => event_cb(EVENT_POSITION, v),
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(Message::Ping(p))) => {
                            let _ = write.send(Message::Pong(p)).await;
                        }
                        Some(Err(e)) => {
                            log::error!("topstepx WS error: {}", e);
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                _ = rx.changed() => {
                    if *rx.borrow() {
                        let _ = write.send(Message::Close(None)).await;
                        break;
                    }
                }
            }
        }
    });
    Ok(tx)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_name_constants_are_unique() {
        assert_ne!(EVENT_FILL, EVENT_POSITION);
    }

    #[test]
    fn shutdown_token_can_signal() {
        let (tx, mut rx) = watch::channel(false);
        tx.send(true).unwrap();
        assert!(*rx.borrow_and_update());
    }

    #[test]
    fn ws_url_includes_account_id() {
        let account_id = 99999u64;
        let url = format!("{}?accountId={}", WS_URL, account_id);
        assert!(url.contains("99999"));
    }
}
