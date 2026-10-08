import { afterEach, describe, expect, it, vi } from 'vitest';
import { getMessages, sendPrompt } from './client';

describe('client', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('getMessages parses the envelope', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ messages: [{ role: 'user', content: 'hi' }] }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await getMessages();

    expect(fetchMock).toHaveBeenCalledWith('/api/messages');
    expect(result).toEqual({ messages: [{ role: 'user', content: 'hi' }] });
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
