/**
 * TK's Mods — consolidated trading settings hub.
 *
 * All trading mods and AI add-ons in one scrolling page.
 * Sections (in order):
 *
 *   1. AI Mode             — mode picker + mascot hotkey (ModesPanelBody)
 *   2. TradingView bridge  — CDP probe/attach/state/symbol (TvBridgePanelBody)
 *   3. Order Flow          — order-flow card
 *   4. TradingView Overlay — in-TV injection panel
 *   5. SL/TP overlay       — draw stop/target horizontal lines on chart
 *   6. Position size calc  — entry/stop/risk → contracts (compute_position_size)
 *   7. Pre-trade checklist — editable checklist, confirm-setup gate
 *   8. Symbol favorites    — quick-switch symbols via TV bridge
 *   9. Walk-away lockout   — daily-loss / consecutive-loss trip + arm-reset UI
 *  10. Theme               — default vs ZETH
 *  11. Risk-hide toggle    — $/% redaction in Whiskey messages
 *
 * Lockout banner renders at the TOP if currently locked.
 *
 * Style conventions match the rest of the settings panels:
 *   - rounded-xl border border-stone-200 bg-white p-4 cards
 *   - primary-500 / primary-600 action buttons
 *   - role="alert" + data-testid for error regions
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';

import { THEMES, useTheme } from '../../../hooks/useTheme';
import type { InjectResult, OverlayStatus } from '../../../types/overlay';
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import ModesPanelBody from './ModesPanelBody';
import OrderFlowCard from './OrderFlowCard';
import TvBridgePanelBody from './TvBridgePanelBody';

// ---------------------------------------------------------------------------
// Types shared with Rust commands
// ---------------------------------------------------------------------------

interface TvSltpResult {
  ok: boolean;
  removed: number | null;
  error: string | null;
}

interface SizingResult {
  contracts: number;
  actual_risk_dollars: number;
  risk_per_contract: number;
  error: string | null;
}

interface LockoutConfig {
  max_daily_loss_dollars: number | null;
  max_consecutive_losses: number | null;
  cooldown_minutes: number;
}

interface LockoutStatus {
  is_locked: boolean;
  locked_until_unix: number | null;
  lock_reason: string | null;
  daily_loss_dollars: number;
  consecutive_losses: number;
  config: LockoutConfig;
  /** Unix timestamp until which the arm-reset cooldown runs. null = not armed. */
  armed_for_reset_until: number | null;
}

type TvCdpStatusKind = 'attached' | 'detached' | 'reattached' | 'reconnect_failed';
interface TvCdpStatusEvent {
  kind: TvCdpStatusKind;
}

interface ChecklistItem {
  id: string;
  label: string;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const RISK_HIDE_KEY = 'tk-hide-risk-pct';
const MAX_FAVORITES = 20;

const SPEC_IDS = ['MNQ', 'MES', 'NQ', 'ES', 'MYM', 'M2K', 'CL', 'GC', 'STOCK'] as const;
type SpecId = (typeof SPEC_IDS)[number];

const DEFAULT_CHECKLIST: ChecklistItem[] = [
  { id: 'catalog-match', label: 'Catalog match confirmed (A+ setup in playbook)' },
  { id: 'stop-defined', label: 'Stop price defined' },
  { id: 'size-calculated', label: 'Position size calculated' },
  { id: 'risk-budget', label: 'Risk fits daily budget' },
  { id: 'no-revenge', label: 'Not revenge trading (>15min since last loss)' },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function readRiskHide(): boolean {
  try {
    return localStorage.getItem(RISK_HIDE_KEY) === 'true';
  } catch {
    return false;
  }
}

function rMultiple(entry: number, stop: number, target: number): string | null {
  const risk = Math.abs(entry - stop);
  const reward = Math.abs(target - entry);
  if (risk === 0) return null;
  return (reward / risk).toFixed(2) + 'R';
}

function formatLockoutUntil(unix: number | null): string {
  if (unix === null) return '';
  const d = new Date(unix * 1000);
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function formatCountdown(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

const TksModsPanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();
  const { theme, setTheme } = useTheme();

  // TV bridge attached state (surfaced from TvBridgePanelBody via callback)
  const [tvAttached, setTvAttached] = useState(false);

  // ── SL/TP overlay ─────────────────────────────────────────────────────────
  const [entry, setEntry] = useState('');
  const [stop, setStop] = useState('');
  const [target, setTarget] = useState('');
  const [sltpPending, setSltpPending] = useState(false);
  const [sltpError, setSltpError] = useState<string | null>(null);
  const [sltpSuccess, setSltpSuccess] = useState<string | null>(null);

  const entryNum = parseFloat(entry);
  const stopNum = parseFloat(stop);
  const targetNum = parseFloat(target);
  const rLabel =
    !Number.isNaN(entryNum) && !Number.isNaN(stopNum) && !Number.isNaN(targetNum)
      ? rMultiple(entryNum, stopNum, targetNum)
      : null;

  // ── Position size calculator ───────────────────────────────────────────────
  const [psEntry, setPsEntry] = useState('');
  const [psStop, setPsStop] = useState('');
  const [psRisk, setPsRisk] = useState('');
  const [psSpec, setPsSpec] = useState<SpecId>('MNQ');
  const [psResult, setPsResult] = useState<SizingResult | null>(null);
  const [psPending, setPsPending] = useState(false);
  const [psError, setPsError] = useState<string | null>(null);

  // ── Pre-trade checklist ────────────────────────────────────────────────────
  const [checklist, setChecklist] = useState<ChecklistItem[]>(DEFAULT_CHECKLIST);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [newItemLabel, setNewItemLabel] = useState('');
  const [confirmSuccess, setConfirmSuccess] = useState(false);

  const allChecked = checklist.length > 0 && checklist.every(item => checked.has(item.id));

  // ── Symbol favorites ───────────────────────────────────────────────────────
  const [favorites, setFavorites] = useState<string[]>([]);
  const [favInput, setFavInput] = useState('');
  const [favError, setFavError] = useState<string | null>(null);
  const [favSwitchPending, setFavSwitchPending] = useState<string | null>(null);

  // ── Walk-away lockout ──────────────────────────────────────────────────────
  const [lockoutStatus, setLockoutStatus] = useState<LockoutStatus | null>(null);
  const [lockoutPending, setLockoutPending] = useState(false);
  const [forceResetArmed, setForceResetArmed] = useState(false);
  const [maxDailyLoss, setMaxDailyLoss] = useState('');
  const [maxConsecLosses, setMaxConsecLosses] = useState('');
  const [cooldownMins, setCooldownMins] = useState('60');
  const [lockoutConfigSaved, setLockoutConfigSaved] = useState(false);
  const [lockoutResetError, setLockoutResetError] = useState<string | null>(null);
  // Countdown: re-derive from lockoutStatus.armed_for_reset_until on every tick.
  const [armCountdownSecs, setArmCountdownSecs] = useState<number | null>(null);

  // ── TV bridge CDP-status dot ────────────────────────────────────────────────
  type TvBridgeDotState = 'attached' | 'reconnecting' | 'detached';
  const [tvBridgeDotState, setTvBridgeDotState] = useState<TvBridgeDotState>('detached');

  // Track whether the overlay was user-enabled so we can re-inject on reattach.
  const overlayEnabledRef = useRef(false);

  // ── TV Overlay panel ───────────────────────────────────────────────────────
  const [overlayStatus, setOverlayStatus] = useState<OverlayStatus>('not_injected');
  const [overlayPending, setOverlayPending] = useState(false);
  const [overlayError, setOverlayError] = useState<string | null>(null);

  // ── Risk-hide toggle ───────────────────────────────────────────────────────
  const [hideRisk, setHideRiskState] = useState<boolean>(readRiskHide);

  // ── Load lockout on mount ──────────────────────────────────────────────────
  const refreshLockout = useCallback(async () => {
    try {
      const s = await invoke<LockoutStatus>('lockout_status');
      setLockoutStatus(s);
      // Sync config inputs from loaded state.
      setMaxDailyLoss(s.config.max_daily_loss_dollars?.toString() ?? '');
      setMaxConsecLosses(s.config.max_consecutive_losses?.toString() ?? '');
      setCooldownMins(s.config.cooldown_minutes.toString());
    } catch {
      // Non-fatal; show unlocked state.
    }
  }, []);

  useEffect(() => {
    void refreshLockout();
  }, [refreshLockout]);

  // ── Arm-reset countdown ticker ─────────────────────────────────────────────
  // Recomputes every second while armed; clears itself when no longer armed.
  useEffect(() => {
    const armedUntil = lockoutStatus?.armed_for_reset_until ?? null;
    if (armedUntil === null) {
      setArmCountdownSecs(null);
      return;
    }
    const tick = () => {
      const remaining = armedUntil - Math.floor(Date.now() / 1000);
      setArmCountdownSecs(remaining > 0 ? remaining : 0);
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [lockoutStatus?.armed_for_reset_until]);

  // ── TV CDP-status event listener ───────────────────────────────────────────
  // Drives the status dot on the TV bridge card and triggers re-inject when
  // the supervisor fires 'reattached'.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const setup = async () => {
      const unlistenFn = await listen<TvCdpStatusEvent>('tv-cdp-status', event => {
        if (cancelled) return;
        const kind = event.payload.kind;
        if (kind === 'attached' || kind === 'reattached') {
          setTvBridgeDotState('attached');
          // Re-inject overlay if the user had it enabled and TV reloaded.
          if (kind === 'reattached' && overlayEnabledRef.current) {
            void invoke<{ ok: boolean; error: string | null }>('tv_overlay_inject').then(result => {
              if (result.ok) {
                setOverlayStatus('injected');
              }
            });
          }
        } else if (kind === 'detached' || kind === 'reconnect_failed') {
          setTvBridgeDotState('detached');
        }
      });
      if (!cancelled) {
        unlisten = unlistenFn;
      } else {
        unlistenFn?.();
      }
    };

    void setup();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // When TV bridge detaches, reflect that in overlay status.
  useEffect(() => {
    if (!tvAttached) {
      // Only downgrade to tv_not_attached if user actively lost the bridge —
      // initial render starts as not_injected (bridge was never connected).
      setOverlayStatus(prev => {
        if (prev === 'injected') return 'not_injected';
        return prev; // keep not_injected as-is; don't flip to tv_not_attached on cold start
      });
    }
  }, [tvAttached]);

  // ── Handlers: TV overlay ──────────────────────────────────────────────────

  const handleOverlayEnable = useCallback(async () => {
    if (!tvAttached) {
      setOverlayStatus('tv_not_attached');
      return;
    }
    setOverlayError(null);
    setOverlayPending(true);
    try {
      const result = await invoke<InjectResult>('tv_overlay_inject');
      if (result.ok) {
        setOverlayStatus('injected');
        overlayEnabledRef.current = true;
      } else {
        setOverlayError(result.error ?? 'Inject failed.');
      }
    } catch (err) {
      setOverlayError(err instanceof Error ? err.message : String(err));
    } finally {
      setOverlayPending(false);
    }
  }, [tvAttached]);

  const handleOverlayDisable = useCallback(async () => {
    setOverlayError(null);
    setOverlayPending(true);
    try {
      await invoke('tv_overlay_remove');
      setOverlayStatus('not_injected');
      overlayEnabledRef.current = false;
    } catch (err) {
      setOverlayError(err instanceof Error ? err.message : String(err));
    } finally {
      setOverlayPending(false);
    }
  }, []);

  const handleOverlayTestMessage = useCallback(async () => {
    setOverlayError(null);
    setOverlayPending(true);
    try {
      await invoke('tv_overlay_send_state', {
        newState: {
          favorites: favorites.length > 0 ? favorites : ['NQ1!', 'ES1!'],
          lockout: lockoutStatus ?? {
            is_locked: false,
            locked_until_unix: null,
            lock_reason: null,
            daily_loss_dollars: 0,
            consecutive_losses: 0,
          },
          default_sltp: [
            parseFloat(entry) || 0,
            parseFloat(stop) || 0,
            parseFloat(target) || 0,
          ] as [number, number, number],
          active_tag: null,
        },
      });
    } catch (err) {
      setOverlayError(err instanceof Error ? err.message : String(err));
    } finally {
      setOverlayPending(false);
    }
  }, [favorites, lockoutStatus, entry, stop, target]);

  // ── Handlers: SL/TP ───────────────────────────────────────────────────────

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

  // ── Handlers: position sizer ───────────────────────────────────────────────

  const handleComputeSize = useCallback(async () => {
    setPsError(null);
    setPsResult(null);
    const e = parseFloat(psEntry);
    const s = parseFloat(psStop);
    const r = parseFloat(psRisk);
    if (Number.isNaN(e) || Number.isNaN(s) || Number.isNaN(r)) {
      setPsError('Enter valid numbers for Entry, Stop, and Risk $.');
      return;
    }
    setPsPending(true);
    try {
      const result = await invoke<SizingResult>('compute_position_size', {
        entry: e,
        stop: s,
        riskDollars: r,
        specId: psSpec,
      });
      setPsResult(result);
      if (result.error) {
        setPsError(result.error);
      }
    } catch (err) {
      setPsError(err instanceof Error ? err.message : String(err));
    } finally {
      setPsPending(false);
    }
  }, [psEntry, psStop, psRisk, psSpec]);

  // ── Handlers: checklist ────────────────────────────────────────────────────

  const toggleCheck = useCallback((id: string) => {
    setChecked(prev => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
    setConfirmSuccess(false);
  }, []);

  const addChecklistItem = useCallback(() => {
    const label = newItemLabel.trim();
    if (!label) return;
    const id = `custom-${Date.now()}`;
    setChecklist(prev => [...prev, { id, label }]);
    setNewItemLabel('');
    setConfirmSuccess(false);
  }, [newItemLabel]);

  const removeChecklistItem = useCallback((id: string) => {
    setChecklist(prev => prev.filter(item => item.id !== id));
    setChecked(prev => {
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
    setConfirmSuccess(false);
  }, []);

  const moveItem = useCallback((index: number, direction: -1 | 1) => {
    setChecklist(prev => {
      const arr = [...prev];
      const target = index + direction;
      if (target < 0 || target >= arr.length) return arr;
      [arr[index], arr[target]] = [arr[target], arr[index]];
      return arr;
    });
  }, []);

  const handleConfirmSetup = useCallback(() => {
    if (!allChecked) return;
    const ts = new Date().toISOString();
    console.info(`[tks-mods] Pre-trade checklist passed at ${ts}`);
    setConfirmSuccess(true);
    setChecked(new Set());
  }, [allChecked]);

  // ── Handlers: symbol favorites ─────────────────────────────────────────────

  const addFavorite = useCallback(() => {
    const sym = favInput.trim().toUpperCase();
    if (!sym) return;
    if (favorites.length >= MAX_FAVORITES) {
      setFavError(`Maximum ${MAX_FAVORITES} favorites — remove one before adding.`);
      return;
    }
    if (favorites.includes(sym)) {
      setFavError(`${sym} is already in your favorites.`);
      return;
    }
    setFavError(null);
    setFavorites(prev => [...prev, sym]);
    setFavInput('');
  }, [favInput, favorites]);

  const removeFavorite = useCallback((sym: string) => {
    setFavError(null);
    setFavorites(prev => prev.filter(s => s !== sym));
  }, []);

  const switchToFavorite = useCallback(
    async (sym: string) => {
      if (!tvAttached) return;
      setFavSwitchPending(sym);
      setFavError(null);
      try {
        const result = await invoke<{ ok: boolean; symbol: string | null; error: string | null }>(
          'tv_cdp_set_symbol',
          { symbol: sym }
        );
        if (!result.ok) {
          setFavError(result.error ?? `Failed to switch to ${sym}.`);
        }
      } catch (err) {
        setFavError(err instanceof Error ? err.message : String(err));
      } finally {
        setFavSwitchPending(null);
      }
    },
    [tvAttached]
  );

  // ── Handlers: lockout ──────────────────────────────────────────────────────

  const handleManualTrip = useCallback(async () => {
    setLockoutPending(true);
    try {
      const s = await invoke<LockoutStatus>('lockout_trip', { reason: 'Manual walk-away' });
      setLockoutStatus(s);
    } catch {
      // silently ignore
    } finally {
      setLockoutPending(false);
    }
  }, []);

  const handleArmReset = useCallback(async () => {
    setLockoutPending(true);
    setLockoutResetError(null);
    try {
      const s = await invoke<LockoutStatus>('lockout_arm_reset');
      setLockoutStatus(s);
    } catch {
      // silently ignore
    } finally {
      setLockoutPending(false);
    }
  }, []);

  const handleForceReset = useCallback(async () => {
    setLockoutPending(true);
    setLockoutResetError(null);
    try {
      const s = await invoke<LockoutStatus>('lockout_reset');
      setLockoutStatus(s);
      setForceResetArmed(false);
    } catch (err) {
      // lockout_reset returns Result<LockoutStatus, String> — the Err payload
      // is a human-readable message from request_force_reset (e.g. "Reset armed
      // but cooldown active. 243 seconds remaining.").
      const msg = err instanceof Error ? err.message : String(err);
      setLockoutResetError(msg.includes('cooldown') ? 'Wait until cooldown ends.' : msg);
    } finally {
      setLockoutPending(false);
    }
  }, []);

  const handleSaveLockoutConfig = useCallback(async () => {
    setLockoutPending(true);
    setLockoutConfigSaved(false);
    const mdl = maxDailyLoss.trim() === '' ? null : parseFloat(maxDailyLoss);
    const mcl = maxConsecLosses.trim() === '' ? null : parseInt(maxConsecLosses, 10);
    const cm = parseInt(cooldownMins, 10) || 60;
    try {
      const s = await invoke<LockoutStatus>('lockout_set_config', {
        maxDailyLossDollars: Number.isNaN(mdl as number) ? null : mdl,
        maxConsecutiveLosses: Number.isNaN(mcl as number) ? null : mcl,
        cooldownMinutes: cm,
      });
      setLockoutStatus(s);
      setLockoutConfigSaved(true);
    } catch {
      // silently ignore
    } finally {
      setLockoutPending(false);
    }
  }, [maxDailyLoss, maxConsecLosses, cooldownMins]);

  // ── Handler: risk-hide toggle ──────────────────────────────────────────────

  const handleRiskHideToggle = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const next = e.target.checked;
    try {
      localStorage.setItem(RISK_HIDE_KEY, String(next));
    } catch {
      // ignore
    }
    setHideRiskState(next);
    // Notify the overlay bus cache so the risk-sanitizer picks up the new
    // value immediately — without this, publish_attention keeps using the
    // stale cached value until the next app restart.
    void invoke('tks_mods_invalidate_cache');
  }, []);

  // ── Handler: theme ─────────────────────────────────────────────────────────

  const handleThemeChange = useCallback(
    (e: React.ChangeEvent<HTMLSelectElement>) => {
      setTheme(e.target.value as 'default' | 'zeth');
    },
    [setTheme]
  );

  // ── Render ─────────────────────────────────────────────────────────────────

  const isLocked = lockoutStatus?.is_locked ?? false;

  return (
    <div className="flex h-full w-full flex-col bg-stone-50">
      <SettingsHeader breadcrumbs={breadcrumbs} onBack={navigateBack} title="TK's Mods" />

      {/* ── Lockout banner (shown at top when locked) ──────────────────────── */}
      {isLocked ? (
        <div
          data-testid="tks-mods-lockout-banner"
          role="alert"
          className="mx-6 mt-4 rounded-xl border border-red-300 bg-red-50 p-3">
          <p className="text-xs font-semibold text-red-800">
            Locked out until {formatLockoutUntil(lockoutStatus?.locked_until_unix ?? null)} —
            Whiskey will not surface setups.
          </p>
          {lockoutStatus?.lock_reason ? (
            <p className="mt-1 text-[11px] text-red-700">{lockoutStatus.lock_reason}</p>
          ) : null}
        </div>
      ) : null}

      <div className="flex-1 space-y-6 overflow-y-auto p-6">
        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 1 — AI Mode
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            AI Mode
          </h3>
          <div className="space-y-3">
            <ModesPanelBody />
          </div>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 2 — TradingView Bridge
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            TradingView Bridge
          </h3>
          <div className="space-y-3">
            {/* CDP-status dot — driven by listen('tv-cdp-status') events */}
            <div className="flex items-center gap-2">
              <span
                data-testid="tks-mods-tv-bridge-status-dot"
                className={`inline-block h-2 w-2 rounded-full ${
                  tvBridgeDotState === 'attached'
                    ? 'bg-green-500'
                    : tvBridgeDotState === 'reconnecting'
                      ? 'bg-amber-400'
                      : 'bg-red-400'
                }`}
              />
              <span className="text-[11px] text-stone-500">
                {tvBridgeDotState === 'attached'
                  ? 'Attached'
                  : tvBridgeDotState === 'reconnecting'
                    ? 'Reconnecting…'
                    : 'Detached'}
              </span>
            </div>
            <TvBridgePanelBody onAttachedChange={setTvAttached} />
          </div>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 3 — Order Flow
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            Order Flow
          </h3>
          <div className="space-y-3">
            <OrderFlowCard tvAttached={tvAttached} />
          </div>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 4 — TradingView Overlay Panel (in-TV injection)
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            TradingView Overlay
          </h3>
          <section
            data-testid="tks-mods-tv-overlay-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">In-chart overlay panel</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              Injects a floating panel directly into TradingView Desktop — symbol favorites, quick
              SL/TP, order-flow tags, and lockout banner. Requires the TV bridge to be attached.
            </p>

            {/* Status indicator */}
            <div className="mt-3 flex items-center gap-2">
              <span
                data-testid="tks-mods-overlay-status-dot"
                className={`inline-block h-2 w-2 rounded-full ${
                  overlayStatus === 'injected'
                    ? 'bg-green-500'
                    : overlayStatus === 'tv_not_attached'
                      ? 'bg-amber-400'
                      : 'bg-stone-300'
                }`}
              />
              <span
                data-testid="tks-mods-overlay-status-label"
                className="text-[11px] text-stone-600">
                {overlayStatus === 'injected'
                  ? 'Panel injected and active'
                  : overlayStatus === 'tv_not_attached'
                    ? 'TV bridge not attached'
                    : 'Not injected'}
              </span>
            </div>

            {/* Enable / Disable toggle */}
            <div className="mt-3 flex flex-wrap gap-2">
              {overlayStatus !== 'injected' ? (
                <button
                  type="button"
                  onClick={() => void handleOverlayEnable()}
                  disabled={overlayPending || !tvAttached}
                  data-testid="tks-mods-overlay-enable"
                  className="rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
                  {overlayPending ? 'Working…' : 'Enable overlay'}
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => void handleOverlayDisable()}
                  disabled={overlayPending}
                  data-testid="tks-mods-overlay-disable"
                  className="rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
                  {overlayPending ? 'Working…' : 'Remove overlay'}
                </button>
              )}
              {overlayStatus === 'injected' ? (
                <button
                  type="button"
                  onClick={() => void handleOverlayTestMessage()}
                  disabled={overlayPending}
                  data-testid="tks-mods-overlay-test"
                  className="rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
                  Test message
                </button>
              ) : null}
            </div>

            {overlayError ? (
              <div
                role="alert"
                data-testid="tks-mods-overlay-error"
                className="mt-3 rounded-md border border-red-200 bg-red-50 p-2 text-xs text-red-800">
                {overlayError}
              </div>
            ) : null}

            <p className="mt-3 text-[11px] text-stone-400">
              The panel disappears when Whiskey is closed. Position is saved to TV&apos;s
              localStorage (per TV account).
            </p>
          </section>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 5 — SL/TP Overlay
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            SL/TP Overlay
          </h3>
          <section
            data-testid="tks-mods-sltp-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">Stop / Target lines</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              Draws native TV horizontal lines for your stop and target — works even when prop-firm
              broker panels hide the default order lines. Requires the TV bridge to be attached.
            </p>

            <div className="mt-3 grid grid-cols-3 gap-2">
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

            {sltpError ? (
              <div
                role="alert"
                data-testid="tks-mods-error"
                className="mt-3 rounded-md border border-red-200 bg-red-50 p-2 text-xs text-red-800">
                {sltpError}
              </div>
            ) : null}
          </section>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 6 — Position Size Calculator
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            Position Size Calculator
          </h3>
          <section
            data-testid="tks-mods-position-sizer-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">Size my position</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              Floors to whole contracts — never rounds up and never overruns your risk budget.
            </p>

            <div className="mt-3 grid grid-cols-2 gap-2">
              <div className="flex flex-col gap-1">
                <label htmlFor="ps-entry" className="text-[11px] text-stone-500">
                  Entry price
                </label>
                <input
                  id="ps-entry"
                  type="number"
                  step="any"
                  value={psEntry}
                  onChange={e => setPsEntry(e.target.value)}
                  disabled={psPending}
                  placeholder="e.g. 19800"
                  data-testid="tks-mods-ps-entry"
                  className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
                />
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="ps-stop" className="text-[11px] text-stone-500">
                  Stop price
                </label>
                <input
                  id="ps-stop"
                  type="number"
                  step="any"
                  value={psStop}
                  onChange={e => setPsStop(e.target.value)}
                  disabled={psPending}
                  placeholder="e.g. 19750"
                  data-testid="tks-mods-ps-stop"
                  className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
                />
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="ps-risk" className="text-[11px] text-stone-500">
                  Risk $ (per trade)
                </label>
                <input
                  id="ps-risk"
                  type="number"
                  step="any"
                  min="0"
                  value={psRisk}
                  onChange={e => setPsRisk(e.target.value)}
                  disabled={psPending}
                  placeholder="e.g. 200"
                  data-testid="tks-mods-ps-risk"
                  className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
                />
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="ps-spec" className="text-[11px] text-stone-500">
                  Instrument
                </label>
                <select
                  id="ps-spec"
                  value={psSpec}
                  onChange={e => setPsSpec(e.target.value as SpecId)}
                  disabled={psPending}
                  data-testid="tks-mods-ps-spec"
                  className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400">
                  {SPEC_IDS.map(id => (
                    <option key={id} value={id}>
                      {id}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <button
              type="button"
              onClick={() => void handleComputeSize()}
              disabled={psPending}
              data-testid="tks-mods-ps-compute"
              className="mt-3 rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
              {psPending ? 'Calculating…' : 'Calculate'}
            </button>

            {psResult && !psResult.error ? (
              <div
                data-testid="tks-mods-ps-result"
                className="mt-3 rounded-md border border-green-200 bg-green-50 p-3">
                <p className="text-sm font-semibold text-green-900">
                  <span data-testid="tks-mods-ps-contracts">{psResult.contracts}</span> contract
                  {psResult.contracts !== 1 ? 's' : ''}
                </p>
                <p className="mt-0.5 text-[11px] text-green-700">
                  Actual risk: ${psResult.actual_risk_dollars.toFixed(2)} ( $
                  {psResult.risk_per_contract.toFixed(2)}/contract)
                </p>
              </div>
            ) : null}

            {psError ? (
              <div
                role="alert"
                data-testid="tks-mods-ps-error"
                className="mt-3 rounded-md border border-red-200 bg-red-50 p-2 text-xs text-red-800">
                {psError}
              </div>
            ) : null}
          </section>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 7 — Pre-trade Checklist
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            Pre-trade Checklist
          </h3>
          <section
            data-testid="tks-mods-checklist-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">Confirm setup</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              Check every item before entering. &quot;Confirm setup&quot; logs the all-clear and
              resets the checkboxes.
            </p>

            <ul className="mt-3 space-y-2" data-testid="tks-mods-checklist-list">
              {checklist.map((item, idx) => (
                <li key={item.id} className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    id={`cl-${item.id}`}
                    checked={checked.has(item.id)}
                    onChange={() => toggleCheck(item.id)}
                    data-testid={`tks-mods-cl-check-${item.id}`}
                    className="h-4 w-4 rounded border-stone-300 text-primary-500 focus:ring-primary-500"
                  />
                  <label
                    htmlFor={`cl-${item.id}`}
                    className="flex-1 text-xs text-stone-700"
                    data-testid={`tks-mods-cl-label-${item.id}`}>
                    {item.label}
                  </label>
                  <button
                    type="button"
                    onClick={() => moveItem(idx, -1)}
                    disabled={idx === 0}
                    aria-label="Move up"
                    className="shrink-0 rounded px-1 py-0.5 text-[11px] text-stone-400 hover:text-stone-700 disabled:opacity-30">
                    ▲
                  </button>
                  <button
                    type="button"
                    onClick={() => moveItem(idx, 1)}
                    disabled={idx === checklist.length - 1}
                    aria-label="Move down"
                    className="shrink-0 rounded px-1 py-0.5 text-[11px] text-stone-400 hover:text-stone-700 disabled:opacity-30">
                    ▼
                  </button>
                  <button
                    type="button"
                    onClick={() => removeChecklistItem(item.id)}
                    aria-label={`Remove "${item.label}"`}
                    data-testid={`tks-mods-cl-remove-${item.id}`}
                    className="shrink-0 rounded px-1 py-0.5 text-[11px] text-stone-400 hover:text-red-600">
                    ✕
                  </button>
                </li>
              ))}
            </ul>

            {/* Add item */}
            <div className="mt-3 flex items-center gap-2">
              <input
                type="text"
                value={newItemLabel}
                onChange={e => setNewItemLabel(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter') addChecklistItem();
                }}
                placeholder="Add a checklist item…"
                data-testid="tks-mods-cl-new-input"
                className="flex-1 rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500"
              />
              <button
                type="button"
                onClick={addChecklistItem}
                disabled={newItemLabel.trim().length === 0}
                data-testid="tks-mods-cl-add"
                className="shrink-0 rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
                Add
              </button>
            </div>

            {confirmSuccess ? (
              <p
                data-testid="tks-mods-cl-confirm-success"
                className="mt-3 text-[11px] text-green-700">
                Pre-trade checklist passed at {new Date().toLocaleTimeString()}.
              </p>
            ) : null}

            <button
              type="button"
              onClick={handleConfirmSetup}
              disabled={!allChecked}
              data-testid="tks-mods-cl-confirm"
              className="mt-3 rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
              Confirm setup
            </button>
          </section>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 8 — Symbol Favorites
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            Symbol Favorites
          </h3>
          <section
            data-testid="tks-mods-favorites-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">Quick-switch symbols</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              Click a symbol to switch the active TV chart. Only active when the TV bridge is
              attached. Max {MAX_FAVORITES} entries.
            </p>

            {!tvAttached ? (
              <p className="mt-2 text-[11px] text-amber-700">
                TV bridge not attached — connect in the section above to enable switching.
              </p>
            ) : null}

            {favorites.length > 0 ? (
              <div className="mt-3 flex flex-wrap gap-2" data-testid="tks-mods-favorites-list">
                {favorites.map(sym => (
                  <div key={sym} className="flex items-center gap-1">
                    <button
                      type="button"
                      onClick={() => void switchToFavorite(sym)}
                      disabled={!tvAttached || favSwitchPending === sym}
                      data-testid={`tks-mods-fav-btn-${sym}`}
                      className="rounded-md border border-stone-200 bg-white px-2.5 py-1 text-xs font-mono font-medium text-stone-700 hover:bg-primary-50 hover:border-primary-300 disabled:cursor-not-allowed disabled:opacity-50">
                      {favSwitchPending === sym ? '…' : sym}
                    </button>
                    <button
                      type="button"
                      onClick={() => removeFavorite(sym)}
                      aria-label={`Remove ${sym}`}
                      data-testid={`tks-mods-fav-remove-${sym}`}
                      className="rounded px-0.5 text-[11px] text-stone-300 hover:text-red-500">
                      ✕
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <p className="mt-2 text-[11px] text-stone-400">
                No favorites yet — add your first symbol below.
              </p>
            )}

            <div className="mt-3 flex items-center gap-2">
              <input
                type="text"
                value={favInput}
                onChange={e => setFavInput(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter') addFavorite();
                }}
                placeholder="CME_MINI:NQ1!"
                maxLength={64}
                data-testid="tks-mods-fav-input"
                className="flex-1 rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500"
              />
              <button
                type="button"
                onClick={addFavorite}
                disabled={favInput.trim().length === 0}
                data-testid="tks-mods-fav-add"
                className="shrink-0 rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
                Add
              </button>
            </div>

            {favError ? (
              <div
                role="alert"
                data-testid="tks-mods-fav-error"
                className="mt-2 rounded-md border border-red-200 bg-red-50 p-2 text-xs text-red-800">
                {favError}
              </div>
            ) : null}
          </section>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 9 — Walk-away Lockout
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            Walk-away Lockout
          </h3>
          <section
            data-testid="tks-mods-lockout-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">Risk governor</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              Survives app restarts — you cannot dodge the lockout by relaunching. Set your limits
              before you start trading.
            </p>

            {/* Current state */}
            <div className="mt-3 rounded-md bg-stone-50 px-3 py-2 text-[11px] text-stone-600">
              Today:{' '}
              <span data-testid="tks-mods-lockout-daily">
                -${lockoutStatus?.daily_loss_dollars.toFixed(2) ?? '0.00'}
              </span>{' '}
              /{' '}
              <span data-testid="tks-mods-lockout-consec">
                {lockoutStatus?.consecutive_losses ?? 0} losses
              </span>{' '}
              /{' '}
              <span
                data-testid="tks-mods-lockout-state"
                className={isLocked ? 'font-semibold text-red-700' : 'text-green-700'}>
                {isLocked
                  ? `locked until ${formatLockoutUntil(lockoutStatus?.locked_until_unix ?? null)}`
                  : 'unlocked'}
              </span>
            </div>

            {/* Threshold config */}
            <div className="mt-3 grid grid-cols-3 gap-2">
              <div className="flex flex-col gap-1">
                <label htmlFor="lo-max-dl" className="text-[11px] text-stone-500">
                  Max daily loss $
                </label>
                <input
                  id="lo-max-dl"
                  type="number"
                  step="any"
                  min="0"
                  value={maxDailyLoss}
                  onChange={e => setMaxDailyLoss(e.target.value)}
                  disabled={lockoutPending}
                  placeholder="e.g. 300"
                  data-testid="tks-mods-lockout-max-dl"
                  className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50"
                />
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="lo-max-cl" className="text-[11px] text-stone-500">
                  Max consec. losses
                </label>
                <input
                  id="lo-max-cl"
                  type="number"
                  min="1"
                  step="1"
                  value={maxConsecLosses}
                  onChange={e => setMaxConsecLosses(e.target.value)}
                  disabled={lockoutPending}
                  placeholder="e.g. 3"
                  data-testid="tks-mods-lockout-max-cl"
                  className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50"
                />
              </div>
              <div className="flex flex-col gap-1">
                <label htmlFor="lo-cooldown" className="text-[11px] text-stone-500">
                  Cooldown (min)
                </label>
                <input
                  id="lo-cooldown"
                  type="number"
                  min="1"
                  step="1"
                  value={cooldownMins}
                  onChange={e => setCooldownMins(e.target.value)}
                  disabled={lockoutPending}
                  placeholder="60"
                  data-testid="tks-mods-lockout-cooldown"
                  className="rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50"
                />
              </div>
            </div>

            <div className="mt-3 flex flex-wrap gap-2">
              <button
                type="button"
                onClick={() => void handleSaveLockoutConfig()}
                disabled={lockoutPending}
                data-testid="tks-mods-lockout-save-config"
                className="rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:bg-stone-300">
                Save limits
              </button>
              {lockoutConfigSaved ? (
                <span className="self-center text-[11px] text-green-700">Saved.</span>
              ) : null}
            </div>

            <div className="mt-4 border-t border-stone-100 pt-3">
              <p className="text-[11px] font-medium text-stone-600">Manual controls</p>
              <div className="mt-2 flex flex-wrap gap-2">
                <button
                  type="button"
                  onClick={() => void handleManualTrip()}
                  disabled={lockoutPending || isLocked}
                  data-testid="tks-mods-lockout-trip"
                  className="rounded-md border border-red-200 bg-red-50 px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-100 disabled:cursor-not-allowed disabled:opacity-50">
                  Trip lockout now
                </button>
              </div>

              {/* Arm-reset UI — only shown when locked */}
              {isLocked ? (
                <div
                  data-testid="tks-mods-lockout-arm-reset-section"
                  className="mt-3 rounded-md border border-stone-100 bg-stone-50 p-3">
                  <p className="text-[11px] text-stone-500">
                    Override requires a 5-minute cooldown after arming — prevents impulsive bypass
                    while in drawdown.
                  </p>

                  {/* Phase 1: not yet armed — show "Arm reset" button */}
                  {lockoutStatus?.armed_for_reset_until === null ||
                  lockoutStatus?.armed_for_reset_until === undefined ? (
                    <button
                      type="button"
                      onClick={() => void handleArmReset()}
                      disabled={lockoutPending}
                      data-testid="tks-mods-lockout-arm-reset-btn"
                      className="mt-2 rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-100 disabled:cursor-not-allowed disabled:opacity-50">
                      Arm reset (5 min cooldown)
                    </button>
                  ) : armCountdownSecs !== null && armCountdownSecs > 0 ? (
                    /* Phase 2: armed but cooldown still running — show countdown */
                    <p
                      data-testid="tks-mods-lockout-arm-countdown"
                      className="mt-2 text-[11px] font-mono text-amber-700">
                      Reset available in {formatCountdown(armCountdownSecs)}
                    </p>
                  ) : (
                    /* Phase 3: armed AND cooldown expired — show Confirm button */
                    <button
                      type="button"
                      onClick={() => void handleForceReset()}
                      disabled={lockoutPending}
                      data-testid="tks-mods-lockout-confirm-reset"
                      className="mt-2 rounded-md bg-red-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-50">
                      {lockoutPending ? 'Resetting…' : 'Confirm reset'}
                    </button>
                  )}

                  {lockoutResetError ? (
                    <p
                      role="alert"
                      data-testid="tks-mods-lockout-reset-error"
                      className="mt-2 text-[11px] text-red-700">
                      {lockoutResetError}
                    </p>
                  ) : null}
                </div>
              ) : null}
            </div>
          </section>
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 10 — Theme
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            Theme
          </h3>
          <section
            data-testid="tks-mods-theme-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">UI Theme</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              Applies to Whiskey UI surfaces only — TradingView&apos;s own UI is unaffected. Switch
              takes effect instantly with no reload required.
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
        </div>

        {/* ═══════════════════════════════════════════════════════════════════
            SECTION 11 — Risk-hide toggle
        ═══════════════════════════════════════════════════════════════════ */}
        <div>
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-stone-400">
            Psychology
          </h3>
          <section
            data-testid="tks-mods-risk-hide-card"
            className="rounded-xl border border-stone-200 bg-white p-4">
            <h2 className="text-sm font-semibold text-stone-900">Hide risk %</h2>
            <p className="mt-1 text-[11px] text-stone-500">
              When on, Whiskey replaces dollar amounts and percentages in messages with abstract
              terms (e.g. &quot;$250 risk&quot; → &quot;risk unit&quot;, &quot;0.5% account
              risk&quot; → &quot;small position&quot;). R-multiples like &quot;1.5R&quot; are
              preserved.
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
        </div>
      </div>
    </div>
  );
};

export default TksModsPanel;
