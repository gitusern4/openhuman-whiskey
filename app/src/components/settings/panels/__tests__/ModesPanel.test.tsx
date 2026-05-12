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

const FIXTURE_DEFAULT_HOTKEY = 'CmdOrCtrl+Shift+Space';

function setupHappyPath(activeId = 'default') {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'list_whiskey_modes') return FIXTURE_MODES;
    if (cmd === 'get_active_whiskey_mode_id') return activeId;
    if (cmd === 'set_whiskey_mode') return undefined;
    if (cmd === 'get_mascot_summon_hotkey') return FIXTURE_DEFAULT_HOTKEY;
    if (cmd === 'register_mascot_summon_hotkey') return undefined;
    if (cmd === 'unregister_mascot_summon_hotkey') return undefined;
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
      if (cmd === 'get_mascot_summon_hotkey') return FIXTURE_DEFAULT_HOTKEY;
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
      if (cmd === 'get_mascot_summon_hotkey') return FIXTURE_DEFAULT_HOTKEY;
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

  // --- Mascot summon hotkey card -------------------------------------------
  it('renders the mascot summon hotkey card with the loaded value', async () => {
    setupHappyPath();
    render(<ModesPanel />);

    const card = await screen.findByTestId('mascot-summon-hotkey-card');
    expect(card).toBeInTheDocument();

    await waitFor(() =>
      expect(screen.getByTestId('mascot-summon-hotkey-current')).toHaveTextContent(
        FIXTURE_DEFAULT_HOTKEY
      )
    );

    const input = screen.getByTestId('mascot-summon-hotkey-input') as HTMLInputElement;
    expect(input.value).toBe(FIXTURE_DEFAULT_HOTKEY);
  });

  it('invokes register_mascot_summon_hotkey with the entered shortcut on Save', async () => {
    setupHappyPath();
    render(<ModesPanel />);

    const input = (await screen.findByTestId('mascot-summon-hotkey-input')) as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe(FIXTURE_DEFAULT_HOTKEY));

    fireEvent.change(input, { target: { value: 'CmdOrCtrl+Alt+M' } });
    fireEvent.click(screen.getByTestId('mascot-summon-hotkey-save'));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith('register_mascot_summon_hotkey', {
        shortcut: 'CmdOrCtrl+Alt+M',
      })
    );

    await waitFor(() =>
      expect(screen.getByTestId('mascot-summon-hotkey-current')).toHaveTextContent(
        'CmdOrCtrl+Alt+M'
      )
    );
  });

  it('surfaces a register_mascot_summon_hotkey error in the alert box', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_whiskey_modes') return FIXTURE_MODES;
      if (cmd === 'get_active_whiskey_mode_id') return 'default';
      if (cmd === 'get_mascot_summon_hotkey') return FIXTURE_DEFAULT_HOTKEY;
      if (cmd === 'register_mascot_summon_hotkey') {
        throw new Error('shortcut already in use');
      }
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    render(<ModesPanel />);

    const input = (await screen.findByTestId('mascot-summon-hotkey-input')) as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe(FIXTURE_DEFAULT_HOTKEY));

    fireEvent.change(input, { target: { value: 'CmdOrCtrl+Alt+J' } });
    fireEvent.click(screen.getByTestId('mascot-summon-hotkey-save'));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(/Could not register mascot summon hotkey/);
    expect(alert).toHaveTextContent(/shortcut already in use/);

    // The "currently registered" label should still show the original
    // value — we only flip it on success.
    expect(screen.getByTestId('mascot-summon-hotkey-current')).toHaveTextContent(
      FIXTURE_DEFAULT_HOTKEY
    );
  });
});
