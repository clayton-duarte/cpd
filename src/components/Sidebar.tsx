import { useState } from 'react';
import { Group, Stack, Text, UnstyledButton } from '@mantine/core';
import { IconChevronDown, IconChevronRight, IconLayoutSidebarLeftCollapse } from '@tabler/icons-react';
import { selectLead, selectPlan, type NavState } from '../model/navigation';
import { leadsLevel, plansLevel, jobsLevel } from '../model/levels';
import type { CpdData } from '../model/types';

export interface SidebarProps {
  data: CpdData;
  nav: NavState;
  onNavigate: (next: NavState) => void;
}

interface RowProps {
  label: string;
  depth: number;
  selected: boolean;
  testId: string;
  expandTestId?: string;
  expanded?: boolean;
  onToggleExpand?: () => void;
  onClick: () => void;
}

/** One tree row: optional expand caret, label, selection highlight. */
function Row({ label, depth, selected, testId, expandTestId, expanded, onToggleExpand, onClick }: RowProps) {
  return (
    <Group
      gap="var(--gap)"
      wrap="nowrap"
      pl={`calc(var(--pad) + ${depth} * var(--space-5))`}
      pr="var(--pad)"
      py="var(--space-1)"
      data-testid={testId}
      data-selected={selected}
      style={{
        backgroundColor: selected ? 'var(--blue-tint)' : undefined,
        cursor: 'pointer',
      }}
      onClick={onClick}
    >
      {expandTestId ? (
        <UnstyledButton
          data-testid={expandTestId}
          onClick={(e) => {
            e.stopPropagation();
            onToggleExpand?.();
          }}
          style={{ display: 'flex', color: 'var(--fg-faint)' }}
        >
          {expanded ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
        </UnstyledButton>
      ) : (
        <span style={{ width: 14, display: 'inline-block' }} />
      )}
      <Text size="sm" c={selected ? 'var(--fg-bright)' : 'var(--fg)'} truncate style={{ minWidth: 0 }}>
        {label}
      </Text>
    </Group>
  );
}

/**
 * Collapsible left sidebar tree (Lead -> Plan -> Job), the primary navigation
 * affordance per D82. Drives NavState through the existing navigation.ts
 * helpers only -- no second source of truth for "what is selected".
 *
 * Expansion is local component state, independent of selection: it is keyed
 * by lead/plan id so it survives descending into and returning from a level.
 */
export function Sidebar({ data, nav, onNavigate }: SidebarProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [expandedLeads, setExpandedLeads] = useState<Set<string>>(new Set());
  const [expandedPlans, setExpandedPlans] = useState<Set<string>>(new Set());

  const toggleLead = (leadId: string) =>
    setExpandedLeads((prev) => {
      const next = new Set(prev);
      if (next.has(leadId)) next.delete(leadId);
      else next.add(leadId);
      return next;
    });

  const togglePlan = (planId: string) =>
    setExpandedPlans((prev) => {
      const next = new Set(prev);
      if (next.has(planId)) next.delete(planId);
      else next.add(planId);
      return next;
    });

  if (collapsed) {
    return (
      <UnstyledButton
        data-testid="sidebar-toggle"
        onClick={() => setCollapsed(false)}
        p="var(--pad)"
        style={{ color: 'var(--fg-faint)', borderRight: '1px solid var(--line)' }}
      >
        <IconChevronRight size={16} />
      </UnstyledButton>
    );
  }

  const leads = leadsLevel(data);

  return (
    <Stack
      gap={0}
      w={260}
      style={{ backgroundColor: 'var(--bg-panel)', borderRight: '1px solid var(--line)', overflow: 'auto' }}
    >
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
        {leads.map((lead) => {
          const leadExpanded = expandedLeads.has(lead.id);
          const leadSelected = nav.leadId === lead.id;
          return (
            <Stack key={lead.id} gap="var(--space-1)">
              <Row
                label={lead.name}
                depth={0}
                selected={leadSelected}
                testId={`row-${lead.id}`}
                expandTestId={`expand-${lead.id}`}
                expanded={leadExpanded}
                onToggleExpand={() => toggleLead(lead.id)}
                onClick={() => onNavigate(selectLead(lead.id))}
              />
              {leadExpanded &&
                plansLevel(data, lead.id).map((plan) => {
                  const planExpanded = expandedPlans.has(plan.id);
                  const planSelected = nav.level === 'jobs' && nav.planId === plan.id;
                  const planTitle = plan.ticket ? plan.ticket.title : plan.title;
                  return (
                    <Stack key={plan.id} gap="var(--space-1)">
                      <Row
                        label={planTitle}
                        depth={1}
                        selected={planSelected}
                        testId={`row-${plan.id}`}
                        expandTestId={`expand-${plan.id}`}
                        expanded={planExpanded}
                        onToggleExpand={() => togglePlan(plan.id)}
                        onClick={() => onNavigate(selectPlan({ level: 'plans', leadId: lead.id }, plan.id))}
                      />
                      {planExpanded &&
                        jobsLevel(data, plan.id).map((job) => (
                          <Row
                            key={job.id}
                            label={job.title}
                            depth={2}
                            selected={false}
                            testId={`row-${job.id}`}
                            onClick={() => onNavigate(selectPlan({ level: 'plans', leadId: lead.id }, plan.id))}
                          />
                        ))}
                    </Stack>
                  );
                })}
            </Stack>
          );
        })}
      </Stack>
    </Stack>
  );
}
