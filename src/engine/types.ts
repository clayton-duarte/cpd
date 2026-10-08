export type Message = { id: number; role: 'user' | 'assistant' | 'system'; content: string };

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

export type ForkResponse = { id: number };

/** Daemon's minimal job shape (H4/H7). See src/model/fromPlan.ts's DaemonJob -- same shape. */
export type PlanJob = {
  id: string;
  title: string;
  status: 'draft' | 'queued' | 'running' | 'done' | 'failed';
  needs: string[];
  command?: string;
};

export type PlanResponse = { jobs: PlanJob[] };

export type CreateJobResponse = { job: PlanJob };

export type RunJobResponse = { taskId: string };

export type StreamEvent =
  | { type: 'messages'; messages: Message[] }
  | { type: 'conversations' }
  | { type: 'plan'; conversation: number; jobs: PlanJob[] };
