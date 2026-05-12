/**
 * Order-flow types — shared contract between UI and the Rust command layer.
 *
 * Source of truth: the assist agent mirrors these shapes in Rust.
 * Do NOT change field names without coordinating with order_flow.rs.
 *
 * Philosophy: observation + journaling only. No order-placement fields exist.
 */

// ---------------------------------------------------------------------------
// Persisted config (stored to disk by Rust layer)
// ---------------------------------------------------------------------------

/** Which patterns the polling loop should watch for. */
export interface OrderFlowAlertToggles {
  delta_divergence: boolean;
  absorption: boolean;
  single_print_rejection: boolean;
}

/** Full persisted config shape. */
export interface OrderFlowConfig {
  /** Whether the order-flow card is enabled at all. */
  enabled: boolean;
  /** Polling interval in milliseconds — capped at 500 (2 Hz). */
  poll_interval_ms: number;
  /** Active workspace preset name, or null when none is applied. */
  active_preset: string | null;
  /** Per-pattern alert toggles. */
  alert_toggles: OrderFlowAlertToggles;
}

// ---------------------------------------------------------------------------
// Runtime state (read from TV CDP, computed each poll cycle)
// ---------------------------------------------------------------------------

/** Delta / volume data for the current bar, read from TV or entered manually. */
export interface OrderFlowBarState {
  /** Ask volume minus bid volume for the current bar. */
  bar_delta: number | null;
  /** Bid volume for the current bar. */
  bid_volume: number | null;
  /** Ask volume for the current bar. */
  ask_volume: number | null;
  /** Total volume for the current bar. */
  total_volume: number | null;
}

/** Cumulative delta and volume-profile fields, updated each poll cycle. */
export interface OrderFlowState {
  /** ISO-8601 timestamp of the last successful read. */
  last_read_at: string | null;
  /** Source of the data: "cdp" for live TV read, "manual" for user-entered. */
  source: 'cdp' | 'manual';
  /** Current bar data. */
  bar: OrderFlowBarState;
  /** Running cumulative delta for the session (sum of bar deltas). */
  cumulative_delta: number | null;
  /** Value Area High from the visible-range volume profile, if readable. */
  vah: number | null;
  /** Value Area Low from the visible-range volume profile, if readable. */
  val: number | null;
  /** Point of Control (highest-volume price node), if readable. */
  poc: number | null;
  /** Any raw CDP error the last cycle produced — null on clean read. */
  cdp_error: string | null;
}

// ---------------------------------------------------------------------------
// Per-trade journal tags
// ---------------------------------------------------------------------------

/** Canonical tag values for order-flow trade annotations. */
export type OrderFlowTagValue =
  | 'absorbed'
  | 'delta_div'
  | 'single_print'
  | 'value_area_reject'
  | 'responsive_buyer'
  | 'responsive_seller';

/** A single tag attached to an active trade. */
export interface OrderFlowTag {
  tag: OrderFlowTagValue;
  /** Unix ms timestamp when the tag was applied. */
  tagged_at: number;
  /** Optional free-text note attached at tag time. */
  note: string | null;
}

// ---------------------------------------------------------------------------
// Workspace presets
// ---------------------------------------------------------------------------

/** One indicator to add when applying a workspace preset. */
export interface OrderFlowIndicatorSpec {
  /** TV Pine Script indicator name as shown in the indicator search. */
  name: string;
  /** Key-value pairs to set on the indicator inputs after adding. */
  params: Record<string, string | number | boolean>;
}

/** A named TV-layout preset — applying it adds a set of indicators to the chart. */
export interface OrderFlowWorkspacePreset {
  /** Unique slug, e.g. "vwap_vpvr_avwap" */
  id: string;
  /** Human-readable label shown in the dropdown. */
  label: string;
  /** Short description shown in the UI tooltip. */
  description: string;
  /** Ordered list of indicators to add via TV CDP. */
  indicators: OrderFlowIndicatorSpec[];
}

// ---------------------------------------------------------------------------
// Tauri command result shapes
// (Assist mirrors these as Rust serde structs)
// ---------------------------------------------------------------------------

/** Result of order_flow_apply_preset(name). */
export interface OrderFlowApplyPresetResult {
  ok: boolean;
  preset_id: string | null;
  indicators_added: number;
  error: string | null;
}

/** Result of order_flow_tag_active_trade(tag). */
export interface OrderFlowTagResult {
  ok: boolean;
  tag: OrderFlowTagValue | null;
  error: string | null;
}

/** Result of tv_cdp_get_order_flow_state(). */
export interface OrderFlowStateResult {
  ok: boolean;
  state: OrderFlowState | null;
  error: string | null;
}

/** Result of order_flow_save_config(config). */
export interface OrderFlowConfigResult {
  ok: boolean;
  config: OrderFlowConfig | null;
  error: string | null;
}
