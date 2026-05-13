/**
 * Step 3 — Customize the mascot summon hotkey.
 *
 * Shows the current hotkey (from get_mascot_summon_hotkey), lets the user
 * change it via register_mascot_summon_hotkey. Default value pre-filled.
 * "Skip" leaves the default in place.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

interface HotkeyStepProps {
  onNext: (hotkey: string) => void;
  onSkip: () => void;
}

/** Mirror of looksLikeValidShortcut in ModesPanel — keeps validation DRY. */
const looksLikeValidShortcut = (s: string): boolean => {
  const t = s.trim();
  if (t.length === 0) return false;
  if (t.includes('+')) return true;
  return /^F([1-9]|1\d|2[0-4])$/i.test(t);
};

const HotkeyStep = ({ onNext, onSkip }: HotkeyStepProps) => {
  const [currentHotkey, setCurrentHotkey] = useState('');
  const [inputValue, setInputValue] = useState('');
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    invoke<string>('get_mascot_summon_hotkey')
      .then(hotkey => {
        setCurrentHotkey(hotkey);
        setInputValue(hotkey);
      })
      .catch(err => {
        console.warn('[HotkeyStep] get_mascot_summon_hotkey failed:', err);
        setInputValue('CmdOrCtrl+Shift+Space');
      });
  }, []);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleSave = useCallback(async () => {
    const trimmed = inputValue.trim();
    if (!looksLikeValidShortcut(trimmed)) {
      setError('Enter a valid shortcut, e.g. CmdOrCtrl+Shift+Space or F12.');
      return;
    }
    setError(null);
    setSaving(true);
    try {
      await invoke('register_mascot_summon_hotkey', { shortcut: trimmed });
      setCurrentHotkey(trimmed);
      setSaved(true);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Could not register hotkey: ${msg}`);
    } finally {
      setSaving(false);
    }
  }, [inputValue]);

  const handleNext = useCallback(() => {
    onNext(currentHotkey);
  }, [currentHotkey, onNext]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        handleSave();
      }
    },
    [handleSave]
  );

  return (
    <div className="flex flex-col gap-6">
      <div>
        <p className="text-2xl font-semibold text-stone-900">Summon Hotkey</p>
        <p className="mt-2 text-sm text-stone-500">
          Choose the global shortcut that shows or hides the Whiskey mascot from any app. The
          default is{' '}
          <kbd className="rounded border border-stone-300 bg-stone-100 px-1 py-0.5 text-xs font-mono">
            CmdOrCtrl+Shift+Space
          </kbd>
          .
        </p>
      </div>

      <div className="flex flex-col gap-2">
        <label htmlFor="hotkey-input" className="text-sm font-medium text-stone-700">
          Shortcut
        </label>
        <div className="flex gap-2">
          <input
            ref={inputRef}
            id="hotkey-input"
            type="text"
            value={inputValue}
            onChange={e => {
              setInputValue(e.target.value);
              setSaved(false);
              setError(null);
            }}
            onKeyDown={handleKeyDown}
            placeholder="e.g. CmdOrCtrl+Shift+Space"
            aria-describedby={error ? 'hotkey-error' : undefined}
            className="flex-1 rounded-lg border border-stone-300 bg-white px-3 py-2 text-sm text-stone-900 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500"
          />
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="rounded-lg bg-primary-500 px-4 py-2 text-sm font-medium text-white hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-50">
            {saving ? 'Saving…' : 'Apply'}
          </button>
        </div>

        {error && (
          <p id="hotkey-error" role="alert" className="text-sm text-red-600">
            {error}
          </p>
        )}
        {saved && (
          <p role="status" aria-live="polite" className="text-sm text-green-600">
            Hotkey registered as{' '}
            <kbd className="rounded border border-stone-300 bg-stone-100 px-1 py-0.5 text-xs font-mono">
              {currentHotkey}
            </kbd>
            .
          </p>
        )}
      </div>

      <div className="flex justify-between gap-3">
        <button
          type="button"
          onClick={onSkip}
          className="rounded-lg px-4 py-2 text-sm text-stone-500 hover:text-stone-800 focus:outline-none focus:ring-2 focus:ring-stone-400">
          Skip
        </button>
        <button
          type="button"
          onClick={handleNext}
          className="rounded-lg bg-primary-500 px-5 py-2 text-sm font-medium text-white hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-500">
          Next
        </button>
      </div>
    </div>
  );
};

export default HotkeyStep;
