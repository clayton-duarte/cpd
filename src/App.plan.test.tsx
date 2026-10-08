import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, within, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import App from './App';
import * as client from './engine/client';

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

describe('App renders the real plan at the jobs level (H9)', () => {
  it('shows a daemon-backed job rather than the fixture job for that plan', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });
    vi.mocked(client.getPlan).mockResolvedValue({
      jobs: [{ id: 'real-1', title: 'Fetched from the daemon', status: 'done', needs: [] }],
    });

    renderApp();
    fireEvent.click(canvas().getByText('Grove automation'));
    fireEvent.click(canvas().getByText('Automated drip-irrigation scheduling'));

    expect(await canvas().findByText('Fetched from the daemon')).toBeTruthy();
    expect(client.getPlan).toHaveBeenCalled();
    // The fixture job that used to render at this level does not appear.
    expect(canvas().queryByText('Model soil moisture thresholds')).toBeNull();
  });

  it('an empty plan renders an empty canvas rather than crashing', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();
    fireEvent.click(canvas().getByText('Grove automation'));
    fireEvent.click(canvas().getByText('Automated drip-irrigation scheduling'));

    await waitFor(() => expect(client.getPlan).toHaveBeenCalled());
    expect(canvas().queryByText('Model soil moisture thresholds')).toBeNull();
  });
});
