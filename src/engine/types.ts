export type Message = { role: 'user' | 'assistant' | 'system'; content: string };

export type HealthResponse = { ok: true; conversationId: number };

export type MessagesResponse = { messages: Message[] };

export type PromptResponse = { status: 'done' | 'unanswered'; reason?: string };

export type StreamEvent = { type: 'messages'; messages: Message[] };
