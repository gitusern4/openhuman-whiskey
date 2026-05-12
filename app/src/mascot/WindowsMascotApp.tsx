import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useRef, useState } from 'react';

import { type MascotFace, YellowMascot } from '../features/human/Mascot';

/**
 * Trailing-edge debounce on `tauri://move` so a single drag fires one
 * disk write instead of dozens. WHISKEY_AUDIT.md H2: dragging the
 * mascot generated a hot disk-write loop because the `tauri://move`
 * event fires per-pixel during a drag and the previous handler
 * invoked the save command on every fire. 300 ms feels instant but
 * cleanly batches a continuous drag into one save.
 */
const SAVE_POSITION_DEBOUNCE_MS = 300;

/**
 * Hosted in a Tauri WebviewWindow created by `mascot_windows_window.rs`
 * on Windows. The window is built with `transparent + always_on_top +
 * decorations(false) + skip_taskbar`.
 *
 * Whether the transparency hint actually takes effect depends on the
 * vendored CEF runtime's Windows behaviour. If it doesn't, this
 * component still renders correctly — it just sits inside an opaque
 * 96×96 square instead of floating freely. Either way the mascot is
 * draggable, always-on-top, and click-to-pop-out.
 *
 * Distinct from `MascotWindowApp` (the macOS passive mascot, hosted
 * outside Tauri in an NSPanel + WKWebView). Mounted by `main.tsx` when
 * the URL query string contains `?window=mascot-win`.
 */

const DEFAULT_FACE: MascotFace = 'idle';

/**
 * Drag activation threshold in CSS pixels. Below this, a mousedown is
 * treated as a click (which pops the main window); above it, a drag is
 * initiated via Tauri's `start_dragging`. Matches the UX-research spec
 * "ignore drags <4px so single-clicks register".
 */
const DRAG_THRESHOLD_PX = 4;

const WindowsMascotApp = () => {
  const [face] = useState<MascotFace>(DEFAULT_FACE);
  const dragOriginRef = useRef<{ x: number; y: number } | null>(null);
  const dragStartedRef = useRef<boolean>(false);

  /**
   * Persist the window's current position. Fires on Tauri's
   * `tauri://move` event during/after a drag. WHISKEY_AUDIT.md H2:
   * `tauri://move` fires on every position change during a drag (tens
   * to hundreds per second), so the call has to be debounced — without
   * the timer below the previous implementation hammered the disk
   * with `mascot_windows_state.toml` writes for every pixel of a drag.
   *
   * `useRef<number | null>` instead of `useState` because the timer
   * id is implementation detail; flipping React state on every pointer
   * tick would itself defeat the purpose.
   */
  const savePositionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const savePosition = useCallback(() => {
    if (!isTauri()) return;
    if (savePositionTimerRef.current !== null) {
      clearTimeout(savePositionTimerRef.current);
    }
    savePositionTimerRef.current = setTimeout(() => {
      savePositionTimerRef.current = null;
      invoke('mascot_window_save_position').catch(err => {
        console.warn('[mascot-win] save position failed', err);
      });
    }, SAVE_POSITION_DEBOUNCE_MS);
  }, []);

  /**
   * Subscribe to the window's `tauri://move` event so any drag (mouse
   * or programmatic) eventually saves the resting position. Cleanup
   * also flushes any pending debounce timer so an unmount mid-drag
   * doesn't leak a callback that runs against a torn-down isTauri.
   */
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: undefined | (() => void);
    const w = getCurrentWindow();
    w.listen('tauri://move', () => {
      savePosition();
    })
      .then(unlistenFn => {
        unlisten = unlistenFn;
      })
      .catch(err => {
        console.warn('[mascot-win] failed to subscribe to tauri://move', err);
      });
    return () => {
      if (unlisten) unlisten();
      if (savePositionTimerRef.current !== null) {
        clearTimeout(savePositionTimerRef.current);
        savePositionTimerRef.current = null;
      }
    };
  }, [savePosition]);

  /**
   * On mousedown, capture the origin so mousemove can decide if we've
   * crossed the drag threshold. We never call `start_dragging` from
   * mousedown directly because that would defeat single-click pops.
   */
  const onPointerDown = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    dragOriginRef.current = { x: event.clientX, y: event.clientY };
    dragStartedRef.current = false;
  }, []);

  const onPointerMove = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    const origin = dragOriginRef.current;
    if (!origin || dragStartedRef.current) return;
    const dx = event.clientX - origin.x;
    const dy = event.clientY - origin.y;
    if (Math.hypot(dx, dy) < DRAG_THRESHOLD_PX) return;

    // Threshold crossed — promote to drag and hand off to Tauri's
    // native window-drag (which generates the OS-level move events
    // we listen for above).
    dragStartedRef.current = true;
    if (!isTauri()) return;
    getCurrentWindow()
      .startDragging()
      .catch(err => {
        console.warn('[mascot-win] startDragging failed', err);
      });
  }, []);

  const onPointerUp = useCallback(() => {
    const wasDrag = dragStartedRef.current;
    dragOriginRef.current = null;
    dragStartedRef.current = false;
    if (wasDrag) return;
    // Single-click: pop the main window. Tauri command from lib.rs.
    if (!isTauri()) return;
    invoke('activate_main_window').catch(err => {
      console.warn('[mascot-win] activate_main_window failed', err);
    });
  }, []);

  return (
    <div
      data-testid="windows-mascot-root"
      data-face={face}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'transparent',
        cursor: 'grab',
        // Keyframe-driven idle "breathing": ~6s sine on scale.
        // GPU-composited (transform-only), no JS animation loop.
        // The animation lives in WindowsMascotApp.css to keep this
        // component focused on behaviour.
        animation: 'whiskey-mascot-breathing 6s ease-in-out infinite',
      }}>
      <style>{`
        @keyframes whiskey-mascot-breathing {
          0%   { transform: scale(1.00); }
          50%  { transform: scale(1.02); }
          100% { transform: scale(1.00); }
        }
      `}</style>
      <YellowMascot face={face} groundShadowOpacity={0.75} compactArmShading />
    </div>
  );
};

export default WindowsMascotApp;
