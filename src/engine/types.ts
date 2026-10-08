export type Message = { role: 'user' | 'assistant' | 'system'; content: string };

export type HealthResponse = { ok: true; conversationId: number };

export type MessagesResponse = { messages: Message[] };

export type PromptResponse = { status: 'done' | 'unanswered'; reason?: string };

/** Branded so a raw number can't be passed where a conversation id is expected by accident. */
export type ConversationId = number & { readonly __brand: 'ConversationId' };

export type ConversationNode = {
  id: ConversationId;
  parentId: ConversationId | null;
  at: number | null;
  title: string;
};

export type ConversationsResponse = { conversations: ConversationNode[] };

export type StreamEvent =
  | { type: 'messages'; messages: Message[] }
  | { type: 'conversations' };
