import { useEffect, useRef, useState } from 'react';
import {
  ActionIcon,
  Alert,
  Badge,
  EmptyState,
  Group,
  Paper,
  ScrollArea,
  Stack,
  Text,
  Textarea,
  Tooltip,
} from '@mantine/core';
import { abortPrompt, forkConversation, sendPrompt } from '../engine/client';
import { useEngineStream } from '../engine/useEngineStream';
import type { ConversationId, Message } from '../engine/types';

export interface ChatPanelProps {
  /** Selected thread from the sidebar's additive conversation tree. Scopes the transcript and
   * outgoing prompts to that conversation; undefined keeps the default/root stream. Selecting a
   * thread must change ONLY this -- it must never touch canvas nav state. */
  conversationId?: ConversationId;
  /** Called after a successful fork with the new thread's id, so the caller can select it and
   * land the user there. Reuses the caller's own selection state -- no second source of truth. */
  onForked?: (id: ConversationId) => void;
}

/**
 * Lead chat panel. Messages come from the shared SSE store (useEngineStream);
 * sending a prompt optimistically appends the user's message locally, then
 * the next stream event replaces the list with the authoritative transcript.
 */
export function ChatPanel({ conversationId, onForked }: ChatPanelProps) {
  const { messages, status } = useEngineStream(conversationId);
  const [pending, setPending] = useState<Message | null>(null);
  const [draft, setDraft] = useState('');
  const [inFlight, setInFlight] = useState(false);
  const [unanswered, setUnanswered] = useState<{ reason: string; detail?: string } | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // The optimistic message is cleared once the authoritative transcript
  // actually contains it -- clearing it in a `finally` instead would make it
  // flicker away and reappear if the stream hasn't caught up yet.
  useEffect(() => {
    if (pending && messages.some((m) => m.role === pending.role && m.content === pending.content)) {
      setPending(null);
    }
  }, [messages, pending]);

  const visible = [...messages, ...(pending ? [pending] : [])].filter((m) => m.role !== 'system');

  useEffect(() => {
    bottomRef.current?.scrollIntoView?.({ block: 'end' });
  }, [visible.length]);

  async function submit() {
    const text = draft.trim();
    if (!text || inFlight) return;

    // Optimistic id is a placeholder -- it never reaches the fork affordance because the
    // pending message is cleared the instant the authoritative transcript (with its real
    // entry id) catches up to it; see the effect above.
    const userMessage: Message = { id: -1, role: 'user', content: text };
    setPending(userMessage);
    setDraft('');
    setInFlight(true);

    try {
      const result = await sendPrompt(text, conversationId);
      if (result.status !== 'done') {
        setUnanswered({ reason: result.reason ?? 'unanswered', detail: result.detail });
      } else {
        setUnanswered(null);
      }
    } finally {
      setInFlight(false);
    }
  }

  async function fork(at: number) {
    const result = await forkConversation(at);
    onForked?.(result.id as ConversationId);
  }

  function abort() {
    void abortPrompt(conversationId);
    setInFlight(false);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void submit();
    } else if (e.key === 'Escape' && inFlight) {
      e.preventDefault();
      abort();
    }
  }

  return (
    <Stack h="100%" gap={0}>
      <ScrollArea style={{ flex: 1, minHeight: 0 }}>
        {visible.length === 0 ? (
          <EmptyState
            title="No messages yet"
            description="The lead conversation is empty."
            p="var(--pad)"
            styles={{ title: { fontSize: 'var(--mantine-font-size-sm)' } }}
          />
        ) : (
          <Stack gap="var(--space-2)" w="100%" p="var(--pad)">
            {visible.map((message, i) => {
              const isUser = message.role === 'user';
              const isLeadReply = message.role === 'assistant';
              return (
                <Group key={`${message.id}:${i}`} justify={isUser ? 'flex-end' : 'flex-start'} w="100%" wrap="nowrap">
                  {isLeadReply && (
                    <Tooltip label="Fork into new plan">
                      <ActionIcon
                        size="sm"
                        variant="subtle"
                        data-testid={`fork-${message.id}`}
                        onClick={() => void fork(message.id)}
                      >
                        <Text size="xs">⑂</Text>
                      </ActionIcon>
                    </Tooltip>
                  )}
                  <Paper
                    p="sm"
                    radius="sm"
                    bg={isUser ? 'var(--blue-tint)' : 'var(--bg-panel)'}
                    maw="80%"
                    style={{ overflowWrap: 'anywhere' }}
                  >
                    <Text size="sm">{message.content}</Text>
                  </Paper>
                </Group>
              );
            })}
            {unanswered && (
              <Alert color="red" variant="light" title={unanswered.reason} data-testid="unanswered-alert">
                <Text size="sm" style={{ overflowWrap: 'anywhere', whiteSpace: 'pre-wrap' }}>
                  {unanswered.detail ?? unanswered.reason}
                </Text>
              </Alert>
            )}
            <div ref={bottomRef} />
          </Stack>
        )}
      </ScrollArea>
      <Stack gap="var(--space-2)" p="var(--pad)" style={{ borderTop: '1px solid var(--line)' }}>
        {status !== 'open' && (
          <Badge color="var(--fg-muted)" variant="light">
            reconnecting
          </Badge>
        )}
        <Group gap="var(--space-2)" wrap="nowrap" align="flex-end">
          <Textarea
            ref={textareaRef}
            style={{ flex: 1 }}
            autosize
            minRows={1}
            maxRows={6}
            value={draft}
            onChange={(e) => setDraft(e.currentTarget.value)}
            onKeyDown={onKeyDown}
          />
          <ActionIcon
            size="lg"
            onClick={() => (inFlight ? abort() : void submit())}
            disabled={!inFlight && !draft.trim()}
            data-testid={inFlight ? 'stop-button' : 'send-button'}
            c="var(--on-solid)"
          >
            {inFlight ? <Text size="xs">◼</Text> : '→'}
          </ActionIcon>
        </Group>
      </Stack>
    </Stack>
  );
}
