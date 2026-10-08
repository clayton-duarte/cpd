import type {
  ConversationId,
  ConversationsResponse,
  CreateJobResponse,
  ForkResponse,
  MessagesResponse,
  PlanResponse,
  PromptResponse,
  RunJobResponse,
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

/** I2: create a new conversation (optionally titled). Same error-handling style as createJob --
 * a non-2xx throws so the caller can surface it rather than silently doing nothing. */
export async function createConversation(title?: string): Promise<{ id: number }> {
  const res = await fetch(`${BASE}/conversation`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(title === undefined ? {} : { title }),
  });
  return parseOrThrow<{ id: number }>(res);
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

/** A 404 means "no such conversation" -- a normal race when a fork is being created (or the
 * conversation hasn't been selected yet). Resolve to an empty job list rather than throwing, so
 * an unlucky request never blanks the UI. */
export async function getPlan(conversation: ConversationId): Promise<PlanResponse> {
  const res = await fetch(`${BASE}/plan?conversation=${conversation}`);
  if (res.status === 404) return { jobs: [] };
  return parseOrThrow<PlanResponse>(res);
}

/** H13: create a job in the plan scoped to `conversation` (D112: a plan is a conversation). */
export async function createJob(
  conversation: ConversationId,
  job: { title: string; command?: string; needs?: string[] },
): Promise<CreateJobResponse> {
  const res = await fetch(`${BASE}/plan/job?conversation=${conversation}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(job),
  });
  return parseOrThrow<CreateJobResponse>(res);
}

/** H13: run an existing job's command. Returns the durable task id (D113). */
export async function runJob(conversation: ConversationId, id: string): Promise<RunJobResponse> {
  const res = await fetch(`${BASE}/plan/job/run?conversation=${conversation}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id }),
  });
  return parseOrThrow<RunJobResponse>(res);
}

/** H13: delete a job. The id goes in the request body, not the URL path. */
export async function deleteJob(conversation: ConversationId, id: string): Promise<void> {
  const res = await fetch(`${BASE}/plan/job?conversation=${conversation}`, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id }),
  });
  await parseOrThrow<unknown>(res);
}
