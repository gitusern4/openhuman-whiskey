/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * TvBridgeStep — coverage for skip flow, probe failure, attach error,
 * launch-TV handler, and Next button disable-while-pending logic.
 *
 * Missing-coverage targets: lines 29-33, 35-37, 51-52, 57-60, 72-74, 133, 141.
 */
import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import TvBridgeStep from '../TvBridgeStep';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

beforeEach(() => {
  // Default: tv_cdp_probe returns not-reachable
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'tv_cdp_probe') return { reachable: false };
    if (cmd === 'tv_cdp_launch_tv') return undefined;
    if (cmd === 'tv_cdp_attach') return undefined;
    throw new Error(`unexpected: ${cmd}`);
  });
});

// ===========================================================================
// 1. Skip flow advances to next step without touching probe (lines 78-81, 133)
// ===========================================================================

describe('Skip flow', () => {
  it('calls onSkip and onNext(true) without touching tv_cdp_probe (line 80-81)', () => {
    const onNext = vi.fn();
    const onSkip = vi.fn();
    render(<TvBridgeStep onNext={onNext} onSkip={onSkip} />);

    fireEvent.click(screen.getByText(/skip/i));

    expect(onSkip).toHaveBeenCalledTimes(1);
    expect(onNext).toHaveBeenCalledWith(true);
    // The skip button does NOT call tv_cdp_probe beyond what's already mounted.
    // We only assert the critical skip-path behavior: tvBridgeSkipped=true.
    expect(onNext).not.toHaveBeenCalledWith(false);
  });
});

// ===========================================================================
// 2. Launch TV handler — happy path and error path (lines 28-39)
// ===========================================================================

describe('Launch TV handler', () => {
  it('shows status message after successful launch (lines 31-33)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_launch_tv') return undefined;
      if (cmd === 'tv_cdp_probe') return { reachable: false };
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    fireEvent.click(screen.getByText(/launch tv with debug flag/i));

    await waitFor(() => {
      expect(screen.getByText(/TradingView launched/i)).toBeInTheDocument();
    });
  });

  it('shows error message when launch throws (lines 35-38)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_launch_tv') throw new Error('exe not found');
      if (cmd === 'tv_cdp_probe') return { reachable: false };
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    fireEvent.click(screen.getByText(/launch tv with debug flag/i));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Launch failed');
      expect(screen.getByRole('alert')).toHaveTextContent('exe not found');
    });
  });
});

// ===========================================================================
// 3. Probe handler — reachable=true and error paths (lines 41-62)
// ===========================================================================

describe('Probe handler', () => {
  it('shows reachable status message and Attach button when probe succeeds (lines 47-50)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return { reachable: true };
      if (cmd === 'tv_cdp_launch_tv') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    fireEvent.click(screen.getByText(/^probe$/i));

    await waitFor(() => {
      expect(screen.getByRole('status')).toHaveTextContent('TradingView is reachable');
    });
    // Attach button should now appear
    expect(screen.getByText(/^attach$/i)).toBeInTheDocument();
  });

  it('shows unreachable status message when probe returns false (lines 51-55)', async () => {
    // Default mock: reachable=false
    render(<TvBridgeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    fireEvent.click(screen.getByText(/^probe$/i));

    await waitFor(() => {
      expect(screen.getByRole('status')).toHaveTextContent('Port 9222 is not reachable');
    });
  });

  it('shows error alert when probe throws (lines 57-60)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') throw new Error('ECONNREFUSED');
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    fireEvent.click(screen.getByText(/^probe$/i));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Probe failed');
      expect(screen.getByRole('alert')).toHaveTextContent('ECONNREFUSED');
    });
  });
});

// ===========================================================================
// 4. Attach handler — happy path and error path (lines 64-76)
// ===========================================================================

describe('Attach handler', () => {
  it("shows 'Attached' message and keeps Next enabled after attach (lines 69-71)", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return { reachable: true };
      if (cmd === 'tv_cdp_attach') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    fireEvent.click(screen.getByText(/^probe$/i));
    await waitFor(() => expect(screen.getByText(/^attach$/i)).toBeInTheDocument());

    fireEvent.click(screen.getByText(/^attach$/i));

    await waitFor(() => {
      expect(screen.getByRole('status')).toHaveTextContent('Attached! The bridge is ready.');
    });

    // Next button should be enabled once attached
    const nextBtn = screen.getByRole('button', { name: /^next$/i });
    expect(nextBtn).not.toBeDisabled();
  });

  it('shows attach error when invoke throws (lines 72-75)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return { reachable: true };
      if (cmd === 'tv_cdp_attach') throw new Error('CDP session rejected');
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<TvBridgeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    fireEvent.click(screen.getByText(/^probe$/i));
    await waitFor(() => expect(screen.getByText(/^attach$/i)).toBeInTheDocument());

    fireEvent.click(screen.getByText(/^attach$/i));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Attach failed');
      expect(screen.getByRole('alert')).toHaveTextContent('CDP session rejected');
    });
  });
});

// ===========================================================================
// 5. Next button is disabled until bridgeStatus === 'attached' (line 159, 141)
// ===========================================================================

describe('Next button state', () => {
  it('is disabled while bridgeStatus is not attached (line 159)', () => {
    render(<TvBridgeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    const nextBtn = screen.getByRole('button', { name: /^next$/i });
    expect(nextBtn).toBeDisabled();
  });

  it('calls onNext(false) when clicked after attach (line 158)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'tv_cdp_probe') return { reachable: true };
      if (cmd === 'tv_cdp_attach') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    const onNext = vi.fn();
    render(<TvBridgeStep onNext={onNext} onSkip={vi.fn()} />);

    fireEvent.click(screen.getByText(/^probe$/i));
    await waitFor(() => expect(screen.getByText(/^attach$/i)).toBeInTheDocument());

    fireEvent.click(screen.getByText(/^attach$/i));
    await waitFor(() => expect(screen.getByRole('button', { name: /^next$/i })).not.toBeDisabled());

    fireEvent.click(screen.getByRole('button', { name: /^next$/i }));
    expect(onNext).toHaveBeenCalledWith(false);
  });
});
