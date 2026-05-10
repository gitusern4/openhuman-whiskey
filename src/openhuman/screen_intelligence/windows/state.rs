//! Snapshot trading-state derived from successive OCR passes.
//!
//! The engine holds a single `Arc<RwLock<TradingState>>` that the overlay UI
//! reads cheaply (no pipeline blocking). Each parser pass produces a
//! candidate `TradingState`; we diff against the previous snapshot to emit
//! `TradingEvent`s on the broadcast channel.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::TradingEvent;

/// One open position parsed from the broker UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub side: String,
    pub size: f64,
    pub entry: f64,
    pub last: Option<f64>,
    pub unrealized_pnl: Option<f64>,
}

/// One working order parsed from the broker UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkingOrder {
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub size: f64,
    pub price: Option<f64>,
}

/// Last-seen quote per symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub last: Option<f64>,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
}

/// Aggregate snapshot of what we believe the trading platform is showing
/// right now. Updated atomically (whole struct replaced) by the engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TradingState {
    pub positions: Vec<Position>,
    pub working_orders: Vec<WorkingOrder>,
    /// Sum of unrealized P/L across positions, when the platform shows it.
    pub session_unrealized_pnl: Option<f64>,
    /// Realized P/L for the session, when the platform shows it.
    pub session_realized_pnl: Option<f64>,
    /// Per-symbol last quotes.
    pub quotes: HashMap<String, Quote>,
    /// Wall-clock millis when this snapshot was taken.
    pub at_ms: i64,
}

impl TradingState {
    /// Diff `prev` -> `self` and produce events.
    ///
    /// Rules:
    /// - A symbol present in `self.positions` but not in `prev.positions` →
    ///   `PositionOpened`.
    /// - A symbol present in `prev.positions` but not in `self.positions` →
    ///   `PositionClosed`.
    /// - A change in `session_unrealized_pnl` or `session_realized_pnl` →
    ///   one `PnLUpdated` event.
    /// - Any quote whose `last` changed → `QuoteUpdated`.
    pub fn diff_events(&self, prev: &TradingState) -> Vec<TradingEvent> {
        let mut out = Vec::new();
        let prev_syms: std::collections::HashSet<&str> =
            prev.positions.iter().map(|p| p.symbol.as_str()).collect();
        let self_syms: std::collections::HashSet<&str> =
            self.positions.iter().map(|p| p.symbol.as_str()).collect();

        for p in &self.positions {
            if !prev_syms.contains(p.symbol.as_str()) {
                out.push(TradingEvent::PositionOpened {
                    symbol: p.symbol.clone(),
                    side: p.side.clone(),
                    size: p.size,
                    entry: p.entry,
                    at_ms: self.at_ms,
                });
            }
        }
        for p in &prev.positions {
            if !self_syms.contains(p.symbol.as_str()) {
                out.push(TradingEvent::PositionClosed {
                    symbol: p.symbol.clone(),
                    size: p.size,
                    exit: p.last.unwrap_or(p.entry),
                    realized_pnl: p.unrealized_pnl,
                    at_ms: self.at_ms,
                });
            }
        }

        if self.session_unrealized_pnl != prev.session_unrealized_pnl
            || self.session_realized_pnl != prev.session_realized_pnl
        {
            out.push(TradingEvent::PnLUpdated {
                symbol: None,
                unrealized_pnl: self.session_unrealized_pnl,
                realized_pnl_session: self.session_realized_pnl,
                at_ms: self.at_ms,
            });
        }

        for (sym, q) in &self.quotes {
            let prev_last = prev.quotes.get(sym).and_then(|p| p.last);
            if q.last != prev_last {
                out.push(TradingEvent::QuoteUpdated {
                    symbol: sym.clone(),
                    last: q.last,
                    bid: q.bid,
                    ask: q.ask,
                    at_ms: self.at_ms,
                });
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(symbol: &str, side: &str, size: f64, entry: f64) -> Position {
        Position {
            symbol: symbol.into(),
            side: side.into(),
            size,
            entry,
            last: None,
            unrealized_pnl: None,
        }
    }

    #[test]
    fn diff_emits_position_opened() {
        let prev = TradingState::default();
        let mut next = TradingState::default();
        next.positions.push(pos("ES", "long", 1.0, 5300.0));
        next.at_ms = 1000;
        let events = next.diff_events(&prev);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], TradingEvent::PositionOpened { .. }));
    }

    #[test]
    fn diff_emits_position_closed() {
        let mut prev = TradingState::default();
        prev.positions.push(pos("ES", "long", 1.0, 5300.0));
        let next = TradingState::default();
        let events = next.diff_events(&prev);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], TradingEvent::PositionClosed { .. }));
    }

    #[test]
    fn diff_emits_pnl_updated_only_on_change() {
        let mut prev = TradingState::default();
        prev.session_unrealized_pnl = Some(10.0);
        let mut next = prev.clone();
        let events = next.diff_events(&prev);
        assert!(events.is_empty());
        next.session_unrealized_pnl = Some(20.0);
        let events = next.diff_events(&prev);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], TradingEvent::PnLUpdated { .. }));
    }

    #[test]
    fn diff_emits_quote_updated_on_last_change() {
        let mut prev = TradingState::default();
        prev.quotes.insert(
            "ES".into(),
            Quote {
                last: Some(5300.0),
                bid: None,
                ask: None,
            },
        );
        let mut next = prev.clone();
        next.quotes.insert(
            "ES".into(),
            Quote {
                last: Some(5301.25),
                bid: None,
                ask: None,
            },
        );
        let events = next.diff_events(&prev);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], TradingEvent::QuoteUpdated { .. }));
    }
}
