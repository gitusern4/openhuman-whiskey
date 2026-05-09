import { invoke, isTauri } from '@tauri-apps/api/core';

export async function openWorkspacePath(path: string): Promise<void> {
  if (!isTauri()) {
    console.warn('Workspace links are only supported in Tauri.');
    return;
  }
  await invoke('open_workspace_path', { path });
}

export async function revealWorkspacePath(path: string): Promise<void> {
  if (!isTauri()) {
    console.warn('Workspace links are only supported in Tauri.');
    return;
  }
  await invoke('reveal_workspace_path', { path });
}

export async function readWorkspaceFileString(path: string): Promise<string> {
  if (!isTauri()) {
    throw new Error('Workspace file preview is only supported in Tauri.');
  }
  return await invoke<string>('read_workspace_file_string', { path });
}

export function isWorkspaceLink(href: string): boolean {
  try {
    const url = new URL(href);
    return url.protocol === 'file:';
  } catch {
    // If it's not a valid URL (e.g., relative path like /wiki/summaries or wiki/summaries),
    // it's a potential workspace link
    return href.startsWith('/') || !href.includes('://');
  }
}

export function getWorkspacePathFromHref(href: string): string | null {
  if (isWorkspaceLink(href)) {
    try {
      const url = new URL(href);
      if (url.protocol === 'file:') {
        return url.pathname;
      }
    } catch {
      return href;
    }
  }
  return null;
}
