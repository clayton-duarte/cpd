import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import App from './App';
import * as client from './engine/client';
import type { ConversationId } from './engine/types';

vi.mock('./engine/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./engine/client')>();
  return { ...actual, getConversations: vi.fn() };
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

describe('App wires conversation threads additively, without touching canvas nav', () => {
  it('selecting a thread does not change the canvas level/nav', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Lead thread' }],
    });

    renderApp();

    await waitFor(() => expect(screen.getByText('Threads')).toBeTruthy());
    // Still on the Leads canvas level -- clicking a thread must not descend/navigate the canvas.
    const canvas = within(screen.getByTestId('canvas-area'));
    expect(canvas.getByText('Harvest planner')).toBeTruthy();

    fireEvent.click(screen.getByTestId('thread-1'));

    // Canvas is unaffected: still the Leads level, same leads visible.
    expect(canvas.getByText('Harvest planner')).toBeTruthy();
    expect(screen.getByTestId('thread-1').getAttribute('data-selected')).toBe('true');
  });
});
