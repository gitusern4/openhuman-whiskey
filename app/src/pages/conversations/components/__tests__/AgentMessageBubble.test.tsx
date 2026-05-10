import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { openWorkspacePath } from '../../../../utils/workspaceLinks';
import { AgentMessageBubble } from '../AgentMessageBubble';

vi.mock('../../../../utils/workspaceLinks', () => ({
  openWorkspacePath: vi.fn().mockResolvedValue(undefined),
  isWorkspaceLink: vi.fn(href => href.startsWith('/')),
  getWorkspacePathFromHref: vi.fn(href => href),
}));

describe('AgentMessageBubble interactions', () => {
  it('opens workspace links via openWorkspacePath', async () => {
    render(<AgentMessageBubble content="Here is a link: [test](/workspace/test.txt)" />);

    const link = await screen.findByText('test');
    fireEvent.click(link);

    await waitFor(() => {
      expect(openWorkspacePath).toHaveBeenCalledWith('/workspace/test.txt');
    });
  });
});
