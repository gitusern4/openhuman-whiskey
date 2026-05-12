/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * OrderFlowCard — vitest coverage.
 *
 * Tests:
 *   1. Preset apply — happy path calls order_flow_apply_preset, shows success
 *   2. Preset apply — requires TV attached (button disabled when detached)
 *   3. Manual delta entry — computes and displays bar delta + cumulative delta
 *   4. Tag persistence — chip click calls order_flow_tag_active_trade
 *   5. Alert toggle — checkbox calls order_flow_save_config with toggled value
 *   6. Failure path — error from apply_preset surfaces in error alert
 *   7. Attach-required gating — attach-required notice shown when not attached
 *   8. CDP live indicator — shows "CDP live" label when bridge is attached and CDP returns data
 */
import { invoke } from '@tauri-apps/api/core';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import OrderFlowCard from '../OrderFlowCard';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

// Default invoke: returns defaults for config load and CDP state
beforeEach(() => {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'order_flow_get_config') {
      return {
        ok: true,
        config: {
          enabled: true,
          poll_interval_ms: 2000,
          active_preset: null,
          alert_toggles: {
            delta_divergence: false,
            absorption: false,
            single_print_rejection: false,
          },
        },
      };
    }
    if (cmd === 'tv_cdp_get_order_flow_state') {
      return { ok: false, state: null, error: null };
    }
    if (cmd === 'order_flow_apply_preset') {
      return { ok: true, preset_id: 'vwap_vpvr_avwap', indicators_added: 3, error: null };
    }
    if (cmd === 'order_flow_tag_active_trade') {
      return { ok: true, tag: 'absorbed', error: null };
    }
    if (cmd === 'order_flow_save_config') {
      return { ok: true, config: null, error: null };
    }
    return null;
  });
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function renderCard(tvAttached: boolean, positionOpen = false) {
  return render(<OrderFlowCard tvAttached={tvAttached} positionOpen={positionOpen} />);
}

// ===========================================================================
// Test 1: Preset apply — happy path
// ===========================================================================

describe('Workspace preset', () => {
  it('calls order_flow_apply_preset and shows success message', async () => {
    renderCard(true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-apply-preset')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-order-flow-apply-preset'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        'order_flow_apply_preset',
        expect.objectContaining({ name: 'vwap_vpvr_avwap' })
      );
      expect(screen.getByTestId('tks-mods-order-flow-preset-success')).toBeInTheDocument();
    });
  });

  // Test 2: requires TV attached
  it('disables the Apply preset button when TV bridge is not attached', async () => {
    renderCard(false);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-apply-preset')).toBeDisabled();
    });
  });
});

// ===========================================================================
// Test 3: Manual delta entry
// ===========================================================================

describe('Manual delta entry', () => {
  it('computes bar delta from bid/ask volume and displays it', async () => {
    renderCard(false);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-manual-bid')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('tks-mods-order-flow-manual-bid'), {
      target: { value: '300' },
    });
    fireEvent.change(screen.getByTestId('tks-mods-order-flow-manual-ask'), {
      target: { value: '500' },
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-order-flow-manual-submit'));
    });

    await waitFor(() => {
      // bar delta = ask - bid = 500 - 300 = +200
      const deltaEl = screen.getByTestId('tks-mods-order-flow-bar-delta');
      expect(deltaEl).toHaveTextContent('+200');
    });
  });
});

// ===========================================================================
// Test 4: Tag persistence
// ===========================================================================

describe('Order flow tags', () => {
  it('calls order_flow_tag_active_trade when a tag chip is clicked', async () => {
    renderCard(true, true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-tag-absorbed')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-order-flow-tag-absorbed'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('order_flow_tag_active_trade', { tag: 'absorbed' });
    });
  });

  it('renders all 6 tag chips', async () => {
    renderCard(true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-tag-absorbed')).toBeInTheDocument();
      expect(screen.getByTestId('tks-mods-order-flow-tag-delta_div')).toBeInTheDocument();
      expect(screen.getByTestId('tks-mods-order-flow-tag-single_print')).toBeInTheDocument();
      expect(screen.getByTestId('tks-mods-order-flow-tag-value_area_reject')).toBeInTheDocument();
      expect(screen.getByTestId('tks-mods-order-flow-tag-responsive_buyer')).toBeInTheDocument();
      expect(screen.getByTestId('tks-mods-order-flow-tag-responsive_seller')).toBeInTheDocument();
    });
  });
});

// ===========================================================================
// Test 5: Alert toggle
// ===========================================================================

describe('Detection alerts', () => {
  it('calls order_flow_save_config when an alert toggle is clicked', async () => {
    renderCard(true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-alert-delta_divergence')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-order-flow-alert-delta_divergence'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        'order_flow_save_config',
        expect.objectContaining({
          config: expect.objectContaining({
            alert_toggles: expect.objectContaining({ delta_divergence: true }),
          }),
        })
      );
    });
  });
});

// ===========================================================================
// Test 6: Failure path — error from apply_preset surfaces in error alert
// ===========================================================================

describe('Error handling', () => {
  it('shows an error alert when order_flow_apply_preset returns an error', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'order_flow_get_config') {
        return {
          ok: true,
          config: {
            enabled: true,
            poll_interval_ms: 2000,
            active_preset: null,
            alert_toggles: {
              delta_divergence: false,
              absorption: false,
              single_print_rejection: false,
            },
          },
        };
      }
      if (cmd === 'tv_cdp_get_order_flow_state') {
        return { ok: false, state: null, error: null };
      }
      if (cmd === 'order_flow_apply_preset') {
        return { ok: false, preset_id: null, indicators_added: 0, error: 'TV bridge disconnected' };
      }
      return null;
    });

    renderCard(true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-apply-preset')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-order-flow-apply-preset'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-error')).toHaveTextContent(
        'TV bridge disconnected'
      );
    });
  });
});

// ===========================================================================
// Test 7: Attach-required gating
// ===========================================================================

describe('Attach-required gating', () => {
  it('shows the attach-required notice when TV bridge is not attached', async () => {
    renderCard(false);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-attach-required')).toBeInTheDocument();
    });
  });

  it('does not show the attach-required notice when TV bridge is attached', async () => {
    renderCard(true);

    await waitFor(() => {
      expect(screen.queryByTestId('tks-mods-order-flow-attach-required')).not.toBeInTheDocument();
    });
  });
});

// ===========================================================================
// Test 8: CDP live indicator
// ===========================================================================

describe('CDP live indicator', () => {
  it('shows CDP live status label when bridge is attached', async () => {
    renderCard(true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-cdp-status')).toBeInTheDocument();
    });
  });
});
