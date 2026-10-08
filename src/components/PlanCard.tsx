import { Badge, Group, Text } from '@mantine/core';
import { IconGitBranch, IconGitPullRequest, IconTicket } from '@tabler/icons-react';
import { CardShell } from './CardShell';
import type { PlanCardData } from '../model/levels';

export interface PlanCardProps {
  plan: PlanCardData;
  onClick?: () => void;
}

/**
 * Plans-level card: workflow identity per D77 (title, and ticket/branch/PR
 * only when they exist), and a count of its jobs (counts only).
 */
export function PlanCard({ plan, onClick }: PlanCardProps) {
  const title = plan.ticket ? plan.ticket.title : plan.title;
  return (
    <div onClick={onClick} style={{ cursor: onClick ? 'pointer' : undefined }}>
      <CardShell borderColor="var(--line-strong)">
        <Group gap="var(--space-1)" wrap="nowrap" style={{ minWidth: 0 }}>
          {plan.ticket && <IconTicket size={14} color="var(--fg-faint)" />}
          <Text size="sm" fw={600} c="var(--fg-bright)" truncate style={{ minWidth: 0 }}>
            {title}
          </Text>
        </Group>
        <Group gap="var(--space-2)" wrap="nowrap" mt="var(--gap)">
          {plan.branch && (
            <Group gap="var(--space-1)" wrap="nowrap">
              <IconGitBranch size={14} color="var(--fg-faint)" />
              <Text size="xs" c="var(--fg-faint)" truncate>
                {plan.branch.name}
              </Text>
            </Group>
          )}
          {plan.pr && (
            <Group gap="var(--space-1)" wrap="nowrap">
              <IconGitPullRequest size={14} color="var(--fg-faint)" />
              <Text size="xs" c="var(--fg-faint)">
                #{plan.pr.number}
              </Text>
            </Group>
          )}
        </Group>
        <Group justify="flex-end" mt="var(--gap)">
          <Badge color="dark" variant="light" c="var(--fg-faint)">
            {plan.jobCount} {plan.jobCount === 1 ? 'job' : 'jobs'}
          </Badge>
        </Group>
      </CardShell>
    </div>
  );
}
