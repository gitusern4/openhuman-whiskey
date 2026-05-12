import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import TradingViewBridgePanel from '../TradingViewBridgePanel';

// --- Tauri invoke mock -------------------------------------------------------
// The panel calls invoke() five ways:
//   tv_cdp_probe(port?)            -> ProbeResult
//   tv_cdp_attach(port?)           -> ProbeResult
//   tv_cdp_get_chart_state()       -> TvChartState
//   tv_cdp_detach()                -> void
//   tv_cdp_eval(expression)        -> not exercised by v1 UI
//
// We mock at the @tauri-apps/api/core layer so each test can dictate
// the exact return / throw shape.
type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
const mockInvoke = vi.fn<InvokeFn>();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => mockInvoke(cmd, args),
}));

vi.mock('../../components/SettingsHeader', () => ({
  default: ({ title }: { title: string }) => (
    <header data-testid="settings-header-stub">{title}</header>
  ),
}));

vi.mock('../../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({ navigateBack: vi.fn(), breadcrumbs: [] }),
}));

const REACHABLE_PROBE = {
  reachable: true,
  port: 9222,
  browser_ws_url: 'ws://127.0.0.1:9222/devtools/browser/abc',
  tv_targets: [
    { id: 'target-1', url: 'https://www.tradingview.com/chart/abc/', title: 'NQ — TradingView' },
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

const FIXTURE_CHART_STATE = {
  symbol: 'CME_MINI:NQ1!',
  resolution: '5',
  price: null,
  indicator_count: 3,
  raw: { _probe: { has_chartWidget: true } },
};

describe('TradingViewBridgePanel', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('runs an initial probe on mount and shows reachable status when TV responds', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<TradingViewBridgePanel />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    expect(mockInvoke).toHaveBeenCalledWith('tv_cdp_probe', { port: 9222 });
  });

  it('shows an error when the initial probe finds TV unreachable', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<TradingViewBridgePanel />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('unreachable')
    );
    expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent(/TV CDP unreachable/);
  });

  it('disables the Attach button until the probe says TV is reachable', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return UNREACHABLE_PROBE;
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<TradingViewBridgePanel />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('unreachable')
    );
    const attachBtn = screen.getByTestId('tv-bridge-attach-button') as HTMLButtonElement;
    expect(attachBtn.disabled).toBe(true);
  });

  it('attaches when the user clicks Attach and renders the chart-state card', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<TradingViewBridgePanel />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('attached')
    );
    // Chart-state card only renders once attached.
    expect(screen.getByTestId('tv-bridge-state-card')).toBeInTheDocument();
  });

  it('reads chart state on demand and renders the symbol + indicator count', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_get_chart_state') return FIXTURE_CHART_STATE;
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<TradingViewBridgePanel />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('attached')
    );

    fireEvent.click(screen.getByTestId('tv-bridge-refresh-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-symbol')).toHaveTextContent('CME_MINI:NQ1!')
    );
    expect(screen.getByTestId('tv-bridge-resolution')).toHaveTextContent('5');
    expect(screen.getByTestId('tv-bridge-indicator-count')).toHaveTextContent('3');
  });

  it('detaches and clears the chart-state card', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_detach') return undefined;
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<TradingViewBridgePanel />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('attached')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-detach-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    // Chart-state card unmounts on detach.
    expect(screen.queryByTestId('tv-bridge-state-card')).not.toBeInTheDocument();
  });

  it('surfaces an attach error in the alert region without flipping to attached', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') {
        throw new Error('No TradingView page target found');
      }
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<TradingViewBridgePanel />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    fireEvent.click(screen.getByTestId('tv-bridge-attach-button'));
    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-error')).toHaveTextContent(/No TradingView page target/)
    );
    expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable');
  });

  it('respects a user-overridden port in probe and attach calls', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return REACHABLE_PROBE;
      if (cmd === 'tv_cdp_attach') return REACHABLE_PROBE;
      throw new Error(`unexpected invoke: ${cmd}`);
    });
    render(<TradingViewBridgePanel />);

    await waitFor(() =>
      expect(screen.getByTestId('tv-bridge-status')).toHaveTextContent('reachable')
    );
    const portInput = screen.getByTestId('tv-bridge-port-input') as HTMLInputElement;
    fireEvent.change(portInput, { target: { value: '9333' } });
    fireEvent.click(screen.getByTestId('tv-bridge-probe-button'));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith('tv_cdp_probe', { port: 9333 }));
  });
});
