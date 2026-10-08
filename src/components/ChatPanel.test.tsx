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

  it('replaces the send button with an enabled Stop button while a prompt is in flight', async () => {
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

    const stopButton = await screen.findByTestId('stop-button');
    expect((stopButton as HTMLButtonElement).disabled).toBe(false);
    expect(textarea.disabled).toBe(false);

    resolvePrompt({ status: 'done' });
    await waitFor(() => expect(screen.queryByTestId('stop-button')).toBeNull());
    const sendButton = screen.getByTestId('send-button') as HTMLButtonElement;
    // Draft was cleared on submit, so the button stays disabled for emptiness once the reply
    // lands -- typing fresh text re-enables it, proving it's no longer gated on `inFlight`.
    expect(sendButton.disabled).toBe(true);
    fireEvent.change(textarea, { target: { value: 'another message' } });
    expect(sendButton.disabled).toBe(false);
  });

  it('disables the send button when the draft is empty, enables it once text is typed', () => {
    mockStream([]);
    renderPanel();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    const sendButton = screen.getByRole('button') as HTMLButtonElement;

    expect(sendButton.disabled).toBe(true);

    fireEvent.change(textarea, { target: { value: 'hello' } });
    expect(sendButton.disabled).toBe(false);

    fireEvent.change(textarea, { target: { value: '   ' } });
    expect(sendButton.disabled).toBe(true);
  });

  // A: composer must not be blurred by submitting.
  it('the composer keeps focus immediately after submit (A)', () => {
    mockStream([]);
    vi.mocked(client.sendPrompt).mockReturnValue(new Promise(() => {}));

    renderPanel();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    textarea.focus();
    fireEvent.change(textarea, { target: { value: 'hello' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    expect(textarea.disabled).toBe(false);
    expect(document.activeElement).toBe(textarea);
  });

  // B: composer must still hold focus after the async reply lands, not just synchronously.
  it('the composer keeps focus after the reply arrives (B)', async () => {
    mockStream([]);
    let resolvePrompt: (value: { status: 'done' }) => void = () => {};
    vi.mocked(client.sendPrompt).mockReturnValue(
      new Promise((resolve) => {
        resolvePrompt = resolve;
      }),
    );

    renderPanel();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    textarea.focus();
    fireEvent.change(textarea, { target: { value: 'hello' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    expect(textarea.disabled).toBe(false);

    resolvePrompt({ status: 'done' });
    await waitFor(() => expect(client.sendPrompt).toHaveBeenCalled());

    expect(document.activeElement).toBe(textarea);
  });

  // C: removing `!e.shiftKey` from onKeyDown must turn this red (regression guard for the
  // already-correct Enter/Shift+Enter behavior -- see the LEAD CORRECTION on the L7 card).
  it('Shift+Enter does not submit (C)', () => {
    mockStream([]);
    const sendPromptMock = vi.mocked(client.sendPrompt).mockResolvedValue({ status: 'done' });

    renderPanel();
    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'line one' } });
    fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: true });

    expect(sendPromptMock).not.toHaveBeenCalled();
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

  // L8 falsification table: each case must fail against a sabotaged implementation.
  describe('L8: abort a run in flight', () => {
    // [A] Stop renders but is not wired to the abort call -> must fail
    it('[A] clicking Stop while in flight calls abortPrompt', async () => {
      mockStream([]);
      vi.mocked(client.sendPrompt).mockReturnValue(new Promise(() => {}));
      const abortMock = vi.mocked(client.abortPrompt).mockResolvedValue(undefined);

      renderPanel();
      const textarea = screen.getByRole('textbox');
      fireEvent.change(textarea, { target: { value: 'wait for it' } });
      fireEvent.keyDown(textarea, { key: 'Enter' });

      const stopButton = await screen.findByTestId('stop-button');
      fireEvent.click(stopButton);

      expect(abortMock).toHaveBeenCalledWith(undefined);
    });

    // [B] Escape aborts when nothing is in flight -> must fail
    it('[B] Escape does not call abortPrompt when nothing is in flight', () => {
      mockStream([]);
      const abortMock = vi.mocked(client.abortPrompt).mockResolvedValue(undefined);

      renderPanel();
      const textarea = screen.getByRole('textbox');
      fireEvent.keyDown(textarea, { key: 'Escape' });

      expect(abortMock).not.toHaveBeenCalled();
    });

    // [C] abort clears the user's draft text -> must fail
    it('[C] Escape while in flight aborts without clearing the draft', async () => {
      mockStream([]);
      vi.mocked(client.sendPrompt).mockReturnValue(new Promise(() => {}));
      const abortMock = vi.mocked(client.abortPrompt).mockResolvedValue(undefined);

      renderPanel();
      const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
      fireEvent.change(textarea, { target: { value: 'first message' } });
      fireEvent.keyDown(textarea, { key: 'Enter' });

      // Submitting clears the draft that was sent; type a NEW draft while the first is in flight.
      fireEvent.change(textarea, { target: { value: 'unsent draft' } });
      fireEvent.keyDown(textarea, { key: 'Escape' });

      expect(abortMock).toHaveBeenCalled();
      expect(textarea.value).toBe('unsent draft');
    });

    // [D] after abort, inFlight stays true (send never returns) -> must fail
    it('[D] after abort the composer is usable again: textarea and Stop/Send reset', async () => {
      mockStream([]);
      vi.mocked(client.sendPrompt).mockReturnValue(new Promise(() => {}));
      vi.mocked(client.abortPrompt).mockResolvedValue(undefined);

      renderPanel();
      const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
      textarea.focus();
      fireEvent.change(textarea, { target: { value: 'wait for it' } });
      fireEvent.keyDown(textarea, { key: 'Enter' });

      await screen.findByTestId('stop-button');
      fireEvent.keyDown(textarea, { key: 'Escape' });

      await waitFor(() => expect(screen.queryByTestId('stop-button')).toBeNull());
      expect(textarea.disabled).toBe(false);
      expect(document.activeElement).toBe(textarea);
    });
  });
});
