import { afterEach, describe, expect, it, vi } from 'vitest';
import { getConversations, getMessages, sendPrompt } from './client';
import type { ConversationId } from './types';

describe('client', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('getMessages parses the envelope', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ messages: [{ id: 7, role: 'user', content: 'hi' }] }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await getMessages();

    expect(fetchMock).toHaveBeenCalledWith('/api/messages');
    expect(result).toEqual({ messages: [{ id: 7, role: 'user', content: 'hi' }] });
  });

  it('getMessages appends ?conversation=<id> when given an id', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ messages: [] }),
    });
    vi.stubGlobal('fetch', fetchMock);

    await getMessages(16 as ConversationId);

    expect(fetchMock).toHaveBeenCalledWith('/api/messages?conversation=16');
  });

  it('getConversations parses the envelope', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        conversations: [
          { id: 1, parentId: null, at: null, title: 'Lead' },
          { id: 16, parentId: 1, at: 7, title: 'Test thread' },
        ],
      }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await getConversations();

    expect(fetchMock).toHaveBeenCalledWith('/api/conversations');
    expect(result).toEqual({
      conversations: [
        { id: 1, parentId: null, at: null, title: 'Lead' },
        { id: 16, parentId: 1, at: 7, title: 'Test thread' },
      ],
    });
  });

  it('sendPrompt POSTs the right body and returns the response', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ status: 'done' }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await sendPrompt('hello there');

    expect(fetchMock).toHaveBeenCalledWith('/api/prompt', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 'hello there' }),
    });
    expect(result).toEqual({ status: 'done' });
  });

  it('sendPrompt includes the conversation id in the body when given one', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ status: 'done' }),
    });
    vi.stubGlobal('fetch', fetchMock);

    await sendPrompt('hello there', 16 as ConversationId);

    expect(fetchMock).toHaveBeenCalledWith('/api/prompt', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 'hello there', conversation: 16 }),
    });
  });

  it('throws on a non-2xx response instead of returning silent undefined', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 500,
      statusText: 'Internal Server Error',
      json: async () => ({}),
    });
    vi.stubGlobal('fetch', fetchMock);

    await expect(getMessages()).rejects.toThrow(/500/);
  });
});
