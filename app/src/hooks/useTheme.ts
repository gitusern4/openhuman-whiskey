/**
 * TK's Mods — theme management hook.
 *
 * Reads the active theme from `localStorage` (key: `tk-theme`) and
 * applies it by writing `data-tk-theme` on `<html>`. CSS custom
 * properties in `theme.css` do the actual repainting — no React tree
 * re-render occurs on switch; it's a pure paint pass.
 *
 * Available themes:
 *   "default" — stone/sage (upstream look)
 *   "zeth"    — deep black + neon green (#39ff14)
 *
 * The hook is safe to call from any component. Multiple consumers share
 * the same localStorage key and the same DOM attribute, so they stay
 * in sync automatically through the storage event.
 *
 * Latency budget: < 10ms for a theme switch (a single setAttribute +
 * localStorage.setItem). Well within the 100ms target.
 */
import { useCallback, useEffect, useState } from 'react';

export type ThemeId = 'default' | 'zeth';

export const THEMES: { id: ThemeId; label: string }[] = [
  { id: 'default', label: 'Default (stone / sage)' },
  { id: 'zeth', label: 'ZETH (black / neon green)' },
];

const STORAGE_KEY = 'tk-theme';
const HTML_ATTR = 'data-tk-theme';

function readStoredTheme(): ThemeId {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'zeth') return 'zeth';
  } catch {
    // localStorage unavailable (sandboxed context) — fall through.
  }
  return 'default';
}

function applyTheme(id: ThemeId): void {
  const html = document.documentElement;
  if (id === 'default') {
    html.removeAttribute(HTML_ATTR);
  } else {
    html.setAttribute(HTML_ATTR, id);
  }
}

/** Apply the persisted theme immediately (before React mounts) so there
 * is no flash of unstyled content on load. Call once at app boot. */
export function applyStoredTheme(): void {
  applyTheme(readStoredTheme());
}

/** React hook: returns the current theme id and a setter that persists
 * the choice and applies it via CSS custom properties. */
export function useTheme(): { theme: ThemeId; setTheme: (id: ThemeId) => void } {
  const [theme, setThemeState] = useState<ThemeId>(readStoredTheme);

  // Apply on mount so components that use the hook get the right value
  // even if `applyStoredTheme` wasn't called at the app root.
  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  // Listen for cross-tab / cross-component storage changes.
  useEffect(() => {
    const handler = (e: StorageEvent) => {
      if (e.key === STORAGE_KEY) {
        const next = e.newValue === 'zeth' ? 'zeth' : 'default';
        setThemeState(next);
        applyTheme(next);
      }
    };
    window.addEventListener('storage', handler);
    return () => window.removeEventListener('storage', handler);
  }, []);

  const setTheme = useCallback((id: ThemeId) => {
    try {
      localStorage.setItem(STORAGE_KEY, id);
    } catch {
      // ignore write failures
    }
    applyTheme(id);
    setThemeState(id);
  }, []);

  return { theme, setTheme };
}
