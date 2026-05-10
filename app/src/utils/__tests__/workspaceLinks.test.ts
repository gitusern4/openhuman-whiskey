import { invoke, isTauri } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  getWorkspacePathFromHref,
  isWorkspaceLink,
  openWorkspacePath,
  readWorkspaceFileString,
  revealWorkspacePath,
} from '../workspaceLinks';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));

describe('workspaceLinks', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('isWorkspaceLink', () => {
    it('returns true for file: protocols', () => {
      expect(isWorkspaceLink('file:///test/path')).toBe(true);
    });

    it('returns true for relative paths without protocols', () => {
      expect(isWorkspaceLink('/test/path')).toBe(true);
      expect(isWorkspaceLink('test/path')).toBe(true);
    });

    it('returns false for http: or https: protocols', () => {
      expect(isWorkspaceLink('http://example.com')).toBe(false);
      expect(isWorkspaceLink('https://example.com')).toBe(false);
    });
  });

  describe('getWorkspacePathFromHref', () => {
    it('returns pathname for file: URLs', () => {
      expect(getWorkspacePathFromHref('file:///test/path')).toBe('/test/path');
    });

    it('returns the raw string for relative paths', () => {
      expect(getWorkspacePathFromHref('/test/path')).toBe('/test/path');
    });

    it('returns null for non-workspace links', () => {
      expect(getWorkspacePathFromHref('https://example.com')).toBe(null);
    });
  });

  describe('Tauri wrappers', () => {
    it('openWorkspacePath calls invoke if isTauri is true', async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      await openWorkspacePath('/test/path');
      expect(invoke).toHaveBeenCalledWith('open_workspace_path', { path: '/test/path' });
    });

    it('revealWorkspacePath calls invoke if isTauri is true', async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      await revealWorkspacePath('/test/path');
      expect(invoke).toHaveBeenCalledWith('reveal_workspace_path', { path: '/test/path' });
    });

    it('readWorkspaceFileString calls invoke if isTauri is true', async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue('test content');
      const result = await readWorkspaceFileString('/test/path');
      expect(invoke).toHaveBeenCalledWith('read_workspace_file_string', { path: '/test/path' });
      expect(result).toBe('test content');
    });

    it('throws error for readWorkspaceFileString if isTauri is false', async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(readWorkspaceFileString('/test/path')).rejects.toThrow(
        'Workspace file preview is only supported in Tauri.'
      );
    });
  });
});
