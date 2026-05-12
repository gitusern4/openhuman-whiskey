//! Position size calculator — pure math, no side-effects.
//!
//! `size_position` takes entry price, stop price, risk-per-trade in
//! dollars, and a `ContractSpec` and returns the maximum whole-contract
//! count that keeps the actual dollar risk at or below the requested
//! amount (floors to zero — never rounds up and never overruns risk).
//!
//! Contract specs are baked in as `const` slices for common futures
//! instruments. Unknown instruments fall back to `GENERIC_STOCK`.
//!
//! # Example
//! ```
//! use openhuman_core::openhuman::modes::position_sizer::{size_position, SPECS};
//! let mnq = SPECS.iter().find(|s| s.name == "MNQ").unwrap();
//! let result = size_position(19800.0, 19750.0, 100.0, mnq);
//! // 50-point stop × 4 ticks/pt × $0.50/tick = $100/contract → 1 contract
//! assert_eq!(result.contracts, 1);
//! ```

use serde::{Deserialize, Serialize};

/// One contract specification (exchange-defined).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContractSpec {
    /// Short symbol used for lookup (e.g. `"MNQ"`).
    pub name: &'static str,
    /// Smallest price increment in points (e.g. 0.25 for MNQ).
    pub tick_size: f64,
    /// Dollar value of one tick movement (e.g. 0.50 for MNQ).
    pub dollars_per_tick: f64,
}

impl ContractSpec {
    /// Dollar value gained or lost per contract per full-point move.
    pub fn dollars_per_point(&self) -> f64 {
        (1.0 / self.tick_size) * self.dollars_per_tick
    }
}

/// All baked-in instrument specs.
pub const SPECS: &[ContractSpec] = &[
    ContractSpec {
        name: "MNQ",
        tick_size: 0.25,
        dollars_per_tick: 0.50,
    },
    ContractSpec {
        name: "MES",
        tick_size: 0.25,
        dollars_per_tick: 1.25,
    },
    ContractSpec {
        name: "NQ",
        tick_size: 0.25,
        dollars_per_tick: 5.00,
    },
    ContractSpec {
        name: "ES",
        tick_size: 0.25,
        dollars_per_tick: 12.50,
    },
    ContractSpec {
        name: "MYM",
        tick_size: 1.0,
        dollars_per_tick: 0.50,
    },
    ContractSpec {
        name: "M2K",
        tick_size: 0.1,
        dollars_per_tick: 0.50,
    },
    ContractSpec {
        name: "CL",
        tick_size: 0.01,
        dollars_per_tick: 10.00,
    },
    ContractSpec {
        name: "GC",
        tick_size: 0.1,
        dollars_per_tick: 10.00,
    },
    ContractSpec {
        name: "STOCK",
        tick_size: 0.01,
        dollars_per_tick: 1.00,
    },
];

/// Sizing result returned to callers and serialised to the frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SizingResult {
    /// Whole-contract count that keeps risk at or below `risk_dollars`.
    /// Zero when sizing cannot be computed (see `error`).
    pub contracts: u32,
    /// Actual dollar risk after flooring to whole contracts.
    pub actual_risk_dollars: f64,
    /// Dollar risk per single contract.
    pub risk_per_contract: f64,
    /// Human-readable error reason; `None` on success.
    pub error: Option<String>,
}

/// Calculate position size.
///
/// Returns 0 contracts + an `error` string if the inputs make sizing
/// impossible (stop == entry, risk <= 0, negative values). Never panics.
pub fn size_position(
    entry: f64,
    stop: f64,
    risk_dollars: f64,
    spec: &ContractSpec,
) -> SizingResult {
    // Guard: stop-equals-entry produces division-by-zero.
    let price_diff = (entry - stop).abs();
    if price_diff == 0.0 {
        return SizingResult {
            contracts: 0,
            actual_risk_dollars: 0.0,
            risk_per_contract: 0.0,
            error: Some(
                "Stop price equals entry price — no risk defined, cannot size position."
                    .to_string(),
            ),
        };
    }

    // Guard: risk must be positive.
    if risk_dollars <= 0.0 {
        return SizingResult {
            contracts: 0,
            actual_risk_dollars: 0.0,
            risk_per_contract: 0.0,
            error: Some(format!(
                "Risk dollars must be positive (got {risk_dollars:.2})."
            )),
        };
    }

    // Dollar risk per contract = (price distance in ticks) × dollars_per_tick.
    // price distance in ticks = price_diff / tick_size.
    let ticks = price_diff / spec.tick_size;
    let risk_per_contract = ticks * spec.dollars_per_tick;

    if risk_per_contract <= 0.0 {
        return SizingResult {
            contracts: 0,
            actual_risk_dollars: 0.0,
            risk_per_contract: 0.0,
            error: Some("Risk per contract resolved to zero — check the spec.".to_string()),
        };
    }

    // Floor to whole contracts — never round up and never overrun risk.
    let contracts = (risk_dollars / risk_per_contract).floor() as u32;
    let actual_risk = f64::from(contracts) * risk_per_contract;

    SizingResult {
        contracts,
        actual_risk_dollars: (actual_risk * 100.0).round() / 100.0,
        risk_per_contract: (risk_per_contract * 100.0).round() / 100.0,
        error: None,
    }
}

/// Look up a spec by name (case-insensitive). Falls back to STOCK if
/// unknown.
pub fn spec_by_id(id: &str) -> &'static ContractSpec {
    let upper = id.to_uppercase();
    SPECS
        .iter()
        .find(|s| s.name == upper.as_str())
        .unwrap_or_else(|| SPECS.iter().find(|s| s.name == "STOCK").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mnq() -> &'static ContractSpec {
        spec_by_id("MNQ")
    }

    fn es() -> &'static ContractSpec {
        spec_by_id("ES")
    }

    #[test]
    fn long_trade_mnq_one_contract() {
        // Entry 19800, stop 19750, diff = 50 pts
        // Ticks = 50 / 0.25 = 200, risk/contract = 200 * 0.50 = $100
        // Risk budget $100 → 1 contract
        let r = size_position(19800.0, 19750.0, 100.0, mnq());
        assert_eq!(r.contracts, 1);
        assert!((r.risk_per_contract - 100.0).abs() < 0.01);
        assert!(r.error.is_none());
    }

    #[test]
    fn short_trade_mnq_two_contracts() {
        // Short: entry 19800, stop 19850 (stop above entry for short).
        // diff = 50 pts → $100/contract. Budget $200 → 2 contracts.
        let r = size_position(19800.0, 19850.0, 200.0, mnq());
        assert_eq!(r.contracts, 2);
        assert!((r.actual_risk_dollars - 200.0).abs() < 0.01);
        assert!(r.error.is_none());
    }

    #[test]
    fn zero_risk_budget_returns_zero_contracts() {
        let r = size_position(19800.0, 19750.0, 0.0, mnq());
        assert_eq!(r.contracts, 0);
        assert!(r.error.is_some());
    }

    #[test]
    fn negative_risk_budget_returns_zero_contracts() {
        let r = size_position(19800.0, 19750.0, -50.0, mnq());
        assert_eq!(r.contracts, 0);
        assert!(r.error.is_some());
    }

    #[test]
    fn stop_equals_entry_does_not_panic() {
        let r = size_position(19800.0, 19800.0, 500.0, mnq());
        assert_eq!(r.contracts, 0);
        assert!(r.error.is_some());
        assert!(r
            .error
            .as_deref()
            .unwrap()
            .contains("Stop price equals entry price"));
    }

    #[test]
    fn fractional_contract_floors_down() {
        // Budget $150 / $100 per contract = 1.5 → should floor to 1.
        let r = size_position(19800.0, 19750.0, 150.0, mnq());
        assert_eq!(r.contracts, 1);
        assert!((r.actual_risk_dollars - 100.0).abs() < 0.01);
    }

    #[test]
    fn es_trade_correct_dollars_per_point() {
        // ES: tick 0.25, $12.50/tick → $50/point.
        // Entry 4500, stop 4490, diff = 10pts → $500/contract.
        // Budget $1000 → 2 contracts.
        let r = size_position(4500.0, 4490.0, 1000.0, es());
        assert_eq!(r.contracts, 2);
        assert!((r.risk_per_contract - 500.0).abs() < 0.01);
        assert!(r.error.is_none());
    }

    #[test]
    fn spec_by_id_fallback_to_stock() {
        let s = spec_by_id("UNKNOWN_INSTRUMENT_XYZ");
        assert_eq!(s.name, "STOCK");
    }

    #[test]
    fn spec_by_id_case_insensitive() {
        let s = spec_by_id("mnq");
        assert_eq!(s.name, "MNQ");
    }
}
