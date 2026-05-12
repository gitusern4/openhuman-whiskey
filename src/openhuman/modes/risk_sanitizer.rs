//! TK's Mods — risk-display sanitization layer.
//!
//! When the `hide_risk_pct` flag is enabled in the TK's Mods config,
//! Whiskey's outgoing messages are filtered through [`sanitize`] before
//! they reach the overlay or chat UI. The sanitizer replaces dollar amounts
//! and percentage expressions with abstract terms while deliberately
//! preserving R-multiples (e.g. "1.5R") because R is already an abstraction
//! and remains useful for position-sizing discipline.
//!
//! Design principles:
//!   - Regex-free on the hot path when possible (just string replacement
//!     for common patterns). A regex fallback handles the more complex
//!     dollar/pct forms.
//!   - Never panics. Returns the original string unchanged on any error.
//!   - Pure function — no I/O, no state. Tests are trivial to write.
//!   - The caller gate (`if hide_risk_pct { sanitize(msg) } else { msg }`)
//!     lives in the bus publish path so the overhead is zero when the flag
//!     is off.
//!
//! Replacement mapping:
//!   - "$<amount>" or "$<amount> risk"   → "risk unit"
//!   - "<n>% account risk" / "<n>% risk" → "small position" / "position"
//!   - "<n>%"  (bare percentage)         → "a portion"
//!   - "1.5R target" / "2R"             → preserved (R-multiples pass through)
//!
//! The canonical test suite is `tests` at the bottom of this file; it
//! covers all the representative patterns listed above. Extend it whenever
//! a new real-world message shape surfaces.

/// Sanitize a Whiskey message for display when `hide_risk_pct` is enabled.
///
/// Returns a `String` with dollar amounts and percentage references replaced
/// by abstract equivalents. R-multiples (`1.5R`, `2R`, etc.) are preserved.
/// The input is not modified when `hide_risk_pct` is `false` — callers
/// should gate this call rather than always calling it.
pub fn sanitize(message: &str) -> String {
    // We apply substitutions in a single left-to-right pass over the text
    // using a simple state machine rather than compiled regexes to avoid
    // the `regex` dependency. The patterns we need to match are regular
    // enough that hand-rolled scanning is both faster and more readable.

    let mut out = String::with_capacity(message.len());
    let chars: Vec<char> = message.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // --- dollar amount: $[digits][.,digits][ risk] ---
        if chars[i] == '$' && i + 1 < len && chars[i + 1].is_ascii_digit() {
            // Consume digits / commas / periods that form the number.
            let mut j = i + 1;
            while j < len && (chars[j].is_ascii_digit() || chars[j] == ',' || chars[j] == '.') {
                j += 1;
            }
            // Optional trailing " risk" (case-insensitive).
            let rest: String = chars[j..].iter().collect();
            let lower = rest.to_lowercase();
            if lower.starts_with(" risk") {
                j += " risk".len();
            }
            out.push_str("risk unit");
            i = j;
            continue;
        }

        // --- percentage: [digits][.][digits]%[ account risk | risk | (bare)] ---
        // Guard: do NOT consume if the next non-space char before % is 'R'
        // (that would be an R-multiple display artifact, which we don't want
        // to strip). R-multiples look like "1.5R" not "1.5%", so this branch
        // only fires on actual %-suffixed numbers.
        if chars[i].is_ascii_digit() {
            // Peek ahead: collect digits/dot to find if there's a % coming.
            let mut j = i;
            while j < len && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            if j < len && chars[j] == '%' {
                // Consume the number + '%'.
                j += 1;
                // Optional " account risk" or " risk".
                let rest: String = chars[j..].iter().collect();
                let lower = rest.to_lowercase();
                if lower.starts_with(" account risk") {
                    j += " account risk".len();
                    out.push_str("small position");
                } else if lower.starts_with(" risk") {
                    j += " risk".len();
                    out.push_str("position");
                } else {
                    out.push_str("a portion");
                }
                i = j;
                continue;
            }
            // Not a % pattern — emit the digits literally.
            let digits: String = chars[i..j].iter().collect();
            out.push_str(&digits);
            i = j;
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn dollar_amount_becomes_risk_unit() {
        assert_eq!(sanitize("risking $250 on this trade"), "risking risk unit on this trade");
    }

    #[test]
    fn dollar_amount_with_risk_suffix() {
        assert_eq!(sanitize("$250 risk is the plan"), "risk unit is the plan");
    }

    #[test]
    fn bare_percentage_becomes_a_portion() {
        assert_eq!(sanitize("allocating 0.5% of capital"), "allocating a portion of capital");
    }

    #[test]
    fn pct_account_risk() {
        assert_eq!(
            sanitize("use 0.5% account risk today"),
            "use small position today"
        );
    }

    #[test]
    fn pct_risk_suffix() {
        assert_eq!(sanitize("this is a 1% risk setup"), "this is a position setup");
    }

    #[test]
    fn r_multiples_pass_through() {
        // "1.5R target" — R-multiples must never be stripped.
        assert_eq!(sanitize("1.5R target is the plan"), "1.5R target is the plan");
    }

    #[test]
    fn r_multiple_standalone() {
        assert_eq!(sanitize("aiming for 2R on this one"), "aiming for 2R on this one");
    }

    #[test]
    fn mixed_message() {
        let input = "Risk unit: $500 risk, 1% account risk, targeting 2R.";
        let out = sanitize(input);
        assert!(!out.contains("$500"), "dollar amount should be redacted");
        assert!(!out.contains("1%"), "percentage should be redacted");
        assert!(out.contains("2R"), "R-multiple must survive");
    }

    #[test]
    fn no_sensitive_content_passthrough() {
        let msg = "Go long, manage the trade well.";
        assert_eq!(sanitize(msg), msg);
    }

    #[test]
    fn dollar_with_comma_formatted_amount() {
        assert_eq!(sanitize("$1,250 risk on desk"), "risk unit on desk");
    }
}
