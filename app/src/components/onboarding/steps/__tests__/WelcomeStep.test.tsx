/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * WelcomeStep — coverage for mode load failure, mode selection,
 * Next handler, keyboard selection, and skip flow.
 *
 * Missing-coverage targets: lines 36-38, 48, 53, 61-63, 79, 93-94.
 */
import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import WelcomeStep from '../WelcomeStep';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

const FIXTURE_MODES = [
  { id: 'default', display_name: 'Default', description: 'Standard Whiskey trading mentor.' },
  { id: 'whiskey', display_name: 'Whiskey Active', description: 'Full real-time coaching mode.' },
];

beforeEach(() => {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'list_whiskey_modes') return FIXTURE_MODES;
    if (cmd === 'get_active_whiskey_mode_id') return 'default';
    if (cmd === 'set_whiskey_mode') return undefined;
    throw new Error(`unexpected: ${cmd}`);
  });
});

// ===========================================================================
// 1. Mode load failure — error region shows friendly message (lines 36-38)
// ===========================================================================

describe('Mode load failure', () => {
  it('renders error message when list_whiskey_modes rejects (lines 36-38, 79)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_whiskey_modes') throw new Error('backend not running');
      if (cmd === 'get_active_whiskey_mode_id') throw new Error('backend not running');
      throw new Error(`unexpected: ${cmd}`);
    });

    render(<WelcomeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Could not load modes');
    });
  });
});

// ===========================================================================
// 2. Mode selection via click (lines 47-48, 93-94)
// ===========================================================================

describe('Mode selection', () => {
  it('marks the clicked mode as selected (line 48)', async () => {
    render(<WelcomeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByText('Whiskey Active')).toBeInTheDocument());

    const whiskeyCard = screen.getByRole('radio', { name: /whiskey active/i });
    fireEvent.click(whiskeyCard);

    expect(whiskeyCard).toHaveAttribute('aria-checked', 'true');
  });

  it('keyboard Enter selects a mode card (lines 61-63)', async () => {
    render(<WelcomeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByText('Whiskey Active')).toBeInTheDocument());

    const whiskeyCard = screen.getByRole('radio', { name: /whiskey active/i });
    fireEvent.keyDown(whiskeyCard, { key: 'Enter' });

    expect(whiskeyCard).toHaveAttribute('aria-checked', 'true');
  });

  it('keyboard Space selects a mode card (lines 61-63)', async () => {
    render(<WelcomeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByText('Whiskey Active')).toBeInTheDocument());

    const whiskeyCard = screen.getByRole('radio', { name: /whiskey active/i });
    fireEvent.keyDown(whiskeyCard, { key: ' ' });

    expect(whiskeyCard).toHaveAttribute('aria-checked', 'true');
  });
});

// ===========================================================================
// 3. Next button calls set_whiskey_mode and onNext (lines 51-55)
// ===========================================================================

describe('Next button', () => {
  it('calls set_whiskey_mode with the active mode id and fires onNext (lines 51-55)', async () => {
    const onNext = vi.fn();
    render(<WelcomeStep onNext={onNext} onSkip={vi.fn()} />);

    await waitFor(() => expect(screen.getByText('Whiskey Active')).toBeInTheDocument());

    // Select whiskey mode first
    fireEvent.click(screen.getByRole('radio', { name: /whiskey active/i }));

    // Click Next
    fireEvent.click(screen.getByRole('button', { name: /^next$/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('set_whiskey_mode', { id: 'whiskey' });
    });
    expect(onNext).toHaveBeenCalledWith('whiskey');
  });

  it("calls onNext with 'default' when no mode change is made (line 53)", async () => {
    const onNext = vi.fn();
    render(<WelcomeStep onNext={onNext} onSkip={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^next$/i })).toBeInTheDocument()
    );

    fireEvent.click(screen.getByRole('button', { name: /^next$/i }));

    await waitFor(() => {
      expect(onNext).toHaveBeenCalledWith('default');
    });
  });

  it('swallows set_whiskey_mode failure and still calls onNext (line 52-53)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_whiskey_modes') return FIXTURE_MODES;
      if (cmd === 'get_active_whiskey_mode_id') return 'default';
      if (cmd === 'set_whiskey_mode') throw new Error('command not found');
      throw new Error(`unexpected: ${cmd}`);
    });

    const onNext = vi.fn();
    render(<WelcomeStep onNext={onNext} onSkip={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^next$/i })).toBeInTheDocument()
    );

    fireEvent.click(screen.getByRole('button', { name: /^next$/i }));

    await waitFor(() => {
      expect(onNext).toHaveBeenCalled();
    });
    // No error shown — failure is swallowed
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});

// ===========================================================================
// 4. Skip button calls onSkip (line 111)
// ===========================================================================

describe('Skip button', () => {
  it('calls onSkip when clicked', async () => {
    const onSkip = vi.fn();
    render(<WelcomeStep onNext={vi.fn()} onSkip={onSkip} />);

    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^skip$/i })).toBeInTheDocument()
    );

    fireEvent.click(screen.getByRole('button', { name: /^skip$/i }));
    expect(onSkip).toHaveBeenCalledTimes(1);
  });
});

// ===========================================================================
// 5. Loading state — shows "Loading modes…" before data arrives (line 86)
// ===========================================================================

describe('Loading state', () => {
  it('shows loading placeholder before modes load (line 86)', () => {
    // Make invoke block indefinitely to catch the loading state
    mockInvoke.mockImplementation(
      () => new Promise(() => {}) // never resolves
    );

    render(<WelcomeStep onNext={vi.fn()} onSkip={vi.fn()} />);

    expect(screen.getByText(/loading modes/i)).toBeInTheDocument();
  });
});
