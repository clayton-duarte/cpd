export type Message = { id: number; role: 'user' | 'assistant' | 'system'; content: string };

export type HealthResponse = { ok: true; conversationId: number };

export type MessagesResponse = { messages: Message[] };

export type PromptResponse = { status: 'done' | 'unanswered'; reason?: string; detail?: string };

/** Branded so a raw number can't be passed where a conversation id is expected by accident. */
export type ConversationId = number & { readonly __brand: 'ConversationId' };

export type ConversationNode = {
  id: ConversationId;
  parentId: ConversationId | null;
  at: number | null;
  title: string;
  /** L4: CPD-owned archived flag. Optional so existing fixtures/tests that predate archiving
   * don't need updating; treat a missing value as `false` (not archived). */
  archived?: boolean;
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

export type AttentionItem = {
  jobId: string;
  conversationId: number;
  conversationTitle: string;
  jobTitle: string;
  status: string;
  reason: string;
  at: number;
};

export type AttentionResponse = { items: AttentionItem[] };

export type ArchiveResponse = { archived: boolean };

/** `{provider, modelId}` -- same flat shape the daemon persists in `pi.agent` and the one exposed
 * over the API; not the pi-durable `ModelRef` type (private to the daemon). */
export type ModelRef = { provider: string; modelId: string };

export type ModelsResponse = { models: ModelRef[] };

export type ModelResponse = { model: ModelRef };

export type StreamEvent =
  | { type: 'messages'; messages: Message[]; model?: ModelRef }
  | { type: 'conversations' }
  | { type: 'plan'; conversation: number; jobs: PlanJob[] }
  | { type: 'attention'; items: AttentionItem[] };
