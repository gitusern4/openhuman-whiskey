// IMPORTANT: Polyfills must be imported FIRST
import { isTauri as tauriRuntimeAvailable } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import React from 'react';
import ReactDOM from 'react-dom/client';

import App from './App';
import { applyStoredTheme } from './hooks/useTheme';
import './index.css';
import { getCoreStateSnapshot } from './lib/coreState/store';
import MascotWindowApp from './mascot/MascotWindowApp';
// Whiskey fork — Windows mascot path. Distinct from MascotWindowApp
// (which is the macOS NSPanel-hosted passive mascot). The Windows
// version is hosted inside a real Tauri WebviewWindow with
// `transparent + always_on_top + decorations(false)`, so it can use
// the full Tauri IPC (drag, save-position, activate-main).
import WindowsMascotApp from './mascot/WindowsMascotApp';
import OverlayApp from './overlay/OverlayApp';
import './polyfills';
import { initSentry } from './services/analytics';
import { setStoreForApiClient } from './services/apiClient';
import { primeActiveUserId } from './store/userScopedStorage';
import { setupDesktopDeepLinkListener } from './utils/desktopDeepLinkListener';
import { getActiveUserIdFromCore } from './utils/tauriCommands';

setStoreForApiClient(() => getCoreStateSnapshot().snapshot.sessionToken);

// Window-routing branch.
//
//  - `?window=mascot`     → macOS native NSPanel + WKWebView host
//                           (lives OUTSIDE Tauri's runtime; the vendored
//                           tauri-cef can't render transparent
//                           windowed-mode browsers on macOS, so the
//                           panel is built natively in
//                           `app/src-tauri/src/mascot_native_window.rs`).
//                           Webview can't read a Tauri label so the
//                           Rust shell appends the query string itself.
//  - `?window=mascot-win`  → Windows Tauri WebviewWindow with
//                           `transparent + always_on_top + decorations
//                           (false)` (Whiskey fork). Built dynamically
//                           in `app/src-tauri/src/mascot_windows_window.rs`
//                           with a query string parallel to the macOS
//                           path so both branches funnel through this
//                           same `isStandaloneWindow` short-circuit.
//
// We must read the query string BEFORE touching any Tauri APIs because
// the macOS panel host has no Tauri runtime at all.
const urlWindowParam = (() => {
  try {
    return new URLSearchParams(window.location.search).get('window');
  } catch {
    return null;
  }
})();
const isMascotWindow = urlWindowParam === 'mascot';
const isWindowsMascotWindow = urlWindowParam === 'mascot-win';
const currentWindowLabel = isMascotWindow
  ? 'mascot'
  : isWindowsMascotWindow
    ? 'mascot-win'
    : tauriRuntimeAvailable()
      ? getCurrentWindow().label
      : 'main';
const isOverlayWindow = currentWindowLabel === 'overlay';
const isStandaloneWindow = isOverlayWindow || isMascotWindow || isWindowsMascotWindow;

const ensureDefaultHashRoute = () => {
  const hash = window.location.hash;
  if (!hash || hash === '#') {
    window.location.replace(`${window.location.pathname}${window.location.search}#/`);
    return;
  }
  if (!hash.startsWith('#/')) {
    window.location.hash = '/';
  }
};

// Initialize Sentry early (before React renders)
initSentry();

// Apply persisted theme before React mounts to prevent FOUC
// (flash of unstyled content). Sets data-tk-theme on <html> element.
applyStoredTheme();

document.documentElement.dataset.window = currentWindowLabel;

if (!isStandaloneWindow) {
  ensureDefaultHashRoute();

  // Deep link listener — try/catch handles non-Tauri environments
  setupDesktopDeepLinkListener().catch(err => {
    console.error('[DeepLink] setup error:', err);
  });
}

// Prime `userScopedStorage` from the Rust core's `active_user.toml`
// BEFORE redux-persist hydrates. The previous localStorage-only seed was
// bound to the per-user CEF profile dir and went stale across the
// restart-driven user flips that #900 introduced, so the new process
// would read the previous user's namespace, mis-detect a flip, and bounce
// into a second restart. Reading the Rust state up front pins the right
// namespace from the first storage call. (#900)
function bootRender() {
  const root = ReactDOM.createRoot(document.getElementById('root') as HTMLElement);
  const tree = isMascotWindow ? (
    <MascotWindowApp />
  ) : isWindowsMascotWindow ? (
    <WindowsMascotApp />
  ) : isOverlayWindow ? (
    <OverlayApp />
  ) : (
    <App />
  );
  root.render(<React.StrictMode>{tree}</React.StrictMode>);
}

// The macOS mascot lives in a native WKWebView (no Tauri IPC) so
// `getActiveUserIdFromCore()` would just reject after a roundtrip and
// delay first paint for nothing. The Windows mascot is a real Tauri
// WebviewWindow but doesn't render any user-scoped UI either, so we
// skip the user bootstrap on both mascot paths to keep first paint
// fast and avoid an unnecessary IPC.
const activeUserBootstrap =
  isMascotWindow || isWindowsMascotWindow
    ? Promise.resolve<string | null>(null)
    : getActiveUserIdFromCore();

activeUserBootstrap
  .then(id => primeActiveUserId(id))
  .catch(() => primeActiveUserId(null))
  .finally(bootRender);
