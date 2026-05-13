/**
 * Step 4 — All set.
 *
 * Summary card listing the choices made + a CTA button that routes the
 * user to the TK's Mods hub where all trading features now live.
 */
import { useCallback, useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';

interface DoneStepProps {
  selectedModeId: string;
  tvBridgeSkipped: boolean;
  hotkey: string;
  onFinish: () => void;
}

const DoneStep = ({ selectedModeId, tvBridgeSkipped, hotkey, onFinish }: DoneStepProps) => {
  const navigate = useNavigate();
  const finishBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    finishBtnRef.current?.focus();
  }, []);

  const handleTksMods = useCallback(() => {
    onFinish();
    navigate('/settings/tks-mods');
  }, [onFinish, navigate]);

  const summaryItems: { label: string; value: string }[] = [
    {
      label: 'Agent mode',
      value: selectedModeId === 'whiskey' ? 'Whiskey (trading mentor)' : 'Default',
    },
    {
      label: 'TradingView bridge',
      value: tvBridgeSkipped ? 'Skipped — configure later in Settings' : 'Connected',
    },
    { label: 'Summon hotkey', value: hotkey || 'Default (CmdOrCtrl+Shift+Space)' },
  ];

  return (
    <div className="flex flex-col gap-6">
      <div>
        <p className="text-2xl font-semibold text-stone-900">You're all set</p>
        <p className="mt-2 text-sm text-stone-500">
          Here's a summary of your choices. You can change any of these later in Settings.
        </p>
      </div>

      <div className="rounded-xl border border-stone-200 bg-white">
        {summaryItems.map((item, idx) => (
          <div
            key={item.label}
            className={`flex items-start justify-between gap-4 px-4 py-3 ${
              idx < summaryItems.length - 1 ? 'border-b border-stone-100' : ''
            }`}>
            <span className="text-sm font-medium text-stone-600">{item.label}</span>
            <span className="text-right text-sm text-stone-900">{item.value}</span>
          </div>
        ))}
      </div>

      <div className="flex flex-col gap-3 sm:flex-row sm:justify-between">
        <button
          type="button"
          onClick={handleTksMods}
          className="rounded-lg border border-primary-500 px-4 py-2 text-sm font-medium text-primary-600 hover:bg-primary-50 focus:outline-none focus:ring-2 focus:ring-primary-500">
          Take me to TK's Mods
        </button>

        <button
          ref={finishBtnRef}
          type="button"
          onClick={onFinish}
          className="rounded-lg bg-primary-500 px-5 py-2 text-sm font-medium text-white hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-500">
          Finish
        </button>
      </div>
    </div>
  );
};

export default DoneStep;
