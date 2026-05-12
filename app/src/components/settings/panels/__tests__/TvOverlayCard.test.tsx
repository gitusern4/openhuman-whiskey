/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * TK's Mods — TradingView Overlay card tests.
 *
 * Tests:
 *   1. Card renders with correct initial status (not_injected).
 *   2. Enable button calls tv_overlay_inject and updates status to injected.
 *   3. Disable button calls tv_overlay_remove and updates status to not_injected.
 *   4. Test message button calls tv_overlay_send_state when injected.
 *   5. Enable button disabled when TV bridge not attached.
 *   6. Error message shown when inject fails.
 *   7. Status dot turns green on tv-cdp-status 'attached'.
 *   8. Status dot turns red on tv-cdp-status 'detached'.
 *   9. Re-inject fires on 'reattached' when overlay was enabled.
 *  10. Re-inject does NOT fire on 'reattached' when overlay was not enabled.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import TksModsPanel from '../TksModsPanel';

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

// TvBridgePanelBody stub: clicking it simulates bridge attach/detach toggle.
let attachedState = false;
vi.mock('../TvBridgePanelBody', () => ({
  default: ({ onAttachedChange }: { onAttachedChange?: (v: boolean) => void }) => (
    <div
      data-testid="stub-tv-bridge-body"
      onClick={() => {
        attachedState = !attachedState;
        onAttachedChange?.(attachedState);
      }}
    />
  ),
}));

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

function renderPanel() {
  attachedState = false;
  return render(
    <MemoryRouter>
      <TksModsPanel />
    </MemoryRouter>
  );
}

beforeEach(() => {
  attachedState = false;
  // Default listen mock: resolves with a no-op unlisten fn.
  mockListen.mockResolvedValue(() => {});
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
// TradingView Overlay card
// ===========================================================================

describe('TradingView Overlay card', () => {
  it('renders the overlay card with not_injected status initially', async () => {
    renderPanel();
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-tv-overlay-card')).toBeInTheDocument();
    });
    expect(screen.getByTestId('tks-mods-overlay-status-label')).toHaveTextContent('Not injected');
    expect(screen.getByTestId('tks-mods-overlay-enable')).toBeInTheDocument();
    expect(screen.queryByTestId('tks-mods-overlay-disable')).not.toBeInTheDocument();
  });

  it('enable button is disabled when TV bridge is not attached', async () => {
    renderPanel();
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-overlay-enable')).toBeDisabled();
    });
  });

  it('enable button calls tv_overlay_inject and updates status to injected', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_overlay_inject')
        return { ok: true, panel_id: 'whiskey-tv-overlay', skipped: false, error: null };
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('stub-tv-bridge-body')).toBeInTheDocument());

    // Attach the bridge.
    fireEvent.click(screen.getByTestId('stub-tv-bridge-body'));

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-overlay-enable')).not.toBeDisabled();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-overlay-enable'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('tv_overlay_inject');
      expect(screen.getByTestId('tks-mods-overlay-status-label')).toHaveTextContent(
        'Panel injected and active'
      );
    });
  });

  it('disable button calls tv_overlay_remove and reverts status', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_overlay_inject')
        return { ok: true, panel_id: 'whiskey-tv-overlay', skipped: false, error: null };
      if (cmd === 'tv_overlay_remove') return undefined;
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('stub-tv-bridge-body')).toBeInTheDocument());

    // Attach + inject.
    fireEvent.click(screen.getByTestId('stub-tv-bridge-body'));
    await waitFor(() => expect(screen.getByTestId('tks-mods-overlay-enable')).not.toBeDisabled());
    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-overlay-enable'));
    });
    await waitFor(() => expect(screen.getByTestId('tks-mods-overlay-disable')).toBeInTheDocument());

    // Disable.
    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-overlay-disable'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('tv_overlay_remove');
      expect(screen.getByTestId('tks-mods-overlay-status-label')).toHaveTextContent('Not injected');
    });
  });

  it('test message button calls tv_overlay_send_state when panel is injected', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_overlay_inject')
        return { ok: true, panel_id: 'whiskey-tv-overlay', skipped: false, error: null };
      if (cmd === 'tv_overlay_send_state') return undefined;
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('stub-tv-bridge-body')).toBeInTheDocument());

    fireEvent.click(screen.getByTestId('stub-tv-bridge-body'));
    await waitFor(() => expect(screen.getByTestId('tks-mods-overlay-enable')).not.toBeDisabled());
    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-overlay-enable'));
    });
    await waitFor(() => expect(screen.getByTestId('tks-mods-overlay-test')).toBeInTheDocument());

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-overlay-test'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        'tv_overlay_send_state',
        expect.objectContaining({
          newState: expect.objectContaining({ favorites: expect.any(Array) }),
        })
      );
    });
  });

  it('shows error when inject returns ok:false', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_overlay_inject')
        return { ok: false, panel_id: null, skipped: false, error: 'CDP session lost' };
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('stub-tv-bridge-body')).toBeInTheDocument());

    fireEvent.click(screen.getByTestId('stub-tv-bridge-body'));
    await waitFor(() => expect(screen.getByTestId('tks-mods-overlay-enable')).not.toBeDisabled());

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-overlay-enable'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-overlay-error')).toHaveTextContent('CDP session lost');
    });
  });

  it('status indicator dot is green when injected', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_overlay_inject')
        return { ok: true, panel_id: 'whiskey-tv-overlay', skipped: false, error: null };
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('stub-tv-bridge-body')).toBeInTheDocument());

    fireEvent.click(screen.getByTestId('stub-tv-bridge-body'));
    await waitFor(() => expect(screen.getByTestId('tks-mods-overlay-enable')).not.toBeDisabled());
    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-overlay-enable'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-overlay-status-dot')).toHaveClass('bg-green-500');
    });
  });
});

// ===========================================================================
// Gap #4 — TV bridge CDP-status event → status dot
// ===========================================================================

describe('TV bridge CDP-status dot', () => {
  it('dot turns green when tv-cdp-status fires "attached"', async () => {
    // Capture the listener callback registered for 'tv-cdp-status'.
    let capturedCallback: ((event: { payload: { kind: string } }) => void) | null = null;
    mockListen.mockImplementation(
      async (eventName: string, cb: (event: { payload: { kind: string } }) => void) => {
        if (eventName === 'tv-cdp-status') capturedCallback = cb;
        return () => {};
      }
    );

    renderPanel();
    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-tv-bridge-status-dot')).toBeInTheDocument()
    );

    // Initially red (detached).
    expect(screen.getByTestId('tks-mods-tv-bridge-status-dot')).toHaveClass('bg-red-400');

    // Fire the event.
    await act(async () => {
      capturedCallback?.({ payload: { kind: 'attached' } });
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-tv-bridge-status-dot')).toHaveClass('bg-green-500');
    });
  });

  it('dot stays red when tv-cdp-status fires "detached"', async () => {
    let capturedCallback: ((event: { payload: { kind: string } }) => void) | null = null;
    mockListen.mockImplementation(
      async (eventName: string, cb: (event: { payload: { kind: string } }) => void) => {
        if (eventName === 'tv-cdp-status') capturedCallback = cb;
        return () => {};
      }
    );

    renderPanel();
    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-tv-bridge-status-dot')).toBeInTheDocument()
    );

    await act(async () => {
      capturedCallback?.({ payload: { kind: 'detached' } });
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-tv-bridge-status-dot')).toHaveClass('bg-red-400');
    });
  });
});

// ===========================================================================
// Gap #5 — In-TV overlay re-inject on "reattached" event
// ===========================================================================

describe('Overlay re-inject on reattached', () => {
  it('re-injects when overlay was enabled and reattached event fires', async () => {
    let capturedCallback: ((event: { payload: { kind: string } }) => void) | null = null;
    mockListen.mockImplementation(
      async (eventName: string, cb: (event: { payload: { kind: string } }) => void) => {
        if (eventName === 'tv-cdp-status') capturedCallback = cb;
        return () => {};
      }
    );
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'lockout_status') return UNLOCKED_STATUS;
      if (cmd === 'tv_overlay_inject')
        return { ok: true, panel_id: 'whiskey-tv-overlay', skipped: false, error: null };
      return null;
    });

    renderPanel();
    await waitFor(() => expect(screen.getByTestId('stub-tv-bridge-body')).toBeInTheDocument());

    // Attach bridge and enable overlay.
    fireEvent.click(screen.getByTestId('stub-tv-bridge-body'));
    await waitFor(() => expect(screen.getByTestId('tks-mods-overlay-enable')).not.toBeDisabled());
    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-overlay-enable'));
    });
    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-overlay-status-label')).toHaveTextContent(
        'Panel injected and active'
      )
    );

    const invokeCallsBefore = mockInvoke.mock.calls.filter(
      ([c]: [string]) => c === 'tv_overlay_inject'
    ).length;

    // Fire reattached event.
    await act(async () => {
      capturedCallback?.({ payload: { kind: 'reattached' } });
    });

    await waitFor(() => {
      const injectCalls = mockInvoke.mock.calls.filter(
        ([c]: [string]) => c === 'tv_overlay_inject'
      ).length;
      expect(injectCalls).toBeGreaterThan(invokeCallsBefore);
    });
  });

  it('does NOT re-inject when overlay was not enabled and reattached event fires', async () => {
    let capturedCallback: ((event: { payload: { kind: string } }) => void) | null = null;
    mockListen.mockImplementation(
      async (eventName: string, cb: (event: { payload: { kind: string } }) => void) => {
        if (eventName === 'tv-cdp-status') capturedCallback = cb;
        return () => {};
      }
    );

    renderPanel();
    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-tv-bridge-status-dot')).toBeInTheDocument()
    );

    const invokeCallsBefore = mockInvoke.mock.calls.filter(
      ([c]: [string]) => c === 'tv_overlay_inject'
    ).length;

    // Fire reattached without overlay ever being enabled.
    await act(async () => {
      capturedCallback?.({ payload: { kind: 'reattached' } });
    });

    // Give React a tick to process.
    await new Promise(r => setTimeout(r, 50));

    const injectCalls = mockInvoke.mock.calls.filter(
      ([c]: [string]) => c === 'tv_overlay_inject'
    ).length;
    expect(injectCalls).toBe(invokeCallsBefore);
  });
});
