import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  ConfirmationModal,
  hasDontShowAgainPreference,
  setDontShowAgainPreference,
} from '../ConfirmationModal';

describe('ConfirmationModal preferences', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
  });

  describe('hasDontShowAgainPreference', () => {
    it('returns false when no preference is set', () => {
      expect(hasDontShowAgainPreference('test-key')).toBe(false);
    });

    it('returns true when preference is set', () => {
      localStorage.setItem('openhuman:dontShowAgain:test-key', 'true');
      expect(hasDontShowAgainPreference('test-key')).toBe(true);
    });

    it('returns false when localStorage throws', () => {
      const getItemMock = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
        throw new Error('Quota exceeded');
      });
      expect(hasDontShowAgainPreference('test-key')).toBe(false);
      getItemMock.mockRestore();
    });
  });

  describe('setDontShowAgainPreference', () => {
    it('sets the preference in localStorage', () => {
      setDontShowAgainPreference('test-key', true);
      expect(localStorage.getItem('openhuman:dontShowAgain:test-key')).toBe('true');
    });

    it('removes the preference from localStorage', () => {
      localStorage.setItem('openhuman:dontShowAgain:test-key', 'true');
      setDontShowAgainPreference('test-key', false);
      expect(localStorage.getItem('openhuman:dontShowAgain:test-key')).toBeNull();
    });

    it('handles localStorage errors gracefully', () => {
      const warnMock = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const setItemMock = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
        throw new Error('Quota exceeded');
      });

      expect(() => setDontShowAgainPreference('test-key', true)).not.toThrow();
      expect(warnMock).toHaveBeenCalledWith(
        'Failed to save dontShowAgain preference to localStorage'
      );

      setItemMock.mockRestore();
      warnMock.mockRestore();
    });
  });

  describe('ConfirmationModal component', () => {
    const baseModal = {
      isOpen: true,
      title: 'Test Modal',
      message: 'Test message',
      onConfirm: vi.fn(),
      onCancel: vi.fn(),
      showDontShowAgain: true,
      dontShowAgainKey: 'custom-key',
    };

    it('saves preference using dontShowAgainKey when checked and confirmed', async () => {
      const user = userEvent.setup();
      render(<ConfirmationModal modal={baseModal} onClose={vi.fn()} />);

      const checkbox = screen.getByRole('checkbox', { name: /don't show similar/i });
      await user.click(checkbox);

      const confirmButton = screen.getByRole('button', { name: /confirm/i });
      await user.click(confirmButton);

      expect(localStorage.getItem('openhuman:dontShowAgain:custom-key')).toBe('true');
    });

    it('falls back to title if dontShowAgainKey is not provided', async () => {
      const user = userEvent.setup();
      const modalWithoutKey = { ...baseModal, dontShowAgainKey: undefined };
      render(<ConfirmationModal modal={modalWithoutKey} onClose={vi.fn()} />);

      const checkbox = screen.getByRole('checkbox', { name: /don't show similar/i });
      await user.click(checkbox);

      const confirmButton = screen.getByRole('button', { name: /confirm/i });
      await user.click(confirmButton);

      expect(localStorage.getItem('openhuman:dontShowAgain:Test Modal')).toBe('true');
    });

    it('does not save preference if checkbox is not checked', async () => {
      const user = userEvent.setup();
      render(<ConfirmationModal modal={baseModal} onClose={vi.fn()} />);

      const confirmButton = screen.getByRole('button', { name: /confirm/i });
      await user.click(confirmButton);

      expect(localStorage.getItem('openhuman:dontShowAgain:custom-key')).toBeNull();
    });
  });
});
