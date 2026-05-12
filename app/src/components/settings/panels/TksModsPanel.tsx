/**
 * TK's Mods — settings panel.
 *
 * Home for three TK-specific customizations:
 *   1. Theme picker   — default (stone/sage) vs ZETH (black + neon green).
 *   2. SL/TP overlay  — draw stop-loss / take-profit horizontal lines on the
 *                        active TradingView chart via the CDP bridge.
 *   3. Hide risk %    — redact $ amounts and percentages from Whiskey messages.
 *
 * Style conventions match TradingViewBridgePanel + ModesPanel:
 *   - rounded-xl border border-stone-200 bg-white p-4 cards
 *   - role="alert" + data-testid for the error region
 *   - primary-500 / primary-600 action buttons
 *
 * Under the ZETH theme the component picks up neon green accents through
 * CSS custom properties; no per-theme JSX branching needed.
 *
 * Wires to:
 *   - `useTheme` hook            — client-side CSS var switch (< 100ms)
 *   - `tv_cdp_draw_sltp`         — Tauri command (tradingview_cdp.rs)
 *   - `tv_cdp_clear_sltp`        — Tauri command (tradingview_cdp.rs)
 *   - localStorage `tk-hide-risk-pct`  — risk-hide toggle (frontend-only
 *     for now; the Rust sanitizer reads the TOML copy through the Tauri
 *     command `tks_mods_get_config` / `tks_mods_set_config` added in a
 *     follow-up pass — see implementation note below).
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useState } from 'react';

import { THEMES, useTheme } from '../../../hooks/useTheme';
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface TvSltpResult {
  ok: boolean;
  removed: number | null;
  error: string | null;
}

// ---------------------------------------------------------------------------
// Risk-hide persistence (localStorage, frontend layer)
// The Rust sanitizer reads the same flag via the TOML config written by
// the Tauri `tks_mods_set_config` command.  For v1 we persist both sides
// so the UI reflects the choice immediately without a round-trip.
// ---------------------------------------------------------------------------
const RISK_HIDE_KEY = 'tk-hide-risk-pct';

function readRiskHide(): boolean {
  try {
    return localStorage.getItem(RISK_HIDE_KEY) === 'true';
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// R-multiple helper
// ---------------------------------------------------------------------------
function rMultiple(entry: number, stop: number, target: number): string | null {
  const risk = Math.abs(entry - stop);
  const reward = Math.abs(target - entry);
  if (risk === 0) return null;
  return (reward / risk).toFixed(2) + 'R';
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

const TksModsPanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();
  const { theme, setTheme } = useTheme();

  // SL/TP inputs
  const [entry, setEntry] = useState('');
  const [stop, setStop] = useState('');
  const [target, setTarget] = useState('');
  const [sltpPending, setSltpPending] = useState(false);
  const [sltpError, setSltpError] = useState<string | null>(null);
  const [sltpSuccess, setSltpSuccess] = useState<string | null>(null);

  // Risk-hide toggle
  const [hideRisk, setHideRiskState] = useState<boolean>(readRiskHide);

  // Derived R-multiple
  const entryNum = parseFloat(entry);
  const stopNum = parseFloat(stop);
  const targetNum = parseFloat(target);
  const rLabel =
    !Number.isNaN(entryNum) && !Number.isNaN(stopNum) && !Number.isNaN(targetNum)
      ? rMultiple(entryNum, stopNum, targetNum)
      : null;

  // -------------------------------------------------------------------------
  // Handlers
  // -------------------------------------------------------------------------

  const handleThemeChange = useCallback(
    (e: React.ChangeEvent<HTMLSelectElement>) => {
      const val = e.target.value as 'default' | 'zeth';
      setTheme(val);
    },
    [setTheme]
  );

  const handleDrawSltp = useCallback(async () => {
    setSltpError(null);
    setSltpSuccess(null);
    const e = parseFloat(entry);
    const s = parseFloat(stop);
    const t = parseFloat(target);
    if (Number.isNaN(e) || Number.isNaN(s) || Number.isNaN(t)) {
      setSltpError('Enter valid numbers for Entry, Stop, and Target.');
      return;
    }
    setSltpPending(true);
    try {
      const result = await invoke<TvSltpResult>('tv_cdp_draw_sltp', {
        entry: e,
        stop: s,
        target: t,
        zethTheme: theme === 'zeth',
      });
      if (!result.ok) {
        setSltpError(result.error ?? 'Draw failed — is the TV bridge attached?');
      } else {
        setSltpSuccess('Lines drawn on chart.');
      }
    } catch (err) {
      setSltpError(err instanceof Error ? err.message : String(err));
    } finally {
      setSltpPending(false);
    }
  }, [entry, stop, target, theme]);

  const handleClearSltp = useCallback(async () => {
    setSltpError(null);
    setSltpSuccess(null);
    setSltpPending(true);
    try {
      const result = await invoke<TvSltpResult>('tv_cdp_clear_sltp');
      if (!result.ok) {
        setSltpError(result.error ?? 'Clear failed — is the TV bridge attached?');
      } else {
        setSltpSuccess(`Cleared ${result.removed ?? 0} overlay line(s).`);
      }
    } catch (err) {
      setSltpError(err instanceof Error ? err.message : String(err));
    } finally {
      setSltpPending(false);
    }
  }, []);

  const handleRiskHideToggle = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const next = e.target.checked;
    try {
      localStorage.setItem(RISK_HIDE_KEY, String(next));
    } catch {
      // ignore
    }
    setHideRiskState(next);
  }, []);

  // -------------------------------------------------------------------------
  // Render
  // -------------------------------------------------------------------------

  return (
    <div className="flex h-full w-full flex-col bg-stone-50">
      <SettingsHeader breadcrumbs={breadcrumbs} onBack={navigateBack} title="TK's Mods" />
      <div className="flex-1 space-y-4 overflow-y-auto p-6">
        {/* ── 1. Theme picker ───────────────────────────────────────────── */}
        <section
          data-testid="tks-mods-theme-card"
          className="rounded-xl border border-stone-200 bg-white p-4">
          <h2 className="text-sm font-semibold text-stone-900">Theme</h2>
          <p className="mt-1 text-[11px] text-stone-500">
            Applies to Whiskey UI surfaces only — TradingView's own UI is unaffected. Switch takes
            effect instantly with no reload required.
          </p>
          <div className="mt-3 flex items-center gap-3">
            <label htmlFor="tk-theme-select" className="text-xs text-stone-600">
              Active theme
            </label>
            <select
              id="tk-theme-select"
              data-testid="tks-mods-theme-select"
              value={theme}
              onChange={handleThemeChange}
              className="flex-1 rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500">
              {THEMES.map(t => (
                <option key={t.id} value={t.id}>
                  {t.label}
                </option>
              ))}
            </select>
          </div>
          {theme === 'zeth' && (
            <p
              data-testid="tks-mods-zeth-active-label"
              className="mt-2 text-[11px] font-medium"
              style={{ color: 'var(--tk-accent, #39ff14)' }}>
              ZETH active — deep black / neon green palette engaged.
            </p>
          )}
        </section>

        {/* ── 2. SL/TP overlay ─────────────────────────────────────────── */}
        <section
          data-testid="tks-mods-sltp-card"
          className="rounded-xl border border-stone-200 bg-white p-4">
          <h2 className="text-sm font-semibold text-stone-900">SL/TP Overlay</h2>
          <p className="mt-1 text-[11px] text-stone-500">
            Draws native TV horizontal lines for your stop and target — works even when prop-firm
            broker panels hide the default order lines. Requires the TradingView bridge to be
            attached (see TradingView Bridge settings).
          </p>

          <div className="mt-3 grid grid-cols-3 gap-2">
            {/* Entry */}
            <div className="flex flex-col gap-1">
              <label htmlFor="tk-sltp-entry" className="text-[11px] text-stone-500">
                Entry
              </label>
              <input
                id="tk-sltp-entry"
                type="number"
                step="any"
                value={entry}
                onChange={e => setEntry(e.target.value)}
                disabled={sltpPending}
                placeholder="e.g. 19800"
                data-testid="tks-mods-sltp-entry"
                className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
              />
            </div>
            {/* Stop */}
            <div className="flex flex-col gap-1">
              <label htmlFor="tk-sltp-stop" className="text-[11px] text-stone-500">
                Stop
              </label>
              <input
                id="tk-sltp-stop"
                type="number"
                step="any"
                value={stop}
                onChange={e => setStop(e.target.value)}
                disabled={sltpPending}
                placeholder="e.g. 19750"
                data-testid="tks-mods-sltp-stop"
                className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
              />
            </div>
            {/* Target */}
            <div className="flex flex-col gap-1">
              <label htmlFor="tk-sltp-target" className="text-[11px] text-stone-500">
                Target
                {rLabel ? (
                  <span data-testid="tks-mods-r-label" className="ml-1 font-mono text-green-600">
                    {rLabel}
                  </span>
                ) : null}
              </label>
              <input
                id="tk-sltp-target"
                type="number"
                step="any"
                value={target}
                onChange={e => setTarget(e.target.value)}
                disabled={sltpPending}
                placeholder="e.g. 19875"
                data-testid="tks-mods-sltp-target"
                className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
              />
            </div>
          </div>

          {sltpSuccess ? (
            <p className="mt-2 text-[11px] text-green-700" data-testid="tks-mods-sltp-success">
              {sltpSuccess}
            </p>
          ) : null}

          <div className="mt-3 flex gap-2">
            <button
              type="button"
              onClick={() => void handleDrawSltp()}
              disabled={sltpPending}
              data-testid="tks-mods-draw-button"
              className="rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
              {sltpPending ? 'Working…' : 'Draw on chart'}
            </button>
            <button
              type="button"
              onClick={() => void handleClearSltp()}
              disabled={sltpPending}
              data-testid="tks-mods-clear-button"
              className="rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
              Clear my overlays
            </button>
          </div>
        </section>

        {/* ── 3. Risk-hide toggle ───────────────────────────────────────── */}
        <section
          data-testid="tks-mods-risk-hide-card"
          className="rounded-xl border border-stone-200 bg-white p-4">
          <h2 className="text-sm font-semibold text-stone-900">Hide risk %</h2>
          <p className="mt-1 text-[11px] text-stone-500">
            When on, Whiskey replaces dollar amounts and percentages in messages with abstract terms
            (e.g. "$250 risk" → "risk unit", "0.5% account risk" → "small position"). R-multiples
            like "1.5R" are preserved.
          </p>
          <label className="mt-3 flex cursor-pointer items-center gap-3">
            <input
              type="checkbox"
              checked={hideRisk}
              onChange={handleRiskHideToggle}
              data-testid="tks-mods-risk-hide-toggle"
              className="h-4 w-4 rounded border-stone-300 text-primary-500 focus:ring-primary-500"
            />
            <span className="text-xs text-stone-700">
              {hideRisk
                ? 'On — dollar and percentage amounts are redacted'
                : 'Off — amounts shown as-is'}
            </span>
          </label>
        </section>

        {/* ── Shared error region ───────────────────────────────────────── */}
        {sltpError ? (
          <div
            role="alert"
            data-testid="tks-mods-error"
            className="rounded-xl border border-red-200 bg-red-50 p-3 text-xs text-red-800">
            {sltpError}
          </div>
        ) : null}
      </div>
    </div>
  );
};

export default TksModsPanel;
