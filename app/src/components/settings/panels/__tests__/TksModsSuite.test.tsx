/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * TK's Mods Suite — vitest coverage for the new feature cards.
 *
 * Sections tested:
 *   - Position size calculator (3+ tests)
 *   - Pre-trade checklist (4+ tests)
 *   - Symbol favorites (3+ tests)
 *   - Walk-away lockout UI (3+ tests)
 *
 * `@tauri-apps/api/core` is fully mocked by setup.ts so invoke() is a
 * vi.fn() that we configure per-test.
 */
import { invoke } from '@tauri-apps/api/core';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import TksModsPanel from '../TksModsPanel';

// We test TksModsPanel in isolation. The panel uses useTheme and
// useSettingsNavigation; we mock the latter to provide stubs.
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

// ModesPanelBody and TvBridgePanelBody are heavier sub-trees — stub them
// so tests focus only on the new feature cards.
vi.mock('../ModesPanelBody', () => ({ default: () => <div data-testid="stub-modes-body" /> }));

vi.mock('../TvBridgePanelBody', () => ({
  default: ({ onAttachedChange }: { onAttachedChange?: (v: boolean) => void }) => (
    <div data-testid="stub-tv-bridge-body" onClick={() => onAttachedChange?.(true)} />
  ),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

const UNLOCKED_STATUS = {
  is_locked: false,
  locked_until_unix: null,
  lock_reason: null,
  daily_loss_dollars: 0,
  consecutive_losses: 0,
  config: { max_daily_loss_dollars: null, max_consecutive_losses: null, cooldown_minutes: 60 },
};

function renderPanel() {
  return render(
    <MemoryRouter>
      <TksModsPanel />
    </MemoryRouter>
  );
}

// ---------------------------------------------------------------------------
// Setup — default invoke mock returns unlocked status on lockout_status
// ---------------------------------------------------------------------------

beforeEach(() => {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'lockout_status') return UNLOCKED_STATUS;
    if (cmd === 'list_whiskey_modes') return [];
    if (cmd === 'get_active_whiskey_mode_id') return 'default';
    if (cmd === 'get_mascot_summon_hotkey') return 'CmdOrCtrl+Shift+Space';
    if (cmd === 'tv_cdp_probe')
      return { reachable: false, port: 9222, tv_targets: [], error: null };
    return null;
  });
});

// ===========================================================================
// Position size calculator
// ===========================================================================

describe('Position size calculator', () => {
  it('shows the calculator card', async () => {
    renderPanel();
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-position-sizer-card')).toBeInTheDocument();
    });
  });

  it('shows result after successful compute', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'compute_position_size')
        return { contracts: 2, actual_risk_dollars: 200.0, risk_per_contract: 100.0, error: null };
      return null;
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-ps-entry')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('tks-mods-ps-entry'), { target: { value: '19800' } });
    fireEvent.change(screen.getByTestId('tks-mods-ps-stop'), { target: { value: '19750' } });
    fireEvent.change(screen.getByTestId('tks-mods-ps-risk'), { target: { value: '200' } });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-ps-compute'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-ps-contracts')).toHaveTextContent('2');
    });
  });

  it('shows error when compute returns an error reason', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'compute_position_size')
        return {
          contracts: 0,
          actual_risk_dollars: 0,
          risk_per_contract: 0,
          error: 'Stop price equals entry price',
        };
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('tks-mods-ps-entry')).toBeInTheDocument());

    fireEvent.change(screen.getByTestId('tks-mods-ps-entry'), { target: { value: '19800' } });
    fireEvent.change(screen.getByTestId('tks-mods-ps-stop'), { target: { value: '19800' } });
    fireEvent.change(screen.getByTestId('tks-mods-ps-risk'), { target: { value: '200' } });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-ps-compute'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-ps-error')).toHaveTextContent('Stop price equals entry');
    });
  });

  it('shows validation error when inputs are empty', async () => {
    renderPanel();
    await waitFor(() => expect(screen.getByTestId('tks-mods-ps-compute')).toBeInTheDocument());

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-ps-compute'));
    });

    expect(screen.getByTestId('tks-mods-ps-error')).toBeInTheDocument();
  });
});

// ===========================================================================
// Pre-trade checklist
// ===========================================================================

describe('Pre-trade checklist', () => {
  it('renders the five default checklist items', async () => {
    renderPanel();
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-checklist-card')).toBeInTheDocument();
    });
    // Five default items present.
    expect(screen.getByTestId('tks-mods-cl-label-catalog-match')).toBeInTheDocument();
    expect(screen.getByTestId('tks-mods-cl-label-stop-defined')).toBeInTheDocument();
    expect(screen.getByTestId('tks-mods-cl-label-size-calculated')).toBeInTheDocument();
    expect(screen.getByTestId('tks-mods-cl-label-risk-budget')).toBeInTheDocument();
    expect(screen.getByTestId('tks-mods-cl-label-no-revenge')).toBeInTheDocument();
  });

  it('Confirm button is disabled until all items are checked', async () => {
    renderPanel();
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-cl-confirm')).toBeDisabled();
    });

    // Check all items.
    for (const id of [
      'catalog-match',
      'stop-defined',
      'size-calculated',
      'risk-budget',
      'no-revenge',
    ]) {
      fireEvent.click(screen.getByTestId(`tks-mods-cl-check-${id}`));
    }

    expect(screen.getByTestId('tks-mods-cl-confirm')).not.toBeDisabled();
  });

  it('clicking Confirm when all checked shows success message', async () => {
    renderPanel();
    await waitFor(() => expect(screen.getByTestId('tks-mods-cl-confirm')).toBeInTheDocument());

    for (const id of [
      'catalog-match',
      'stop-defined',
      'size-calculated',
      'risk-budget',
      'no-revenge',
    ]) {
      fireEvent.click(screen.getByTestId(`tks-mods-cl-check-${id}`));
    }

    fireEvent.click(screen.getByTestId('tks-mods-cl-confirm'));
    expect(screen.getByTestId('tks-mods-cl-confirm-success')).toBeInTheDocument();
  });

  it('adds a new item via the input', async () => {
    renderPanel();
    await waitFor(() => expect(screen.getByTestId('tks-mods-cl-new-input')).toBeInTheDocument());

    fireEvent.change(screen.getByTestId('tks-mods-cl-new-input'), {
      target: { value: 'Check news calendar' },
    });
    fireEvent.click(screen.getByTestId('tks-mods-cl-add'));

    expect(screen.getByText('Check news calendar')).toBeInTheDocument();
  });

  it('removes an item when the remove button is clicked', async () => {
    renderPanel();
    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-cl-remove-catalog-match')).toBeInTheDocument()
    );

    expect(screen.getByTestId('tks-mods-cl-label-catalog-match')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('tks-mods-cl-remove-catalog-match'));
    expect(screen.queryByTestId('tks-mods-cl-label-catalog-match')).not.toBeInTheDocument();
  });
});

// ===========================================================================
// Symbol favorites
// ===========================================================================

describe('Symbol favorites', () => {
  it('adds a favorite via the input', async () => {
    renderPanel();
    await waitFor(() => expect(screen.getByTestId('tks-mods-fav-input')).toBeInTheDocument());

    fireEvent.change(screen.getByTestId('tks-mods-fav-input'), { target: { value: 'NQ1!' } });
    fireEvent.click(screen.getByTestId('tks-mods-fav-add'));

    expect(screen.getByTestId('tks-mods-fav-btn-NQ1!')).toBeInTheDocument();
  });

  it('removes a favorite when the X button is clicked', async () => {
    renderPanel();
    await waitFor(() => expect(screen.getByTestId('tks-mods-fav-input')).toBeInTheDocument());

    fireEvent.change(screen.getByTestId('tks-mods-fav-input'), { target: { value: 'ES1!' } });
    fireEvent.click(screen.getByTestId('tks-mods-fav-add'));
    expect(screen.getByTestId('tks-mods-fav-btn-ES1!')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tks-mods-fav-remove-ES1!'));
    expect(screen.queryByTestId('tks-mods-fav-btn-ES1!')).not.toBeInTheDocument();
  });

  it('clicking a favorite calls tv_cdp_set_symbol when bridge is attached', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_cdp_set_symbol') return { ok: true, symbol: 'MNQ1!', error: null };
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('tks-mods-fav-input')).toBeInTheDocument());

    // Add a favorite.
    fireEvent.change(screen.getByTestId('tks-mods-fav-input'), { target: { value: 'MNQ1!' } });
    fireEvent.click(screen.getByTestId('tks-mods-fav-add'));

    // Simulate TV bridge attach via the stub's onClick.
    fireEvent.click(screen.getByTestId('stub-tv-bridge-body'));

    // Click the favorite.
    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-fav-btn-MNQ1!'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('tv_cdp_set_symbol', { symbol: 'MNQ1!' });
    });
  });
});

// ===========================================================================
// Walk-away lockout UI
// ===========================================================================

describe('Walk-away lockout', () => {
  it('renders the lockout card with unlocked state', async () => {
    renderPanel();
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-lockout-card')).toBeInTheDocument();
      expect(screen.getByTestId('tks-mods-lockout-state')).toHaveTextContent('unlocked');
    });
  });

  it('shows the lockout banner when locked', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status')
        return {
          ...UNLOCKED_STATUS,
          is_locked: true,
          locked_until_unix: Math.floor(Date.now() / 1000) + 3600,
          lock_reason: 'Manual walk-away',
        };
      return null;
    });

    renderPanel();
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-lockout-banner')).toBeInTheDocument();
    });
  });

  it('Trip lockout now calls lockout_trip and updates state', async () => {
    const locked = {
      ...UNLOCKED_STATUS,
      is_locked: true,
      locked_until_unix: Math.floor(Date.now() / 1000) + 3600,
      lock_reason: 'Manual walk-away',
    };
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'lockout_trip') return locked;
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('tks-mods-lockout-trip')).toBeInTheDocument());

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-lockout-trip'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('lockout_trip', { reason: 'Manual walk-away' });
    });
  });
});
