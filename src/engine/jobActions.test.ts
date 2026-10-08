import { afterEach, describe, expect, it, vi } from 'vitest';
import { abortJob } from './jobActions';
import type { ConversationId } from './types';

describe('abortJob', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('POSTs to /api/plan/job/abort with the conversation query param and id body', async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 200, statusText: 'OK' });
    vi.stubGlobal('fetch', fetchMock);

    await abortJob(7 as ConversationId, 'job-1');

    expect(fetchMock).toHaveBeenCalledWith('/api/plan/job/abort?conversation=7', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id: 'job-1' }),
    });
  });

  it('throws on a non-ok response', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({ ok: false, status: 409, statusText: 'Conflict' }),
    );

    await expect(abortJob(7 as ConversationId, 'job-1')).rejects.toThrow('409');
  });
});
