import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import App from './App';
import * as client from './engine/client';
import type { ConversationId } from './engine/types';

vi.mock('./engine/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./engine/client')>();
  return { ...actual, getConversations: vi.fn(), getPlan: vi.fn() };
});

function renderApp() {
  return render(
    <MantineProvider>
      <App />
    </MantineProvider>,
  );
}

afterEach(() => {
  vi.clearAllMocks();
});

// H14: selecting a thread now points the canvas at that conversation's real plan (previously it
// only switched the chat panel, leaving the canvas on the fixture plan -- see DECISIONS.md H14).
describe('selecting a thread drives the canvas to that conversation (H14)', () => {
  it('descends to the jobs level and fetches that conversation\'s plan, and marks the thread selected', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Lead thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();

    await waitFor(() => expect(screen.getByText('Threads')).toBeTruthy());
    // Still on the Leads canvas level before clicking.
    const canvas = within(screen.getByTestId('canvas-area'));
    expect(canvas.getByText('Harvest planner')).toBeTruthy();

    fireEvent.click(screen.getByTestId('thread-1'));

    // Canvas moved off the Leads level, to that conversation's jobs level.
    await waitFor(() => expect(client.getPlan).toHaveBeenCalledWith(1));
    expect(canvas.queryByText('Harvest planner')).toBeNull();
    expect(screen.getByTestId('thread-1').getAttribute('data-selected')).toBe('true');
    // The breadcrumb shows the conversation's title (H14: title comes from /api/conversations).
    expect(screen.getAllByText('Lead thread').length).toBeGreaterThan(0);
  });
});
