# Order-Flow Contract — Rust-side shape

Branch: `order-flow-assist`
Written by: backend/Rust agent
Last updated: 2026-05-12

This file documents the Rust-side type shapes so the main builder
(`order-flow-main`) can mirror them in `app/src/types/orderFlow.ts`.
If main's TS types land before this contract is consumed, main's
shape takes precedence — update the Rust structs to match and delete
this file.

---

## OrderFlowConfig

```typescript
interface OrderFlowConfig {
  workspace_presets: boolean;        // default true
  manual_delta_tracker: boolean;     // default true
  tag_chips: boolean;                // default true
  detection_alerts: boolean;         // default false
  polling_hz: number;                // u8, clamped 1-2, default 1
  divergence_lookback: number;       // usize, default 5
  absorption_volume_multiplier: number; // f64, default 2.0
  absorption_range_fraction: number; // f64, default 0.5
}
```

## OrderFlowState

```typescript
interface OrderFlowState {
  current_bar_delta: number;         // i64
  cumulative_session_delta: number;  // i64, saturating
  bar_deltas: number[];              // ring buffer, max 200 entries
  vah: number | null;                // Volume Area High
  val: number | null;                // Volume Area Low
  poc: number | null;                // Point of Control
  tags: OrderFlowTag[];              // in-memory only, max 500 entries
  bars: BarDelta[];                  // raw bar ring buffer, max 200
}
```

## BarDelta (input to order_flow_record_bar)

```typescript
interface BarDelta {
  open: number;
  high: number;
  low: number;
  close: number;
  bid_vol: number;   // u64
  ask_vol: number;   // u64
}
```

## OrderFlowTag

```typescript
interface OrderFlowTag {
  id: string;    // must be one of ALLOWED_TAGS (see below)
  ts_ms: number; // unix ms, set server-side on tag_active_trade command
}
```

## Allowed tag ids (const allowlist, Rust-enforced)

```
"absorbed"
"delta_div"
"single_print"
"value_area_reject"
"responsive_buyer"
"responsive_seller"
```

## Workspace preset names (const allowlist, Rust-enforced)

```
"vwap_profile_anchored"   -> VWAP, Volume Profile (Visible Range), Anchored VWAP
"standard_orderflow"      -> VWAP, Volume Profile (Session)
"delta_focused"           -> VWAP, Cumulative Volume Delta
```

## Tauri commands

All commands are registered in `app/src-tauri/src/lib.rs` under the
`order_flow_commands::` prefix.

| Command                        | Args                                              | Returns              |
|-------------------------------|---------------------------------------------------|----------------------|
| `order_flow_get_config`       | —                                                 | `OrderFlowConfig`    |
| `order_flow_set_config`       | `cfg: OrderFlowConfig`                            | `void`               |
| `order_flow_record_bar`       | `open, high, low, close, bid_vol, ask_vol`        | `OrderFlowState`     |
| `order_flow_tag_active_trade` | `tag: string`                                     | `Result<void>`       |
| `order_flow_apply_preset`     | `name: string`                                    | `Result<void>`       |

`order_flow_apply_preset` requires `tv_cdp_attach` to have been called first.
`order_flow_tag_active_trade` and `order_flow_apply_preset` validate the
caller string against const allowlists — unknown values return `Err`.

## Persistence

Config is co-persisted inside `tks_mods.toml` via an `order_flow` field
added to `TksModsConfig`.  The TOML key name is `order_flow`.
