import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import ModesPanel from '../ModesPanel';

// --- Tauri invoke mock -------------------------------------------------------
// The panel calls invoke() three ways:
//   list_whiskey_modes        -> ModeDescriptor[]
//   get_active_whiskey_mode_id -> string
//   set_whiskey_mode (id)     -> void / throws on bad id
//
// We mock all three at the @tauri-apps/api/core layer so each test can
// dictate the exact return / throw shape.
type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
const mockInvoke = vi.fn<InvokeFn>();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => mockInvoke(cmd, args),
}));

// Settings header pulls in router context that's annoying to set up
// for a unit test — replace it with a minimal stub so the panel can
// render outside <BrowserRouter>.
vi.mock('../../components/SettingsHeader', () => ({
  default: ({ title }: { title: string }) => (
    <header data-testid="settings-header-stub">{title}</header>
  ),
}));

vi.mock('../../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({ navigateBack: vi.fn(), breadcrumbs: [] }),
}));

const FIXTURE_MODES = [
  { id: 'default', display_name: 'Default', description: 'Stock OpenHuman assistant.' },
  {
    id: 'whiskey',
    display_name: 'Whiskey',
    description: 'Trading mentor — reads your A+ catalog.',
  },
];

function setupHappyPath(activeId = 'default') {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'list_whiskey_modes') return FIXTURE_MODES;
    if (cmd === 'get_active_whiskey_mode_id') return activeId;
    if (cmd === 'set_whiskey_mode') return undefined;
    throw new Error(`unexpected invoke: ${cmd}`);
  });
}

describe('ModesPanel', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders both registered modes after the initial load', async () => {
    setupHappyPath();
    render(<ModesPanel />);

    await waitFor(() => expect(screen.getByTestId('mode-row-default')).toBeInTheDocument());
    expect(screen.getByTestId('mode-row-whiskey')).toBeInTheDocument();

    expect(screen.getByText('Default')).toBeInTheDocument();
    expect(screen.getByText('Whiskey')).toBeInTheDocument();
  });

  it('marks the active mode with the Active badge and disables its row', async () => {
    setupHappyPath('whiskey');
    render(<ModesPanel />);

    await waitFor(() =>
      expect(screen.getByTestId('mode-active-badge-whiskey')).toBeInTheDocument()
    );

    // Inactive row's Active badge must NOT exist.
    expect(screen.queryByTestId('mode-active-badge-default')).not.toBeInTheDocument();

    // Active row carries data-active="true" and is disabled (so the
    // user can't pointlessly switch to the mode they're already in).
    const activeRow = screen.getByTestId('mode-row-whiskey');
    expect(activeRow).toHaveAttribute('data-active', 'true');
    expect(activeRow).toBeDisabled();
  });

  it('switches mode when an inactive row is clicked', async () => {
    setupHappyPath('default');
    render(<ModesPanel />);

    const whiskeyRow = await screen.findByTestId('mode-row-whiskey');
    expect(whiskeyRow).toHaveAttribute('data-active', 'false');

    fireEvent.click(whiskeyRow);

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith('set_whiskey_mode', { id: 'whiskey' })
    );

    // After the switch, the active id should be whiskey and its
    // badge should appear without a re-fetch (we update local state
    // optimistically on success).
    await waitFor(() =>
      expect(screen.getByTestId('mode-active-badge-whiskey')).toBeInTheDocument()
    );
  });

  it('surfaces a load error in an alert when list_whiskey_modes fails', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_whiskey_modes') throw new Error('rpc down');
      if (cmd === 'get_active_whiskey_mode_id') return 'default';
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    render(<ModesPanel />);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(/Failed to load modes/i);
    expect(alert).toHaveTextContent(/rpc down/);
  });

  it('surfaces a switch error without losing the existing active id', async () => {
    // `const` because we only read it inside the closure — the test
    // captures the *initial* server-side active id and asserts the
    // panel doesn't optimistically flip away from it on a switch
    // failure. We don't need to mutate it.
    const activeId = 'default';
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_whiskey_modes') return FIXTURE_MODES;
      if (cmd === 'get_active_whiskey_mode_id') return activeId;
      if (cmd === 'set_whiskey_mode') {
        throw new Error('unknown id');
      }
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    render(<ModesPanel />);

    const whiskeyRow = await screen.findByTestId('mode-row-whiskey');
    fireEvent.click(whiskeyRow);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(/Could not switch to mode "whiskey"/);
    expect(alert).toHaveTextContent(/unknown id/);

    // Active mode is unchanged — the default row still owns the badge.
    expect(screen.getByTestId('mode-active-badge-default')).toBeInTheDocument();
    expect(screen.queryByTestId('mode-active-badge-whiskey')).not.toBeInTheDocument();

    // Local state never changed.
    expect(activeId).toBe('default');
  });

  it('does not call set_whiskey_mode when the active row is clicked', async () => {
    setupHappyPath('default');
    render(<ModesPanel />);

    const defaultRow = await screen.findByTestId('mode-row-default');
    fireEvent.click(defaultRow);

    // Wait one microtask so any erroneous invoke has a chance to fire.
    await Promise.resolve();

    expect(mockInvoke).not.toHaveBeenCalledWith('set_whiskey_mode', expect.anything());
  });
});
