/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * Revenge-trading UX components — vitest tests.
 *
 * 6+ tests covering:
 *   - LossCounter shows correct count
 *   - LossCounter turns red at max
 *   - DailyPnLGauge renders at 0%
 *   - DailyPnLGauge shows LOCKED at 100%
 *   - WalkAwayLockout hidden when not active
 *   - WalkAwayLockout visible with countdown when active
 */
import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import DailyPnLGauge from '../src/components/settings/panels/DailyPnLGauge';
import LossCounter from '../src/components/settings/panels/LossCounter';
import type { SessionState } from '../src/components/settings/panels/types';
import WalkAwayLockout from '../src/components/settings/panels/WalkAwayLockout';

// NOTE: don't enable fake timers globally — vitest's waitFor polls
// via setTimeout internally; with fake timers + waitFor in the same
// test the polling never advances and we time out at 30s. The two
// WalkAwayLockout tests below need real timers because they assert
// against rendered output that requires the component's setInterval
// callback to fire at least once.

function makeSession(overrides?: Partial<SessionState>): SessionState {
  return {
    daily_pnl: 0,
    session_loss_count: 0,
    consecutive_losses: 0,
    kill_engaged: false,
    walk_away_active: false,
    walk_away_ends_at: null,
    ...overrides,
  };
}

describe('LossCounter', () => {
  it('shows correct loss count', () => {
    const session = makeSession({ consecutive_losses: 1 });
    render(<LossCounter session={session} maxConsecutiveLosses={3} />);
    expect(screen.getByTestId('loss-counter-value').textContent).toContain('1 / 3');
  });

  it('shows LOCKED when at max consecutive losses', () => {
    const session = makeSession({ consecutive_losses: 3 });
    render(<LossCounter session={session} maxConsecutiveLosses={3} />);
    expect(screen.getByTestId('loss-counter-value').textContent).toBe('LOCKED');
  });

  it('renders at 0 losses correctly', () => {
    const session = makeSession({ consecutive_losses: 0 });
    render(<LossCounter session={session} maxConsecutiveLosses={3} />);
    expect(screen.getByTestId('loss-counter-value').textContent).toContain('0 / 3');
  });
});

describe('DailyPnLGauge', () => {
  it('renders at 0% fill when pnl is 0', () => {
    render(<DailyPnLGauge dailyPnl={0} dailyMaxLossUsd={500} />);
    const bar = screen.getByTestId('daily-pnl-bar');
    expect(bar).toHaveStyle({ width: '0%' });
  });

  it('shows LOCKED when daily loss limit hit', () => {
    render(<DailyPnLGauge dailyPnl={-500} dailyMaxLossUsd={500} />);
    expect(screen.getByTestId('daily-pnl-locked')).toBeInTheDocument();
  });

  it('shows partial fill at 60% loss', () => {
    render(<DailyPnLGauge dailyPnl={-300} dailyMaxLossUsd={500} />);
    const bar = screen.getByTestId('daily-pnl-bar');
    expect(bar).toHaveStyle({ width: '60%' });
  });
});

describe('WalkAwayLockout', () => {
  it('renders nothing when not active', () => {
    const { container } = render(<WalkAwayLockout active={false} endsAtUnix={null} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders with countdown when active', async () => {
    const endsAt = Math.floor(Date.now() / 1000) + 300;
    render(<WalkAwayLockout active={true} endsAtUnix={endsAt} />);
    await waitFor(() => {
      expect(screen.getByTestId('walk-away-lockout')).toBeInTheDocument();
    });
    expect(screen.getByTestId('walk-away-countdown')).toBeInTheDocument();
  });

  it('shows done message when countdown completes', async () => {
    const endsAt = Math.floor(Date.now() / 1000) - 1;
    render(<WalkAwayLockout active={true} endsAtUnix={endsAt} />);
    await waitFor(() => {
      expect(screen.getByTestId('walk-away-done')).toBeInTheDocument();
    });
  });
});
