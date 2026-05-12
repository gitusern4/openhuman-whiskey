/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * LLMProposalCard — vitest unit tests.
 *
 * 4+ tests:
 *   - Renders trigger button initially
 *   - Shows proposal card when whiskey_propose_trade returns a proposal
 *   - Shows error when propose_trade returns null
 *   - Opens ConfirmTradeDialog when "Review Trade" clicked
 *   - Dismiss hides proposal card
 */
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import LLMProposalCard from '../src/components/settings/panels/LLMProposalCard';
import type { ProposalShape } from '../src/components/settings/panels/types';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => mockInvoke(...args) }));

// Fake timers + waitFor cause 30s timeouts (waitFor's internal poll
// runs via setTimeout, doesn't advance under vi.useFakeTimers()).
// All tests in this file use waitFor, so use real timers.

const sampleProposal: ProposalShape = {
  proposal_hash: 'b'.repeat(64),
  instrument: 'MNQ',
  action: 'Sell',
  qty: 1,
  entry_price: 19800.0,
  stop_loss_ticks: 8,
  take_profit_ticks: 24,
  r_estimate_dollars: 20.0,
  confidence_pct: 82,
  playbook_match_id: 'fade-gap-v1',
  countdown_seconds: 3,
};

describe('LLMProposalCard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders trigger button initially', () => {
    render(<LLMProposalCard context="context" consecutiveLosses={0} />);
    expect(screen.getByTestId('llm-proposal-trigger')).toBeInTheDocument();
    expect(screen.queryByTestId('llm-proposal-summary')).toBeNull();
  });

  it('shows proposal card when whiskey_propose_trade returns proposal', async () => {
    mockInvoke.mockResolvedValue(sampleProposal);
    render(<LLMProposalCard context="test" consecutiveLosses={0} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId('llm-proposal-trigger'));
    });
    await waitFor(() => {
      expect(screen.getByTestId('llm-proposal-summary')).toBeInTheDocument();
    });
    expect(screen.getByTestId('llm-proposal-summary').textContent).toContain('MNQ');
  });

  it('shows error when propose_trade returns null', async () => {
    mockInvoke.mockResolvedValue(null);
    render(<LLMProposalCard context="test" consecutiveLosses={0} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId('llm-proposal-trigger'));
    });
    await waitFor(() => {
      expect(screen.getByTestId('llm-proposal-error')).toBeInTheDocument();
    });
  });

  it('opens ConfirmTradeDialog when Review Trade clicked', async () => {
    // ConfirmTradeDialog needs kill_switch_status + confirm_bracket_order
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'whiskey_propose_trade') return Promise.resolve(sampleProposal);
      if (cmd === 'kill_switch_status')
        return Promise.resolve({
          engaged: false,
          engaged_at: null,
          trigger: null,
          reset_after_utc: null,
          seconds_until_reset: null,
        });
      return Promise.resolve('order_submitted:test');
    });
    render(<LLMProposalCard context="test" consecutiveLosses={0} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId('llm-proposal-trigger'));
    });
    await waitFor(() => screen.getByTestId('llm-proposal-summary'));
    await act(async () => {
      fireEvent.click(screen.getByTestId('llm-proposal-review-button'));
    });
    expect(screen.getByTestId('confirm-trade-dialog')).toBeInTheDocument();
  });

  it('dismiss hides proposal card', async () => {
    mockInvoke.mockResolvedValue(sampleProposal);
    render(<LLMProposalCard context="test" consecutiveLosses={0} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId('llm-proposal-trigger'));
    });
    await waitFor(() => screen.getByTestId('llm-proposal-summary'));
    fireEvent.click(screen.getByTestId('llm-proposal-dismiss-button'));
    expect(screen.queryByTestId('llm-proposal-summary')).toBeNull();
  });
});
