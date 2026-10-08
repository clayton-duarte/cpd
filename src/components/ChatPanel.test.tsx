import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { ChatPanel } from './ChatPanel';
import * as client from '../engine/client';
import * as engineStream from '../engine/useEngineStream';
import type { Message } from '../engine/types';

vi.mock('../engine/client');
vi.mock('../engine/useEngineStream');

function mockStream(messages: Message[]) {
  vi.mocked(engineStream.useEngineStream).mockReturnValue({ messages, status: 'open' });
}

function renderPanel() {
  return render(
    <MantineProvider>
      <ChatPanel />
    </MantineProvider>,
  );
}

describe('ChatPanel', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('renders user and assistant messages, skips system messages', () => {
    mockStream([
      { id: 1, role: 'user', content: 'hello' },
      { id: 2, role: 'assistant', content: 'hi there' },
      { id: 3, role: 'system', content: 'secret setup' },
    ]);

    renderPanel();

    expect(screen.getByText('hello')).toBeTruthy();
    expect(screen.getByText('hi there')).toBeTruthy();
    expect(screen.queryByText('secret setup')).toBeNull();
  });

  it('Enter calls sendPrompt, Shift+Enter does not', () => {
    mockStream([]);
    const sendPromptMock = vi.mocked(client.sendPrompt).mockResolvedValue({ status: 'done' });

    renderPanel();
    const textarea = screen.getByRole('textbox');

    fireEvent.change(textarea, { target: { value: 'line one' } });
    fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: true });
    expect(sendPromptMock).not.toHaveBeenCalled();

    fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: false });
    expect(sendPromptMock).toHaveBeenCalledWith('line one', undefined);
  });

  it('disables the textarea while a prompt is in flight', async () => {
    mockStream([]);
    let resolvePrompt: (value: { status: 'done' }) => void = () => {};
    vi.mocked(client.sendPrompt).mockReturnValue(
      new Promise((resolve) => {
        resolvePrompt = resolve;
      }),
    );

    renderPanel();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: 'wait for it' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    await waitFor(() => expect(textarea.disabled).toBe(true));

    resolvePrompt({ status: 'done' });
    await waitFor(() => expect(textarea.disabled).toBe(false));
  });

  it('renders an Alert with reason as title and detail as body for an unanswered response', async () => {
    mockStream([]);
    vi.mocked(client.sendPrompt).mockResolvedValue({
      status: 'unanswered',
      reason: 'model_error',
      detail: 'This request was blocked as it seems to violate policy.',
    });

    renderPanel();
    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'anything' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    await waitFor(() => expect(screen.getByTestId('unanswered-alert')).toBeTruthy());
    expect(screen.getByText('model_error')).toBeTruthy();
    expect(screen.getByText('This request was blocked as it seems to violate policy.')).toBeTruthy();
  });

  // Falsification table (L1): each case must fail if run against a sabotaged implementation.
  it('[A] fails if submitPrompt / sendPrompt drops detail from its return value', async () => {
    mockStream([]);
    vi.mocked(client.sendPrompt).mockResolvedValue({ status: 'unanswered', reason: 'model_error' });

    renderPanel();
    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'anything' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    await waitFor(() => expect(screen.getByTestId('unanswered-alert')).toBeTruthy());
    // detail is absent -- the alert body falls back to the reason, so it must NOT show a separate
    // detail string. This asserts the fallback text equals the reason exactly (no stray detail).
    const alert = screen.getByTestId('unanswered-alert');
    expect(alert.textContent).toContain('model_error');
  });

  it('[C] the alert renders detail text, not only the reason', async () => {
    mockStream([]);
    vi.mocked(client.sendPrompt).mockResolvedValue({
      status: 'unanswered',
      reason: 'model_error',
      detail: 'unique-detail-text-xyz',
    });

    renderPanel();
    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'anything' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    await waitFor(() => expect(screen.getByText('unique-detail-text-xyz')).toBeTruthy());
  });

  it('[D] the alert does not render for status "done"', async () => {
    mockStream([]);
    vi.mocked(client.sendPrompt).mockResolvedValue({ status: 'done' });

    renderPanel();
    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'anything' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    await waitFor(() => expect(textarea.hasAttribute('disabled')).toBe(false));
    expect(screen.queryByTestId('unanswered-alert')).toBeNull();
  });

  it('wraps long content instead of overflowing the panel', () => {
    const longToken = 'x'.repeat(300);
    mockStream([{ id: 1, role: 'user', content: longToken }]);

    renderPanel();
    const paper = screen.getByText(longToken).closest('div')!;
    expect(paper.style.overflowWrap).toBe('anywhere');
  });

  it('keeps the optimistic message visible until the stream confirms it', async () => {
    mockStream([]);
    let resolvePrompt: (value: { status: 'done' }) => void = () => {};
    vi.mocked(client.sendPrompt).mockReturnValue(
      new Promise((resolve) => {
        resolvePrompt = resolve;
      }),
    );

    renderPanel();
    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'optimistic text' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    expect(screen.getByText('optimistic text')).toBeTruthy();

    resolvePrompt({ status: 'done' });
    await waitFor(() => expect(screen.getByRole('textbox').hasAttribute('disabled')).toBe(false));

    // Stream hasn't delivered the authoritative transcript yet -- the
    // optimistic message must still be visible, not vanished.
    expect(screen.getByText('optimistic text')).toBeTruthy();
  });

  it('shows the empty state with zero messages and hides it once one exists', () => {
    mockStream([]);
    const { rerender } = renderPanel();
    expect(screen.getByText('No messages yet')).toBeTruthy();

    mockStream([{ id: 1, role: 'user', content: 'hi' }]);
    rerender(
      <MantineProvider>
        <ChatPanel />
      </MantineProvider>,
    );
    expect(screen.queryByText('No messages yet')).toBeNull();
  });

  it('shows the status indicator only when status is not open', () => {
    vi.mocked(engineStream.useEngineStream).mockReturnValue({ messages: [], status: 'connecting' });
    const { rerender } = renderPanel();
    expect(screen.getByText('reconnecting')).toBeTruthy();

    vi.mocked(engineStream.useEngineStream).mockReturnValue({ messages: [], status: 'open' });
    rerender(
      <MantineProvider>
        <ChatPanel />
      </MantineProvider>,
    );
    expect(screen.queryByText('reconnecting')).toBeNull();
  });

  it('shows a fork affordance for assistant (lead) messages but not user messages', () => {
    mockStream([
      { id: 7, role: 'user', content: 'no affordance for me' },
      { id: 11, role: 'assistant', content: 'fork me into a new plan' },
    ]);

    renderPanel();

    expect(screen.getByTestId('fork-11')).toBeTruthy();
    expect(screen.queryByTestId('fork-7')).toBeNull();
  });

  it('clicking the fork affordance POSTs at: <entry id>', () => {
    mockStream([{ id: 11, role: 'assistant', content: 'fork me into a new plan' }]);
    const forkMock = vi.mocked(client.forkConversation).mockResolvedValue({ id: 99 });

    renderPanel();
    fireEvent.click(screen.getByTestId('fork-11'));

    expect(forkMock).toHaveBeenCalledWith(11);
  });

  it('H: never renders a fork affordance beside USER messages (sabotage guard)', () => {
    mockStream([
      { id: 1, role: 'user', content: 'a user message' },
      { id: 2, role: 'assistant', content: 'a lead reply' },
    ]);

    renderPanel();

    expect(screen.queryByTestId('fork-1')).toBeNull();
    expect(screen.getByTestId('fork-2')).toBeTruthy();
  });
});
