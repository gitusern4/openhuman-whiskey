/**
 * LLMProposalCard — renders a Whiskey-generated trade proposal as a card
 * that leads into the ConfirmTradeDialog when the user clicks "Review".
 *
 * Connected to `whiskey_propose_trade` Tauri command. The command returns
 * a structured ProposalShape parsed from LLM JSON output (or null if the
 * LLM did not produce a valid proposal).
 *
 * The card is intentionally read-only — it does NOT submit the order.
 * Clicking "Review Trade" opens ConfirmTradeDialog where the countdown
 * and final confirmation happen.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useState } from 'react';

import ConfirmTradeDialog from './ConfirmTradeDialog';
import type { ProposalShape } from './types';

interface Props {
  context: string;
  consecutiveLosses: number;
  onOrderConfirmed?: (orderId: string) => void;
}

export default function LLMProposalCard({ context, consecutiveLosses, onOrderConfirmed }: Props) {
  const [proposal, setProposal] = useState<ProposalShape | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showConfirm, setShowConfirm] = useState(false);

  const handlePropose = useCallback(async () => {
    setLoading(true);
    setError(null);
    setProposal(null);
    try {
      const result = await invoke<ProposalShape | null>('whiskey_propose_trade', { context });
      setProposal(result);
      if (!result) {
        setError('Whiskey did not generate a trade proposal for this context.');
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [context]);

  const directionColor =
    proposal && proposal.action.toUpperCase() === 'BUY' ? '#16a34a' : '#dc2626';

  return (
    <div data-testid="llm-proposal-card" style={{ marginBottom: 12 }}>
      <button
        data-testid="llm-proposal-trigger"
        onClick={handlePropose}
        disabled={loading}
        style={{
          padding: '8px 16px',
          background: '#4f46e5',
          color: '#fff',
          border: 'none',
          borderRadius: 6,
          cursor: loading ? 'wait' : 'pointer',
          fontWeight: 600,
          fontSize: '0.9rem',
        }}>
        {loading ? 'Whiskey thinking...' : 'Ask Whiskey: Should I take this trade?'}
      </button>

      {error && (
        <div
          role="alert"
          data-testid="llm-proposal-error"
          style={{ color: '#dc2626', fontSize: '0.82rem', marginTop: 6 }}>
          {error}
        </div>
      )}

      {proposal && !showConfirm && (
        <div
          data-testid="llm-proposal-summary"
          style={{
            marginTop: 8,
            background: '#f0f9ff',
            border: '1px solid #bae6fd',
            borderRadius: 8,
            padding: 12,
          }}>
          <div style={{ fontWeight: 700, marginBottom: 6 }}>
            <span style={{ color: directionColor }}>{proposal.action.toUpperCase()}</span>{' '}
            {proposal.instrument} × {proposal.qty}
          </div>
          <div style={{ fontSize: '0.82rem', color: '#374151', marginBottom: 4 }}>
            Stop: {proposal.stop_loss_ticks} ticks &bull; Target: {proposal.take_profit_ticks} ticks
            &bull; R: ${proposal.r_estimate_dollars.toFixed(2)}
          </div>
          <div style={{ fontSize: '0.82rem', color: '#374151', marginBottom: 8 }}>
            Confidence: {proposal.confidence_pct}%
            {proposal.playbook_match_id && (
              <span style={{ marginLeft: 6, color: '#6b7280' }}>
                [{proposal.playbook_match_id}]
              </span>
            )}
          </div>
          <button
            data-testid="llm-proposal-review-button"
            onClick={() => setShowConfirm(true)}
            style={{
              padding: '6px 14px',
              background: '#16a34a',
              color: '#fff',
              border: 'none',
              borderRadius: 5,
              cursor: 'pointer',
              fontWeight: 600,
              fontSize: '0.85rem',
            }}>
            Review Trade
          </button>
          <button
            data-testid="llm-proposal-dismiss-button"
            onClick={() => setProposal(null)}
            style={{
              marginLeft: 8,
              padding: '6px 14px',
              background: 'transparent',
              color: '#6b7280',
              border: '1px solid #d1d5db',
              borderRadius: 5,
              cursor: 'pointer',
              fontSize: '0.85rem',
            }}>
            Pass
          </button>
        </div>
      )}

      {showConfirm && proposal && (
        <ConfirmTradeDialog
          proposal={proposal}
          consecutiveLosses={consecutiveLosses}
          onConfirmed={id => {
            setShowConfirm(false);
            setProposal(null);
            onOrderConfirmed?.(id);
          }}
          onCancelled={() => setShowConfirm(false)}
        />
      )}
    </div>
  );
}
