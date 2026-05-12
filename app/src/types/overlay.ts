/**
 * Types mirroring the Rust overlay structs in `tv_overlay.rs`.
 *
 * Keep in sync with:
 *   app/src-tauri/src/tv_overlay.rs — OverlayState, LockoutStatus, OverlayCommand
 */

export interface OverlayLockoutStatus {
  is_locked: boolean;
  locked_until_unix: number | null;
  lock_reason: string | null;
  daily_loss_dollars: number;
  consecutive_losses: number;
}

/** Full state blob pushed to the injected panel via window.__WHISKEY_OVERLAY_STATE. */
export interface OverlayState {
  favorites: string[];
  lockout: OverlayLockoutStatus;
  /** Tuple: [entry, stop, target] default prices from risk preset. */
  default_sltp: [number, number, number];
  active_tag: string | null;
}

/** Commands that can appear in window.__WHISKEY_OVERLAY_OUTBOX. */
export type OverlayCommandKind = 'set_symbol' | 'draw_sltp' | 'clear_sltp' | 'order_flow_tag';

export interface OverlayCommand {
  type: OverlayCommandKind;
  /** set_symbol */
  symbol?: string;
  /** draw_sltp */
  entry?: number;
  stop?: number;
  target?: number;
  /** order_flow_tag */
  tag?: string;
}

/** Result of tv_overlay_inject Tauri command. */
export interface InjectResult {
  ok: boolean;
  panel_id: string | null;
  skipped: boolean;
  error: string | null;
}

export type OverlayStatus = 'injected' | 'not_injected' | 'tv_not_attached';
