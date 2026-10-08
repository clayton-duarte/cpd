import { afterEach, describe, expect, it, vi } from 'vitest';
import { createJob, deleteJob, getConversations, getMessages, getPlan, runJob, sendPrompt } from './client';
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

    const result = await sendPrompt('hello there', undefined);

    expect(fetchMock).toHaveBeenCalledWith('/api/prompt', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 'hello there' }),
    });
    expect(result).toEqual({ status: 'done' });
  });

  it('sendPrompt appends ?conversation=<id> to the URL when given one', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ status: 'done' }),
    });
    vi.stubGlobal('fetch', fetchMock);

    await sendPrompt('hello there', 16 as ConversationId);

    expect(fetchMock).toHaveBeenCalledWith('/api/prompt?conversation=16', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 'hello there' }),
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

  it('getPlan fetches jobs for a conversation', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ jobs: [{ id: 'j1', title: 'Build', status: 'done', needs: [] }] }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await getPlan(16 as ConversationId);

    expect(fetchMock).toHaveBeenCalledWith('/api/plan?conversation=16');
    expect(result).toEqual({ jobs: [{ id: 'j1', title: 'Build', status: 'done', needs: [] }] });
  });

  it('getPlan resolves to an empty job list on 404 instead of throwing', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 404,
      statusText: 'Not Found',
      json: async () => ({ error: 'Unknown conversation' }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await getPlan(99 as ConversationId);

    expect(result).toEqual({ jobs: [] });
  });

  it('createJob POSTs to /api/plan/job with the conversation and body', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ job: { id: 'j1', title: 'Build', status: 'draft', needs: [] } }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await createJob(16 as ConversationId, { title: 'Build', command: 'echo hi' });

    expect(fetchMock).toHaveBeenCalledWith('/api/plan/job?conversation=16', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: 'Build', command: 'echo hi' }),
    });
    expect(result).toEqual({ job: { id: 'j1', title: 'Build', status: 'draft', needs: [] } });
  });

  it('runJob POSTs to /api/plan/job/run with the job id', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ taskId: 't1' }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await runJob(16 as ConversationId, 'j1');

    expect(fetchMock).toHaveBeenCalledWith('/api/plan/job/run?conversation=16', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id: 'j1' }),
    });
    expect(result).toEqual({ taskId: 't1' });
  });

  it('deleteJob DELETEs /api/plan/job with the id in the body', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true }),
    });
    vi.stubGlobal('fetch', fetchMock);

    await deleteJob(16 as ConversationId, 'j1');

    expect(fetchMock).toHaveBeenCalledWith('/api/plan/job?conversation=16', {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id: 'j1' }),
    });
  });

  it('createJob throws on a 400 so the caller can surface the error', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 400,
      statusText: 'Bad Request',
      json: async () => ({ error: 'command must be a non-empty string' }),
    });
    vi.stubGlobal('fetch', fetchMock);

    await expect(createJob(16 as ConversationId, { title: 'Build', command: '   ' })).rejects.toThrow(/400/);
  });
});
