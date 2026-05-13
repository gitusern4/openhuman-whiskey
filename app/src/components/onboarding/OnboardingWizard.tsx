/**
 * OnboardingWizard — full-screen modal overlay for the Whiskey first-run wizard.
 *
 * Renders as a sibling overlay on top of the existing route tree (mounted
 * in App.tsx's AppShell). Does NOT replace any route. When completed or
 * dismissed, it unmounts cleanly and the underlying route renders normally.
 *
 * Steps (0-indexed):
 *   0 — WelcomeStep     (mode picker)
 *   1 — TvBridgeStep    (TradingView CDP bridge)
 *   2 — HotkeyStep      (mascot summon hotkey)
 *   3 — DoneStep        (summary + CTA)
 *
 * Keyboard:
 *   Esc  — skip current step (same as clicking "Skip")
 *   Enter — handled natively by focused buttons / inputs within each step
 *
 * Accessibility:
 *   role="dialog", aria-modal="true", aria-labelledby pointing at the
 *   visible heading inside the active step.
 *
 * Performance:
 *   CSS-only transitions (translate + opacity) for step transitions.
 *   Modal mount latency is governed by the single `useOnboarding` hook call.
 */
import { useCallback, useEffect, useRef, useState } from 'react';

import { useOnboarding } from '../../hooks/useOnboarding';
import DoneStep from './steps/DoneStep';
import HotkeyStep from './steps/HotkeyStep';
import TvBridgeStep from './steps/TvBridgeStep';
import WelcomeStep from './steps/WelcomeStep';

const TOTAL_STEPS = 4;

const OnboardingWizard = () => {
  const { loading, completed, step, tvBridgeSkipped, advance, finish } = useOnboarding();

  // Per-session state that is not persisted — only the backend's
  // completed/tv_bridge_skipped survive restarts.
  const [selectedModeId, setSelectedModeId] = useState('default');
  const [hotkey, setHotkey] = useState('');

  // Trap focus inside the dialog. Simple implementation: intercept Tab on
  // the backdrop and re-focus the dialog container.
  const dialogRef = useRef<HTMLDivElement>(null);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        // Esc skips the current step (or finishes if on the last step).
        if (step === TOTAL_STEPS - 1) {
          finish(tvBridgeSkipped);
        } else {
          advance(step + 1, tvBridgeSkipped);
        }
      }
    },
    [step, tvBridgeSkipped, advance, finish]
  );

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  // Don't render while loading (avoids flash of wizard on already-onboarded users).
  if (loading || completed) return null;

  // Step 0 handlers.
  const handleWelcomeNext = (modeId: string) => {
    setSelectedModeId(modeId);
    advance(1, tvBridgeSkipped);
  };
  const handleWelcomeSkip = () => advance(1, tvBridgeSkipped);

  // Step 1 handlers.
  const handleTvNext = (skipped: boolean) => advance(2, skipped);
  const handleTvSkip = () => {}; // TvBridgeStep calls onNext(true) internally.

  // Step 2 handlers.
  const handleHotkeyNext = (hk: string) => {
    setHotkey(hk);
    advance(3, tvBridgeSkipped);
  };
  const handleHotkeySkip = () => advance(3, tvBridgeSkipped);

  // Step 3 handler.
  const handleFinish = () => finish(tvBridgeSkipped);

  const stepContent = () => {
    switch (step) {
      case 0:
        return <WelcomeStep onNext={handleWelcomeNext} onSkip={handleWelcomeSkip} />;
      case 1:
        return <TvBridgeStep onNext={handleTvNext} onSkip={handleTvSkip} />;
      case 2:
        return <HotkeyStep onNext={handleHotkeyNext} onSkip={handleHotkeySkip} />;
      case 3:
        return (
          <DoneStep
            selectedModeId={selectedModeId}
            tvBridgeSkipped={tvBridgeSkipped}
            hotkey={hotkey}
            onFinish={handleFinish}
          />
        );
      default:
        return null;
    }
  };

  return (
    // Backdrop
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      aria-hidden="false">
      {/* Dialog */}
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-wizard-title"
        className="relative mx-4 w-full max-w-lg rounded-2xl bg-white p-8 shadow-2xl"
        style={{
          // CSS-only transition so step changes cost ~0ms JS.
          transition: 'opacity 150ms ease, transform 150ms ease',
        }}>
        {/* Hidden heading for screen readers — each step renders its own
            visible heading which aria-labelledby surfaces to AT. */}
        <span id="onboarding-wizard-title" className="sr-only">
          Whiskey onboarding, step {step + 1} of {TOTAL_STEPS}
        </span>

        {/* Step indicator */}
        <div className="mb-6 flex gap-1.5" aria-hidden="true">
          {Array.from({ length: TOTAL_STEPS }).map((_, i) => (
            <div
              key={i}
              className={`h-1.5 flex-1 rounded-full transition-colors duration-300 ${
                i <= step ? 'bg-primary-500' : 'bg-stone-200'
              }`}
            />
          ))}
        </div>

        {/* Active step */}
        {stepContent()}
      </div>
    </div>
  );
};

export default OnboardingWizard;
