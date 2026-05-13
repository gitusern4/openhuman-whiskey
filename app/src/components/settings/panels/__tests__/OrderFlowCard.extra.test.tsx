/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * OrderFlowCard — supplementary tests for uncovered branches:
 *   - Each of the 6 tag chips invokes order_flow_tag_active_trade with the
 *     correct tag id (lines 219, 261-262, 278-281, 285, 292-295, 299, 302,
 *     307, 312, 317-318)
 *   - Preset-apply failure surfaces the error message (line 278-281)
 *   - tag chip shows transient checkmark confirmation (lastTag state)
 *   - preset error from a thrown exception
 *
 * Complements OrderFlowCard.test.tsx.
 */
import { invoke } from '@tauri-apps/api/core';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import OrderFlowCard from '../OrderFlowCard';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

const DEFAULT_CONFIG = {
  ok: true,
  config: {
    enabled: true,
    poll_interval_ms: 2000,
    active_preset: null,
    alert_toggles: { delta_divergence: false, absorption: false, single_print_rejection: false },
  },
};

beforeEach(() => {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'order_flow_get_config') return DEFAULT_CONFIG;
    if (cmd === 'tv_cdp_get_order_flow_state') return { ok: false, state: null, error: null };
    if (cmd === 'order_flow_apply_preset')
      return { ok: true, preset_id: 'vwap_profile_anchored', indicators_added: 3, error: null };
    if (cmd === 'order_flow_tag_active_trade') return { ok: true, tag: null, error: null };
    if (cmd === 'order_flow_set_config') return { ok: true, config: null, error: null };
    return null;
  });
});

function renderCard(tvAttached = true, positionOpen = true) {
  return render(<OrderFlowCard tvAttached={tvAttached} positionOpen={positionOpen} />);
}

// ===========================================================================
// Each tag chip invokes order_flow_tag_active_trade with the correct tag id
// (lines 393-394 of OrderFlowCard — the onClick handler)
// ===========================================================================

describe('Tag chips — each chip invokes correct tag id', () => {
  const TAG_CASES: Array<{ testId: string; tag: string }> = [
    { testId: 'tks-mods-order-flow-tag-absorbed', tag: 'absorbed' },
    { testId: 'tks-mods-order-flow-tag-delta_div', tag: 'delta_div' },
    { testId: 'tks-mods-order-flow-tag-single_print', tag: 'single_print' },
    { testId: 'tks-mods-order-flow-tag-value_area_reject', tag: 'value_area_reject' },
    { testId: 'tks-mods-order-flow-tag-responsive_buyer', tag: 'responsive_buyer' },
    { testId: 'tks-mods-order-flow-tag-responsive_seller', tag: 'responsive_seller' },
  ];

  for (const { testId, tag } of TAG_CASES) {
    it(`invokes order_flow_tag_active_trade with tag="${tag}" when chip clicked`, async () => {
      renderCard(true, true);

      await waitFor(() => {
        expect(screen.getByTestId(testId)).toBeInTheDocument();
      });

      await act(async () => {
        fireEvent.click(screen.getByTestId(testId));
      });

      await waitFor(() => {
        expect(mockInvoke).toHaveBeenCalledWith('order_flow_tag_active_trade', { tag });
      });
    });
  }
});

// ===========================================================================
// Tag chip shows transient checkmark when lastTag matches (line 399)
// ===========================================================================

describe('Tag chip transient confirmation', () => {
  it('shows checkmark on the clicked chip immediately after click (line 399)', async () => {
    renderCard(true, true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-tag-absorbed')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-order-flow-tag-absorbed'));
    });

    // The chip text gains " ✓" when lastTag === t.value
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-tag-absorbed')).toHaveTextContent('✓');
    });
  });
});

// ===========================================================================
// Preset apply — failure from exception (useOrderFlow catch block, line 167)
// ===========================================================================

describe('Preset apply — exception path', () => {
  it('surfaces exception message as error when order_flow_apply_preset throws', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'order_flow_get_config') return DEFAULT_CONFIG;
      if (cmd === 'tv_cdp_get_order_flow_state') return { ok: false, state: null, error: null };
      if (cmd === 'order_flow_apply_preset') throw new Error('CDP bridge dropped');
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
        'CDP bridge dropped'
      );
    });
  });
});

// ===========================================================================
// Tag chip — tagActiveTrade returns ok=false (useOrderFlow line 181-182)
// ===========================================================================

describe('Tag chip — backend returns ok=false', () => {
  it('surfaces error when order_flow_tag_active_trade returns ok=false', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'order_flow_get_config') return DEFAULT_CONFIG;
      if (cmd === 'tv_cdp_get_order_flow_state') return { ok: false, state: null, error: null };
      if (cmd === 'order_flow_tag_active_trade')
        return { ok: false, tag: null, error: 'No active trade to tag' };
      return null;
    });

    renderCard(true, true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-tag-absorbed')).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-order-flow-tag-absorbed'));
    });

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-error')).toHaveTextContent(
        'No active trade to tag'
      );
    });
  });
});

// ===========================================================================
// Preset select change (line 219)
// ===========================================================================

describe('Preset select', () => {
  it('applies the correct preset when a different preset is selected and Apply is clicked (line 219)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'order_flow_get_config') return DEFAULT_CONFIG;
      if (cmd === 'tv_cdp_get_order_flow_state') return { ok: false, state: null, error: null };
      if (cmd === 'order_flow_apply_preset')
        return { ok: true, preset_id: 'delta_focused', indicators_added: 2, error: null };
      return null;
    });

    renderCard(true);

    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-preset-select')).toBeInTheDocument();
    });

    // Switch to the delta_focused preset
    fireEvent.change(screen.getByTestId('tks-mods-order-flow-preset-select'), {
      target: { value: 'delta_focused' },
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId('tks-mods-order-flow-apply-preset'));
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('order_flow_apply_preset', { name: 'delta_focused' });
    });
    // Success message should name the preset label
    await waitFor(() => {
      expect(screen.getByTestId('tks-mods-order-flow-preset-success')).toHaveTextContent(
        'CVD + Volume Delta'
      );
    });
  });
});
