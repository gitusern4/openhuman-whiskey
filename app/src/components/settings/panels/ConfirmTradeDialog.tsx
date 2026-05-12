/**
 * ConfirmTradeDialog — mandatory countdown before order confirmation.
 *
 * - 3-second base countdown (+1s per consecutive session loss).
 * - Confirm button DISABLED until countdown completes.
 * - Cancel button always enabled.
 * - Shows: instrument, direction, qty, entry, stop ticks, target ticks,
 *   R estimate in dollars, confidence pct, playbook match id.
 *
 * The research doc §3.2: "Modal with 3-second mandatory display before
 * Confirm button becomes active."
 *
 * Sensitive data note: proposal_hash is included for the confirm call but
 * never logged to console.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

export interface ProposalShape {
  proposal_hash: string;
  instrument: string;
  action: string;
  qty: number;
  entry_price: number | null;
  stop_loss_ticks: number;
  take_profit_ticks: number;
  r_estimate_dollars: number;
  confidence_pct: number;
  playbook_match_id: string | null;
  countdown_seconds: number;
}

interface Props {
  proposal: ProposalShape;
  consecutiveLosses: number;
  onConfirmed: (orderId: string) => void;
  onCancelled: () => void;
}

export default function ConfirmTradeDialog({
  proposal,
  consecutiveLosses,
  onConfirmed,
  onCancelled,
}: Props) {
  const totalCountdown = proposal.countdown_seconds + consecutiveLosses;
  const [remaining, setRemaining] = useState(totalCountdown);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (remaining <= 0) return;
    intervalRef.current = setInterval(() => {
      setRemaining((r) => {
        if (r <= 1) {
          clearInterval(intervalRef.current!);
          return 0;
        }
        return r - 1;
      });
    }, 1000);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  const canConfirm = remaining <= 0 && !confirming;

  const handleConfirm = useCallback(async () => {
    if (!canConfirm) return;
    setConfirming(true);
    setError(null);
    try {
      const result = await invoke<string>('confirm_bracket_order', {
        proposalHash: proposal.proposal_hash,
      });
      onConfirmed(result);
    } catch (e) {
      setError(String(e));
      setConfirming(false);
    }
  }, [canConfirm, proposal.proposal_hash, onConfirmed]);

  const directionLabel = proposal.action.toUpperCase() === 'BUY' ? 'LONG' : 'SHORT';
  const directionColor = proposal.action.toUpperCase() === 'BUY' ? '#16a34a' : '#dc2626';

  return (
    <div
      data-testid="confirm-trade-dialog"
      role="dialog"
      aria-modal="true"
      aria-label="Confirm trade"
      style={{
        position: 'fixed',
        inset: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        backgroundColor: 'rgba(0,0,0,0.6)',
        zIndex: 10000,
      }}
    >
      <div
        style={{
          background: '#fff',
          borderRadius: 12,
          padding: 24,
          minWidth: 320,
          maxWidth: 420,
          boxShadow: '0 25px 50px rgba(0,0,0,0.25)',
        }}
      >
        <h2 style={{ margin: '0 0 16px', fontSize: '1.1rem', fontWeight: 700 }}>
          Confirm Trade
        </h2>

        {/* Proposal summary */}
        <div
          data-testid="proposal-summary"
          style={{
            background: '#f9fafb',
            borderRadius: 8,
            padding: 12,
            marginBottom: 16,
            fontSize: '0.9rem',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
            <span style={{ color: '#6b7280' }}>Instrument</span>
            <span style={{ fontWeight: 600 }} data-testid="proposal-instrument">
              {proposal.instrument}
            </span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
            <span style={{ color: '#6b7280' }}>Direction</span>
            <span style={{ fontWeight: 700, color: directionColor }} data-testid="proposal-direction">
              {directionLabel}
            </span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
            <span style={{ color: '#6b7280' }}>Qty</span>
            <span data-testid="proposal-qty">{proposal.qty}</span>
          </div>
          {proposal.entry_price !== null && (
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
              <span style={{ color: '#6b7280' }}>Entry</span>
              <span data-testid="proposal-entry">{proposal.entry_price.toFixed(2)}</span>
            </div>
          )}
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
            <span style={{ color: '#6b7280' }}>Stop</span>
            <span data-testid="proposal-stop">{proposal.stop_loss_ticks} ticks</span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
            <span style={{ color: '#6b7280' }}>Target</span>
            <span data-testid="proposal-target">{proposal.take_profit_ticks} ticks</span>
          </div>
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              marginBottom: 4,
              borderTop: '1px solid #e5e7eb',
              paddingTop: 6,
              marginTop: 6,
            }}
          >
            <span style={{ color: '#6b7280' }}>R estimate</span>
            <span style={{ fontWeight: 700 }} data-testid="proposal-r-estimate">
              ${proposal.r_estimate_dollars.toFixed(2)}
            </span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
            <span style={{ color: '#6b7280' }}>Confidence</span>
            <span data-testid="proposal-confidence">{proposal.confidence_pct}%</span>
          </div>
          {proposal.playbook_match_id && (
            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span style={{ color: '#6b7280' }}>Playbook</span>
              <span
                style={{ fontSize: '0.8rem', color: '#374151' }}
                data-testid="proposal-playbook"
              >
                {proposal.playbook_match_id}
              </span>
            </div>
          )}
        </div>

        {error && (
          <div
            role="alert"
            data-testid="confirm-error"
            style={{ color: '#dc2626', fontSize: '0.8rem', marginBottom: 12 }}
          >
            {error}
          </div>
        )}

        {/* Countdown */}
        {remaining > 0 && (
          <div
            data-testid="confirm-countdown"
            style={{
              textAlign: 'center',
              fontSize: '1.4rem',
              fontWeight: 700,
              color: '#dc2626',
              marginBottom: 12,
            }}
          >
            {remaining}
          </div>
        )}

        {/* Action buttons */}
        <div style={{ display: 'flex', gap: 8 }}>
          <button
            data-testid="confirm-cancel-button"
            onClick={onCancelled}
            style={{
              flex: 1,
              padding: '10px',
              borderRadius: 6,
              border: '1px solid #d1d5db',
              background: '#fff',
              cursor: 'pointer',
              fontWeight: 600,
            }}
          >
            Cancel
          </button>
          <button
            data-testid="confirm-submit-button"
            onClick={handleConfirm}
            disabled={!canConfirm}
            style={{
              flex: 1,
              padding: '10px',
              borderRadius: 6,
              border: 'none',
              background: canConfirm ? '#16a34a' : '#9ca3af',
              color: '#fff',
              cursor: canConfirm ? 'pointer' : 'not-allowed',
              fontWeight: 700,
            }}
          >
            {confirming ? 'Submitting...' : 'Confirm'}
          </button>
        </div>
      </div>
    </div>
  );
}
