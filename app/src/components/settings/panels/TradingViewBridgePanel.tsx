/**
 * Whiskey fork — TradingView Desktop CDP bridge UI.
 *
 * The bridge is a Chrome DevTools Protocol attach against TradingView
 * Desktop's renderer process. The user must launch TV Desktop with
 * `--remote-debugging-port=9222` for any of this to resolve a target;
 * this panel walks them through that one-time setup, probes the port,
 * lets them attach/detach, and shows the live chart state Whiskey
 * sees.
 *
 * Wires to the Tauri commands in
 * `app/src-tauri/src/tradingview_cdp.rs`:
 *
 *   - `tv_cdp_probe(port?) -> { reachable, port, browser_ws_url, tv_targets, error }`
 *   - `tv_cdp_attach(port?) -> ProbeResult`
 *   - `tv_cdp_get_chart_state() -> { symbol, resolution, price, indicator_count, raw }`
 *   - `tv_cdp_detach() -> void`
 *
 * Stylistic conventions follow `ModesPanel`: rounded-xl card, stone
 * neutrals, primary-500 buttons, role="alert" error region.
 *
 * The "evaluate JavaScript" textarea is intentionally NOT exposed in
 * v1 — `tv_cdp_eval` is a power tool that runs arbitrary JS against a
 * logged-in TV session. v2 will gate it behind a confirmation dialog
 * and only when WhiskeyMode is the active mode.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useState } from 'react';

import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

interface TvCdpTargetSummary {
  id: string;
  url: string;
  title: string;
}

interface TvCdpProbeResult {
  reachable: boolean;
  port: number;
  browser_ws_url: string | null;
  tv_targets: TvCdpTargetSummary[];
  error: string | null;
}

interface TvIndicatorSummary {
  id: string | null;
  name: string | null;
}

interface TvShapeSummary {
  id: string | null;
  name: string | null;
}

interface TvChartState {
  symbol: string | null;
  resolution: string | null;
  price: number | null;
  indicator_count: number | null;
  indicators: TvIndicatorSummary[] | null;
  shapes: TvShapeSummary[] | null;
  alert_count: number | null;
  raw: unknown;
}

interface TvSetSymbolResult {
  ok: boolean;
  symbol: string | null;
  error: string | null;
}

interface TvLaunchResult {
  launched: boolean;
  path: string | null;
  port: number;
  error: string | null;
}

const DEFAULT_PORT = 9222;

const TradingViewBridgePanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();
  const [port, setPort] = useState<number>(DEFAULT_PORT);
  const [probe, setProbe] = useState<TvCdpProbeResult | null>(null);
  const [chartState, setChartState] = useState<TvChartState | null>(null);
  const [attached, setAttached] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [symbolDraft, setSymbolDraft] = useState<string>('');

  const launchTv = useCallback(async () => {
    setPending(true);
    setError(null);
    try {
      const result = await invoke<TvLaunchResult>('tv_cdp_launch_tv', { port });
      if (!result.launched) {
        setError(
          result.error ??
            `Could not auto-launch TradingView (path: ${result.path ?? 'unknown'}). Launch it manually with --remote-debugging-port=${port}.`
        );
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Launch failed: ${msg}`);
    } finally {
      setPending(false);
    }
  }, [port]);

  const setSymbol = useCallback(async () => {
    const next = symbolDraft.trim();
    if (next.length === 0) return;
    setPending(true);
    setError(null);
    try {
      const result = await invoke<TvSetSymbolResult>('tv_cdp_set_symbol', { symbol: next });
      if (!result.ok) {
        setError(result.error ?? 'TV refused the symbol change (active chart unavailable?).');
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Set symbol failed: ${msg}`);
    } finally {
      setPending(false);
    }
  }, [symbolDraft]);

  const runProbe = useCallback(async () => {
    setPending(true);
    setError(null);
    try {
      const result = await invoke<TvCdpProbeResult>('tv_cdp_probe', { port });
      setProbe(result);
      if (!result.reachable) {
        setError(
          result.error ??
            `TV Desktop is not listening on port ${port}. Launch it with --remote-debugging-port=${port}.`
        );
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Probe failed: ${msg}`);
    } finally {
      setPending(false);
    }
  }, [port]);

  const attach = useCallback(async () => {
    setPending(true);
    setError(null);
    try {
      const result = await invoke<TvCdpProbeResult>('tv_cdp_attach', { port });
      setProbe(result);
      setAttached(true);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Attach failed: ${msg}`);
      setAttached(false);
    } finally {
      setPending(false);
    }
  }, [port]);

  const detach = useCallback(async () => {
    setPending(true);
    setError(null);
    try {
      await invoke('tv_cdp_detach');
      setAttached(false);
      setChartState(null);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Detach failed: ${msg}`);
    } finally {
      setPending(false);
    }
  }, []);

  const refreshChartState = useCallback(async () => {
    setPending(true);
    setError(null);
    try {
      const result = await invoke<TvChartState>('tv_cdp_get_chart_state');
      setChartState(result);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Could not read chart state: ${msg}`);
    } finally {
      setPending(false);
    }
  }, []);

  // Cheap initial probe on mount so the user lands on the panel and
  // immediately sees whether TV is reachable, without having to click.
  useEffect(() => {
    void runProbe();
  }, [runProbe]);

  return (
    <div className="flex h-full w-full flex-col bg-stone-50">
      <SettingsHeader breadcrumbs={breadcrumbs} onBack={navigateBack} title="TradingView bridge" />
      <div className="flex-1 space-y-4 overflow-y-auto p-6">
        <section
          data-testid="tv-bridge-setup-card"
          className="rounded-xl border border-stone-200 bg-white p-4">
          <h2 className="text-sm font-semibold text-stone-900">One-time setup</h2>
          <ol className="mt-2 list-inside list-decimal space-y-1 text-xs text-stone-600">
            <li>Quit TradingView Desktop if it's running.</li>
            <li>
              Relaunch with{' '}
              <code className="rounded bg-stone-100 px-1 py-0.5 text-stone-800">
                --remote-debugging-port={port}
              </code>{' '}
              appended to its shortcut.
            </li>
            <li>Open a chart, then click Probe below.</li>
          </ol>
        </section>

        <section
          data-testid="tv-bridge-probe-card"
          className="rounded-xl border border-stone-200 bg-white p-4">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-semibold text-stone-900">Connection</h2>
            <span
              data-testid="tv-bridge-status"
              className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${
                attached
                  ? 'bg-green-100 text-green-800'
                  : probe?.reachable
                    ? 'bg-amber-100 text-amber-800'
                    : 'bg-stone-100 text-stone-600'
              }`}>
              {attached ? 'attached' : probe?.reachable ? 'reachable' : 'unreachable'}
            </span>
          </div>
          <div className="mt-3 flex items-center gap-2">
            <label className="text-xs text-stone-600" htmlFor="tv-cdp-port">
              Port
            </label>
            <input
              id="tv-cdp-port"
              type="number"
              min={1}
              max={65535}
              value={port}
              onChange={e => setPort(Number(e.target.value) || DEFAULT_PORT)}
              disabled={pending || attached}
              data-testid="tv-bridge-port-input"
              className="w-20 rounded-md border border-stone-200 bg-white px-2 py-1 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
            />
            <button
              type="button"
              onClick={() => void runProbe()}
              disabled={pending || attached}
              data-testid="tv-bridge-probe-button"
              className="shrink-0 rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
              Probe
            </button>
            <button
              type="button"
              onClick={() => void attach()}
              disabled={pending || attached || !probe?.reachable}
              data-testid="tv-bridge-attach-button"
              className="shrink-0 rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
              Attach
            </button>
            <button
              type="button"
              onClick={() => void detach()}
              disabled={pending || !attached}
              data-testid="tv-bridge-detach-button"
              className="shrink-0 rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
              Detach
            </button>
            <button
              type="button"
              onClick={() => void launchTv()}
              disabled={pending || attached}
              data-testid="tv-bridge-launch-button"
              title="Best-effort: auto-spawn TradingView Desktop with the debug port set."
              className="shrink-0 rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
              Launch TV
            </button>
          </div>
          {probe?.tv_targets.length ? (
            <div className="mt-3 text-[11px] text-stone-600">
              Discovered TV pages:
              <ul data-testid="tv-bridge-target-list" className="mt-1 space-y-1">
                {probe.tv_targets.map(t => (
                  <li key={t.id} className="font-mono text-[10px] text-stone-500">
                    {t.title || '(untitled)'} — {t.url}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </section>

        {attached ? (
          <section
            data-testid="tv-bridge-state-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-stone-900">Chart state</h2>
              <button
                type="button"
                onClick={() => void refreshChartState()}
                disabled={pending}
                data-testid="tv-bridge-refresh-button"
                className="shrink-0 rounded-md border border-stone-200 bg-white px-3 py-1 text-[11px] font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
                {pending ? 'Reading…' : 'Read now'}
              </button>
            </div>
            {chartState ? (
              <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
                <dt className="text-stone-500">Symbol</dt>
                <dd data-testid="tv-bridge-symbol" className="text-right font-mono text-stone-900">
                  {chartState.symbol ?? '—'}
                </dd>
                <dt className="text-stone-500">Resolution</dt>
                <dd
                  data-testid="tv-bridge-resolution"
                  className="text-right font-mono text-stone-900">
                  {chartState.resolution ?? '—'}
                </dd>
                <dt className="text-stone-500">Indicators</dt>
                <dd
                  data-testid="tv-bridge-indicator-count"
                  className="text-right font-mono text-stone-900">
                  {chartState.indicator_count ?? '—'}
                </dd>
                <dt className="text-stone-500">Shapes</dt>
                <dd
                  data-testid="tv-bridge-shape-count"
                  className="text-right font-mono text-stone-900">
                  {chartState.shapes?.length ?? '—'}
                </dd>
                <dt className="text-stone-500">Alerts visible</dt>
                <dd
                  data-testid="tv-bridge-alert-count"
                  className="text-right font-mono text-stone-900">
                  {chartState.alert_count ?? '—'}
                </dd>
              </dl>
            ) : (
              <p className="mt-2 text-xs text-stone-500">
                Not read yet. Click "Read now" to fetch live state.
              </p>
            )}
          </section>
        ) : null}

        {attached ? (
          <section
            data-testid="tv-bridge-write-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">Switch symbol</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              Writes the active chart's symbol via TV's internal API. Use exchange prefixes when
              ambiguous (e.g. <code>CME_MINI:NQ1!</code>, <code>NASDAQ:AAPL</code>).
            </p>
            <div className="mt-3 flex items-center gap-2">
              <input
                type="text"
                value={symbolDraft}
                onChange={e => setSymbolDraft(e.target.value)}
                disabled={pending}
                placeholder="CME_MINI:NQ1!"
                data-testid="tv-bridge-symbol-input"
                maxLength={64}
                className="flex-1 rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
              />
              <button
                type="button"
                onClick={() => void setSymbol()}
                disabled={pending || symbolDraft.trim().length === 0}
                data-testid="tv-bridge-set-symbol-button"
                className="shrink-0 rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
                Set
              </button>
            </div>
          </section>
        ) : null}

        {error ? (
          <div
            role="alert"
            data-testid="tv-bridge-error"
            className="rounded-xl border border-red-200 bg-red-50 p-3 text-xs text-red-800">
            {error}
          </div>
        ) : null}
      </div>
    </div>
  );
};

export default TradingViewBridgePanel;
