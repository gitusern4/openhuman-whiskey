/**
 * Whiskey fork — agent-mode picker.
 *
 * Lets the user switch between registered agent modes (Default,
 * Whiskey trading mentor, …). Each mode supplies its own system-prompt
 * prefix, reflection prompt, additional memory roots, and tool
 * allowlist (see `crate::openhuman::modes` for the trait + registry).
 *
 * Wires to three Tauri commands in `app/src-tauri/src/lib.rs`:
 *
 *   - `list_whiskey_modes() -> ModeDescriptor[]`
 *   - `get_active_whiskey_mode_id() -> string`
 *   - `set_whiskey_mode(id: string) -> void` (rejects unknown ids)
 *
 * Stylistic conventions taken from the existing settings panels
 * (`AboutPanel`, `DeveloperOptionsPanel`): `rounded-xl border
 * border-stone-200 bg-white p-4` cards, `bg-primary-500` primary
 * buttons, `SettingsHeader` for the page chrome.
 */
import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useState } from 'react';

import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

export interface ModeDescriptor {
  id: string;
  display_name: string;
  description: string;
}

const ModesPanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();
  const [modes, setModes] = useState<ModeDescriptor[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /**
   * Pull the registered modes + active id in parallel. Both calls hit
   * the in-memory registry in the core process, so they're effectively
   * instant — no spinner needed for happy-path renders.
   */
  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [list, active] = await Promise.all([
        invoke<ModeDescriptor[]>('list_whiskey_modes'),
        invoke<string>('get_active_whiskey_mode_id'),
      ]);
      setModes(list);
      setActiveId(active);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(`Failed to load modes: ${msg}`);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const switchTo = useCallback(
    async (id: string) => {
      if (id === activeId || pending) return;
      setPending(true);
      setError(null);
      try {
        await invoke('set_whiskey_mode', { id });
        setActiveId(id);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setError(`Could not switch to mode "${id}": ${msg}`);
      } finally {
        setPending(false);
      }
    },
    [activeId, pending]
  );

  return (
    <div className="z-10 relative" data-testid="modes-panel-root">
      <SettingsHeader
        title="Modes"
        showBackButton={true}
        onBack={navigateBack}
        breadcrumbs={breadcrumbs}
      />

      <div className="p-4 space-y-4">
        <div className="rounded-xl border border-stone-200 bg-white p-4">
          <div className="text-sm font-medium text-stone-900">Active mode</div>
          <p className="mt-1 text-xs text-stone-500 leading-relaxed">
            Modes change the agent&apos;s persona, reflection prompts, and which tools it can call.
            Switching takes effect on your next message — no restart needed. The Default mode is
            byte-identical to upstream OpenHuman.
          </p>
        </div>

        {error && (
          <div
            role="alert"
            className="rounded-xl border border-rose-200 bg-rose-50 p-4 text-xs text-rose-700">
            {error}
          </div>
        )}

        {modes.length === 0 && !error && (
          <div className="rounded-xl border border-stone-200 bg-white p-4 text-xs text-stone-500">
            Loading modes…
          </div>
        )}

        {modes.map(mode => {
          const isActive = mode.id === activeId;
          return (
            <button
              key={mode.id}
              type="button"
              onClick={() => void switchTo(mode.id)}
              disabled={pending || isActive}
              data-testid={`mode-row-${mode.id}`}
              data-active={isActive ? 'true' : 'false'}
              className={`w-full rounded-xl border p-4 text-left transition-colors ${
                isActive
                  ? 'border-primary-500 bg-primary-50'
                  : 'border-stone-200 bg-white hover:bg-stone-50'
              } disabled:cursor-default`}>
              <div className="flex items-start justify-between gap-3">
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium text-stone-900">{mode.display_name}</div>
                  <p className="mt-1 text-xs text-stone-500 leading-relaxed">{mode.description}</p>
                </div>
                {isActive ? (
                  <span
                    className="shrink-0 px-2 py-0.5 rounded-md bg-primary-500 text-white text-[11px] font-medium"
                    data-testid={`mode-active-badge-${mode.id}`}>
                    Active
                  </span>
                ) : (
                  <span className="shrink-0 px-2 py-0.5 text-[11px] text-stone-400">
                    {pending ? '…' : 'Switch'}
                  </span>
                )}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
};

export default ModesPanel;
