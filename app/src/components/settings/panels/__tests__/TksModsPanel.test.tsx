/**
 * TksModsPanel — Vitest / React Testing Library unit tests.
 *
 * Coverage targets (6+ tests required):
 *   1. Theme switch — dropdown triggers useTheme.setTheme and renders
 *      the ZETH-active label.
 *   2. SL/TP draw happy path — calls tv_cdp_draw_sltp with correct args.
 *   3. SL/TP draw failure — backend error surfaces in the alert region.
 *   4. Risk-hide toggle — checkbox flip persists to localStorage.
 *   5. Clear-overlays call — invokes tv_cdp_clear_sltp, shows success msg.
 *   6. Alphabetical-ordering invariant — Entry/Stop/Target inputs render
 *      in the documented order (Entry first, Stop second, Target third).
 *   7. R-multiple display — computed and shown next to Target label.
 *   8. Missing TV bridge (invoke throws) — error surfaces gracefully.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import TksModsPanel from '../TksModsPanel';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

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

// Stub useTheme so we can inspect setTheme calls without DOM attribute logic.
const mockSetTheme = vi.fn();
let mockThemeId = 'default';

vi.mock('../../../hooks/useTheme', () => ({
  THEMES: [
    { id: 'default', label: 'Default (stone / sage)' },
    { id: 'zeth', label: 'ZETH (black / neon green)' },
  ],
  useTheme: () => ({ theme: mockThemeId, setTheme: mockSetTheme }),
}));

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const OK_DRAW_RESULT = { ok: true, removed: null, error: null };
const OK_CLEAR_RESULT = { ok: true, removed: 3, error: null };
const FAIL_DRAW_RESULT = { ok: false, removed: null, error: 'createMultipointShape unavailable' };

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('TksModsPanel', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockSetTheme.mockReset();
    mockThemeId = 'default';
    localStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // 1. Theme switch
  it('calls setTheme with "zeth" when the user picks the ZETH option', async () => {
    render(<TksModsPanel />);
    const select = screen.getByTestId('tks-mods-theme-select') as HTMLSelectElement;
    fireEvent.change(select, { target: { value: 'zeth' } });
    expect(mockSetTheme).toHaveBeenCalledWith('zeth');
  });

  it('shows the ZETH-active label when theme is zeth', () => {
    mockThemeId = 'zeth';
    render(<TksModsPanel />);
    expect(screen.getByTestId('tks-mods-zeth-active-label')).toBeInTheDocument();
  });

  // 2. SL/TP draw happy path
  it('draws SL/TP lines and shows success feedback', async () => {
    mockInvoke.mockResolvedValue(OK_DRAW_RESULT);
    render(<TksModsPanel />);

    fireEvent.change(screen.getByTestId('tks-mods-sltp-entry'), { target: { value: '19800' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-stop'), { target: { value: '19750' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-target'), { target: { value: '19900' } });
    fireEvent.click(screen.getByTestId('tks-mods-draw-button'));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith('tv_cdp_draw_sltp', {
        entry: 19800,
        stop: 19750,
        target: 19900,
        zethTheme: false,
      })
    );
    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-sltp-success')).toHaveTextContent('Lines drawn')
    );
    expect(screen.queryByTestId('tks-mods-error')).not.toBeInTheDocument();
  });

  // 3. SL/TP draw failure
  it('surfaces a draw error in the alert region', async () => {
    mockInvoke.mockResolvedValue(FAIL_DRAW_RESULT);
    render(<TksModsPanel />);

    fireEvent.change(screen.getByTestId('tks-mods-sltp-entry'), { target: { value: '100' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-stop'), { target: { value: '90' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-target'), { target: { value: '120' } });
    fireEvent.click(screen.getByTestId('tks-mods-draw-button'));

    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-error')).toHaveTextContent(
        /createMultipointShape unavailable/
      )
    );
  });

  // 4. Risk-hide toggle
  it('persists the risk-hide state to localStorage when toggled', () => {
    render(<TksModsPanel />);
    const checkbox = screen.getByTestId('tks-mods-risk-hide-toggle') as HTMLInputElement;
    expect(checkbox.checked).toBe(false);

    fireEvent.click(checkbox);
    expect(checkbox.checked).toBe(true);
    expect(localStorage.getItem('tk-hide-risk-pct')).toBe('true');

    fireEvent.click(checkbox);
    expect(checkbox.checked).toBe(false);
    expect(localStorage.getItem('tk-hide-risk-pct')).toBe('false');
  });

  // 5. Clear overlays
  it('invokes tv_cdp_clear_sltp and shows removed count', async () => {
    mockInvoke.mockResolvedValue(OK_CLEAR_RESULT);
    render(<TksModsPanel />);

    fireEvent.click(screen.getByTestId('tks-mods-clear-button'));

    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith('tv_cdp_clear_sltp', undefined));
    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-sltp-success')).toHaveTextContent('Cleared 3')
    );
  });

  // 6. Alphabetical ordering of price inputs: Entry < Stop < Target
  it('renders Entry, Stop, Target inputs in correct order', () => {
    render(<TksModsPanel />);
    const entry = screen.getByTestId('tks-mods-sltp-entry');
    const stop = screen.getByTestId('tks-mods-sltp-stop');
    const target = screen.getByTestId('tks-mods-sltp-target');

    // Verify all three are present.
    expect(entry).toBeInTheDocument();
    expect(stop).toBeInTheDocument();
    expect(target).toBeInTheDocument();

    // Verify DOM order: Entry comes before Stop, Stop comes before Target.
    const all = screen.getAllByRole('spinbutton'); // <input type="number">
    const ids = all.map((el: HTMLElement) => (el as HTMLInputElement).dataset.testid);
    const entryIdx = ids.indexOf('tks-mods-sltp-entry');
    const stopIdx = ids.indexOf('tks-mods-sltp-stop');
    const targetIdx = ids.indexOf('tks-mods-sltp-target');
    expect(entryIdx).toBeLessThan(stopIdx);
    expect(stopIdx).toBeLessThan(targetIdx);
  });

  // 7. R-multiple display
  it('computes and shows the R-multiple label next to Target', async () => {
    render(<TksModsPanel />);
    fireEvent.change(screen.getByTestId('tks-mods-sltp-entry'), { target: { value: '100' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-stop'), { target: { value: '90' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-target'), { target: { value: '120' } });

    // Risk = 10, Reward = 20 → 2.00R
    await waitFor(() => expect(screen.getByTestId('tks-mods-r-label')).toHaveTextContent('2.00R'));
  });

  // 8. Invoke throw (bridge not attached) — graceful error display
  it('shows an error when the CDP bridge is not attached (invoke throws)', async () => {
    mockInvoke.mockRejectedValue(new Error('Not attached to TV. Call tv_cdp_attach first.'));
    render(<TksModsPanel />);

    fireEvent.change(screen.getByTestId('tks-mods-sltp-entry'), { target: { value: '200' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-stop'), { target: { value: '190' } });
    fireEvent.change(screen.getByTestId('tks-mods-sltp-target'), { target: { value: '220' } });
    fireEvent.click(screen.getByTestId('tks-mods-draw-button'));

    await waitFor(() =>
      expect(screen.getByTestId('tks-mods-error')).toHaveTextContent(/Not attached/)
    );
  });
});
