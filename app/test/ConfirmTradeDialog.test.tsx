/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * ConfirmTradeDialog — vitest unit tests.
 *
 * 6+ tests covering:
 *   - Confirm button disabled during countdown
 *   - Cancel button always enabled
 *   - Confirm button enabled after countdown
 *   - Loss-streak escalation (+1s per consecutive loss)
 *   - Confirm path calls confirm_bracket_order
 *   - Cancel path calls onCancelled without invoking confirm
 */
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import ConfirmTradeDialog from '../src/components/settings/panels/ConfirmTradeDialog';
import type { ProposalShape } from '../src/components/settings/panels/ConfirmTradeDialog';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => mockInvoke(...args) }));

// Fake timers + waitFor in the same suite leads to 30s timeouts because
// vitest's waitFor polls via setTimeout and fake timers don't advance.
// The countdown tests below use real timers + small explicit waits.

function makeProposal(overrides?: Partial<ProposalShape>): ProposalShape {
  return {
    proposal_hash: 'a'.repeat(64),
    instrument: 'MES',
    action: 'Buy',
    qty: 1,
    entry_price: 5200.25,
    stop_loss_ticks: 8,
    take_profit_ticks: 16,
    r_estimate_dollars: 10.0,
    confidence_pct: 75,
    playbook_match_id: 'orb-v2',
    countdown_seconds: 3,
    ...overrides,
  };
}

describe('ConfirmTradeDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.clearAllTimers();
    mockInvoke.mockResolvedValue('order_submitted:abc123');
  });

  it('confirm button is disabled during countdown', () => {
    render(
      <ConfirmTradeDialog
        proposal={makeProposal()}
        consecutiveLosses={0}
        onConfirmed={vi.fn()}
        onCancelled={vi.fn()}
      />
    );
    expect(screen.getByTestId('confirm-submit-button')).toBeDisabled();
  });

  it('cancel button is always enabled', () => {
    render(
      <ConfirmTradeDialog
        proposal={makeProposal()}
        consecutiveLosses={0}
        onConfirmed={vi.fn()}
        onCancelled={vi.fn()}
      />
    );
    expect(screen.getByTestId('confirm-cancel-button')).not.toBeDisabled();
  });

  it('confirm button enabled after countdown completes', async () => {
    vi.useFakeTimers();
    try {
      render(
        <ConfirmTradeDialog
          proposal={makeProposal()}
          consecutiveLosses={0}
          onConfirmed={vi.fn()}
          onCancelled={vi.fn()}
        />
      );
      expect(screen.getByTestId('confirm-submit-button')).toBeDisabled();
      act(() => vi.advanceTimersByTime(3500));
      expect(screen.getByTestId('confirm-submit-button')).not.toBeDisabled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('loss-streak escalation adds 1s per consecutive loss', () => {
    vi.useFakeTimers();
    try {
      render(
        <ConfirmTradeDialog
          proposal={makeProposal({ countdown_seconds: 3 })}
          consecutiveLosses={2}
          onConfirmed={vi.fn()}
          onCancelled={vi.fn()}
        />
      );
      // Total countdown = 3 + 2 = 5 seconds
      expect(screen.getByTestId('confirm-countdown').textContent).toBe('5');
      act(() => vi.advanceTimersByTime(3500));
      // Still disabled (4 seconds remain → now showing 2)
      expect(screen.getByTestId('confirm-submit-button')).toBeDisabled();
      act(() => vi.advanceTimersByTime(2000));
      expect(screen.getByTestId('confirm-submit-button')).not.toBeDisabled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('confirm path calls confirm_bracket_order and invokes onConfirmed', async () => {
    vi.useFakeTimers();
    const onConfirmed = vi.fn();
    render(
      <ConfirmTradeDialog
        proposal={makeProposal()}
        consecutiveLosses={0}
        onConfirmed={onConfirmed}
        onCancelled={vi.fn()}
      />
    );
    act(() => vi.advanceTimersByTime(4000));
    expect(screen.getByTestId('confirm-submit-button')).not.toBeDisabled();
    // Switch to real timers BEFORE the async invoke + waitFor block —
    // the broker call goes through Promise microtasks not setTimeout.
    vi.useRealTimers();
    await act(async () => {
      fireEvent.click(screen.getByTestId('confirm-submit-button'));
    });
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('confirm_bracket_order', {
        proposalHash: 'a'.repeat(64),
      });
      expect(onConfirmed).toHaveBeenCalledWith('order_submitted:abc123');
    });
  });

  it('cancel path calls onCancelled without invoking confirm_bracket_order', async () => {
    const onCancelled = vi.fn();
    render(
      <ConfirmTradeDialog
        proposal={makeProposal()}
        consecutiveLosses={0}
        onConfirmed={vi.fn()}
        onCancelled={onCancelled}
      />
    );
    fireEvent.click(screen.getByTestId('confirm-cancel-button'));
    expect(onCancelled).toHaveBeenCalledOnce();
    expect(mockInvoke).not.toHaveBeenCalledWith('confirm_bracket_order', expect.anything());
  });

  it('renders proposal summary fields', () => {
    render(
      <ConfirmTradeDialog
        proposal={makeProposal()}
        consecutiveLosses={0}
        onConfirmed={vi.fn()}
        onCancelled={vi.fn()}
      />
    );
    expect(screen.getByTestId('proposal-instrument').textContent).toBe('MES');
    expect(screen.getByTestId('proposal-direction').textContent).toBe('LONG');
    expect(screen.getByTestId('proposal-r-estimate').textContent).toBe('$10.00');
    expect(screen.getByTestId('proposal-confidence').textContent).toBe('75%');
  });
});
