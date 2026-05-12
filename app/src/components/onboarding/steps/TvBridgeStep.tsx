/**
 * Step 2 — TradingView bridge setup.
 *
 * Walks the user through the one-time --remote-debugging-port=9222 setup.
 * Buttons call tv_cdp_launch_tv, tv_cdp_probe, tv_cdp_attach.
 * "Skip" sets tv_bridge_skipped = true and moves to step 3.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

interface TvBridgeStepProps {
  onNext: (tvBridgeSkipped: boolean) => void;
  onSkip: () => void;
}

type BridgeStatus = 'idle' | 'probing' | 'reachable' | 'unreachable' | 'attached';

const TvBridgeStep = ({ onNext, onSkip }: TvBridgeStepProps) => {
  const [bridgeStatus, setBridgeStatus] = useState<BridgeStatus>('idle');
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const skipBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    skipBtnRef.current?.focus();
  }, []);

  const handleLaunchTv = useCallback(async () => {
    setError(null);
    setStatusMessage('Launching TradingView Desktop with debug flag…');
    try {
      await invoke('tv_cdp_launch_tv');
      setStatusMessage('TradingView launched. Wait a few seconds, then click Probe.');
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Launch failed: ${msg}`);
      setStatusMessage(null);
    }
  }, []);

  const handleProbe = useCallback(async () => {
    setError(null);
    setBridgeStatus('probing');
    setStatusMessage('Probing CDP port 9222…');
    try {
      const result = await invoke<{ reachable: boolean }>('tv_cdp_probe');
      if (result.reachable) {
        setBridgeStatus('reachable');
        setStatusMessage('TradingView is reachable on port 9222.');
      } else {
        setBridgeStatus('unreachable');
        setStatusMessage(
          'Port 9222 is not reachable. Make sure TradingView Desktop is running with --remote-debugging-port=9222.'
        );
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setBridgeStatus('unreachable');
      setError(`Probe failed: ${msg}`);
      setStatusMessage(null);
    }
  }, []);

  const handleAttach = useCallback(async () => {
    setError(null);
    setStatusMessage('Attaching CDP session…');
    try {
      await invoke('tv_cdp_attach');
      setBridgeStatus('attached');
      setStatusMessage('Attached! The bridge is ready.');
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Attach failed: ${msg}`);
      setStatusMessage(null);
    }
  }, []);

  const handleSkip = useCallback(() => {
    onSkip();
    onNext(true /* tvBridgeSkipped */);
  }, [onSkip, onNext]);

  return (
    <div className="flex flex-col gap-6">
      <div>
        <p className="text-2xl font-semibold text-stone-900">TradingView Bridge</p>
        <p className="mt-2 text-sm text-stone-500">
          One-time setup: launch TradingView Desktop with the CDP debug flag so Whiskey can read
          chart state, set symbols, and more.
        </p>
      </div>

      <div className="rounded-xl border border-stone-200 bg-stone-50 p-4 text-sm text-stone-700">
        <p className="font-medium">Manual launch (if auto-launch fails):</p>
        <p className="mt-1 font-mono text-xs text-stone-500">
          tradingview.exe --remote-debugging-port=9222
        </p>
      </div>

      <div className="flex flex-wrap gap-3">
        <button
          type="button"
          onClick={handleLaunchTv}
          className="rounded-lg border border-stone-300 bg-white px-4 py-2 text-sm font-medium text-stone-700 hover:bg-stone-50 focus:outline-none focus:ring-2 focus:ring-primary-500">
          Launch TV with debug flag
        </button>

        <button
          type="button"
          onClick={handleProbe}
          disabled={bridgeStatus === 'probing'}
          className="rounded-lg bg-primary-500 px-4 py-2 text-sm font-medium text-white hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-50">
          {bridgeStatus === 'probing' ? 'Probing…' : 'Probe'}
        </button>

        {bridgeStatus === 'reachable' && (
          <button
            type="button"
            onClick={handleAttach}
            className="rounded-lg bg-green-600 px-4 py-2 text-sm font-medium text-white hover:bg-green-700 focus:outline-none focus:ring-2 focus:ring-green-500">
            Attach
          </button>
        )}
      </div>

      {statusMessage && (
        <p
          role="status"
          aria-live="polite"
          className={`text-sm ${
            bridgeStatus === 'reachable' || bridgeStatus === 'attached'
              ? 'text-green-700'
              : bridgeStatus === 'unreachable'
                ? 'text-amber-700'
                : 'text-stone-600'
          }`}>
          {statusMessage}
        </p>
      )}

      {error && (
        <p role="alert" className="rounded-lg bg-red-50 px-4 py-2 text-sm text-red-700">
          {error}
        </p>
      )}

      <div className="flex justify-between gap-3">
        <button
          ref={skipBtnRef}
          type="button"
          onClick={handleSkip}
          className="rounded-lg px-4 py-2 text-sm text-stone-500 hover:text-stone-800 focus:outline-none focus:ring-2 focus:ring-stone-400">
          Skip — set up later
        </button>

        <button
          type="button"
          onClick={() => onNext(false /* not skipped */)}
          disabled={bridgeStatus !== 'attached'}
          className="rounded-lg bg-primary-500 px-5 py-2 text-sm font-medium text-white hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-50">
          Next
        </button>
      </div>
    </div>
  );
};

export default TvBridgeStep;
