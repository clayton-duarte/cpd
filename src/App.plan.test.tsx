import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, within, waitFor } from '@testing-library/react';
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

function canvas() {
  return within(screen.getByTestId('canvas-area'));
}

afterEach(() => {
  vi.clearAllMocks();
});

// H14: the jobs level only renders a real daemon-backed plan for a REAL conversation (selected
// via a thread under THREADS). A fixture plan id (e.g. 'w3') issues no `/api/plan` request at
// all -- see App.threads.test.tsx for that half of the contract.
describe('App renders the real plan at the jobs level for a selected conversation (H14)', () => {
  it('shows a daemon-backed job for the selected conversation, not a fixture job', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Harvest thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({
      jobs: [{ id: 'real-1', title: 'Fetched from the daemon', status: 'done', needs: [] }],
    });

    renderApp();
    await waitFor(() => expect(screen.getByText('Threads')).toBeTruthy());
    fireEvent.click(screen.getByTestId('thread-1'));

    expect(await canvas().findByText('Fetched from the daemon')).toBeTruthy();
    expect(client.getPlan).toHaveBeenCalledWith(1);
    // The fixture job that used to render at this level does not appear.
    expect(canvas().queryByText('Model soil moisture thresholds')).toBeNull();
  });

  it('an empty plan renders an empty canvas rather than crashing', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Harvest thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();
    await waitFor(() => expect(screen.getByText('Threads')).toBeTruthy());
    fireEvent.click(screen.getByTestId('thread-1'));

    await waitFor(() => expect(client.getPlan).toHaveBeenCalledWith(1));
    expect(canvas().queryByText('Model soil moisture thresholds')).toBeNull();
  });

  it('a fixture plan id issues no /api/plan request and renders an empty canvas', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });

    renderApp();
    fireEvent.click(canvas().getByText('Grove automation'));
    fireEvent.click(canvas().getByText('Automated drip-irrigation scheduling'));

    // Give any (unwanted) effect a tick to fire, then assert it never did.
    await new Promise((r) => setTimeout(r, 0));
    expect(client.getPlan).not.toHaveBeenCalled();
    expect(canvas().queryByText('Model soil moisture thresholds')).toBeNull();
  });
});
