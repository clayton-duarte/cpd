import type {
  ConversationId,
  ConversationsResponse,
  ForkResponse,
  MessagesResponse,
  PromptResponse,
} from './types';

const BASE = (import.meta.env.VITE_CPD_API as string | undefined) ?? '/api';

async function parseOrThrow<T>(res: Response): Promise<T> {
  if (!res.ok) {
    throw new Error(`Request failed: ${res.status} ${res.statusText}`);
  }
  return (await res.json()) as T;
}

export async function getMessages(conversation?: ConversationId): Promise<MessagesResponse> {
  const suffix = conversation === undefined ? '' : `?conversation=${conversation}`;
  const res = await fetch(`${BASE}/messages${suffix}`);
  return parseOrThrow<MessagesResponse>(res);
}

export async function getConversations(): Promise<ConversationsResponse> {
  const res = await fetch(`${BASE}/conversations`);
  return parseOrThrow<ConversationsResponse>(res);
}

export async function sendPrompt(text: string, conversation: ConversationId | undefined): Promise<PromptResponse> {
  const suffix = conversation === undefined ? '' : `?conversation=${conversation}`;
  const res = await fetch(`${BASE}/prompt${suffix}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
  return parseOrThrow<PromptResponse>(res);
}

/** Fork a plan thread at entry `at`. Title is intentionally omitted -- the daemon derives it
 * (G1's `deriveTitle`), so there is exactly one implementation of that rule. */
export async function forkConversation(at: number): Promise<ForkResponse> {
  const res = await fetch(`${BASE}/fork`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ at }),
  });
  return parseOrThrow<ForkResponse>(res);
}
