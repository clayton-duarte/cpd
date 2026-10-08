import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import { Button, Group, Stack, Text, Tree, UnstyledButton, useTree, type RenderTreeNodePayload, type TreeNodeData } from '@mantine/core';
import { IconChevronDown, IconChevronRight, IconLayoutSidebarLeftCollapse } from '@tabler/icons-react';
import { selectLead, selectPlan, selectConversation, type NavState } from '../model/navigation';
import { leadsLevel, plansLevel, jobsLevel } from '../model/levels';
import type { CpdData } from '../model/types';
import { createConversation, getConversations } from '../engine/client';
import { notifications } from '@mantine/notifications';
import { onConversationsSignal } from '../engine/useEngineStream';
import type { ConversationId, ConversationNode } from '../engine/types';
import { buildConversationTreeData, allExpandedState } from './conversationTree';

export interface SidebarProps {
  /** The fixture Lead/Plan/Job tree, rendered only when `showFixtureTree` is true (default).
   * Optional because I2's running app never renders that tree -- only Sidebar.test.tsx and
   * `/gallery`-adjacent callers that opt into it need to pass fixture data. */
  data?: CpdData;
  /** Renders the fixture-driven Lead/Plan/Job tree above the real conversation tree. Defaults to
   * true so existing callers (Sidebar.test.tsx) keep their current behavior unchanged. I2's App
   * passes `false`: the running app shows only real conversations (D-I2). */
  showFixtureTree?: boolean;
  nav: NavState;
  onNavigate: (next: NavState) => void;
  /** Currently selected conversation thread, or undefined for none. Additive to `nav` -- the
   * lead/plan/job fixture tree and the real conversation tree are deliberately NOT reconciled
   * in this card (see lead clarification on t_6848ce0e). */
  selectedConversationId?: ConversationId;
  onSelectConversation?: (id: ConversationId) => void;
  /** H14: called whenever the fetched/streamed conversation list changes, so callers that need a
   * conversation's title (e.g. the breadcrumb) don't need a second fetch of the same list. */
  onConversationsChange?: (conversations: ConversationNode[]) => void;
  /** I2: called once a "New session" click creates a conversation, with its new id, so the
   * caller can select it and land the user there immediately. */
  onConversationCreated?: (id: ConversationId) => void;
}

export const MIN_WIDTH = 180;
export const MAX_WIDTH = 480;
const DEFAULT_WIDTH = 260;
const STORAGE_KEY = 'cpd.sidebar.width';

function clamp(value: number): number {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, value));
}

function readStoredWidth(): number {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_WIDTH;
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) return DEFAULT_WIDTH;
    return clamp(parsed);
  } catch {
    return DEFAULT_WIDTH;
  }
}

function writeStoredWidth(width: number): void {
  try {
    sessionStorage.setItem(STORAGE_KEY, String(width));
  } catch {
    // storage unavailable -- ignore, rendering must not break
  }
}

/** Path-style node values (`lead/<id>`, `lead/<id>/plan/<id>`, `lead/<id>/plan/<id>/job/<id>`)
 * are globally unique across the whole tree, unlike raw fixture ids which repeat across leads. */
type ParsedValue =
  | { kind: 'lead'; leadId: string }
  | { kind: 'plan'; leadId: string; planId: string }
  | { kind: 'job'; leadId: string; planId: string; jobId: string };

function parseValue(value: string): ParsedValue {
  const parts = value.split('/');
  if (parts.length === 2) return { kind: 'lead', leadId: parts[1] };
  if (parts.length === 4) return { kind: 'plan', leadId: parts[1], planId: parts[3] };
  return { kind: 'job', leadId: parts[1], planId: parts[3], jobId: parts[5] };
}

/** Build the Tree `data` array from the fixture data -- callers must wrap this
 * in `useMemo` keyed on `data` so the array stays referentially stable. */
function buildTreeData(data: CpdData): TreeNodeData[] {
  return leadsLevel(data).map((lead) => {
    const leadValue = `lead/${lead.id}`;
    return {
      value: leadValue,
      label: lead.name,
      children: plansLevel(data, lead.id).map((plan) => {
        const planValue = `${leadValue}/plan/${plan.id}`;
        const planTitle = plan.ticket ? plan.ticket.title : plan.title;
        return {
          value: planValue,
          label: planTitle,
          children: jobsLevel(data, plan.id).map((job) => ({
            value: `${planValue}/job/${job.id}`,
            label: job.title,
          })),
        };
      }),
    };
  });
}

/**
 * Collapsible left sidebar tree (Lead -> Plan -> Job), the primary navigation
 * affordance per D82. Drives NavState through the existing navigation.ts
 * helpers only -- no second source of truth for "what is selected".
 *
 * Expansion comes from Mantine's `useTree` and lives in that hook's own state
 * (uncontrolled), independent of selection -- it stays mounted across nav
 * prop changes because the Sidebar component itself never unmounts.
 *
 * Highlight is never read from the tree hook's selection: it is derived fresh
 * from `nav` on every render inside `renderNode`, so navigating by any means
 * (canvas click, Escape, keyboard) keeps the sidebar in sync.
 */
export function Sidebar({
  data,
  showFixtureTree = true,
  nav,
  onNavigate,
  selectedConversationId,
  onSelectConversation,
  onConversationsChange,
  onConversationCreated,
}: SidebarProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [width, setWidth] = useState<number>(() => readStoredWidth());
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const tree = useTree();
  const [conversations, setConversations] = useState<ConversationNode[]>([]);
  const [creating, setCreating] = useState(false);
  const conversationTree = useTree();
  const onConversationsChangeRef = useRef(onConversationsChange);
  onConversationsChangeRef.current = onConversationsChange;
  // Threads panel is the scrollable container -- used to reveal a newly selected row (K3).
  const threadsPanelRef = useRef<HTMLDivElement>(null);
  const threadRowRefs = useRef(new Map<ConversationId, HTMLElement>());

  useEffect(() => {
    let cancelled = false;
    async function refetch() {
      try {
        const { conversations: fetched } = await getConversations();
        if (!cancelled) {
          setConversations(fetched);
          onConversationsChangeRef.current?.(fetched);
        }
      } catch {
        // daemon unreachable -- leave the previous (possibly empty) list, the chat panel already
        // surfaces a connection-status badge.
      }
    }
    void refetch();
    const unsubscribe = onConversationsSignal(() => void refetch());
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, []);

  const conversationTreeData = useMemo(() => buildConversationTreeData(conversations), [conversations]);

  // K3: once the tree re-renders with the newly selected row present, scroll it into view if the
  // panel is scrollable and the row isn't already fully visible. Runs after conversationTreeData
  // updates (not in the same tick as the create response) so the new row actually exists in the DOM.
  useEffect(() => {
    if (selectedConversationId === undefined) return;
    const panel = threadsPanelRef.current;
    const row = threadRowRefs.current.get(selectedConversationId);
    if (!panel || !row) return;
    if (panel.scrollHeight <= panel.clientHeight) return;
    const panelRect = panel.getBoundingClientRect();
    const rowRect = row.getBoundingClientRect();
    const fullyVisible = rowRect.top >= panelRect.top && rowRect.bottom <= panelRect.bottom;
    if (fullyVisible) return;
    row.scrollIntoView?.({ block: 'nearest' });
  }, [selectedConversationId, conversationTreeData]);

  // Threads exist to be seen: keep every node expanded always, including newly arrived ones from
  // SSE refetches and the node a just-created fork lands under. Per the card, expand-all is the
  // simplest correct behavior (collapsing/persisting state is explicitly out of scope).
  useEffect(() => {
    conversationTree.setExpandedState(allExpandedState(conversationTreeData));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationTreeData]);

  const treeData = useMemo(() => (data ? buildTreeData(data) : []), [data]);

  const handleNewSession = async () => {
    setCreating(true);
    try {
      const { id } = await createConversation();
      onConversationCreated?.(id as ConversationId);
    } catch {
      notifications.show({
        color: 'red',
        title: 'Could not create session',
        message: 'Failed to create a new session. Please try again.',
      });
    } finally {
      setCreating(false);
    }
  };

  const handlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    dragRef.current = { startX: e.clientX, startWidth: width };
  };

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    setWidth(clamp(drag.startWidth + (e.clientX - drag.startX)));
  };

  const endDrag = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragRef.current) return;
    dragRef.current = null;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    setWidth((current) => {
      writeStoredWidth(current);
      return current;
    });
  };

  const handleRowClick = (parsed: ParsedValue) => {
    if (parsed.kind === 'lead') {
      onNavigate(selectLead(parsed.leadId));
    } else if (parsed.kind === 'plan') {
      onNavigate(selectPlan({ level: 'plans', leadId: parsed.leadId }, parsed.planId));
    } else {
      onNavigate(selectPlan({ level: 'plans', leadId: parsed.leadId }, parsed.planId));
    }
  };

  const renderNode = ({ node, level, expanded, hasChildren, elementProps }: RenderTreeNodePayload) => {
    const parsed = parseValue(node.value);
    const selected =
      parsed.kind === 'lead'
        ? nav.leadId === parsed.leadId
        : parsed.kind === 'plan'
          ? nav.level === 'jobs' && nav.planId === parsed.planId
          : false;
    const testId = `row-${parsed.kind === 'lead' ? parsed.leadId : parsed.kind === 'plan' ? parsed.planId : parsed.jobId}`;

    return (
      <Group
        {...elementProps}
        gap="var(--gap)"
        wrap="nowrap"
        pl={`calc(var(--pad) + ${level - 1} * var(--space-5))`}
        pr="var(--pad)"
        py="var(--space-1)"
        data-testid={testId}
        data-selected={selected}
        style={{
          backgroundColor: selected ? 'var(--blue-tint)' : undefined,
          cursor: 'pointer',
        }}
        onClick={() => handleRowClick(parsed)}
      >
        {hasChildren ? (
          <UnstyledButton
            data-testid={`expand-${parsed.kind === 'lead' ? parsed.leadId : parsed.kind === 'plan' ? parsed.planId : ''}`}
            onClick={(e) => {
              e.stopPropagation();
              tree.toggleExpanded(node.value);
            }}
            style={{ display: 'flex', color: 'var(--fg-faint)' }}
          >
            {expanded ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
          </UnstyledButton>
        ) : (
          <span style={{ width: 14, display: 'inline-block' }} />
        )}
        <Text size="sm" c={selected ? 'var(--fg-bright)' : 'var(--fg)'} truncate style={{ minWidth: 0 }}>
          {node.label}
        </Text>
      </Group>
    );
  };

  if (collapsed) {
    return (
      <UnstyledButton
        data-testid="sidebar-toggle"
        onClick={() => {
          setWidth(readStoredWidth());
          setCollapsed(false);
        }}
        p="var(--pad)"
        style={{ color: 'var(--fg-faint)', borderRight: '1px solid var(--line)' }}
      >
        <IconChevronRight size={16} />
      </UnstyledButton>
    );
  }

  return (
    <Stack
      ref={threadsPanelRef}
      gap={0}
      w={width}
      style={{
        position: 'relative',
        backgroundColor: 'var(--bg-panel)',
        borderRight: '1px solid var(--line)',
        overflow: 'auto',
        flexShrink: 0,
      }}
    >
      <div
        data-testid="sidebar-resize-handle"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        style={{
          position: 'absolute',
          top: 0,
          right: 0,
          width: 6,
          height: '100%',
          cursor: 'col-resize',
          backgroundColor: 'var(--line)',
        }}
        onMouseEnter={(e) => {
          e.currentTarget.style.backgroundColor = 'var(--line-strong)';
        }}
        onMouseLeave={(e) => {
          if (!dragRef.current) e.currentTarget.style.backgroundColor = 'var(--line)';
        }}
      />
      <Group justify="flex-end" p="var(--pad)" pb={0}>
        <UnstyledButton
          data-testid="sidebar-toggle"
          onClick={() => setCollapsed(true)}
          style={{ color: 'var(--fg-faint)' }}
        >
          <IconLayoutSidebarLeftCollapse size={16} />
        </UnstyledButton>
      </Group>
      <Stack gap="var(--gap)" p="var(--pad)" pt="var(--space-2)">
        {showFixtureTree && (
          <Tree
            data={treeData}
            tree={tree}
            expandOnClick={false}
            selectOnClick={false}
            renderNode={renderNode}
          />
        )}
        <Group justify="space-between" align="center">
          {conversationTreeData.length > 0 && (
            <Text size="xs" c="var(--fg-faint)" tt="uppercase" fw={600}>
              Threads
            </Text>
          )}
          <Button
            size="xs"
            variant="subtle"
            data-testid="new-session-button"
            onClick={() => void handleNewSession()}
            loading={creating}
            ml="auto"
          >
            New session
          </Button>
        </Group>
        {conversationTreeData.length > 0 ? (
          <Tree
            data={conversationTreeData}
            tree={conversationTree}
            expandOnClick={false}
            selectOnClick={false}
            renderNode={({ node, level, expanded, hasChildren, elementProps }) => {
              const id = Number(node.value) as ConversationId;
              const selected = selectedConversationId === id;
              return (
                <Group
                  {...elementProps}
                  ref={(el: HTMLDivElement | null) => {
                    if (el) threadRowRefs.current.set(id, el);
                    else threadRowRefs.current.delete(id);
                  }}
                  gap="var(--gap)"
                  wrap="nowrap"
                  pl={`calc(var(--pad) + ${level - 1} * var(--space-5))`}
                  pr="var(--pad)"
                  py="var(--space-1)"
                  data-testid={`thread-${node.value}`}
                  data-selected={selected}
                  style={{
                    backgroundColor: selected ? 'var(--blue-tint)' : undefined,
                    cursor: 'pointer',
                  }}
                  onClick={() => {
                    onSelectConversation?.(id);
                    onNavigate(selectConversation(nav, id));
                  }}
                >
                  {hasChildren ? (
                    <UnstyledButton
                      data-testid={`thread-expand-${node.value}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        conversationTree.toggleExpanded(node.value);
                      }}
                      style={{ display: 'flex', color: 'var(--fg-faint)' }}
                    >
                      {expanded ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
                    </UnstyledButton>
                  ) : (
                    <span style={{ width: 14, display: 'inline-block' }} />
                  )}
                  <Text size="sm" c={selected ? 'var(--fg-bright)' : 'var(--fg)'} truncate style={{ minWidth: 0 }}>
                    {node.label}
                  </Text>
                </Group>
              );
            }}
          />
        ) : (
          <Text size="sm" c="var(--fg-faint)" data-testid="no-sessions">
            No sessions yet.
          </Text>
        )}
      </Stack>
    </Stack>
  );
}
