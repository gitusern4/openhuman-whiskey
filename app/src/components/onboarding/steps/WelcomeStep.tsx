/**
 * Step 1 — Welcome + mode picker.
 *
 * Lists modes from `list_whiskey_modes` and lets the user pick one.
 * "Skip" advances to step 2 with the Default mode.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

export interface ModeDescriptor {
  id: string;
  display_name: string;
  description: string;
}

interface WelcomeStepProps {
  onNext: (selectedModeId: string) => void;
  onSkip: () => void;
}

const WelcomeStep = ({ onNext, onSkip }: WelcomeStepProps) => {
  const [modes, setModes] = useState<ModeDescriptor[]>([]);
  const [activeId, setActiveId] = useState<string>('default');
  const [error, setError] = useState<string | null>(null);
  const nextBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    Promise.all([
      invoke<ModeDescriptor[]>('list_whiskey_modes'),
      invoke<string>('get_active_whiskey_mode_id'),
    ])
      .then(([modeList, currentId]) => {
        setModes(modeList);
        setActiveId(currentId ?? 'default');
      })
      .catch(err => {
        console.warn('[WelcomeStep] failed to load modes:', err);
        setError('Could not load modes. You can configure this later in Settings → Modes.');
      });
  }, []);

  // Focus the primary button on mount for keyboard users.
  useEffect(() => {
    nextBtnRef.current?.focus();
  }, []);

  const handleSelect = useCallback((id: string) => {
    setActiveId(id);
  }, []);

  const handleNext = useCallback(() => {
    invoke('set_whiskey_mode', { id: activeId }).catch(err =>
      console.warn('[WelcomeStep] set_whiskey_mode failed:', err)
    );
    onNext(activeId);
  }, [activeId, onNext]);

  // Keyboard: Enter on a mode card selects it; Enter on buttons is native.
  const handleCardKeyDown = useCallback(
    (e: React.KeyboardEvent, id: string) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        handleSelect(id);
      }
    },
    [handleSelect]
  );

  return (
    <div className="flex flex-col gap-6">
      <div>
        <p className="text-2xl font-semibold text-stone-900">Meet Whiskey</p>
        <p className="mt-2 text-sm text-stone-500">
          Whiskey is your trading mentor — reads your A+ catalog and pattern log, scores setups,
          and never executes trades. Start with the Default mode or activate Whiskey now.
        </p>
      </div>

      {error && (
        <p role="alert" className="rounded-lg bg-red-50 px-4 py-2 text-sm text-red-700">
          {error}
        </p>
      )}

      <fieldset aria-label="Agent mode" className="flex flex-col gap-3">
        {modes.length === 0 && !error && (
          <p className="text-sm text-stone-400">Loading modes…</p>
        )}
        {modes.map(mode => (
          <div
            key={mode.id}
            role="radio"
            aria-checked={activeId === mode.id}
            tabIndex={0}
            onClick={() => handleSelect(mode.id)}
            onKeyDown={e => handleCardKeyDown(e, mode.id)}
            className={`cursor-pointer rounded-xl border p-4 transition-colors focus:outline-none focus:ring-2 focus:ring-primary-500 ${
              activeId === mode.id
                ? 'border-primary-500 bg-primary-50'
                : 'border-stone-200 bg-white hover:border-stone-300'
            }`}>
            <p className="font-medium text-stone-900">{mode.display_name}</p>
            {mode.description && (
              <p className="mt-0.5 text-xs text-stone-500">{mode.description}</p>
            )}
          </div>
        ))}
      </fieldset>

      <div className="flex justify-between gap-3">
        <button
          type="button"
          onClick={onSkip}
          className="rounded-lg px-4 py-2 text-sm text-stone-500 hover:text-stone-800 focus:outline-none focus:ring-2 focus:ring-stone-400">
          Skip
        </button>
        <button
          ref={nextBtnRef}
          type="button"
          onClick={handleNext}
          className="rounded-lg bg-primary-500 px-5 py-2 text-sm font-medium text-white hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-500">
          Next
        </button>
      </div>
    </div>
  );
};

export default WelcomeStep;
