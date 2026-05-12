/**
 * useOrderFlow — React hook for order-flow live state and config management.
 *
 * Polls tv_cdp_get_order_flow_state() every 2 seconds (2 Hz cap) while the
 * TV bridge is attached. Falls back to manual bar state when the CDP path
 * returns null or an error.
 *
 * Observation + journaling only. No order-placement logic here.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

import type {
  OrderFlowConfig,
  OrderFlowState,
  OrderFlowStateResult,
  OrderFlowTagValue,
} from '../types/orderFlow';

const POLL_INTERVAL_MS = 2000; // 2 Hz — hard cap per spec

const DEFAULT_CONFIG: OrderFlowConfig = {
  enabled: true,
  poll_interval_ms: POLL_INTERVAL_MS,
  active_preset: null,
  alert_toggles: { delta_divergence: false, absorption: false, single_print_rejection: false },
};

export interface UseOrderFlowReturn {
  /** Current order-flow state (live or manual). */
  state: OrderFlowState | null;
  /** Whether the last CDP poll succeeded. */
  cdpLive: boolean;
  /** Persisted config. */
  config: OrderFlowConfig;
  /** True while the first load is in progress. */
  loading: boolean;
  /** Last error string from any operation. */
  error: string | null;
  /** Manually set bid/ask volumes for the current bar (when CDP unavailable). */
  setManualBar: (bidVol: number, askVol: number) => void;
  /** Apply a named workspace preset via Tauri command. */
  applyPreset: (presetId: string) => Promise<void>;
  /** Tag the active trade. */
  tagActiveTrade: (tag: OrderFlowTagValue) => Promise<void>;
  /** Toggle one alert pattern and persist. */
  toggleAlert: (key: keyof OrderFlowConfig['alert_toggles']) => Promise<void>;
  /** Save config to disk. */
  saveConfig: (patch: Partial<OrderFlowConfig>) => Promise<void>;
}

export function useOrderFlow(tvAttached: boolean): UseOrderFlowReturn {
  const [state, setState] = useState<OrderFlowState | null>(null);
  const [cdpLive, setCdpLive] = useState(false);
  const [config, setConfig] = useState<OrderFlowConfig>(DEFAULT_CONFIG);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Track cumulative delta across manual entries
  const cumulativeDeltaRef = useRef<number>(0);

  // ---------------------------------------------------------------------------
  // Load config on mount
  // ---------------------------------------------------------------------------
  useEffect(() => {
    let cancelled = false;
    async function loadConfig() {
      try {
        const result = await invoke<{ ok: boolean; config: OrderFlowConfig | null }>(
          'order_flow_get_config'
        );
        if (!cancelled && result.ok && result.config) {
          setConfig(result.config);
        }
      } catch {
        // Command may not exist yet (assist agent wires it) — use defaults
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    void loadConfig();
    return () => {
      cancelled = true;
    };
  }, []);

  // ---------------------------------------------------------------------------
  // CDP polling loop — 2 Hz while attached
  // ---------------------------------------------------------------------------
  useEffect(() => {
    if (!tvAttached || !config.enabled) {
      setCdpLive(false);
      return;
    }

    let cancelled = false;

    async function poll() {
      try {
        const result = await invoke<OrderFlowStateResult>('tv_cdp_get_order_flow_state');
        if (cancelled) return;
        if (result.ok && result.state) {
          setState(result.state);
          setCdpLive(true);
          // Sync cumulative delta ref from CDP
          cumulativeDeltaRef.current = result.state.cumulative_delta ?? 0;
        } else {
          setCdpLive(false);
          if (result.error) {
            setError(result.error);
          }
        }
      } catch {
        if (!cancelled) setCdpLive(false);
      }
    }

    void poll();
    const id = window.setInterval(() => void poll(), POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [tvAttached, config.enabled]);

  // ---------------------------------------------------------------------------
  // Manual bar entry
  // ---------------------------------------------------------------------------
  const setManualBar = useCallback((bidVol: number, askVol: number) => {
    const barDelta = askVol - bidVol;
    cumulativeDeltaRef.current += barDelta;
    const now = new Date().toISOString();
    setState({
      last_read_at: now,
      source: 'manual',
      bar: {
        bar_delta: barDelta,
        bid_volume: bidVol,
        ask_volume: askVol,
        total_volume: bidVol + askVol,
      },
      cumulative_delta: cumulativeDeltaRef.current,
      vah: null,
      val: null,
      poc: null,
      cdp_error: null,
    });
    setCdpLive(false);
  }, []);

  // ---------------------------------------------------------------------------
  // Preset apply
  // ---------------------------------------------------------------------------
  const applyPreset = useCallback(async (presetId: string) => {
    setError(null);
    try {
      const result = await invoke<{ ok: boolean; error: string | null }>(
        'order_flow_apply_preset',
        { name: presetId }
      );
      if (!result.ok) {
        setError(result.error ?? 'Preset apply failed');
      } else {
        setConfig(prev => ({ ...prev, active_preset: presetId }));
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  // ---------------------------------------------------------------------------
  // Tag active trade
  // ---------------------------------------------------------------------------
  const tagActiveTrade = useCallback(async (tag: OrderFlowTagValue) => {
    setError(null);
    try {
      const result = await invoke<{ ok: boolean; error: string | null }>(
        'order_flow_tag_active_trade',
        { tag }
      );
      if (!result.ok) {
        setError(result.error ?? 'Tagging failed');
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  // ---------------------------------------------------------------------------
  // Alert toggles
  // ---------------------------------------------------------------------------
  const toggleAlert = useCallback(
    async (key: keyof OrderFlowConfig['alert_toggles']) => {
      setError(null);
      const next: OrderFlowConfig = {
        ...config,
        alert_toggles: { ...config.alert_toggles, [key]: !config.alert_toggles[key] },
      };
      setConfig(next);
      try {
        await invoke('order_flow_set_config', { config: next });
      } catch {
        // Non-fatal — config reverts on next load
      }
    },
    [config]
  );

  // ---------------------------------------------------------------------------
  // Save config
  // ---------------------------------------------------------------------------
  const saveConfig = useCallback(
    async (patch: Partial<OrderFlowConfig>) => {
      setError(null);
      const next = { ...config, ...patch };
      setConfig(next);
      try {
        await invoke('order_flow_set_config', { config: next });
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [config]
  );

  return {
    state,
    cdpLive,
    config,
    loading,
    error,
    setManualBar,
    applyPreset,
    tagActiveTrade,
    toggleAlert,
    saveConfig,
  };
}
