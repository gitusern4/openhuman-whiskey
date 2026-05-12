//! Broadcast bus for overlay attention events.
//!
//! Mirrors the pattern used by `voice::dictation_listener`: a single
//! `tokio::sync::broadcast` channel wrapped in a `Lazy` static so any
//! module in the core can publish without threading a sender around.
//! The Socket.IO bridge in `core::socketio::spawn_web_channel_bridge`
//! subscribes here and forwards every event to the overlay window as
//! an `overlay:attention` Socket.IO message.

use once_cell::sync::Lazy;
use tokio::sync::broadcast;

use super::types::OverlayAttentionEvent;

const LOG_PREFIX: &str = "[overlay]";

static ATTENTION_BUS: Lazy<broadcast::Sender<OverlayAttentionEvent>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(64);
    tx
});

/// Subscribe to overlay attention events. Used by the Socket.IO bridge.
pub fn subscribe_attention_events() -> broadcast::Receiver<OverlayAttentionEvent> {
    ATTENTION_BUS.subscribe()
}

/// Publish an attention event toward the overlay window.
///
/// Fire-and-forget: if nobody is currently subscribed (e.g. the bridge
/// hasn't started yet, or the overlay socket is disconnected) the event
/// is dropped. Returns the number of active subscribers that received
/// the event for diagnostics.
///
/// WHISKEY_AUDIT.md H3 wiring: when the caller hasn't set
/// `event.source` explicitly, default it from the active mode's
/// `Mode::overlay_source()`. That makes the trait method (which used
/// to be dead code) actively load-bearing — every publish gets tagged
/// with the active mode's source label so downstream tracing / UI
/// styling can branch on it without the caller having to remember.
///
/// Risk sanitizer integration: when TK's Mods `hide_risk_pct` flag is true,
/// the message body is passed through `risk_sanitizer::sanitize()` before
/// publishing to mask dollar amounts and percentage expressions while
/// preserving R-multiples.
pub fn publish_attention(mut event: OverlayAttentionEvent) -> usize {
    if event.source.is_none() {
        let mode = crate::openhuman::modes::active_mode();
        event.source = Some(mode.overlay_source().to_string());
    }

    // Apply risk sanitizer if enabled in TK's Mods config.
    let tks_config = crate::openhuman::modes::tks_mods_config::load();
    if tks_config.hide_risk_pct {
        event.message = crate::openhuman::modes::risk_sanitizer::sanitize(&event.message);
    }

    log::debug!(
        "{LOG_PREFIX} publish attention source={:?} tone={:?} message_bytes={} ttl_ms={:?}",
        event.source,
        event.tone,
        event.message.len(),
        event.ttl_ms
    );
    match ATTENTION_BUS.send(event) {
        Ok(n) => n,
        Err(_) => {
            log::debug!("{LOG_PREFIX} no overlay subscribers — attention event dropped");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::overlay::types::OverlayAttentionTone;

    #[tokio::test]
    async fn publish_is_received_by_subscriber() {
        let mut rx = subscribe_attention_events();
        let delivered = publish_attention(
            OverlayAttentionEvent::new("hello overlay")
                .with_tone(OverlayAttentionTone::Accent)
                .with_source("test"),
        );
        assert!(delivered >= 1);
        let event = rx.recv().await.expect("event delivered");
        assert_eq!(event.message, "hello overlay");
        assert_eq!(event.tone, OverlayAttentionTone::Accent);
        assert_eq!(event.source.as_deref(), Some("test"));
    }

    #[test]
    fn publish_with_no_subscribers_is_safe() {
        // Drop any existing subscribers by not holding one.
        let _ = publish_attention(OverlayAttentionEvent::new("dropped"));
    }

    /// WHISKEY_AUDIT.md H3 regression guard: publishing without
    /// setting `source` should default it from the active mode.
    /// DefaultMode's `overlay_source` returns its mode id ("default"),
    /// so the delivered event must carry that label.
    #[tokio::test]
    async fn publish_defaults_source_from_active_mode() {
        let _g = crate::openhuman::modes::ActiveModeGuard::new();
        let _ = crate::openhuman::modes::set_active_mode(crate::openhuman::modes::DefaultMode::ID);
        let mut rx = subscribe_attention_events();
        let _ = publish_attention(OverlayAttentionEvent::new("auto-source"));
        let event = rx.recv().await.expect("event delivered");
        assert_eq!(event.source.as_deref(), Some("default"));
    }

    /// Switching to WhiskeyMode flips the auto-source to "whiskey".
    #[tokio::test]
    async fn publish_auto_source_follows_active_mode_switch() {
        let _g = crate::openhuman::modes::ActiveModeGuard::new();
        let _ = crate::openhuman::modes::set_active_mode(crate::openhuman::modes::WhiskeyMode::ID);
        let mut rx = subscribe_attention_events();
        let _ = publish_attention(OverlayAttentionEvent::new("under-whiskey"));
        let event = rx.recv().await.expect("event delivered");
        assert_eq!(event.source.as_deref(), Some("whiskey"));
    }

    /// Caller-set source still wins over the default.
    #[tokio::test]
    async fn publish_explicit_source_wins_over_active_mode_default() {
        let _g = crate::openhuman::modes::ActiveModeGuard::new();
        let _ = crate::openhuman::modes::set_active_mode(crate::openhuman::modes::WhiskeyMode::ID);
        let mut rx = subscribe_attention_events();
        let _ = publish_attention(
            OverlayAttentionEvent::new("explicit").with_source("manual-override"),
        );
        let event = rx.recv().await.expect("event delivered");
        assert_eq!(event.source.as_deref(), Some("manual-override"));
    }

    /// Risk sanitizer integration: when TK's Mods hide_risk_pct flag is
    /// enabled, published messages should have $ and % stripped. When disabled,
    /// messages pass through unchanged.
    #[tokio::test]
    async fn risk_sanitizer_applied_when_hide_risk_pct_enabled() {
        use tempfile::tempdir;

        let tmp = tempdir().expect("tempdir created");
        let config_path = tmp.path().join("tks_mods.toml");

        // Write a TK's Mods config with hide_risk_pct = true.
        let config_content = "theme = \"default\"\nhide_risk_pct = true\n";
        std::fs::write(&config_path, config_content).expect("config written");

        // Override the config path via env var so load() uses our test file.
        let env_var = "OPENHUMAN_TKS_MODS_FILE";
        let old_val = std::env::var(env_var).ok();
        std::env::set_var(env_var, &config_path);

        let mut rx = subscribe_attention_events();
        let message = "entry at $100.00 with 2.5% account risk, 1.5R target";
        let _ = publish_attention(OverlayAttentionEvent::new(message));

        let event = rx.recv().await.expect("event delivered");
        // $ and % should be removed; 1.5R should remain.
        assert!(!event.message.contains('$'));
        assert!(!event.message.contains('%'));
        assert!(event.message.contains("1.5R"));

        // Cleanup.
        if let Some(old) = old_val {
            std::env::set_var(env_var, old);
        } else {
            std::env::remove_var(env_var);
        }
    }
}
