import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { render, screen, fireEvent, within, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { Notifications } from '@mantine/notifications';
import App from './App';
import * as client from './engine/client';
import * as jobActions from './engine/jobActions';
import type { ConversationId } from './engine/types';

vi.mock('./engine/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./engine/client')>();
  return {
    ...actual,
    getConversations: vi.fn(),
    getPlan: vi.fn(),
    createConversation: vi.fn(),
    getAttention: vi.fn().mockResolvedValue({ items: [] }),
  };
});

vi.mock('./engine/jobActions', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./engine/jobActions')>();
  return {
    ...actual,
    abortJob: vi.fn().mockResolvedValue(undefined),
  };
});

function renderApp() {
  return render(
    <MantineProvider>
      <Notifications />
      <App />
    </MantineProvider>,
  );
}

function canvas() {
  return within(screen.getByTestId('canvas-area'));
}

afterEach(() => {
  vi.clearAllMocks();
  window.location.hash = '';
});

/**
 * I2: the app renders conversations from the daemon only. The fixture-driven navigation (Lead ->
 * Plan -> Job) that used to drive the whole app is gone from App.tsx -- see the "TopBar does not
 * clip" static guard below for the one remaining structural check on App.tsx's source.
 */
describe('App shows only real conversations, no fixtures (I2)', () => {
  it('shows a daemon-backed conversation in the sidebar, never a fixture title', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Harvest thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();

    await waitFor(() => expect(screen.getByText('Harvest thread')).toBeTruthy());
    expect(screen.queryByText('Harvest planner')).toBeNull();
    expect(screen.queryByText('Orchard')).toBeNull();
  });

  it('an empty conversations list shows a "New session" affordance and the app stays usable', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });

    renderApp();

    expect(await screen.findByTestId('new-session-button')).toBeTruthy();
    expect(canvas().getByText('No context selected.')).toBeTruthy();
  });

  it('F: clicking the "New context" FAB calls createConversation and selects the returned id', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });
    vi.mocked(client.createConversation).mockResolvedValue({ id: 42 });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();

    fireEvent.click(await screen.findByTestId('new-context-fab'));

    await waitFor(() => expect(client.createConversation).toHaveBeenCalled());
    await waitFor(() => expect(window.location.hash).toBe('#/c/42'));
  });

  it('clicking "New session" calls createConversation and selects the returned id', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });
    vi.mocked(client.createConversation).mockResolvedValue({ id: 42 });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();

    fireEvent.click(await screen.findByTestId('new-session-button'));

    await waitFor(() => expect(client.createConversation).toHaveBeenCalled());
    await waitFor(() => expect(client.getPlan).toHaveBeenCalledWith(42));
  });

  it('K2: clicking "New session" navigates to the new conversation\'s hash route', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });
    vi.mocked(client.createConversation).mockResolvedValue({ id: 99 });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();

    fireEvent.click(await screen.findByTestId('new-session-button'));

    await waitFor(() => expect(window.location.hash).toBe('#/c/99'));
  });

  it('K2: the New session button is disabled while the create request is in flight', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });
    let resolveCreate: (value: { id: number }) => void = () => {};
    vi.mocked(client.createConversation).mockReturnValue(
      new Promise((resolve) => {
        resolveCreate = resolve;
      }),
    );
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();

    const button = (await screen.findByTestId('new-session-button')) as HTMLButtonElement;
    fireEvent.click(button);

    await waitFor(() => expect(button.disabled).toBe(true));

    resolveCreate({ id: 7 });

    await waitFor(() => expect(button.disabled).toBe(false));
  });

  it('K2: a failed create shows an error notification and does not navigate away', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Harvest thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });
    vi.mocked(client.createConversation).mockRejectedValue(new Error('Request failed: 500 Internal Server Error'));

    renderApp();
    await waitFor(() => expect(screen.getByText('Harvest thread')).toBeTruthy());
    fireEvent.click(screen.getByTestId('thread-1'));
    await waitFor(() => expect(window.location.hash).toBe('#/c/1'));

    fireEvent.click(screen.getByTestId('new-session-button'));

    await waitFor(() => expect(screen.getByText(/could not create session/i)).toBeTruthy());
    expect(window.location.hash).toBe('#/c/1');
  });

  it('selecting a conversation requests /api/plan for that conversation', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 7 as ConversationId, parentId: null, at: null, title: 'Lead thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({
      jobs: [{ id: 'real-1', title: 'Fetched from the daemon', status: 'done', needs: [] }],
    });

    renderApp();
    await waitFor(() => expect(screen.getByText('Lead thread')).toBeTruthy());
    fireEvent.click(screen.getByTestId('thread-7'));

    expect(await canvas().findByText('Fetched from the daemon')).toBeTruthy();
    expect(client.getPlan).toHaveBeenCalledWith(7);
  });

  it('an empty plan renders an empty canvas rather than crashing', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Harvest thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();
    await waitFor(() => expect(screen.getByText('Harvest thread')).toBeTruthy());
    fireEvent.click(screen.getByTestId('thread-1'));

    await waitFor(() => expect(client.getPlan).toHaveBeenCalledWith(1));
  });

  it('the composer is present at the jobs level (no level removes it)', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Harvest thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();
    await waitFor(() => expect(screen.getByText('Harvest thread')).toBeTruthy());

    // Composer present before selecting a conversation...
    expect(screen.getByRole('textbox')).toBeTruthy();

    fireEvent.click(screen.getByTestId('thread-1'));
    await waitFor(() => expect(client.getPlan).toHaveBeenCalledWith(1));

    // ...and still present at the jobs level.
    expect(screen.getByRole('textbox')).toBeTruthy();
  });

  it('clicking an attention item for a different conversation switches to it and opens the action bar', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Harvest thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({
      jobs: [{ id: 'job-1', title: 'Blocked job', status: 'running', needs: [] }],
    });
    vi.mocked(client.getAttention).mockResolvedValue({
      items: [
        {
          jobId: 'job-1',
          conversationId: 1,
          conversationTitle: 'Harvest thread',
          jobTitle: 'Blocked job',
          status: 'blocked',
          reason: 'waiting on input',
          at: 0,
        },
      ],
    });

    renderApp();
    await waitFor(() => expect(screen.getByTestId('attention-item-job-1')).toBeTruthy());

    fireEvent.click(screen.getByTestId('attention-item-job-1'));

    await waitFor(() => expect(client.getPlan).toHaveBeenCalledWith(1));
    await waitFor(() => expect((screen.getByTestId('action-stop') as HTMLButtonElement).disabled).toBe(false));
  });

  it('Stop in the action bar calls abortJob for the selected conversation and job', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 1 as ConversationId, parentId: null, at: null, title: 'Harvest thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({
      jobs: [{ id: 'job-1', title: 'Running job', status: 'running', needs: [] }],
    });
    vi.mocked(client.getAttention).mockResolvedValue({
      items: [
        {
          jobId: 'job-1',
          conversationId: 1,
          conversationTitle: 'Harvest thread',
          jobTitle: 'Running job',
          status: 'blocked',
          reason: 'waiting on input',
          at: 0,
        },
      ],
    });
    const abortSpy = vi.mocked(jobActions.abortJob);

    renderApp();
    await waitFor(() => expect(screen.getByTestId('attention-item-job-1')).toBeTruthy());
    fireEvent.click(screen.getByTestId('attention-item-job-1'));
    await waitFor(() => expect((screen.getByTestId('action-stop') as HTMLButtonElement).disabled).toBe(false));

    fireEvent.click(screen.getByTestId('action-stop'));

    expect(abortSpy).toHaveBeenCalledWith(1, 'job-1');
  });
});

/**
 * jsdom does not perform real layout, so a rendered box-measurement assertion would not be
 * meaningful here. Per the B5 card, the honest alternative is a static guard: fail if the top bar
 * Group is ever given a fixed `h` prop again, which is exactly the defect this card fixes (a
 * content-sized two-line bar snapped to a smaller fixed/token height, clipping its second line).
 */
describe('TopBar does not clip its content', () => {
  it('does not set a fixed height on the top bar Group', () => {
    const appPath = join(process.cwd(), 'src', 'App.tsx');
    const appSource = readFileSync(appPath, 'utf-8');
    const topBarMatch = appSource.match(/function TopBar\([\s\S]*?\n}\n/);
    expect(topBarMatch, 'TopBar function not found in App.tsx').toBeTruthy();
    const topBarSource = topBarMatch![0];

    expect(topBarSource).not.toMatch(/\bh=\{/);
    expect(topBarSource).not.toMatch(/\bh="/);
    expect(topBarSource).toMatch(/py="var\(--space-\d+\)"/);
  });
});

/**
 * I3: reloading the page loses the in-memory selection, so the URL hash is now the durable
 * record of "which conversation". These tests drive the hash directly (as a reload would) and
 * through the UI (selecting, creating), and check both directions stay in sync without ever
 * requesting a plan/messages for a conversation the hash named but the daemon doesn't have.
 */
describe('selected conversation persists in the URL hash (I3)', () => {
  it('a hash naming an existing conversation selects it and requests its plan', async () => {
    window.location.hash = '#/c/7';
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 7 as ConversationId, parentId: null, at: null, title: 'Lead thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({
      jobs: [{ id: 'real-1', title: 'Fetched from the daemon', status: 'done', needs: [] }],
    });

    renderApp();

    expect(await canvas().findByText('Fetched from the daemon')).toBeTruthy();
    expect(client.getPlan).toHaveBeenCalledWith(7);
  });

  it('a hash naming a non-existent conversation falls back to the empty state, no plan request', async () => {
    window.location.hash = '#/c/999';
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 7 as ConversationId, parentId: null, at: null, title: 'Lead thread' }],
    });

    renderApp();

    await waitFor(() => expect(screen.getByText('Lead thread')).toBeTruthy());
    expect(canvas().getByText('No context selected.')).toBeTruthy();
    expect(client.getPlan).not.toHaveBeenCalled();
  });

  it('a hash segment that is not a positive integer is ignored, no request', async () => {
    window.location.hash = '#/c/abc';
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });

    renderApp();

    expect(await screen.findByTestId('new-session-button')).toBeTruthy();
    expect(canvas().getByText('No context selected.')).toBeTruthy();
    expect(client.getPlan).not.toHaveBeenCalled();
  });

  it('selecting a conversation updates window.location.hash', async () => {
    vi.mocked(client.getConversations).mockResolvedValue({
      conversations: [{ id: 7 as ConversationId, parentId: null, at: null, title: 'Lead thread' }],
    });
    vi.mocked(client.getPlan).mockResolvedValue({ jobs: [] });

    renderApp();
    await waitFor(() => expect(screen.getByText('Lead thread')).toBeTruthy());
    fireEvent.click(screen.getByTestId('thread-7'));

    await waitFor(() => expect(window.location.hash).toBe('#/c/7'));
  });

  it('the gallery route still resolves and is not hijacked by the conversation hash logic', async () => {
    window.location.hash = '#/gallery';
    vi.mocked(client.getConversations).mockResolvedValue({ conversations: [] });

    renderApp();

    expect(await screen.findByText('CPD state gallery')).toBeTruthy();
  });
});
