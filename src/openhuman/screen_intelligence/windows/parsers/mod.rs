//! Parsers that turn raw OCR text strings into [`TradingState`] fragments.
//!
//! Broker UIs vary enough that we eventually want one parser per platform
//! (Tradovate, NinjaTrader, ToS, IBKR, …). For now `generic` covers the
//! common dollar-amount + signed-quantity patterns; per-platform parsers
//! can layer on top with priority dispatch in the engine.

pub mod generic;

pub use generic::{
    extract_dollar_amount, extract_pnl_amount, extract_position_size, extract_quote_price,
    extract_symbols, GenericParser,
};
