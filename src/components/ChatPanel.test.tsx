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
      { role: 'user', content: 'hello' },
      { role: 'assistant', content: 'hi there' },
      { role: 'system', content: 'secret setup' },
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
    expect(sendPromptMock).toHaveBeenCalledWith('line one');
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

  it('renders the reason badge for an unanswered response', async () => {
    mockStream([]);
    vi.mocked(client.sendPrompt).mockResolvedValue({ status: 'unanswered', reason: 'no model' });

    renderPanel();
    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'anything' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    await waitFor(() => expect(screen.getByText('no model')).toBeTruthy());
  });
});
