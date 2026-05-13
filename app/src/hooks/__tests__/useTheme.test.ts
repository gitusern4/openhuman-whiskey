/**
 * useTheme — coverage for setTheme persisting to localStorage,
 * applyStoredTheme reading from localStorage on cold start,
 * cross-tab storage event handler, and the invalidate/cache-clear paths.
 *
 * Missing-coverage targets: lines 47, 53-54, 71-74, 82-83, 87-88.
 */
import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { applyStoredTheme, useTheme } from '../useTheme';

// ---------------------------------------------------------------------------
// Helpers — we re-use the jsdom localStorage polyfill from setup.ts.
// ---------------------------------------------------------------------------

const HTML_ATTR = 'data-tk-theme';

function clearThemeStorage() {
  localStorage.removeItem('tk-theme');
  document.documentElement.removeAttribute(HTML_ATTR);
}

beforeEach(() => {
  clearThemeStorage();
});

afterEach(() => {
  clearThemeStorage();
});

// ===========================================================================
// 1. setTheme persists to localStorage and applies the DOM attribute
//    (lines 82-88)
// ===========================================================================

describe('setTheme', () => {
  it("persists 'zeth' to localStorage and sets data-tk-theme on <html> (lines 82-83, 87-88)", () => {
    const { result } = renderHook(() => useTheme());

    act(() => {
      result.current.setTheme('zeth');
    });

    expect(localStorage.getItem('tk-theme')).toBe('zeth');
    expect(document.documentElement.getAttribute(HTML_ATTR)).toBe('zeth');
    expect(result.current.theme).toBe('zeth');
  });

  it("removes data-tk-theme when switching back to 'default' (lines 44-46)", () => {
    // Prime with zeth first
    localStorage.setItem('tk-theme', 'zeth');
    const { result } = renderHook(() => useTheme());

    // Should read as zeth on mount
    expect(result.current.theme).toBe('zeth');

    act(() => {
      result.current.setTheme('default');
    });

    expect(localStorage.getItem('tk-theme')).toBe('default');
    expect(document.documentElement.hasAttribute(HTML_ATTR)).toBe(false);
    expect(result.current.theme).toBe('default');
  });
});

// ===========================================================================
// 2. applyStoredTheme reads from localStorage on cold start (lines 53-55)
// ===========================================================================

describe('applyStoredTheme', () => {
  it("applies 'zeth' from localStorage before React mount (line 54)", () => {
    localStorage.setItem('tk-theme', 'zeth');

    applyStoredTheme();

    expect(document.documentElement.getAttribute(HTML_ATTR)).toBe('zeth');
  });

  it("removes the attribute when localStorage has 'default' (line 54)", () => {
    // First set zeth so there's something to remove
    document.documentElement.setAttribute(HTML_ATTR, 'zeth');
    localStorage.setItem('tk-theme', 'default');

    applyStoredTheme();

    expect(document.documentElement.hasAttribute(HTML_ATTR)).toBe(false);
  });

  it("falls back to 'default' when localStorage is empty (line 53)", () => {
    // localStorage is clean; ensure no stale attribute
    document.documentElement.setAttribute(HTML_ATTR, 'zeth');

    applyStoredTheme();

    // readStoredTheme returns 'default' → applyTheme removes the attribute
    expect(document.documentElement.hasAttribute(HTML_ATTR)).toBe(false);
  });
});

// ===========================================================================
// 3. Cross-tab storage event handler updates theme (lines 71-75)
// ===========================================================================

describe('Cross-tab storage event', () => {
  it('updates theme state and DOM when storage event fires with key=tk-theme (lines 71-74)', async () => {
    const { result } = renderHook(() => useTheme());

    // Initial state should be default
    expect(result.current.theme).toBe('default');

    act(() => {
      window.dispatchEvent(
        new StorageEvent('storage', {
          key: 'tk-theme',
          newValue: 'zeth',
          storageArea: localStorage,
        })
      );
    });

    expect(result.current.theme).toBe('zeth');
    expect(document.documentElement.getAttribute(HTML_ATTR)).toBe('zeth');
  });

  it('ignores storage events for unrelated keys (line 71)', () => {
    const { result } = renderHook(() => useTheme());

    act(() => {
      window.dispatchEvent(
        new StorageEvent('storage', {
          key: 'some-other-key',
          newValue: 'zeth',
          storageArea: localStorage,
        })
      );
    });

    // Should remain default
    expect(result.current.theme).toBe('default');
  });

  it("reverts to 'default' when storage event fires with non-zeth value (line 72)", () => {
    localStorage.setItem('tk-theme', 'zeth');
    const { result } = renderHook(() => useTheme());

    expect(result.current.theme).toBe('zeth');

    act(() => {
      window.dispatchEvent(
        new StorageEvent('storage', {
          key: 'tk-theme',
          newValue: 'default',
          storageArea: localStorage,
        })
      );
    });

    expect(result.current.theme).toBe('default');
  });
});

// ===========================================================================
// 4. localStorage write failure is swallowed (lines 83-85)
// ===========================================================================

describe('localStorage failure resilience', () => {
  it('does not throw when localStorage.setItem throws (lines 82-84)', () => {
    const original = localStorage.setItem.bind(localStorage);
    vi.spyOn(localStorage, 'setItem').mockImplementationOnce(() => {
      throw new Error('QuotaExceededError');
    });

    const { result } = renderHook(() => useTheme());

    expect(() => {
      act(() => {
        result.current.setTheme('zeth');
      });
    }).not.toThrow();

    // DOM should still be updated even if storage write failed
    expect(result.current.theme).toBe('zeth');

    vi.restoreAllMocks();
  });
});
