/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * HotkeyStep — coverage for hotkey load, save-apply paths,
 * validation error, Enter key handling, skip, and next.
 *
 * Missing-coverage targets: lines 21, 38-40, 51-52, 61-62, 74-76, 113, 125.
 */
import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import HotkeyStep from '../HotkeyStep';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

beforeEach(() => {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'get_mascot_summon_hotkey') return 'CmdOrCtrl+Shift+Space';
    if (cmd === 'register_mascot_summon_hotkey') return undefined;
    throw new Error(`unexpected: ${cmd}`);
  });
});

// ===========================================================================
// 1. Hotkey load — pre-fills input with backend value (lines 33-36)
// ===========================================================================

describe('Hotkey load on mount', () => {
  it('pre-fills input with hotkey returned by get_mascot_summon_hotkey (lines 34-36)', async () => {
    render(<HotkeyStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => {
      const input = screen.getByRole('textbox') as HTMLInputElement;
      expect(input.value).toBe('CmdOrCtrl+Shift+Space');
    });
  });

  it('falls back to CmdOrCtrl+Shift+Space when load fails (lines 38-40)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_mascot_summon_hotkey') throw new Error('command not found');
      if (cmd === 'register_mascot_summon_hotkey') return undefined;
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<HotkeyStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => {
      const input = screen.getByRole('textbox') as HTMLInputElement;
      expect(input.value).toBe('CmdOrCtrl+Shift+Space');
    });
  });
});

// ===========================================================================
// 2. Apply button — success path shows "Hotkey registered" (lines 48-59)
// ===========================================================================

describe('Apply button', () => {
  it('calls register_mascot_summon_hotkey and shows success status (lines 57-59)', async () => {
    render(<HotkeyStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole('textbox')).toBeInTheDocument());

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'CmdOrCtrl+Alt+W' } });
    fireEvent.click(screen.getByRole('button', { name: /apply/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('register_mascot_summon_hotkey', {
        shortcut: 'CmdOrCtrl+Alt+W',
      });
      expect(screen.getByRole('status')).toHaveTextContent('Hotkey registered as');
    });
  });

  it('shows validation error for invalid shortcut (lines 50-52)', async () => {
    render(<HotkeyStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole('textbox')).toBeInTheDocument());

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'InvalidKey' } });
    fireEvent.click(screen.getByRole('button', { name: /apply/i }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Enter a valid shortcut');
    });
    // register_mascot_summon_hotkey must NOT have been called
    expect(mockInvoke).not.toHaveBeenCalledWith('register_mascot_summon_hotkey', expect.anything());
  });

  it('shows error when register_mascot_summon_hotkey throws (lines 61-62)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_mascot_summon_hotkey') return 'CmdOrCtrl+Shift+Space';
      if (cmd === 'register_mascot_summon_hotkey') throw new Error('hotkey already in use');
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<HotkeyStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole('textbox')).toBeInTheDocument());

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'CmdOrCtrl+Shift+W' } });
    fireEvent.click(screen.getByRole('button', { name: /apply/i }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Could not register hotkey');
      expect(screen.getByRole('alert')).toHaveTextContent('hotkey already in use');
    });
  });
});

// ===========================================================================
// 3. Enter key on input triggers handleSave (lines 74-76)
// ===========================================================================

describe('Enter key on input', () => {
  it('triggers Apply when Enter is pressed in the input (lines 74-76)', async () => {
    render(<HotkeyStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole('textbox')).toBeInTheDocument());

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'F12' } });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('register_mascot_summon_hotkey', { shortcut: 'F12' });
    });
  });

  it('validates (not saves) when Enter pressed with invalid shortcut (line 50)', async () => {
    render(<HotkeyStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole('textbox')).toBeInTheDocument());

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'bad' } });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });

    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument();
    });
    expect(mockInvoke).not.toHaveBeenCalledWith('register_mascot_summon_hotkey', expect.anything());
  });
});

// ===========================================================================
// 4. looksLikeValidShortcut accepts F-keys (line 21)
// ===========================================================================

describe('looksLikeValidShortcut F-key path (line 21)', () => {
  it("accepts 'F12' as a valid shortcut via Apply click", async () => {
    render(<HotkeyStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole('textbox')).toBeInTheDocument());

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'F12' } });
    fireEvent.click(screen.getByRole('button', { name: /apply/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('register_mascot_summon_hotkey', { shortcut: 'F12' });
    });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});

// ===========================================================================
// 5. Skip button (line 113)
// ===========================================================================

describe('Skip button', () => {
  it('calls onSkip when clicked (line 113)', async () => {
    const onSkip = vi.fn();
    render(<HotkeyStep onNext={vi.fn()} onSkip={onSkip} />);

    await waitFor(() => expect(screen.getByRole('button', { name: /skip/i })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /skip/i }));
    expect(onSkip).toHaveBeenCalledTimes(1);
  });
});

// ===========================================================================
// 6. Next button passes current hotkey to onNext (lines 68-70, 125)
// ===========================================================================

describe('Next button', () => {
  it('calls onNext with the current hotkey value (lines 68-70, 125)', async () => {
    const onNext = vi.fn();
    render(<HotkeyStep onNext={onNext} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole('textbox')).toBeInTheDocument());

    // Apply a new hotkey first
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'CmdOrCtrl+Shift+Q' } });
    fireEvent.click(screen.getByRole('button', { name: /apply/i }));

    await waitFor(() => {
      expect(screen.getByRole('status')).toHaveTextContent('Hotkey registered');
    });

    fireEvent.click(screen.getByRole('button', { name: /^next$/i }));
    expect(onNext).toHaveBeenCalledWith('CmdOrCtrl+Shift+Q');
  });
});
