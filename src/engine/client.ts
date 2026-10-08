import type {
  ConversationId,
  ConversationsResponse,
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

export async function sendPrompt(text: string, conversation?: ConversationId): Promise<PromptResponse> {
  const body: { text: string; conversation?: ConversationId } = { text };
  if (conversation !== undefined) body.conversation = conversation;
  const res = await fetch(`${BASE}/prompt`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  return parseOrThrow<PromptResponse>(res);
}
