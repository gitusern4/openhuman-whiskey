/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * TksModsPanel — supplementary tests for the highest-impact uncovered branches:
 *   - Lockout arm-reset 3-phase flow (Arm → countdown → Confirm → unlocked)
 *   - SL/TP error region: TV not attached + draw throws
 *   - Symbol favorites max-cap reject and duplicate reject
 *
 * These tests complement TksModsSuite.test.tsx; mocking conventions match
 * that file exactly.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import TksModsPanel from '../TksModsPanel';

// ---------------------------------------------------------------------------
// Mocks — identical to TksModsSuite.test.tsx
// ---------------------------------------------------------------------------

vi.mock('../../../hooks/useTheme', () => ({
  THEMES: [
    { id: 'default', label: 'Default' },
    { id: 'zeth', label: 'ZETH' },
  ],
  useTheme: () => ({ theme: 'default', setTheme: vi.fn() }),
}));

vi.mock('../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({ navigateBack: vi.fn(), breadcrumbs: [] }),
}));

vi.mock('../ModesPanelBody', () => ({ default: () => <div data-testid="stub-modes-body" /> }));

vi.mock('../TvBridgePanelBody', () => ({
  default: ({ onAttachedChange }: { onAttachedChange?: (v: boolean) => void }) => (
    <div data-testid="stub-tv-bridge-body" onClick={() => onAttachedChange?.(true)} />
  ),
}));

vi.mock('../OrderFlowCard', () => ({ default: () => <div data-testid="stub-order-flow-card" /> }));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const mockInvoke = invoke as ReturnType<typeof vi.fn>;
const mockListen = listen as ReturnType<typeof vi.fn>;

const UNLOCKED_STATUS = {
  is_locked: false,
  locked_until_unix: null,
  lock_reason: null,
  daily_loss_dollars: 0,
  consecutive_losses: 0,
  config: { max_daily_loss_dollars: null, max_consecutive_losses: null, cooldown_minutes: 60 },
  armed_for_reset_until: null,
};

function lockedStatus(overrides: object = {}) {
  return {
    ...UNLOCKED_STATUS,
    is_locked: true,
    locked_until_unix: Math.floor(Date.now() / 1000) + 3600,
    lock_reason: 'Manual walk-away',
    ...overrides,
  };
}

function renderPanel() {
  return render(
    <MemoryRouter>
      <TksModsPanel />
    </MemoryRouter>
  );
}

beforeEach(() => {
  mockListen.mockResolvedValue(() => {});
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'lockout_status') return UNLOCKED_STATUS;
    if (cmd === 'list_whiskey_modes') return [];
    if (cmd === 'get_active_whiskey_mode_id') return 'default';
    if (cmd === 'get_mascot_summon_hotkey') return 'CmdOrCtrl+Shift+Space';
    if (cmd === 'tv_cdp_probe')
      return { reachable: false, port: 9222, tv_targets: [], error: null };
    if (cmd === 'tv_cdp_get_auto_attach_status')
      return {
        enabled: false,
        attached: false,
        last_event: null,
        last_event_at: null,
        retry_count: 0,
      };
    return null;
  });
});

// ===========================================================================
// Lockout arm-reset 3-phase flow
// ===========================================================================

describe('Lockout arm-reset 3-phase flow', () => {
  it('phase 1 → 2: clicking Arm reset calls lockout_arm_reset and shows countdown', async () => {
    const nowSecs = Math.floor(Date.now() / 1000);
    const lockedNotArmed = lockedStatus({ armed_for_reset_until: null });
    const lockedArmedCooldown = lockedStatus({
      armed_for_reset_until: nowSecs + 300, // 5-min cooldown active
    });

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return lockedNotArmed;
      if (cmd === 'lockout_arm_reset') return lockedArmedCooldown;
      return null;
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-lockout-arm-reset-btn')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-lockout-arm-reset-btn'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('lockout_arm_reset');
    });

    // After arm_reset the new status has armed_for_reset_until in the future →
    // countdown should appear.
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-lockout-arm-countdown')).toBeInTheDocument();
    });
    const countdown = screen.getByTestId('tks-mods-lockout-arm-countdown');
    expect(countdown.textContent).toMatch(/Reset available in \d+:\d{2}/);
  });

  it('phase 2 → 3: armed_for_reset_until in the past shows Confirm reset button', async () => {
    const nowSecs = Math.floor(Date.now() / 1000);
    const lockedArmedExpired = lockedStatus({
      armed_for_reset_until: nowSecs - 5, // cooldown already expired
    });

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return lockedArmedExpired;
      return null;
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-lockout-confirm-reset')).toBeInTheDocument();
    });

    // Arm button and countdown must NOT be shown in this phase.
    expect(screen.queryByTestId('tks-mods-lockout-arm-reset-btn')).not.toBeInTheDocument();
    expect(screen.queryByTestId('tks-mods-lockout-arm-countdown')).not.toBeInTheDocument();
  });

  it('phase 3 → unlocked: Confirm reset calls lockout_reset and unlocks', async () => {
    const nowSecs = Math.floor(Date.now() / 1000);
    const lockedArmedExpired = lockedStatus({ armed_for_reset_until: nowSecs - 1 });

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return lockedArmedExpired;
      if (cmd === 'lockout_reset') return UNLOCKED_STATUS;
      return null;
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-lockout-confirm-reset')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-lockout-confirm-reset'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('lockout_reset');
    });
  });

  it("Confirm reset shows 'Wait until cooldown ends.' when lockout_reset throws cooldown error", async () => {
    const nowSecs = Math.floor(Date.now() / 1000);
    const lockedArmedExpired = lockedStatus({ armed_for_reset_until: nowSecs - 1 });

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return lockedArmedExpired;
      if (cmd === 'lockout_reset')
        throw new Error('Reset armed but cooldown active. 10 seconds remaining.');
      return null;
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-lockout-confirm-reset')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-lockout-confirm-reset'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-lockout-reset-error')).toHaveTextContent(
        'Wait until cooldown ends.'
      );
    });
  });
});

// ===========================================================================
// SL/TP error paths
// ===========================================================================

describe('SL/TP overlay error paths', () => {
  it('shows validation error when Draw is clicked with empty fields', async () => {
    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-draw-button')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-draw-button'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-error')).toHaveTextContent('Enter valid numbers');
    });
  });

  it('shows draw-failed error when tv_cdp_draw_sltp returns ok=false', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_cdp_probe')
        return { reachable: false, port: 9222, tv_targets: [], error: null };
      if (cmd === 'tv_cdp_get_auto_attach_status')
        return {
          enabled: false,
          attached: false,
          last_event: null,
          last_event_at: null,
          retry_count: 0,
        };
      if (cmd === 'tv_cdp_draw_sltp')
        return { ok: false, removed: null, error: 'TV bridge not attached' };
      return null;
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-sltp-entry')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('tks-mods-sltp-entry'), { target: { value: '19800' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-stop'), { target: { value: '19750' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-target'), { target: { value: '19875' } });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-draw-button'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-error')).toHaveTextContent('TV bridge not attached');
    });
  });

  it('shows clear success after tv_cdp_clear_sltp removes lines', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_cdp_probe')
        return { reachable: false, port: 9222, tv_targets: [], error: null };
      if (cmd === 'tv_cdp_get_auto_attach_status')
        return {
          enabled: false,
          attached: false,
          last_event: null,
          last_event_at: null,
          retry_count: 0,
        };
      if (cmd === 'tv_cdp_clear_sltp') return { ok: true, removed: 2, error: null };
      return null;
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-clear-button')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-clear-button'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-sltp-success')).toHaveTextContent(
        'Cleared 2 overlay line(s).'
      );
    });
  });
});

// ===========================================================================
// Symbol favorites: max-cap reject and duplicate reject
// ===========================================================================

describe('Symbol favorites edge cases', () => {
  it('rejects a 21st favorite with a friendly error message (MAX_FAVORITES=20)', async () => {
    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-fav-input')).toBeInTheDocument();
    });

    // Add 20 favorites (the max)
    for (let i = 1; i <= 20; i++) {
      fireEvent.change(screen.getByTestId('tks-mods-fav-input'), { target: { value: `SYM${i}` } });
      fireEvent.click(screen.getByTestId('tks-mods-fav-add'));
    }

    // Try to add the 21st
    fireEvent.change(screen.getByTestId('tks-mods-fav-input'), { target: { value: 'OVERFLOW' } });
    fireEvent.click(screen.getByTestId('tks-mods-fav-add'));

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-fav-error')).toHaveTextContent('Maximum 20 favorites');
    });
    // OVERFLOW must NOT have been added
    expect(screen.queryByTestId('tks-mods-fav-btn-OVERFLOW')).not.toBeInTheDocument();
  });

  it('rejects a duplicate favorite with a friendly error message', async () => {
    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-fav-input')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('tks-mods-fav-input'), { target: { value: 'NQ1!' } });
    fireEvent.click(screen.getByTestId('tks-mods-fav-add'));

    // Try to add the same symbol again
    fireEvent.change(screen.getByTestId('tks-mods-fav-input'), { target: { value: 'NQ1!' } });
    fireEvent.click(screen.getByTestId('tks-mods-fav-add'));

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-fav-error')).toHaveTextContent(
        'NQ1! is already in your favorites'
      );
    });
  });

  it('shows error when tv_cdp_set_symbol fails during favorite switch', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_cdp_set_symbol')
        return { ok: false, symbol: null, error: 'CDP session lost' };
      return null;
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-fav-input')).toBeInTheDocument();
    });

    // Add a favorite
    fireEvent.change(screen.getByTestId('tks-mods-fav-input'), { target: { value: 'ES1!' } });
    fireEvent.click(screen.getByTestId('tks-mods-fav-add'));

    // Simulate TV bridge attached via stub click
    fireEvent.click(screen.getByTestId('stub-tv-bridge-body'));

    // Click the favorite button
    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-fav-btn-ES1!'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-fav-error')).toHaveTextContent('CDP session lost');
    });
  });
});
