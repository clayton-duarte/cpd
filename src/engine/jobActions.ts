import type { ConversationId } from './types';

const BASE = (import.meta.env.VITE_CPD_API as string | undefined) ?? '/api';

/**
 * J3b: abort a running job. Separate file from `src/engine/client.ts` (J3a owns that file in
 * parallel) so the two cards never edit the same file. Follows the same fetch/error convention
 * as the neighbouring client functions.
 */
export async function abortJob(conversation: ConversationId, id: string): Promise<void> {
  const res = await fetch(`${BASE}/plan/job/abort?conversation=${conversation}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id }),
  });
  if (!res.ok) {
    throw new Error(`Request failed: ${res.status} ${res.statusText}`);
  }
}
