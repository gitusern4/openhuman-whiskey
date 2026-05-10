import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { openWorkspacePath } from '../../../utils/workspaceLinks';
import { MemoryGraph } from '../MemoryGraph';

vi.mock('../../../utils/workspaceLinks', () => ({
  openWorkspacePath: vi.fn().mockResolvedValue(undefined),
}));

describe('MemoryGraph interactions', () => {
  it('opens summary nodes via openWorkspacePath', async () => {
    const nodes = [
      {
        id: '1',
        kind: 'summary',
        tree_kind: 'global',
        level: 1,
        file_basename: 'test-summary',
        label: 'Summary',
      },
    ] as any;

    render(<MemoryGraph nodes={nodes} edges={[]} mode="tree" contentRootAbs="/workspace" />);

    // Wait for force simulation to lay out the node.
    const circle = await screen.findByTestId('memory-graph-node-1');
    fireEvent.click(circle);

    await waitFor(() => {
      expect(openWorkspacePath).toHaveBeenCalledWith('/workspace/wiki/summaries');
    });
  });
});
