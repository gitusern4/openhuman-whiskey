/**
 * TvBridgePanelBody — reusable inner content of the TradingView bridge panel.
 *
 * Extracted so it can be embedded in TksModsPanel while keeping the
 * /tradingview-bridge route working via TradingViewBridgePanel wrapper.
 *
 * Accepts an optional `onAttachedChange` callback so TksModsPanel can
 * track attachment state for the symbol-favorites card (which should
 * only activate TV switch when the bridge is attached).
 *
 * Auto-attach (cdp-auto-attach branch):
 *   - Toggle is OFF by default to preserve existing UX.
 *   - When ON: green dot = attached + supervisor running,
 *              amber dot = supervisor retrying,
 *              red dot   = supervisor disabled / stuck.
 *   - When ON: the Attach button becomes "Force re-attach now."
 *   - Listens to `tv-cdp-status` Tauri events from the supervisor task.
 *     Payload: { kind: "attached"|"detached"|"reattached"|"navigated"|
 *                       "reconnect_failed", at: number, error: string|null }
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';

interface TvCdpTargetSummary {
  id: string;
  url: string;
  title: string;
}

export interface TvCdpProbeResult {
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

/** Returned by tv_cdp_get_auto_attach_status */
interface TvAutoAttachStatus {
  enabled: boolean;
  attached: boolean;
  last_event: string | null;
  last_event_at: number | null;
  retry_count: number;
}

/** Payload of the `tv-cdp-status` Tauri event (defined in tv_cdp_supervisor.rs) */
interface TvCdpStatusPayload {
  kind: 'attached' | 'detached' | 'reattached' | 'navigated' | 'reconnect_failed';
  at: number;
  error: string | null;
}

const DEFAULT_PORT = 9222;

/** Poll interval for supervisor status when auto-attach is on (ms). */
const STATUS_POLL_MS = 5000;

interface TvBridgePanelBodyProps {
  onAttachedChange?: (attached: boolean) => void;
}

const TvBridgePanelBody = ({ onAttachedChange }: TvBridgePanelBodyProps) => {
  const [port, setPort] = useState<number>(DEFAULT_PORT);
  const [probe, setProbe] = useState<TvCdpProbeResult | null>(null);
  const [chartState, setChartState] = useState<TvChartState | null>(null);
  const [attached, setAttached] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [symbolDraft, setSymbolDraft] = useState<string>('');

  // Auto-attach state
  const [autoAttach, setAutoAttach] = useState(false);
  const [autoStatus, setAutoStatus] = useState<TvAutoAttachStatus | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const notifyAttached = useCallback(
    (next: boolean) => {
      setAttached(next);
      onAttachedChange?.(next);
    },
    [onAttachedChange]
  );

  // ---------------------------------------------------------------------------
  // Auto-attach status helpers
  // ---------------------------------------------------------------------------

  const refreshAutoStatus = useCallback(async () => {
    try {
      const s = await invoke<TvAutoAttachStatus>('tv_cdp_get_auto_attach_status');
      setAutoStatus(s);
      // Sync attached state from supervisor when auto-attach is on.
      if (s.enabled) {
        notifyAttached(s.attached);
      }
    } catch {
      // Non-fatal — supervisor may not be running.
    }
  }, [notifyAttached]);

  // Subscribe to supervisor events and start polling when auto-attach turns on.
  useEffect(() => {
    if (!autoAttach) {
      // Tear down listener and poll timer.
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
      if (pollTimerRef.current) {
        clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
      return;
    }

    // Subscribe to real-time events from the supervisor.
    let active = true;
    void listen<TvCdpStatusPayload>('tv-cdp-status', ev => {
      if (!active) return;
      const { kind } = ev.payload;
      if (kind === 'attached' || kind === 'reattached') {
        notifyAttached(true);
      } else if (kind === 'detached' || kind === 'reconnect_failed') {
        notifyAttached(false);
      }
      // Always refresh the full status on any event.
      void refreshAutoStatus();
    }).then(fn => {
      if (!active) {
        fn();
        return;
      }
      unlistenRef.current = fn;
    });

    // Poll status every STATUS_POLL_MS as a belt-and-braces fallback.
    void refreshAutoStatus();
    pollTimerRef.current = setInterval(() => {
      void refreshAutoStatus();
    }, STATUS_POLL_MS);

    return () => {
      active = false;
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
      if (pollTimerRef.current) {
        clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
    };
  }, [autoAttach, notifyAttached, refreshAutoStatus]);

  // On mount, read persisted auto-attach state.
  useEffect(() => {
    void invoke<TvAutoAttachStatus>('tv_cdp_get_auto_attach_status').then(s => {
      if (s.enabled) {
        setAutoAttach(true);
        setPort(prev => prev); // port persisted in supervisor; may differ
        notifyAttached(s.attached);
      }
      setAutoStatus(s);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------------------------------------------------------------------------
  // Auto-attach pill appearance
  // ---------------------------------------------------------------------------

  /**
   * Returns the Tailwind color class for the supervisor status dot.
   *   green  = supervisor on + attached
   *   amber  = supervisor on + retrying (retry_count > 0)
   *   red    = supervisor off or stuck (reconnect_failed as last event)
   */
  const supervisorDotClass = (() => {
    if (!autoAttach || !autoStatus?.enabled) return 'bg-red-500';
    if (autoStatus.attached) return 'bg-green-500';
    if ((autoStatus.retry_count ?? 0) > 0) return 'bg-amber-400';
    if (autoStatus.last_event === 'reconnect_failed') return 'bg-red-500';
    return 'bg-amber-400';
  })();

  const supervisorLabel = (() => {
    if (!autoAttach || !autoStatus?.enabled) return 'off';
    if (autoStatus.attached) return 'live';
    if ((autoStatus.retry_count ?? 0) > 0) return `retry ${autoStatus.retry_count}`;
    return 'waiting';
  })();

  // ---------------------------------------------------------------------------
  // Toggle auto-attach
  // ---------------------------------------------------------------------------

  const toggleAutoAttach = useCallback(async () => {
    const next = !autoAttach;
    setPending(true);
    setError(null);
    try {
      await invoke('tv_cdp_set_auto_attach', { enabled: next, port });
      setAutoAttach(next);
      await refreshAutoStatus();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Auto-attach toggle failed: ${msg}`);
    } finally {
      setPending(false);
    }
  }, [autoAttach, port, refreshAutoStatus]);

  // ---------------------------------------------------------------------------
  // Manual commands
  // ---------------------------------------------------------------------------

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
      notifyAttached(true);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Attach failed: ${msg}`);
      notifyAttached(false);
    } finally {
      setPending(false);
    }
  }, [port, notifyAttached]);

  const detach = useCallback(async () => {
    setPending(true);
    setError(null);
    try {
      await invoke('tv_cdp_detach');
      notifyAttached(false);
      setChartState(null);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Detach failed: ${msg}`);
    } finally {
      setPending(false);
    }
  }, [notifyAttached]);

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

  useEffect(() => {
    void runProbe();
  }, [runProbe]);

  return (
    <>
      {/* One-time setup */}
      <section
        data-testid="tv-bridge-setup-card"
        className="rounded-xl border border-stone-200 bg-white p-4">
        <h2 className="text-sm font-semibold text-stone-900">One-time setup</h2>
        <ol className="mt-2 list-inside list-decimal space-y-1 text-xs text-stone-600">
          <li>Quit TradingView Desktop if it&apos;s running.</li>
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

      {/* Connection card */}
      <section
        data-testid="tv-bridge-probe-card"
        className="rounded-xl border border-stone-200 bg-white p-4">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold text-stone-900">Connection</h2>
          <div className="flex items-center gap-2">
            {/* Supervisor status pill — only shown when auto-attach is on */}
            {autoAttach && (
              <span
                data-testid="tv-bridge-supervisor-pill"
                className="flex items-center gap-1 rounded-full border border-stone-200 px-2 py-0.5 text-[10px] font-medium text-stone-600">
                <span
                  data-testid="tv-bridge-supervisor-dot"
                  className={`inline-block h-1.5 w-1.5 rounded-full ${supervisorDotClass}`}
                />
                {supervisorLabel}
              </span>
            )}
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
        </div>

        {/* Auto-attach toggle row */}
        <div className="mt-3 flex items-center gap-2">
          <label
            className="flex cursor-pointer items-center gap-1.5 text-xs text-stone-600"
            htmlFor="tv-auto-attach-toggle">
            <input
              id="tv-auto-attach-toggle"
              type="checkbox"
              checked={autoAttach}
              onChange={() => void toggleAutoAttach()}
              disabled={pending}
              data-testid="tv-bridge-auto-attach-toggle"
              className="h-3.5 w-3.5 accent-primary-500 disabled:cursor-not-allowed"
            />
            Auto-attach
          </label>
          <span className="text-[10px] text-stone-400">
            {autoAttach
              ? 'Supervisor running — reconnects automatically on TV reload.'
              : 'Off — set it once, always works.'}
          </span>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2">
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
          {/* When auto-attach is on, the Attach button becomes Force re-attach */}
          <button
            type="button"
            onClick={() => void attach()}
            disabled={pending || (!autoAttach && (attached || !probe?.reachable))}
            data-testid="tv-bridge-attach-button"
            className="shrink-0 rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
            {autoAttach ? 'Force re-attach now' : 'Attach'}
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

      {/* Chart state */}
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
              Not read yet. Click &quot;Read now&quot; to fetch live state.
            </p>
          )}
        </section>
      ) : null}

      {/* Switch symbol */}
      {attached ? (
        <section
          data-testid="tv-bridge-write-card"
          className="rounded-xl border border-stone-200 bg-white p-4">
          <h2 className="text-sm font-semibold text-stone-900">Switch symbol</h2>
          <p className="mt-1 text-[11px] text-stone-500">
            Writes the active chart&apos;s symbol via TV&apos;s internal API. Use exchange prefixes
            when ambiguous (e.g. <code>CME_MINI:NQ1!</code>, <code>NASDAQ:AAPL</code>).
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
    </>
  );
};

export default TvBridgePanelBody;
