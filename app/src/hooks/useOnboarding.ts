/**
 * useOnboarding — state machine for the Whiskey first-run wizard.
 *
 * Talks to three Tauri commands:
 *   onboarding_status()                        -> { completed, tv_bridge_skipped, current_step }
 *   onboarding_advance(step, tv_bridge_skipped) -> void  (step reached but not finished)
 *   onboarding_complete(tv_bridge_skipped)      -> void  (wizard dismissed / finished)
 *
 * Design:
 * - `status` is loaded once on mount; subsequent mutations are optimistic
 *   (local state updates immediately, backend write is fire-and-forget).
 * - `loading` is true only during the initial fetch, so the wizard never
 *   flickers between "show" and "hide" after the first paint.
 * - `tvBridgeSkipped` is threaded through every step so the Done card can
 *   surface it in the summary.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useState } from 'react';

export interface OnboardingStatus {
  completed: boolean;
  tv_bridge_skipped: boolean;
  current_step: number;
}

export interface UseOnboardingReturn {
  /** True while the initial Tauri call is in flight. */
  loading: boolean;
  /** True if onboarding is complete — wizard should not render. */
  completed: boolean;
  /** 0-indexed step the wizard is currently on (0–3). */
  step: number;
  /** Whether the TV bridge step was skipped. */
  tvBridgeSkipped: boolean;
  /** Advance to the next step. Fire-and-forget backend write. */
  advance: (nextStep: number, tvSkipped?: boolean) => void;
  /** Finish the wizard. Marks completed in backend. */
  finish: (tvSkipped: boolean) => void;
}

export function useOnboarding(): UseOnboardingReturn {
  const [loading, setLoading] = useState(true);
  const [completed, setCompleted] = useState(false);
  const [step, setStep] = useState(0);
  const [tvBridgeSkipped, setTvBridgeSkipped] = useState(false);

  useEffect(() => {
    let cancelled = false;
    invoke<OnboardingStatus>('onboarding_status')
      .then(status => {
        if (cancelled) return;
        setCompleted(status.completed);
        setStep(status.current_step ?? 0);
        setTvBridgeSkipped(status.tv_bridge_skipped ?? false);
      })
      .catch(err => {
        // If the command fails (e.g. non-Tauri web context in tests),
        // treat as fresh install — show the wizard.
        console.warn('[useOnboarding] onboarding_status failed:', err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const advance = useCallback(
    (nextStep: number, tvSkipped = tvBridgeSkipped) => {
      setStep(nextStep);
      setTvBridgeSkipped(tvSkipped);
      // Fire-and-forget — local state is already updated.
      invoke('onboarding_advance', { step: nextStep, tvBridgeSkipped: tvSkipped }).catch(err =>
        console.warn('[useOnboarding] onboarding_advance failed:', err)
      );
    },
    [tvBridgeSkipped]
  );

  const finish = useCallback((tvSkipped: boolean) => {
    setCompleted(true);
    setTvBridgeSkipped(tvSkipped);
    invoke('onboarding_complete', { tvBridgeSkipped: tvSkipped }).catch(err =>
      console.warn('[useOnboarding] onboarding_complete failed:', err)
    );
  }, []);

  return { loading, completed, step, tvBridgeSkipped, advance, finish };
}
