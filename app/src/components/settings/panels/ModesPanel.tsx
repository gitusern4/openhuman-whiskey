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

/**
 * Cheap client-side sanity check on a global-shortcut string before we
 * round-trip to the Tauri plugin (WHISKEY_AUDIT.md L8). The plugin's
 * own validator is canonical — this just spares the user a round-trip
 * for obvious garbage like "asdf". A valid Tauri global shortcut needs
 * either a modifier-prefixed key (contains "+") or a function key
 * (F1–F24). Anything else (a single letter, a punctuation char) is
 * almost certainly a typo.
 */
const looksLikeValidShortcut = (s: string): boolean => {
  const t = s.trim();
  if (t.length === 0) return false;
  if (t.includes('+')) return true;
  return /^F([1-9]|1\d|2[0-4])$/i.test(t);
};

const ModesPanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();
  const [modes, setModes] = useState<ModeDescriptor[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  // WHISKEY_AUDIT.md M8: split the error state into mode + hotkey
  // halves so a hotkey-save failure never clobbers a still-pending
  // mode-switch error (or vice versa). Each card renders its own
  // role="alert" region scoped to that state machine.
  const [modeError, setModeError] = useState<string | null>(null);
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);

  // Mascot summon hotkey card state. v1 is a plain text input — a
  // Raycast-style key recorder is Phase 2 and lives in its own
  // component (HotkeyRecorder.tsx) per the project plan.
  const [hotkeyCurrent, setHotkeyCurrent] = useState<string | null>(null);
  const [hotkeyDraft, setHotkeyDraft] = useState<string>('');
  const [hotkeyPending, setHotkeyPending] = useState(false);

  /**
   * Pull the registered modes + active id in parallel. Both calls hit
   * the in-memory registry in the core process, so they're effectively
   * instant — no spinner needed for happy-path renders.
   */
  const refresh = useCallback(async () => {
    setModeError(null);
    try {
      const [list, active] = await Promise.all([
        invoke<ModeDescriptor[]>('list_whiskey_modes'),
        invoke<string>('get_active_whiskey_mode_id'),
      ]);
      setModes(list);
      setActiveId(active);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setModeError(`Failed to load modes: ${msg}`);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const refreshHotkey = useCallback(async () => {
    try {
      const current = await invoke<string>('get_mascot_summon_hotkey');
      setHotkeyCurrent(current);
      setHotkeyDraft(current);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setHotkeyError(`Failed to load mascot summon hotkey: ${msg}`);
    }
  }, []);

  useEffect(() => {
    void refreshHotkey();
  }, [refreshHotkey]);

  const saveHotkey = useCallback(async () => {
    if (hotkeyPending) return;
    const next = hotkeyDraft.trim();
    if (next.length === 0) {
      setHotkeyError('Mascot summon hotkey cannot be empty.');
      return;
    }
    if (!looksLikeValidShortcut(next)) {
      setHotkeyError(
        `"${next}" doesn't look like a valid shortcut. ` +
          'Use a modifier-prefixed combo (e.g. "CmdOrCtrl+Shift+Space") or a function key (F1–F24).'
      );
      return;
    }
    setHotkeyPending(true);
    setHotkeyError(null);
    try {
      await invoke('register_mascot_summon_hotkey', { shortcut: next });
      setHotkeyCurrent(next);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setHotkeyError(`Could not register mascot summon hotkey: ${msg}`);
    } finally {
      setHotkeyPending(false);
    }
  }, [hotkeyDraft, hotkeyPending]);

  const resetHotkey = useCallback(async () => {
    if (hotkeyPending) return;
    setHotkeyPending(true);
    setHotkeyError(null);
    try {
      await invoke('unregister_mascot_summon_hotkey');
      const fallback = await invoke<string>('get_mascot_summon_hotkey');
      await invoke('register_mascot_summon_hotkey', { shortcut: fallback });
      setHotkeyCurrent(fallback);
      setHotkeyDraft(fallback);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setHotkeyError(`Could not reset mascot summon hotkey: ${msg}`);
    } finally {
      setHotkeyPending(false);
    }
  }, [hotkeyPending]);

  const switchTo = useCallback(
    async (id: string) => {
      if (id === activeId || pending) return;
      setPending(true);
      setModeError(null);
      try {
        await invoke('set_whiskey_mode', { id });
        setActiveId(id);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setModeError(`Could not switch to mode "${id}": ${msg}`);
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

        {modeError && (
          <div
            role="alert"
            data-testid="modes-error-alert"
            className="rounded-xl border border-rose-200 bg-rose-50 p-4 text-xs text-rose-700">
            {modeError}
          </div>
        )}

        {modes.length === 0 && !modeError && (
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

        <div
          className="rounded-xl border border-stone-200 bg-white p-4"
          data-testid="mascot-summon-hotkey-card">
          <div className="text-sm font-medium text-stone-900">Summon hotkey</div>
          <p className="mt-1 text-xs text-stone-500 leading-relaxed">
            Global shortcut that toggles the floating mascot from anywhere. Use the project&apos;s{' '}
            <code className="text-[11px]">CmdOrCtrl</code> convention to bind the same chord on
            macOS and Windows (e.g. <code className="text-[11px]">CmdOrCtrl+Shift+Space</code>).
          </p>
          <div className="mt-2 text-[11px] text-stone-400">
            Currently registered:{' '}
            <span className="text-stone-600" data-testid="mascot-summon-hotkey-current">
              {hotkeyCurrent ?? '…'}
            </span>
          </div>
          <div className="mt-3 flex items-center gap-2">
            <input
              type="text"
              value={hotkeyDraft}
              onChange={e => setHotkeyDraft(e.target.value)}
              disabled={hotkeyPending}
              placeholder="CmdOrCtrl+Shift+Space"
              data-testid="mascot-summon-hotkey-input"
              className="flex-1 rounded-md border border-stone-200 bg-white px-2 py-1.5 text-xs text-stone-900 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:bg-stone-50 disabled:text-stone-400"
            />
            <button
              type="button"
              onClick={() => void saveHotkey()}
              disabled={hotkeyPending || !looksLikeValidShortcut(hotkeyDraft)}
              data-testid="mascot-summon-hotkey-save"
              className="shrink-0 rounded-md bg-primary-500 px-3 py-1.5 text-xs font-medium text-white hover:bg-primary-600 disabled:bg-stone-300 disabled:cursor-not-allowed">
              {hotkeyPending ? 'Saving…' : 'Save'}
            </button>
            <button
              type="button"
              onClick={() => void resetHotkey()}
              disabled={hotkeyPending}
              data-testid="mascot-summon-hotkey-reset"
              className="shrink-0 rounded-md border border-stone-200 bg-white px-3 py-1.5 text-xs font-medium text-stone-700 hover:bg-stone-50 disabled:cursor-not-allowed disabled:text-stone-400">
              Reset
            </button>
          </div>
          {/*
            WHISKEY_AUDIT.md M8: hotkey-scoped error alert. Lives
            inside the hotkey card so a failed save/register doesn't
            visually bleed up into the modes-list error region (and
            vice versa — a failed mode switch can't clobber a still-
            shown hotkey error).
          */}
          {hotkeyError && (
            <div
              role="alert"
              data-testid="mascot-summon-hotkey-error"
              className="mt-3 rounded-md border border-rose-200 bg-rose-50 p-2 text-xs text-rose-700">
              {hotkeyError}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default ModesPanel;
