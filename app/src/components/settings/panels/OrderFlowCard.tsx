/**
 * OrderFlowCard — Order Flow observation card for TK's Mods.
 *
 * Sub-sections:
 *   a. Workspace presets — one-click TV indicator setup via CDP
 *   b. Live delta — current bar delta + CVD, polls 2 Hz when attached
 *   c. Order flow tags — chip list for tagging the active position
 *   d. Detection alerts — toggle list per pattern (Whiskey events, not popups)
 *
 * Observation and journaling only. No order placement.
 */
import { useCallback, useState } from 'react';

import { useOrderFlow } from '../../../hooks/useOrderFlow';
import type {
  OrderFlowConfig,
  OrderFlowTagValue,
  OrderFlowWorkspacePreset,
} from '../../../types/orderFlow';

// ---------------------------------------------------------------------------
// Workspace preset definitions (fallback set per spec)
// ---------------------------------------------------------------------------

const WORKSPACE_PRESETS: OrderFlowWorkspacePreset[] = [
  {
    id: 'vwap_vpvr_avwap',
    label: 'VWAP + VPVR + Anchored VWAP',
    description: 'Session VWAP, Visible Range Volume Profile, and Anchored VWAP at session open.',
    indicators: [
      { name: 'VWAP', params: { hideOnDailyBars: false } },
      { name: 'Volume Profile Visible Range', params: { rowSize: 24 } },
      { name: 'Anchored VWAP', params: {} },
    ],
  },
  {
    id: 'delta_cvd',
    label: 'CVD + Volume Delta',
    description: 'Cumulative Volume Delta and per-bar volume delta for order flow reading.',
    indicators: [
      { name: 'Cumulative Volume Delta', params: {} },
      { name: 'Volume Delta', params: {} },
    ],
  },
  {
    id: 'full_order_flow',
    label: 'Full Order Flow Suite',
    description: 'VWAP, VPVR, CVD, and Volume Delta — complete order flow context.',
    indicators: [
      { name: 'VWAP', params: {} },
      { name: 'Volume Profile Visible Range', params: { rowSize: 24 } },
      { name: 'Cumulative Volume Delta', params: {} },
      { name: 'Volume Delta', params: {} },
    ],
  },
];

// ---------------------------------------------------------------------------
// Tag chip definitions
// ---------------------------------------------------------------------------

const ORDER_FLOW_TAGS: { value: OrderFlowTagValue; label: string; color: string }[] = [
  { value: 'absorbed', label: 'Absorbed', color: 'bg-blue-100 text-blue-800 border-blue-200' },
  { value: 'delta_div', label: 'Delta Div', color: 'bg-amber-100 text-amber-800 border-amber-200' },
  {
    value: 'single_print',
    label: 'Single Print',
    color: 'bg-purple-100 text-purple-800 border-purple-200',
  },
  {
    value: 'value_area_reject',
    label: 'VA Reject',
    color: 'bg-red-100 text-red-800 border-red-200',
  },
  {
    value: 'responsive_buyer',
    label: 'Resp. Buyer',
    color: 'bg-green-100 text-green-800 border-green-200',
  },
  {
    value: 'responsive_seller',
    label: 'Resp. Seller',
    color: 'bg-orange-100 text-orange-800 border-orange-200',
  },
];

// ---------------------------------------------------------------------------
// Alert pattern definitions
// ---------------------------------------------------------------------------

const ALERT_PATTERNS: {
  key: keyof OrderFlowConfig['alert_toggles'];
  label: string;
  description: string;
}[] = [
  {
    key: 'delta_divergence',
    label: 'Delta Divergence',
    description: 'Price makes new high/low but delta does not confirm.',
  },
  {
    key: 'absorption',
    label: 'Absorption',
    description: 'Large volume with minimal price movement — aggressive sellers/buyers absorbed.',
  },
  {
    key: 'single_print_rejection',
    label: 'Single Print Rejection',
    description: 'Price quickly reverses from a TPO single-print area.',
  },
];

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface OrderFlowCardProps {
  /** Whether TV CDP bridge is currently attached. */
  tvAttached: boolean;
  /** Whether a position is currently open (gates the tag chips). */
  positionOpen?: boolean;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const OrderFlowCard = ({ tvAttached, positionOpen = false }: OrderFlowCardProps) => {
  const {
    state,
    cdpLive,
    config,
    loading,
    error,
    setManualBar,
    applyPreset,
    tagActiveTrade,
    toggleAlert,
  } = useOrderFlow(tvAttached);

  // ── Workspace preset ──────────────────────────────────────────────────────
  const [selectedPreset, setSelectedPreset] = useState<string>(WORKSPACE_PRESETS[0].id);
  const [presetPending, setPresetPending] = useState(false);
  const [presetSuccess, setPresetSuccess] = useState<string | null>(null);

  const handleApplyPreset = useCallback(async () => {
    setPresetSuccess(null);
    setPresetPending(true);
    try {
      await applyPreset(selectedPreset);
      const found = WORKSPACE_PRESETS.find(p => p.id === selectedPreset);
      setPresetSuccess(`Applied: ${found?.label ?? selectedPreset}`);
    } finally {
      setPresetPending(false);
    }
  }, [applyPreset, selectedPreset]);

  // ── Manual delta entry ────────────────────────────────────────────────────
  const [manualBid, setManualBid] = useState('');
  const [manualAsk, setManualAsk] = useState('');

  const handleManualEntry = useCallback(() => {
    const bid = parseFloat(manualBid);
    const ask = parseFloat(manualAsk);
    if (!Number.isNaN(bid) && !Number.isNaN(ask)) {
      setManualBar(bid, ask);
      setManualBid('');
      setManualAsk('');
    }
  }, [manualBid, manualAsk, setManualBar]);

  // ── Tag persistence feedback ──────────────────────────────────────────────
  const [lastTag, setLastTag] = useState<OrderFlowTagValue | null>(null);

  const handleTag = useCallback(
    async (tag: OrderFlowTagValue) => {
      await tagActiveTrade(tag);
      setLastTag(tag);
      setTimeout(() => setLastTag(null), 2000);
    },
    [tagActiveTrade]
  );

  // ── Render ────────────────────────────────────────────────────────────────

  const barDelta = state?.bar.bar_delta ?? null;
  const cumDelta = state?.cumulative_delta ?? null;
  const dataSource = state?.source ?? null;

  return (
    <section
      data-testid="tks-mods-order-flow-card"
      className="rounded-xl border border-stone-200 bg-white p-4">
      <h2 className="text-sm font-semibold text-stone-900">Order Flow</h2>
      <p className="mt-1 text-[11px] text-stone-500">
        Observation and journaling only — no orders are placed. Requires TV bridge for live data.
      </p>

      {!tvAttached ? (
        <p
          data-testid="tks-mods-order-flow-attach-required"
          className="mt-2 rounded-md bg-amber-50 border border-amber-200 px-3 py-2 text-[11px] text-amber-800">
          TV bridge not attached — live data unavailable. Manual delta entry is still available
          below.
        </p>
      ) : null}

      {/* ── a. Workspace presets ───────────────────────────────────────────── */}
      <div
        data-testid="tks-mods-order-flow-presets"
        className="mt-4 border-t border-stone-100 pt-3">
        <p className="text-[11px] font-medium text-stone-600">Workspace presets</p>
        <p className="mt-0.5 text-[11px] text-stone-400">
          Adds indicators to the active TV chart via the CDP bridge.
        </p>
        <div className="mt-2 flex items-center gap-2">
          <select
            value={selectedPreset}
            onChange={e => setSelectedPreset(e.target.value)}
            disabled={presetPending || !tvAttached}
            data-testid="tks-mods-order-flow-preset-select"
            className="flex-1 rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400">
            {WORKSPACE_PRESETS.map(p => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
          <button
            type="button"
            onClick={() => void handleApplyPreset()}
            disabled={presetPending || !tvAttached}
            data-testid="tks-mods-order-flow-apply-preset"
            className="shrink-0 rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
            {presetPending ? 'Applying…' : 'Apply preset'}
          </button>
        </div>
        {presetSuccess ? (
          <p
            data-testid="tks-mods-order-flow-preset-success"
            className="mt-1.5 text-[11px] text-green-700">
            {presetSuccess}
          </p>
        ) : null}
        {/* Preset description tooltip */}
        {(() => {
          const preset = WORKSPACE_PRESETS.find(p => p.id === selectedPreset);
          return preset ? (
            <p className="mt-1 text-[11px] text-stone-400">{preset.description}</p>
          ) : null;
        })()}
      </div>

      {/* ── b. Live delta ──────────────────────────────────────────────────── */}
      <div data-testid="tks-mods-order-flow-delta" className="mt-4 border-t border-stone-100 pt-3">
        <div className="flex items-center justify-between">
          <p className="text-[11px] font-medium text-stone-600">Live delta</p>
          {tvAttached ? (
            <span
              data-testid="tks-mods-order-flow-cdp-status"
              className={`text-[10px] font-medium ${cdpLive ? 'text-green-600' : 'text-stone-400'}`}>
              {cdpLive ? 'CDP live' : 'CDP: no data'}
            </span>
          ) : null}
        </div>

        {loading ? <p className="mt-1 text-[11px] text-stone-400">Loading…</p> : null}

        {/* Live read display */}
        {state && !loading ? (
          <div
            data-testid="tks-mods-order-flow-delta-display"
            className="mt-2 grid grid-cols-2 gap-2">
            <div className="rounded-md bg-stone-50 px-3 py-2">
              <p className="text-[10px] text-stone-400">Bar delta</p>
              <p
                data-testid="tks-mods-order-flow-bar-delta"
                className={`text-sm font-mono font-semibold ${
                  (barDelta ?? 0) > 0
                    ? 'text-green-700'
                    : (barDelta ?? 0) < 0
                      ? 'text-red-700'
                      : 'text-stone-600'
                }`}>
                {barDelta !== null ? (barDelta > 0 ? '+' : '') + barDelta.toLocaleString() : '—'}
              </p>
            </div>
            <div className="rounded-md bg-stone-50 px-3 py-2">
              <p className="text-[10px] text-stone-400">Cum. delta</p>
              <p
                data-testid="tks-mods-order-flow-cum-delta"
                className={`text-sm font-mono font-semibold ${
                  (cumDelta ?? 0) > 0
                    ? 'text-green-700'
                    : (cumDelta ?? 0) < 0
                      ? 'text-red-700'
                      : 'text-stone-600'
                }`}>
                {cumDelta !== null ? (cumDelta > 0 ? '+' : '') + cumDelta.toLocaleString() : '—'}
              </p>
            </div>
            {state.vah !== null || state.val !== null || state.poc !== null ? (
              <>
                <div className="rounded-md bg-stone-50 px-3 py-2">
                  <p className="text-[10px] text-stone-400">VAH / VAL</p>
                  <p className="text-xs font-mono text-stone-700">
                    {state.vah ?? '—'} / {state.val ?? '—'}
                  </p>
                </div>
                <div className="rounded-md bg-stone-50 px-3 py-2">
                  <p className="text-[10px] text-stone-400">POC</p>
                  <p className="text-xs font-mono text-stone-700">{state.poc ?? '—'}</p>
                </div>
              </>
            ) : null}
            <p className="col-span-2 text-[10px] text-stone-400">
              Source: {dataSource === 'cdp' ? 'TV CDP (live)' : 'manual entry'} ·{' '}
              {state.last_read_at
                ? new Date(state.last_read_at).toLocaleTimeString([], {
                    hour: '2-digit',
                    minute: '2-digit',
                    second: '2-digit',
                  })
                : '—'}
            </p>
          </div>
        ) : null}

        {/* Manual entry form — always shown when CDP data is unavailable */}
        {(!state || !cdpLive) && !loading ? (
          <div data-testid="tks-mods-order-flow-manual-entry" className="mt-2 space-y-2">
            <p className="text-[11px] text-stone-400">
              Enter bid and ask volume for the current bar to compute delta manually.
            </p>
            <div className="flex items-end gap-2">
              <div className="flex flex-col gap-1">
                <label htmlFor="of-bid-vol" className="text-[11px] text-stone-500">
                  Bid vol
                </label>
                <input
                  id="of-bid-vol"
                  type="number"
                  step="1"
                  min="0"
                  value={manualBid}
                  onChange={e => setManualBid(e.target.value)}
                  placeholder="0"
                  data-testid="tks-mods-order-flow-manual-bid"
                  className="w-24 rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500"
                />
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="of-ask-vol" className="text-[11px] text-stone-500">
                  Ask vol
                </label>
                <input
                  id="of-ask-vol"
                  type="number"
                  step="1"
                  min="0"
                  value={manualAsk}
                  onChange={e => setManualAsk(e.target.value)}
                  placeholder="0"
                  data-testid="tks-mods-order-flow-manual-ask"
                  className="w-24 rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500"
                />
              </div>
              <button
                type="button"
                onClick={handleManualEntry}
                disabled={manualBid.trim() === '' || manualAsk.trim() === ''}
                data-testid="tks-mods-order-flow-manual-submit"
                className="rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
                Add bar
              </button>
            </div>
          </div>
        ) : null}
      </div>

      {/* ── c. Order flow tags ─────────────────────────────────────────────── */}
      <div data-testid="tks-mods-order-flow-tags" className="mt-4 border-t border-stone-100 pt-3">
        <p className="text-[11px] font-medium text-stone-600">Order flow tags</p>
        <p className="mt-0.5 text-[11px] text-stone-400">
          Tag the active position for playbook journaling. Persists to the trade log on close.
          {!positionOpen ? ' (No active position — tags will still be recorded.)' : ''}
        </p>
        <div data-testid="tks-mods-order-flow-tag-chips" className="mt-2 flex flex-wrap gap-1.5">
          {ORDER_FLOW_TAGS.map(t => (
            <button
              key={t.value}
              type="button"
              onClick={() => void handleTag(t.value)}
              data-testid={`tks-mods-order-flow-tag-${t.value}`}
              className={`rounded-full border px-2.5 py-1 text-[11px] font-medium transition-all hover:opacity-80 ${
                lastTag === t.value ? 'bg-primary-500 text-white border-primary-500' : t.color
              }`}>
              {t.label}
              {lastTag === t.value ? ' ✓' : ''}
            </button>
          ))}
        </div>
      </div>

      {/* ── d. Detection alerts ────────────────────────────────────────────── */}
      <div data-testid="tks-mods-order-flow-alerts" className="mt-4 border-t border-stone-100 pt-3">
        <p className="text-[11px] font-medium text-stone-600">Detection alerts</p>
        <p className="mt-0.5 text-[11px] text-stone-400">
          When enabled, Whiskey surfaces these patterns via the mascot bubble — not as popups.
        </p>
        <ul className="mt-2 space-y-2">
          {ALERT_PATTERNS.map(pattern => (
            <li key={pattern.key} className="flex items-start gap-2">
              <input
                type="checkbox"
                id={`of-alert-${pattern.key}`}
                checked={config.alert_toggles[pattern.key]}
                onChange={() => void toggleAlert(pattern.key)}
                data-testid={`tks-mods-order-flow-alert-${pattern.key}`}
                className="mt-0.5 h-4 w-4 shrink-0 rounded border-stone-300 text-primary-500 focus:ring-primary-500"
              />
              <label htmlFor={`of-alert-${pattern.key}`} className="flex flex-col">
                <span className="text-xs font-medium text-stone-700">{pattern.label}</span>
                <span className="text-[11px] text-stone-400">{pattern.description}</span>
              </label>
            </li>
          ))}
        </ul>
      </div>

      {/* Error display */}
      {error ? (
        <div
          role="alert"
          data-testid="tks-mods-order-flow-error"
          className="mt-3 rounded-md border border-red-200 bg-red-50 p-2 text-xs text-red-800">
          {error}
        </div>
      ) : null}
    </section>
  );
};

export default OrderFlowCard;
