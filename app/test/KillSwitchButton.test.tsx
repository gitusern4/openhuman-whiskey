/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * KillSwitchButton — vitest unit tests.
 *
 * 4+ tests covering:
 *   - Renders red button when not engaged
 *   - Button triggers kill_switch_trigger on click
 *   - Shows countdown when engaged + cooldown running
 *   - Reset panel appears when cooldown has elapsed
 *   - Reset error shown on wrong phrase
 */
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// Mock @tauri-apps/api/core
const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

import KillSwitchButton from '../src/components/settings/panels/KillSwitchButton';

const notEngagedStatus = {
  engaged: false,
  engaged_at: null,
  trigger: null,
  reset_after_utc: null,
  seconds_until_reset: null,
};

const engagedWithCooldown = {
  engaged: true,
  engaged_at: Math.floor(Date.now() / 1000) - 60,
  trigger: 'manual_button',
  reset_after_utc: Math.floor(Date.now() / 1000) + 1740,
  seconds_until_reset: 1740,
};

const engagedCooldownElapsed = {
  engaged: true,
  engaged_at: Math.floor(Date.now() / 1000) - 9999,
  trigger: 'manual_button',
  reset_after_utc: Math.floor(Date.now() / 1000) - 8199,
  seconds_until_reset: 0,
};

describe('KillSwitchButton', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue(notEngagedStatus);
  });

  it('renders red kill switch button when not engaged', async () => {
    mockInvoke.mockResolvedValue(notEngagedStatus);
    render(<KillSwitchButton />);
    await waitFor(() => {
      expect(screen.getByTestId('kill-switch-button')).toBeInTheDocument();
    });
    const btn = screen.getByTestId('kill-switch-button');
    expect(btn).not.toBeDisabled();
    expect(btn.textContent).toMatch(/KILL SWITCH/i);
  });

  it('calls kill_switch_trigger when clicked', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'kill_switch_status') return Promise.resolve(notEngagedStatus);
      if (cmd === 'kill_switch_trigger') return Promise.resolve();
      return Promise.resolve(notEngagedStatus);
    });
    render(<KillSwitchButton />);
    await waitFor(() => screen.getByTestId('kill-switch-button'));
    await act(async () => {
      fireEvent.click(screen.getByTestId('kill-switch-button'));
    });
    expect(mockInvoke).toHaveBeenCalledWith('kill_switch_trigger', {
      reason: 'manual_button',
    });
  });

  it('shows countdown when engaged and cooldown running', async () => {
    mockInvoke.mockResolvedValue(engagedWithCooldown);
    render(<KillSwitchButton />);
    await waitFor(() => {
      expect(screen.getByTestId('kill-switch-countdown')).toBeInTheDocument();
    });
    expect(screen.getByTestId('kill-switch-countdown').textContent).toMatch(/Reset eligible/);
  });

  it('shows reset panel when cooldown elapsed', async () => {
    mockInvoke.mockResolvedValue(engagedCooldownElapsed);
    render(<KillSwitchButton />);
    await waitFor(() => {
      expect(screen.getByTestId('kill-switch-reset-panel')).toBeInTheDocument();
    });
  });

  it('shows error when reset phrase is wrong', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'kill_switch_status') return Promise.resolve(engagedCooldownElapsed);
      if (cmd === 'kill_switch_request_reset')
        return Promise.reject('reset phrase mismatch');
      return Promise.resolve();
    });
    render(<KillSwitchButton />);
    await waitFor(() => screen.getByTestId('kill-switch-reset-panel'));
    fireEvent.change(screen.getByTestId('kill-switch-reset-input'), {
      target: { value: 'wrong' },
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId('kill-switch-reset-button'));
    });
    await waitFor(() => {
      expect(screen.getByTestId('kill-switch-reset-error')).toBeInTheDocument();
    });
  });
});
