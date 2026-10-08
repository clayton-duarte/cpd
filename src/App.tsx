import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import { ReactFlowProvider } from '@xyflow/react';
import {
  ActionBar,
  Affix,
  Alert,
  Button,
  EmptyState,
  Group,
  Indicator,
  Paper,
  ScrollArea,
  Stack,
  Text,
  Tooltip,
} from '@mantine/core';
import { IconMessagePlus, IconPlayerPlay, IconSquarePlus } from '@tabler/icons-react';
import { Canvas } from './canvas/Canvas';
import { Sidebar } from './components/Sidebar';
import { ChatPanel } from './components/ChatPanel';
import { Gallery } from './Gallery';
import { useLevelTransition } from './canvas/useLevelTransition';
import { usePlan } from './engine/usePlan';
import { useAttention } from './engine/useAttention';
import { runJob } from './engine/client';
import { abortJob } from './engine/jobActions';
import { planToCpdData } from './model/fromPlan';
import { NewJobModal } from './components/NewJobModal';
import { parseConversationId } from './model/navigation';
import type { ConversationId, ConversationNode } from './engine/types';
import type { AttentionItem } from './engine/types';
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

/**
 * J2: left column, top -- wraps the existing real conversation tree (Sidebar in
 * `showFixtureTree={false}` mode) inside a floating panel instead of a reserved navbar column.
 */
function TreePanel({
  selectedConversationId,
  onSelectConversation,
  onConversationsChange,
  onConversationCreated,
}: {
  selectedConversationId?: ConversationId;
  onSelectConversation: (id: ConversationId) => void;
  onConversationsChange: (conversations: ConversationNode[]) => void;
  onConversationCreated: (id: ConversationId) => void;
}) {
  return (
    <Paper withBorder radius="md" shadow="sm" data-testid="panel-tree" style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
      <Sidebar
        showFixtureTree={false}
        nav={{ level: 'leads' }}
        onNavigate={() => {}}
        selectedConversationId={selectedConversationId}
        onSelectConversation={onSelectConversation}
        onConversationsChange={onConversationsChange}
        onConversationCreated={onConversationCreated}
      />
    </Paper>
  );
}

/**
 * J3b: one real attention item per job needing the user (blocked jobs, failures). Clicking an
 * item selects its job on the canvas -- if the item belongs to a different conversation than the
 * one currently selected, the conversation is switched first (via `onSelectItem`, which drives
 * `selectedConversationId` and therefore the URL hash -- D133/I3's existing mechanism, not a
 * parallel state container). The action bar is the only thing that acts on a job; this panel is
 * purely a selector (per lead decision, J3b card).
 */
function AttentionQueuePanel({
  items,
  selectedConversationId,
  onSelectItem,
}: {
  items: AttentionItem[];
  selectedConversationId?: ConversationId;
  onSelectItem: (item: AttentionItem) => void;
}) {
  return (
    <Paper withBorder radius="md" shadow="sm" p="var(--pad)" data-testid="panel-attention-queue">
      <Group justify="space-between" mb="var(--space-2)">
        <Text size="sm" fw={600} c="var(--fg-bright)">
          Attention
        </Text>
        {items.length > 0 ? (
          <Indicator
            label={items.length}
            size={18}
            color="var(--red-solid)"
            position="middle-end"
            offset={-4}
            inline
          >
            <span />
          </Indicator>
        ) : null}
      </Group>
      <ScrollArea.Autosize mah="100%">
        {items.length === 0 ? (
          <EmptyState title="Nothing needs you" description="You're all caught up." />
        ) : (
          <Stack gap="var(--space-2)">
            {items.map((item) => (
              <Alert
                key={item.jobId}
                color="var(--red-solid)"
                variant="light"
                title={`${item.status}: ${item.jobTitle}`}
                data-testid={`attention-item-${item.jobId}`}
                style={{ cursor: 'pointer' }}
                onClick={() => onSelectItem(item)}
              >
                {item.reason}
                {item.conversationId !== selectedConversationId ? (
                  <Text size="xs" c="var(--fg-faint)" mt="var(--space-1)">
                    in {item.conversationTitle}
                  </Text>
                ) : null}
              </Alert>
            ))}
          </Stack>
        )}
      </ScrollArea.Autosize>
    </Paper>
  );
}

/**
 * J3b: the action bar acts only on the canvas selection (`selectedJobId`) -- never a separate
 * target. Stop is wired to the daemon's real abort route; Comment and Skip have no backend route
 * today (verified: the daemon exposes only `plan/job`, `plan/job/run`, `plan/job/abort`) so they
 * render visibly disabled with a tooltip rather than silently doing nothing.
 */
function ActionBarPanel({
  opened,
  canStop,
  onStop,
}: {
  opened: boolean;
  canStop: boolean;
  onStop: () => void;
}) {
  return (
    <ActionBar opened={opened} data-testid="panel-action-bar" withinPortal={false} shadow="sm" radius="md">
      <Tooltip label="Not implemented yet">
        <Button variant="subtle" size="xs" disabled data-testid="action-comment">
          Comment
        </Button>
      </Tooltip>
      <ActionBar.Divider />
      <Button
        variant="subtle"
        size="xs"
        c="var(--red)"
        disabled={!canStop}
        data-testid="action-stop"
        onClick={onStop}
      >
        Stop
      </Button>
      <ActionBar.Divider />
      <Tooltip label="Not implemented yet">
        <Button variant="subtle" size="xs" disabled data-testid="action-skip">
          Skip
        </Button>
      </Tooltip>
    </ActionBar>
  );
}

/**
 * J2: right column -- transcript grows up from the bottom (flex column, reverse-anchored content)
 * and scrolls once full; composer sits below it. Wraps the existing ChatPanel, which already
 * owns its own ScrollArea and keeps newest-at-the-bottom behavior (I3/H2).
 *
 * L7: user-resizable via a drag handle on its left edge, mirroring Sidebar's own resize pattern
 * (pointer capture + clamp + persisted size) rather than a new Mantine primitive -- Mantine 9.7
 * has no resizable-panel component. Width is persisted to `localStorage` (not `sessionStorage`
 * like the sidebar) per the card, and clamped so the panel can never be dragged to zero or over
 * the whole viewport.
 */
export const CHAT_PANEL_MIN_WIDTH = 280;
export const CHAT_PANEL_MAX_WIDTH = 720;
const CHAT_PANEL_DEFAULT_WIDTH = 400;
const CHAT_PANEL_STORAGE_KEY = 'cpd.chatPanel.width';

function clampChatPanelWidth(value: number): number {
  return Math.min(CHAT_PANEL_MAX_WIDTH, Math.max(CHAT_PANEL_MIN_WIDTH, value));
}

function readStoredChatPanelWidth(): number {
  try {
    const raw = localStorage.getItem(CHAT_PANEL_STORAGE_KEY);
    if (!raw) return CHAT_PANEL_DEFAULT_WIDTH;
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) return CHAT_PANEL_DEFAULT_WIDTH;
    return clampChatPanelWidth(parsed);
  } catch {
    return CHAT_PANEL_DEFAULT_WIDTH;
  }
}

function writeStoredChatPanelWidth(width: number): void {
  try {
    localStorage.setItem(CHAT_PANEL_STORAGE_KEY, String(width));
  } catch {
    // storage unavailable -- ignore, rendering must not break
  }
}

function TranscriptAndComposerPanel({
  conversationId,
  onForked,
}: {
  conversationId: ConversationId | undefined;
  onForked: (id: ConversationId) => void;
}) {
  const [width, setWidth] = useState(() => readStoredChatPanelWidth());
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);

  const handlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    dragRef.current = { startX: e.clientX, startWidth: width };
  };

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    // Dragging left (toward the canvas) grows the panel since it's anchored to the right edge
    // of the viewport -- the handle sits on its LEFT side, so the sign is inverted vs Sidebar's
    // right-edge handle.
    setWidth(clampChatPanelWidth(drag.startWidth - (e.clientX - drag.startX)));
  };

  const endDrag = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragRef.current) return;
    dragRef.current = null;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    setWidth((current) => {
      writeStoredChatPanelWidth(current);
      return current;
    });
  };

  return (
    <Paper
      withBorder
      radius="md"
      shadow="sm"
      data-testid="panel-transcript"
      style={{
        flex: 'none',
        width,
        minHeight: 0,
        display: 'flex',
        flexDirection: 'column',
        position: 'relative',
      }}
    >
      <div
        data-testid="chat-panel-resize-handle"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          width: 6,
          height: '100%',
          cursor: 'col-resize',
          // Sits on top of the Paper's own border; a transparent hit target keeps the resize
          // affordance invisible until hovered, matching Sidebar's handle treatment.
          backgroundColor: 'transparent',
        }}
      />
      <ChatPanel conversationId={conversationId} onForked={onForked} />
    </Paper>
  );
}

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
    <Stack align="center" justify="center" style={{ height: '100%' }}>
      <EmptyState
        title="No session selected."
        description='Use "New session" in the sidebar to start one.'
      />
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

  // J3b: the attention queue is a selector, never its own action path (lead decision) -- clicking
  // an item just drives the same `selectedConversationId`/`selectedJobId` state the sidebar and
  // canvas already use, so the action bar always acts on the canvas selection.
  const { items: attentionItems } = useAttention();
  // Rides the same refcounted stream connection `usePlan` already shares (H1's useEngineStream) --
  // calling it again here is not a second connection, just a second subscriber to the job list so
  // the action bar can know the selected job's status without threading it up from ConversationView.
  const { jobs: selectedConversationJobs } = usePlan(selectedConversationId);
  const selectedJob = selectedConversationJobs.find((job) => job.id === selectedJobId);

  // When an attention click changes the conversation, the job-selection-clearing effect below
  // would immediately wipe the job we just asked for (it runs on every `selectedConversationId`
  // change). This ref carries the job id across that one render so the click's intent survives.
  const pendingJobIdRef = useRef<string | null>(null);

  const handleSelectAttentionItem = (item: AttentionItem) => {
    if (item.conversationId !== selectedConversationId) {
      pendingJobIdRef.current = item.jobId;
      setSelectedConversationId(item.conversationId as ConversationId);
    } else {
      setSelectedJobId(item.jobId);
    }
  };

  const handleStop = () => {
    if (selectedConversationId === undefined || selectedJobId === null) return;
    void abortJob(selectedConversationId, selectedJobId);
  };

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
  // whenever the conversation changes so it never points at a stale job from a different plan --
  // UNLESS an attention-item click just requested a specific job in the conversation it switched
  // us to (pendingJobIdRef), in which case honour that instead of clobbering it.
  useEffect(() => {
    if (pendingJobIdRef.current !== null) {
      setSelectedJobId(pendingJobIdRef.current);
      pendingJobIdRef.current = null;
      return;
    }
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
    <div style={{ position: 'relative', height: '100vh', width: '100vw', overflow: 'hidden' }}>
      {/* Full-bleed canvas behind everything (D132/J2): not clipped to the centre column, fills
       * the whole viewport. The floating panels below sit on top of it, pointer-events untouched
       * outside their own bounds so panning/zooming still works in the gaps. */}
      <div data-testid="canvas-area" style={{ position: 'absolute', inset: 0 }}>
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
      </div>

      <TopBar conversationTitle={conversationTitle} />

      {/* Left column (20%): Tree panel on top, attention queue placeholder below. */}
      <Stack
        gap="var(--space-2)"
        p="var(--space-2)"
        style={{
          position: 'absolute',
          top: 48,
          left: 0,
          bottom: 0,
          width: '20%',
          pointerEvents: 'none',
        }}
      >
        <div style={{ flex: 1, minHeight: 0, display: 'flex', pointerEvents: 'auto' }}>
          <TreePanel
            selectedConversationId={selectedConversationId}
            onSelectConversation={setSelectedConversationId}
            onConversationsChange={setConversations}
            onConversationCreated={setSelectedConversationId}
          />
        </div>
        <div style={{ flexShrink: 0, pointerEvents: 'auto' }}>
          <AttentionQueuePanel
            items={attentionItems}
            selectedConversationId={selectedConversationId}
            onSelectItem={handleSelectAttentionItem}
          />
        </div>
      </Stack>

      {/* Centre column (40%): canvas shows through; action bar placeholder floats at its bottom. */}
      <div
        style={{
          position: 'absolute',
          top: 48,
          left: '20%',
          width: '40%',
          bottom: 'var(--space-4)',
          display: 'flex',
          alignItems: 'flex-end',
          justifyContent: 'center',
          pointerEvents: 'none',
        }}
      >
        <div style={{ pointerEvents: 'auto' }}>
          <ActionBarPanel
            opened={selectedJobId !== null}
            canStop={selectedJob?.status === 'running'}
            onStop={handleStop}
          />
        </div>
      </div>

      {/* Right column (40%): transcript grows up from the bottom and scrolls; composer lives
       * inside ChatPanel, below the transcript, per D132/J2. */}
      <div
        style={{
          position: 'absolute',
          top: 108,
          right: 0,
          bottom: 0,
          width: '40%',
          padding: 'var(--space-2)',
          display: 'flex',
          pointerEvents: 'none',
        }}
      >
        <div style={{ flex: 1, pointerEvents: 'auto', display: 'flex' }}>
          <TranscriptAndComposerPanel conversationId={selectedConversationId} onForked={setSelectedConversationId} />
        </div>
      </div>

      {/* FABs, top-right (D132: New session / New workflow / New job). */}
      <Affix position={{ top: 56, right: 'var(--space-3)' }}>
        <Group gap="var(--space-2)">
          <Button size="sm" leftSection={<IconMessagePlus size={16} />} variant="filled">
            New session
          </Button>
          <Button size="sm" leftSection={<IconSquarePlus size={16} />} variant="filled">
            New workflow
          </Button>
          <Button size="sm" leftSection={<IconPlayerPlay size={16} />} variant="filled">
            New job
          </Button>
        </Group>
      </Affix>
    </div>
  );
}

export default App;
