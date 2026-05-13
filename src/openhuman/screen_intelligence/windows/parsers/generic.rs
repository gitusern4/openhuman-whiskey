//! Generic parser — regex-based extraction of common broker-UI patterns.
//!
//! v1 ships the regex primitives + a `GenericParser` shell that callers
//! can compose. Per-platform parsers (Tradovate, NinjaTrader, …) layer
//! on top with priority dispatch in the engine.

use once_cell::sync::Lazy;
use regex::Regex;

/// Extracts the first `$N.NN` (or `$-N.NN`, `+$N.NN`) dollar amount.
///
/// Examples that match:
///   "$123.45", "+$123.45", "$-50.00", "P/L: $1,234.56"
/// Returns the numeric value as `f64`, or `None` if no match.
pub fn extract_dollar_amount(text: &str) -> Option<f64> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[+\-]?\$\s*-?([\d,]+(?:\.\d+)?)").unwrap());
    let caps = RE.captures(text)?;
    let raw = caps.get(1)?.as_str().replace(',', "");
    raw.parse::<f64>().ok().map(|v| {
        if text.contains("$-") || text.contains("-$") {
            -v
        } else {
            v
        }
    })
}

/// Specialised PnL extractor — same regex as dollar-amount but biased
/// toward picking up the value adjacent to a "P/L", "PnL", "Profit",
/// "Loss" label.
pub fn extract_pnl_amount(text: &str) -> Option<f64> {
    static LABEL_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)(?:p\s*[/&]?\s*l|pnl|profit|loss)\s*[:=]?\s*([+\-]?\$?[\d,]+(?:\.\d+)?)")
            .unwrap()
    });
    if let Some(caps) = LABEL_RE.captures(text) {
        let raw = caps.get(1)?.as_str().replace([',', '$', '+'], "");
        return raw.parse::<f64>().ok();
    }
    extract_dollar_amount(text)
}

/// Extracts a signed integer position size (e.g. "+5", "-2", "10 contracts").
/// Returns the size as `i32`, or `None` if no match.
pub fn extract_position_size(text: &str) -> Option<i32> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"([+\-]?\d+)").unwrap());
    let caps = RE.captures(text)?;
    caps.get(1)?.as_str().parse::<i32>().ok()
}

/// Extracts the first plausible quote price — a decimal number with at
/// least one digit after the dot. Reuses [`extract_dollar_amount`] when
/// a `$` prefix is present, otherwise picks the first bare decimal.
pub fn extract_quote_price(text: &str) -> Option<f64> {
    if text.contains('$') {
        return extract_dollar_amount(text);
    }
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+\.\d+)").unwrap());
    let caps = RE.captures(text)?;
    caps.get(1)?.as_str().parse::<f64>().ok()
}

/// Extracts plausible ticker symbols — uppercase ASCII runs of 1–5
/// chars that don't look like numbers. Filters out a small set of
/// common non-ticker words.
pub fn extract_symbols(text: &str) -> Vec<String> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b([A-Z]{1,5})\b").unwrap());
    const STOP: &[&str] = &[
        "PNL", "P", "L", "USD", "EUR", "GBP", "JPY", "USDT", "USDC", "BUY", "SELL", "LONG",
        "SHORT", "BID", "ASK", "LAST", "OPEN", "HIGH", "LOW", "CLOSE", "VOL",
    ];
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for cap in RE.captures_iter(text) {
        if let Some(m) = cap.get(1) {
            let s = m.as_str();
            if STOP.contains(&s) {
                continue;
            }
            if seen.insert(s.to_string()) {
                out.push(s.to_string());
            }
        }
    }
    out
}

/// Stateless generic parser. v1 shell: callers compose the
/// per-field extractors directly. The struct exists so the engine
/// can swap in a platform-specific parser via `Box<dyn Parser>` later.
#[derive(Debug, Default, Clone, Copy)]
pub struct GenericParser;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dollar_amount_basic() {
        assert_eq!(extract_dollar_amount("$123.45"), Some(123.45));
        assert_eq!(extract_dollar_amount("P/L: $1,234.56"), Some(1234.56));
    }

    #[test]
    fn dollar_amount_negative() {
        let val = extract_dollar_amount("$-50.00");
        assert!(val.is_some());
        assert!(val.unwrap() < 0.0);
    }

    #[test]
    fn dollar_amount_none_when_no_match() {
        assert_eq!(extract_dollar_amount("hello world"), None);
    }

    #[test]
    fn pnl_picks_labeled_value_first() {
        assert_eq!(
            extract_pnl_amount("Account $50,000.00 | P/L: $123.45"),
            Some(123.45)
        );
    }

    #[test]
    fn position_size_signed() {
        assert_eq!(extract_position_size("+5"), Some(5));
        assert_eq!(extract_position_size("-2 contracts"), Some(-2));
        assert_eq!(extract_position_size("10"), Some(10));
    }

    #[test]
    fn quote_price_bare_decimal() {
        assert_eq!(extract_quote_price("21000.25"), Some(21000.25));
    }

    #[test]
    fn quote_price_dollar_prefix_uses_dollar_path() {
        assert_eq!(extract_quote_price("$2,100.50"), Some(2100.50));
    }

    #[test]
    fn symbols_extracted_and_deduped() {
        let s = extract_symbols("MNQ + ES, MNQ again, BUY");
        assert!(s.contains(&"MNQ".to_string()));
        assert!(s.contains(&"ES".to_string()));
        // "BUY" is in STOP list.
        assert!(!s.contains(&"BUY".to_string()));
        // Deduped — "MNQ" appears once.
        assert_eq!(s.iter().filter(|x| x.as_str() == "MNQ").count(), 1);
    }
}
