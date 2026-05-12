/**
 * KillSwitchButton — the execution e-stop.
 *
 * Red, large, fixed-position top-right of TksModsPanel. Always visible.
 * One click calls kill_switch_trigger immediately — NO confirm dialog.
 * Killing fast is the point. The research doc is unambiguous on this.
 *
 * When the kill switch is engaged:
 *   - Button turns grey and shows countdown to reset eligibility.
 *   - A reset input appears once the cooldown lapses.
 *
 * Sensitive fields (account numbers, position size) are never rendered here.
 * Only the kill status and countdown are displayed.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useState } from 'react';

interface KillSwitchStatus {
  engaged: boolean;
  engaged_at: number | null;
  trigger: string | null;
  reset_after_utc: number | null;
  seconds_until_reset: number | null;
}

function formatSecondsRemaining(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return m > 0 ? `${m}m ${s}s` : `${s}s`;
}

export default function KillSwitchButton() {
  const [status, setStatus] = useState<KillSwitchStatus>({
    engaged: false,
    engaged_at: null,
    trigger: null,
    reset_after_utc: null,
    seconds_until_reset: null,
  });
  const [resetPhrase, setResetPhrase] = useState('');
  const [resetError, setResetError] = useState<string | null>(null);
  const [isTriggering, setIsTriggering] = useState(false);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await invoke<KillSwitchStatus>('kill_switch_status');
      // Compute live countdown from reset_after_utc
      if (s.reset_after_utc !== null) {
        const remaining = s.reset_after_utc - Math.floor(Date.now() / 1000);
        s.seconds_until_reset = remaining > 0 ? remaining : 0;
      }
      setStatus(s);
    } catch {
      // Non-fatal — UI degrades gracefully
    }
  }, []);

  useEffect(() => {
    refreshStatus();
    const id = setInterval(refreshStatus, 1000);
    return () => clearInterval(id);
  }, [refreshStatus]);

  const handleKill = useCallback(async () => {
    if (status.engaged || isTriggering) return;
    setIsTriggering(true);
    try {
      await invoke('kill_switch_trigger', { reason: 'manual_button' });
      await refreshStatus();
    } catch (e) {
      // Still refresh — the state may have changed
      await refreshStatus();
    } finally {
      setIsTriggering(false);
    }
  }, [status.engaged, isTriggering, refreshStatus]);

  const handleReset = useCallback(async () => {
    setResetError(null);
    try {
      await invoke('kill_switch_request_reset', { phrase: resetPhrase });
      setResetPhrase('');
      await refreshStatus();
    } catch (e) {
      setResetError(String(e));
    }
  }, [resetPhrase, refreshStatus]);

  const canReset =
    status.engaged &&
    (status.seconds_until_reset === null || status.seconds_until_reset <= 0);

  return (
    <div
      style={{ position: 'fixed', top: 12, right: 12, zIndex: 9999 }}
      data-testid="kill-switch-container"
    >
      {/* Main kill button */}
      <button
        data-testid="kill-switch-button"
        onClick={handleKill}
        disabled={status.engaged || isTriggering}
        aria-label={status.engaged ? 'Kill switch engaged' : 'Trigger kill switch'}
        style={{
          backgroundColor: status.engaged ? '#6b7280' : '#dc2626',
          color: '#fff',
          fontWeight: 700,
          fontSize: '0.95rem',
          padding: '10px 20px',
          borderRadius: 8,
          border: 'none',
          cursor: status.engaged ? 'not-allowed' : 'pointer',
          boxShadow: status.engaged ? 'none' : '0 0 0 2px #991b1b',
          minWidth: 120,
          letterSpacing: '0.04em',
        }}
      >
        {status.engaged ? 'KILL ENGAGED' : isTriggering ? 'KILLING...' : 'KILL SWITCH'}
      </button>

      {/* Countdown when engaged and cooldown not elapsed */}
      {status.engaged && status.seconds_until_reset !== null && status.seconds_until_reset > 0 && (
        <div
          data-testid="kill-switch-countdown"
          style={{
            marginTop: 6,
            fontSize: '0.78rem',
            color: '#9ca3af',
            textAlign: 'center',
          }}
        >
          Reset eligible in {formatSecondsRemaining(status.seconds_until_reset)}
        </div>
      )}

      {/* Reset controls when cooldown elapsed */}
      {canReset && (
        <div data-testid="kill-switch-reset-panel" style={{ marginTop: 8 }}>
          <input
            data-testid="kill-switch-reset-input"
            type="text"
            value={resetPhrase}
            onChange={(e) => setResetPhrase(e.target.value)}
            placeholder="I am ready to trade"
            style={{
              width: '100%',
              padding: '4px 8px',
              borderRadius: 4,
              border: '1px solid #d1d5db',
              fontSize: '0.8rem',
            }}
          />
          <button
            data-testid="kill-switch-reset-button"
            onClick={handleReset}
            style={{
              marginTop: 4,
              width: '100%',
              backgroundColor: '#16a34a',
              color: '#fff',
              padding: '4px',
              borderRadius: 4,
              border: 'none',
              cursor: 'pointer',
              fontSize: '0.8rem',
            }}
          >
            Reset
          </button>
          {resetError && (
            <div
              data-testid="kill-switch-reset-error"
              role="alert"
              style={{ color: '#dc2626', fontSize: '0.75rem', marginTop: 4 }}
            >
              {resetError}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
