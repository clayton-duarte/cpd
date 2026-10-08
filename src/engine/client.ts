import type { MessagesResponse, PromptResponse } from './types';

const BASE = (import.meta.env.VITE_CPD_API as string | undefined) ?? '/api';

async function parseOrThrow<T>(res: Response): Promise<T> {
  if (!res.ok) {
    throw new Error(`Request failed: ${res.status} ${res.statusText}`);
  }
  return (await res.json()) as T;
}

export async function getMessages(): Promise<MessagesResponse> {
  const res = await fetch(`${BASE}/messages`);
  return parseOrThrow<MessagesResponse>(res);
}

export async function sendPrompt(text: string): Promise<PromptResponse> {
  const res = await fetch(`${BASE}/prompt`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
  return parseOrThrow<PromptResponse>(res);
}
