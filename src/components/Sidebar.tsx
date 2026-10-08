import { useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import { Group, Stack, Text, Tree, UnstyledButton, useTree, type RenderTreeNodePayload, type TreeNodeData } from '@mantine/core';
import { IconChevronDown, IconChevronRight, IconLayoutSidebarLeftCollapse } from '@tabler/icons-react';
import { selectLead, selectPlan, type NavState } from '../model/navigation';
import { leadsLevel, plansLevel, jobsLevel } from '../model/levels';
import type { CpdData } from '../model/types';

export interface SidebarProps {
  data: CpdData;
  nav: NavState;
  onNavigate: (next: NavState) => void;
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
export function Sidebar({ data, nav, onNavigate }: SidebarProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [width, setWidth] = useState<number>(() => readStoredWidth());
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const tree = useTree();

  const treeData = useMemo(() => buildTreeData(data), [data]);

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
        <Tree
          data={treeData}
          tree={tree}
          expandOnClick={false}
          selectOnClick={false}
          renderNode={renderNode}
        />
      </Stack>
    </Stack>
  );
}
