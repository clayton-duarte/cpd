import { useEffect, useRef, useState } from 'react';
import {
  ActionIcon,
  Badge,
  EmptyState,
  Group,
  Loader,
  Paper,
  ScrollArea,
  Stack,
  Text,
  Textarea,
} from '@mantine/core';
import { sendPrompt } from '../engine/client';
import { useEngineStream } from '../engine/useEngineStream';
import type { Message } from '../engine/types';

/**
 * Lead chat panel. Messages come from the shared SSE store (useEngineStream);
 * sending a prompt optimistically appends the user's message locally, then
 * the next stream event replaces the list with the authoritative transcript.
 */
export function ChatPanel() {
  const { messages, status } = useEngineStream();
  const [pending, setPending] = useState<Message | null>(null);
  const [draft, setDraft] = useState('');
  const [inFlight, setInFlight] = useState(false);
  const [reason, setReason] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

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

    const userMessage: Message = { role: 'user', content: text };
    setPending(userMessage);
    setDraft('');
    setInFlight(true);

    try {
      const result = await sendPrompt(text);
      if (result.status !== 'done') {
        setReason(result.reason ?? 'unanswered');
      } else {
        setReason(null);
      }
    } finally {
      setInFlight(false);
    }
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  }

  return (
    <Stack h="100%" gap={0}>
      <ScrollArea style={{ flex: 1, minHeight: 0 }} p="var(--pad)">
        {visible.length === 0 ? (
          <EmptyState
            title="No messages yet"
            description="The lead conversation is empty."
            styles={{ title: { fontSize: 'var(--mantine-font-size-sm)' } }}
          />
        ) : (
          <Stack gap="var(--space-2)" w="100%">
            {visible.map((message, i) => {
              const isUser = message.role === 'user';
              return (
                <Group key={i} justify={isUser ? 'flex-end' : 'flex-start'} w="100%">
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
        {reason && (
          <Badge color="var(--red)" variant="light">
            {reason}
          </Badge>
        )}
        <Group gap="var(--space-2)" wrap="nowrap" align="flex-end">
          <Textarea
            style={{ flex: 1 }}
            autosize
            minRows={1}
            maxRows={6}
            disabled={inFlight}
            value={draft}
            onChange={(e) => setDraft(e.currentTarget.value)}
            onKeyDown={onKeyDown}
          />
          <ActionIcon size="lg" onClick={() => void submit()} disabled={inFlight} c="var(--on-solid)">
            {inFlight ? <Loader size="xs" /> : '→'}
          </ActionIcon>
        </Group>
      </Stack>
    </Stack>
  );
}
