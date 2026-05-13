/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * TvBridgePanelBody — coverage for auto-attach toggle, supervisor events,
 * probe failure paths, attach error, detach, and chart-state read flows.
 *
 * Missing-coverage targets: lines 126-128, 130-131, 143-144, 147-148,
 * 154-161, 164-168, 170, 174-176, 179-183, 185-187, 196-198, 217-220,
 * 225-227, 235-241, 243-244, 246, 261-262, 266-267, 284-285, 299,
 * 304-305, 335-336, 349-350, 388, 422, 430, 453, 466, 492, 520, 526,
 * 532, 538, 544.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import TvBridgePanelBody from '../TvBridgePanelBody';

// ---------------------------------------------------------------------------
// Shared fixtures
// ---------------------------------------------------------------------------

const AUTO_STATUS_OFF = {
  enabled: false,
  attached: false,
  last_event: null,
  last_event_at: null,
  retry_count: 0,
};

const AUTO_STATUS_ON_ATTACHED = {
  enabled: true,
  attached: true,
  last_event: 'attached',
  last_event_at: Date.now(),
  retry_count: 0,
};

const AUTO_STATUS_ON_RETRYING = {
  enabled: true,
  attached: false,
  last_event: null,
  last_event_at: null,
  retry_count: 3,
};

const AUTO_STATUS_ON_FAILED = {
  enabled: true,
  attached: false,
  last_event: 'reconnect_failed',
  last_event_at: Date.now(),
  retry_count: 0,
};

const REACHABLE_PROBE = {
  reachable: true,
  port: 9222,
  browser_ws_url: 'ws://127.0.0.1:9222/devtools/browser/abc',
  tv_targets: [
    { id: 'target-1', url: 'https://www.tradingview.com/chart/', title: 'NQ — TradingView' },
  ],
  error: null,
};

const UNREACHABLE_PROBE = {
  reachable: false,
  port: 9222,
  browser_ws_url: null,
  tv_targets: [],
  error: 'TV CDP unreachable',
};

const CHART_STATE = {
  symbol: 'CME_MINI:NQ1!',
  resolution: '5',
  price: null,
  indicator_count: 2,
  indicators: [],
  shapes: [{ id: 'sh_1', name: 'Line' }],
  alert_count: 1,
  raw: {},
};

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
const mockInvoke = invoke as ReturnType<typeof vi.fn>;
const mockListen = listen as ReturnType<typeof vi.fn>;

// Capture event listeners registered via listen() so tests can fire them.
type ListenerMap = Record<string, ((ev: { payload: unknown }) => void)[]>;
let capturedListeners: ListenerMap = {};

beforeEach(() => {
  capturedListeners = {};

  // Default listen — captures the callback and returns a no-op unlisten fn.
  mockListen.mockImplementation(async (event: string, cb: (ev: { payload: unknown }) => void) => {
    capturedListeners[event] = capturedListeners[event] ?? [];
    capturedListeners[event].push(cb);
    return () => {};
  });

  // Default invoke — safe defaults so noise-free for tests that override only
  // one command.
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
    if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
    if (cmd === 'tv_cdp_set_auto_attach') return undefined;
    throw new Error(`unexpected invoke: ${cmd}`);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

// ---------------------------------------------------------------------------
// Helper: fire a tv-cdp-status event to all registered listeners
// ---------------------------------------------------------------------------
function fireTvCdpStatus(payload: { kind: string; at: number; error: string | null }) {
  const listeners = capturedListeners['tv-cdp-status'] ?? [];
  listeners.forEach(cb => cb({ payload }));
}

// ===========================================================================
// 1. On-mount: reads persisted auto-attach status (lines 193-203)
// ===========================================================================

describe('On-mount auto-attach state sync', () => {
  it('sets autoAttach=true when persisted enabled=true (lines 195-199)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_ON_ATTACHED;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    const onAttachedChange = vi.fn();
    render(<TvBridgePanelBody onAttachedChange={onAttachedChange} />);

    // After mount the auto-attach toggle should be checked and callback fired.
    await waitFor(() => {
      const toggle = screen.getByTestId('tv-bridge-auto-attach-toggle') as HTMLInputElement;
      expect(toggle.checked).toBe(true);
    });
    // onAttachedChange should be called with true from s.attached=true.
    await waitFor(() => {
      expect(onAttachedChange).toHaveBeenCalledWith(true);
    });
  });

  it('leaves autoAttach=false when persisted enabled=false (line 194)', async () => {
    // default mockInvoke returns AUTO_STATUS_OFF
    render(<TvBridgePanelBody />);

    await waitFor(() => {
      const toggle = screen.getByTestId('tv-bridge-auto-attach-toggle') as HTMLInputElement;
      expect(toggle.checked).toBe(false);
    });
  });
});

// ===========================================================================
// 2. Auto-attach toggle: invoke path (lines 235-247)
// ===========================================================================

describe('Auto-attach toggle', () => {
  it('calls tv_cdp_set_auto_attach with enabled=true when toggled on (lines 239-241)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-auto-attach-toggle')).toBeInTheDocument()
    );

    fireEvent.click(screen.getByTestId('tv-bridge-auto-attach-toggle'));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('tv_cdp_set_auto_attach', {
        enabled: true,
        port: 9222,
      });
    });
  });

  it('shows error when tv_cdp_set_auto_attach throws (lines 242-244)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') throw new Error('supervisor start failed');
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-auto-attach-toggle')).toBeInTheDocument()
    );

    fireEvent.click(screen.getByTestId('tv-bridge-auto-attach-toggle'));

    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('Auto-attach toggle failed');
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('supervisor start failed');
    });
  });
});

// ===========================================================================
// 3. Supervisor status dot appearance (lines 215-228)
// ===========================================================================

describe('Supervisor status dot', () => {
  it('shows green dot when supervisor is on and attached (lines 217-218)', async () => {
    // Mount with auto-attach already on (enabled=true) so the dot renders immediately.
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_ON_ATTACHED;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    // When enabled=true on mount, auto-attach toggle should be checked.
    await waitFor(() => {
      const toggle = screen.getByTestId('tv-bridge-auto-attach-toggle') as HTMLInputElement;
      expect(toggle.checked).toBe(true);
    });

    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-supervisor-pill')).toBeInTheDocument();
      expect(screen.getByTestId('tv-bridge-supervisor-dot')).toHaveClass('bg-green-500');
    });
    expect(screen.getByTestId('tv-bridge-supervisor-pill')).toHaveTextContent('live');
  });

  it('shows amber dot when supervisor is retrying (lines 218-220)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_ON_RETRYING;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    await waitFor(() => {
      const toggle = screen.getByTestId('tv-bridge-auto-attach-toggle') as HTMLInputElement;
      expect(toggle.checked).toBe(true);
    });

    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-supervisor-dot')).toHaveClass('bg-amber-400');
    });
    expect(screen.getByTestId('tv-bridge-supervisor-pill')).toHaveTextContent(/retry 3/);
  });

  it('shows red dot when last_event=reconnect_failed (lines 219-220)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_ON_FAILED;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    await waitFor(() => {
      const toggle = screen.getByTestId('tv-bridge-auto-attach-toggle') as HTMLInputElement;
      expect(toggle.checked).toBe(true);
    });

    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-supervisor-dot')).toHaveClass('bg-red-500');
    });
  });
});

// ===========================================================================
// 4. tv-cdp-status event listener: reattached → notifyAttached(true)
//    (lines 154-161, 164-170)
// ===========================================================================

describe('tv-cdp-status event listener', () => {
  it('fires notifyAttached(true) when kind=reattached (lines 158-159)', async () => {
    const onAttachedChange = vi.fn();

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') return undefined;
      return AUTO_STATUS_ON_ATTACHED;
    });

    render(<TvBridgePanelBody onAttachedChange={onAttachedChange} />);

    // Enable auto-attach so listener is registered.
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-auto-attach-toggle')).toBeInTheDocument()
    );
    fireEvent.click(screen.getByTestId('tv-bridge-auto-attach-toggle'));

    // Wait for listener to be registered.
    await waitFor(() => {
      expect(capturedListeners['tv-cdp-status']).toBeDefined();
    });

    onAttachedChange.mockClear();
    fireTvCdpStatus({ kind: 'reattached', at: Date.now(), error: null });

    await waitFor(() => {
      expect(onAttachedChange).toHaveBeenCalledWith(true);
    });
  });

  it('fires notifyAttached(false) when kind=reconnect_failed (lines 160-161)', async () => {
    const onAttachedChange = vi.fn();

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') return undefined;
      return AUTO_STATUS_ON_ATTACHED;
    });

    render(<TvBridgePanelBody onAttachedChange={onAttachedChange} />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-auto-attach-toggle')).toBeInTheDocument()
    );
    fireEvent.click(screen.getByTestId('tv-bridge-auto-attach-toggle'));

    await waitFor(() => {
      expect(capturedListeners['tv-cdp-status']).toBeDefined();
    });

    onAttachedChange.mockClear();
    fireTvCdpStatus({ kind: 'reconnect_failed', at: Date.now(), error: null });

    await waitFor(() => {
      expect(onAttachedChange).toHaveBeenCalledWith(false);
    });
  });
});

// ===========================================================================
// 5. runProbe failure path: probe returns reachable=false (lines 297-304)
// ===========================================================================

describe('Probe failure path', () => {
  it('populates error region when probe returns reachable=false (lines 297-300)', async () => {
    // Default mock already returns UNREACHABLE_PROBE. Just verify error region.
    render(<TvBridgePanelBody />);

    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('TV CDP unreachable');
    });
    expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('unreachable');
  });

  it('populates error region when probe throws (lines 303-305)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') throw new Error('ECONNREFUSED');
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('Probe failed');
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('ECONNREFUSED');
    });
  });
});

// ===========================================================================
// 6. Attach error path: error region populated, status stays reachable
//    (lines 316-322)
// ===========================================================================

describe('Attach error path', () => {
  it('populates error and stays not-attached on attach failure (lines 319-321)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') throw new Error('No TradingView page found');
      throw new Error(`unexpected: ${cmd}`);
    });

    const onAttachedChange = vi.fn();
    render(<TvBridgePanelBody onAttachedChange={onAttachedChange} />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );

    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));

    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('Attach failed');
    });
    expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('No TradingView page found');
    expect(screen.getByTestId('tv-bridge-status')).not.toHaveTextContent('attached');
    // notifyAttached(false) called on error
    expect(onAttachedChange).toHaveBeenCalledWith(false);
  });
});

// ===========================================================================
// 7. Detach clears chart state card and calls notifyAttached(false)
//    (lines 327-339)
// ===========================================================================

describe('Detach flow', () => {
  it('clears chart-state card after detach and calls notifyAttached(false) (lines 331-333)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_detach') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    const onAttachedChange = vi.fn();
    render(<TvBridgePanelBody onAttachedChange={onAttachedChange} />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('attached')
    );

    fireEvent.click(screen.getByTestId('tv-bridge-detach-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).not.toHaveTextContent('attached')
    );

    expect(screen.queryByTestId('tv-bridge-state-card')).not.toBeInTheDocument();
    expect(onAttachedChange).toHaveBeenCalledWith(false);
  });

  it('shows error when detach throws (lines 334-336)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_detach') throw new Error('CDP pipe broken');
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('attached')
    );

    fireEvent.click(screen.getByTestId('tv-bridge-detach-button'));
    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('Detach failed');
    });
  });
});

// ===========================================================================
// 8. Chart-state read — refreshChartState failure path (lines 346-353)
// ===========================================================================

describe('Chart-state read', () => {
  it('shows chart state values after successful read (lines 346-348)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_get_chart_state') return CHART_STATE;
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));
    await waitFor(() => expect(screen.getByTestId('tv-bridge-state-card')).toBeInTheDocument());

    fireEvent.click(screen.getByTestId('tv-bridge-refresh-button'));
    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-symbol')).toHaveTextContent('CME_MINI:NQ1!');
    });
    expect(screen.getByTestId('tv-bridge-shape-count')).toHaveTextContent('1');
    expect(screen.getByTestId('tv-bridge-alert-count')).toHaveTextContent('1');
  });

  it('shows error when tv_cdp_get_chart_state throws (lines 349-351)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_get_chart_state') throw new Error('CDP eval timeout');
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgePanelBody />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));
    await waitFor(() => expect(screen.getByTestId('tv-bridge-state-card')).toBeInTheDocument());

    fireEvent.click(screen.getByTestId('tv-bridge-refresh-button'));
    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('Could not read chart state');
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent('CDP eval timeout');
    });
  });
});

// ===========================================================================
// 9. Force re-attach label when auto-attach is on (line 466)
// ===========================================================================

describe('Force re-attach label', () => {
  it("shows 'Force re-attach now' on attach button when auto-attach is on (line 466)", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      if (cmd === 'tv_cdp_set_auto_attach') return undefined;
      return AUTO_STATUS_ON_ATTACHED;
    });

    render(<TvBridgePanelBody />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-auto-attach-toggle')).toBeInTheDocument()
    );
    fireEvent.click(screen.getByTestId('tv-bridge-auto-attach-toggle'));

    await waitFor(() => {
      expect(screen.getByTestId('tv-bridge-attach-button')).toHaveTextContent(
        'Force re-attach now'
      );
    });
  });
});

// ===========================================================================
// 10. onAttachedChange callback wired from props (lines 113-119)
// ===========================================================================

describe('onAttachedChange callback', () => {
  it('is called with true on successful attach (line 317)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_get_auto_attach_status') return AUTO_STATUS_OFF;
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      throw new Error(`unexpected: ${cmd}`);
    });

    const onAttachedChange = vi.fn();
    render(<TvBridgePanelBody onAttachedChange={onAttachedChange} />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));

    await waitFor(() => {
      expect(onAttachedChange).toHaveBeenCalledWith(true);
    });
  });
});
