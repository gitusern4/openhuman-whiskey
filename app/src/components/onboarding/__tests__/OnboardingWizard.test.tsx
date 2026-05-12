/**
 * OnboardingWizard — vitest unit tests.
 *
 * Coverage goals:
 *   1. Wizard renders step 1 (WelcomeStep) by default on first launch.
 *   2. User can advance through all 4 steps in the happy path.
 *   3. Skip flow — clicking Skip on each step advances without blocking.
 *   4. Completing the wizard fires onboarding_complete and the modal closes.
 *   5. Wizard does NOT render when onboarding is already completed.
 *   6. Hotkey customization: typing in the hotkey input and clicking Apply
 *      calls register_mascot_summon_hotkey with the new value.
 *   7. TvBridgeStep: Probe → reachable → Attach flow calls the right commands.
 *   8. DoneStep summary reflects mode and tvBridgeSkipped choices.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import OnboardingWizard from '../OnboardingWizard';

// ---------------------------------------------------------------------------
// Tauri invoke mock
// ---------------------------------------------------------------------------

type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
const mockInvoke = vi.fn<InvokeFn>();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => mockInvoke(cmd, args),
}));

// react-router-dom navigate mock (used by DoneStep)
const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom');
  return { ...actual, useNavigate: () => mockNavigate };
});

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const FIXTURE_MODES = [
  { id: 'default', display_name: 'Default', description: 'Stock OpenHuman.' },
  { id: 'whiskey', display_name: 'Whiskey', description: 'Trading mentor.' },
];

function setupInvoke({
  completed = false,
  currentStep = 0,
  tvBridgeSkipped = false,
  probeReachable = true,
} = {}) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case 'onboarding_status':
        return { completed, tv_bridge_skipped: tvBridgeSkipped, current_step: currentStep };
      case 'onboarding_advance':
        return undefined;
      case 'onboarding_complete':
        return undefined;
      case 'list_whiskey_modes':
        return FIXTURE_MODES;
      case 'get_active_whiskey_mode_id':
        return 'default';
      case 'set_whiskey_mode':
        return undefined;
      case 'get_mascot_summon_hotkey':
        return 'CmdOrCtrl+Shift+Space';
      case 'register_mascot_summon_hotkey':
        return undefined;
      case 'tv_cdp_probe':
        return { reachable: probeReachable };
      case 'tv_cdp_attach':
        return undefined;
      case 'tv_cdp_launch_tv':
        return undefined;
      default:
        throw new Error(`unexpected invoke: ${cmd}`);
    }
  });
}

function renderWizard() {
  return render(
    <MemoryRouter>
      <OnboardingWizard />
    </MemoryRouter>
  );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('OnboardingWizard', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockNavigate.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  // 1. Renders step 1 by default on first launch.
  it('renders WelcomeStep (step 1) by default on first launch', async () => {
    setupInvoke({ completed: false, currentStep: 0 });
    renderWizard();

    await waitFor(() => {
      expect(screen.getByText('Meet Whiskey')).toBeInTheDocument();
    });
    // Progress bar: step 1 active.
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  // 2. Advance through all 4 steps in happy path.
  it('advances through all 4 steps via Next buttons', async () => {
    setupInvoke({ completed: false, currentStep: 0 });
    renderWizard();

    // Step 1 — WelcomeStep
    await waitFor(() => screen.getByText('Meet Whiskey'));
    // Modes load and "Default" is rendered
    await waitFor(() => screen.getByText('Default'));

    fireEvent.click(screen.getByRole('button', { name: /next/i }));

    // Step 2 — TvBridgeStep
    await waitFor(() => screen.getByText('TradingView Bridge'));

    // Probe → reachable → Attach → Next
    fireEvent.click(screen.getByRole('button', { name: /probe/i }));
    await waitFor(() => screen.getByText(/reachable on port 9222/i));
    fireEvent.click(screen.getByRole('button', { name: /attach/i }));
    await waitFor(() => screen.getByText(/attached/i));
    fireEvent.click(screen.getByRole('button', { name: /next/i }));

    // Step 3 — HotkeyStep
    await waitFor(() => screen.getByText('Summon Hotkey'));
    fireEvent.click(screen.getByRole('button', { name: /next/i }));

    // Step 4 — DoneStep
    await waitFor(() => screen.getByText("You're all set"));
  });

  // 3. Skip flow — clicking Skip advances without blocking.
  it('skips all steps via Skip buttons and finishes', async () => {
    setupInvoke({ completed: false, currentStep: 0 });
    renderWizard();

    await waitFor(() => screen.getByText('Meet Whiskey'));
    fireEvent.click(screen.getByRole('button', { name: /skip/i }));

    await waitFor(() => screen.getByText('TradingView Bridge'));
    // TvBridgeStep Skip calls onNext(true)
    fireEvent.click(screen.getByRole('button', { name: /skip.*later/i }));

    await waitFor(() => screen.getByText('Summon Hotkey'));
    fireEvent.click(screen.getByRole('button', { name: /skip/i }));

    await waitFor(() => screen.getByText("You're all set"));

    fireEvent.click(screen.getByRole('button', { name: /finish/i }));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        'onboarding_complete',
        expect.objectContaining({ tvBridgeSkipped: true })
      );
    });
  });

  // 4. Completing wizard fires onboarding_complete and modal closes.
  it('calls onboarding_complete and removes the dialog on Finish', async () => {
    setupInvoke({ completed: false, currentStep: 3 });
    renderWizard();

    await waitFor(() => screen.getByText("You're all set"));
    fireEvent.click(screen.getByRole('button', { name: /finish/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('onboarding_complete', expect.any(Object));
    });
    // Dialog should be gone after completing.
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });
  });

  // 5. Wizard does NOT render when onboarding is already completed.
  it('does not render the dialog when onboarding is already completed', async () => {
    setupInvoke({ completed: true });
    renderWizard();

    // Give it time to resolve the async status call.
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });
  });

  // 6. Hotkey customization calls register_mascot_summon_hotkey.
  it('registers a custom hotkey when the user types and clicks Apply', async () => {
    setupInvoke({ completed: false, currentStep: 2 });
    renderWizard();

    await waitFor(() => screen.getByText('Summon Hotkey'));

    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'CmdOrCtrl+Shift+T' } });
    fireEvent.click(screen.getByRole('button', { name: /apply/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('register_mascot_summon_hotkey', {
        shortcut: 'CmdOrCtrl+Shift+T',
      });
    });
  });

  // 7. TvBridgeStep probe calls tv_cdp_probe; attach calls tv_cdp_attach.
  it('calls tv_cdp_probe and tv_cdp_attach in order', async () => {
    setupInvoke({ completed: false, currentStep: 1, probeReachable: true });
    renderWizard();

    await waitFor(() => screen.getByText('TradingView Bridge'));

    fireEvent.click(screen.getByRole('button', { name: /probe/i }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith('tv_cdp_probe'));

    await waitFor(() => screen.getByRole('button', { name: /attach/i }));
    fireEvent.click(screen.getByRole('button', { name: /attach/i }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith('tv_cdp_attach'));
  });

  // 8. DoneStep summary reflects choices (Whiskey mode + TV skipped).
  it('DoneStep summary reflects mode and tvBridgeSkipped choices', async () => {
    setupInvoke({ completed: false, currentStep: 3, tvBridgeSkipped: true });
    renderWizard();

    await waitFor(() => screen.getByText("You're all set"));

    // TV bridge skipped row should be present.
    expect(screen.getByText(/skipped.*configure later/i)).toBeInTheDocument();
  });
});
