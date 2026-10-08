import { useEffect, useMemo, useRef, useState } from 'react';
import { ReactFlowProvider } from '@xyflow/react';
import { AppShell, Button, Group, Stack, Text } from '@mantine/core';
import { Canvas } from './canvas/Canvas';
import { Sidebar } from './components/Sidebar';
import { ChatPanel } from './components/ChatPanel';
import { Gallery } from './Gallery';
import { useLevelTransition } from './canvas/useLevelTransition';
import { usePlan } from './engine/usePlan';
import { runJob } from './engine/client';
import { planToCpdData } from './model/fromPlan';
import { NewJobModal } from './components/NewJobModal';
import { parseConversationId } from './model/navigation';
import type { ConversationId, ConversationNode } from './engine/types';
import './canvas/xyflow-theme.css';

/** Trivial hash-based routing: no router dependency, per the A2-4 spec. Also used by I3 to carry
 * the selected conversation (`#/c/<id>`) -- the single `hashchange` listener here is shared by
 * both routes, deliberately not duplicated. */
function useHashRoute(): string {
  const [hash, setHash] = useState(() => window.location.hash);
  useEffect(() => {
    const onHashChange = () => setHash(window.location.hash);
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, []);
  return hash;
}

/** I3: pulls the raw segment out of a `#/c/<segment>` hash, or undefined if the hash doesn't
 * name a conversation at all (e.g. `#/gallery`, or no hash). The segment is handed to
 * `parseConversationId` (H14) by the caller -- this function does no validation of its own. */
function conversationHashSegment(hash: string): string | undefined {
  const match = /^#\/c\/(.*)$/.exec(hash);
  return match ? match[1] : undefined;
}

/** Sentinel distinct from `undefined` (a valid "no conversation" segment) so the hash-sync
 * effect below can tell "never run yet" apart from "last ran on an empty/no-conversation hash". */
const UNSET = Symbol('unset');

function TopBar({ conversationTitle }: { conversationTitle?: string }) {
  return (
    <Group
      px="md"
      py="var(--space-2)"
      justify="space-between"
      style={{ borderBottom: '1px solid var(--line)', flexShrink: 0 }}
    >
      <Stack gap={0} justify="center">
        <Text size="sm" fw={600} c="var(--fg-bright)">
          {conversationTitle ?? 'CPD'}
        </Text>
      </Stack>
      <Text size="xs" c="var(--fg-faint)">
        <a href="#/gallery" style={{ color: 'inherit' }}>
          gallery
        </a>
      </Text>
    </Group>
  );
}

/**
 * The real plan for the selected conversation (I2, successor to H14). Every id reaching this
 * component is now a real conversation id -- there is no fixture-backed branch left. A 404 or an
 * empty plan both render an empty canvas via `planToCpdData([], ...)`, never a crash or an
 * infinite spinner (D115).
 */
function JobsLevelContent({
  conversationId,
  conversationTitle,
  selectedJobId,
  onSelectJob,
}: {
  conversationId: ConversationId;
  conversationTitle: string;
  selectedJobId: string | null;
  onSelectJob: (jobId: string | null) => void;
}) {
  const { jobs } = usePlan(conversationId);
  const data = planToCpdData(jobs, { id: conversationId, title: conversationTitle });
  const [modalOpened, setModalOpened] = useState(false);

  const handleRun = (jobId: string) => {
    void runJob(conversationId, jobId);
  };

  return (
    <Stack gap={0} style={{ height: '100%' }}>
      <Group justify="flex-end" px="md" py="var(--space-1)">
        <Button size="xs" onClick={() => setModalOpened(true)}>
          New job
        </Button>
      </Group>
      <div style={{ flex: 1, minHeight: 0 }}>
        <Canvas data={data} selectedJobId={selectedJobId} onSelectJob={onSelectJob} onRunJob={handleRun} />
      </div>
      <NewJobModal
        conversation={conversationId}
        opened={modalOpened}
        onClose={() => setModalOpened(false)}
        onCreated={() => setModalOpened(false)}
      />
    </Stack>
  );
}

/** Rendered when no conversation is selected yet -- a cold, empty database must still be a
 * usable app (D-I2), so this is a clear instruction rather than a blank canvas. */
function EmptyCanvasState() {
  return (
    <Stack align="center" justify="center" gap="var(--space-2)" style={{ height: '100%' }}>
      <Text c="var(--fg-faint)">No session selected.</Text>
      <Text size="sm" c="var(--fg-faint)">
        Use "New session" in the sidebar to start one.
      </Text>
    </Stack>
  );
}

/**
 * Drives the camera transition whenever the selected conversation changes. Must render inside
 * <ReactFlowProvider> -- useReactFlow throws outside it (C5 wiring requirement).
 */
function LevelTransitionEffect({ conversationId }: { conversationId: ConversationId | undefined }) {
  const transition = useLevelTransition();
  const previous = useRef(conversationId);

  useEffect(() => {
    if (previous.current !== conversationId) {
      transition('descend');
      previous.current = conversationId;
    }
  }, [conversationId, transition]);

  return null;
}

function App() {
  const hash = useHashRoute();
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [selectedConversationId, setSelectedConversationId] = useState<ConversationId | undefined>(
    undefined,
  );
  const [conversations, setConversations] = useState<ConversationNode[]>([]);
  const conversationTitle = useMemo(
    () => conversations.find((c) => c.id === selectedConversationId)?.title,
    [conversations, selectedConversationId],
  );

  // I3: the hash is the source of truth for "which conversation", selection is derived from it
  // once the conversation list is known (so an unknown/deleted id can fall back to the empty
  // state instead of requesting a 404'd plan). `appliedHashRef` guards against the ping-pong of
  // "hash -> state -> hash" by recording the last hash segment this effect already acted on --
  // the hash-writing effect below never fires for a change this effect itself caused.
  const appliedHashRef = useRef<string | undefined | typeof UNSET>(UNSET);
  const hydratedRef = useRef(false);
  useEffect(() => {
    const segment = conversationHashSegment(hash);
    if (segment === appliedHashRef.current) return;
    const id = parseConversationId(segment);
    if (id === undefined) {
      appliedHashRef.current = segment;
      hydratedRef.current = true;
      setSelectedConversationId(undefined);
      return;
    }
    if (conversations.length === 0) return; // list not loaded yet -- wait, don't guess, don't mark applied
    appliedHashRef.current = segment;
    hydratedRef.current = true;
    const exists = conversations.some((c) => c.id === id);
    setSelectedConversationId(exists ? (id as ConversationId) : undefined);
  }, [hash, conversations]);

  // Mirror selection back into the hash whenever it changes for a reason OTHER than the hash
  // itself just having been parsed (guarded by appliedHashRef so this never re-triggers the
  // effect above in a loop). Gated on `hydratedRef` so this never fires before the initial hash
  // has had a chance to resolve against the loaded conversation list -- otherwise a cold load
  // with `#/c/7` in the URL would overwrite it with an empty hash before `conversations` arrives.
  useEffect(() => {
    if (hash === '#/gallery') return;
    if (!hydratedRef.current) return;
    const desiredSegment = selectedConversationId === undefined ? undefined : String(selectedConversationId);
    if (desiredSegment === conversationHashSegment(hash)) return;
    appliedHashRef.current = desiredSegment;
    // Setting `hash` to '' still fires `hashchange` (unlike `history.replaceState`), so this
    // stays a single code path for both "select" and "clear" -- no special-casing needed, and
    // `useHashRoute`'s state stays in sync with `window.location.hash` either way.
    window.location.hash = desiredSegment === undefined ? '' : `/c/${desiredSegment}`;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedConversationId]);

  // Selection is deliberately NOT part of navigation state (see D3 card): the selected
  // conversation answers "which session", selection answers "which job within it". Clear it
  // whenever the conversation changes so it never points at a stale job from a different plan.
  useEffect(() => {
    setSelectedJobId(null);
  }, [selectedConversationId]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && selectedJobId !== null) {
        setSelectedJobId(null);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [selectedJobId]);

  if (hash === '#/gallery') {
    return <Gallery />;
  }

  return (
    <AppShell
      header={{ height: 48 }}
      navbar={{ width: 280, breakpoint: 'sm' }}
      aside={{ width: 360, breakpoint: 'sm' }}
      padding="md"
    >
      <AppShell.Header>
        <TopBar conversationTitle={conversationTitle} />
      </AppShell.Header>
      <AppShell.Navbar>
        <Sidebar
          showFixtureTree={false}
          nav={{ level: 'leads' }}
          onNavigate={() => {}}
          selectedConversationId={selectedConversationId}
          onSelectConversation={setSelectedConversationId}
          onConversationsChange={setConversations}
          onConversationCreated={setSelectedConversationId}
        />
      </AppShell.Navbar>
      <AppShell.Main data-testid="canvas-area" style={{ height: '100vh' }}>
        <ReactFlowProvider>
          <LevelTransitionEffect conversationId={selectedConversationId} />
          {selectedConversationId === undefined ? (
            <EmptyCanvasState />
          ) : (
            <JobsLevelContent
              conversationId={selectedConversationId}
              conversationTitle={conversationTitle ?? `Session ${selectedConversationId}`}
              selectedJobId={selectedJobId}
              onSelectJob={setSelectedJobId}
            />
          )}
        </ReactFlowProvider>
      </AppShell.Main>
      <AppShell.Aside>
        <ChatPanel conversationId={selectedConversationId} onForked={setSelectedConversationId} />
      </AppShell.Aside>
    </AppShell>
  );
}

export default App;
