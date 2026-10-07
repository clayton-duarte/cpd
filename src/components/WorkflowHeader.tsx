import { Badge, Group, Text } from '@mantine/core';
import { IconGitBranch, IconGitPullRequest, IconTicket } from '@tabler/icons-react';
import type { Workflow } from '../model/types';

export function WorkflowHeader({ workflow }: { workflow: Workflow }) {
  const title = workflow.ticket ? workflow.ticket.title : workflow.title;

  return (
    <Group gap="var(--space-4)" wrap="nowrap">
      <Group gap="var(--space-2)" wrap="nowrap">
        {workflow.ticket && (
          <Group gap="var(--space-1)" wrap="nowrap">
            <IconTicket size={14} color="var(--fg-faint)" />
            <Text
              component="a"
              href={workflow.ticket.url}
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
              c="var(--fg-faint)"
            >
              {workflow.ticket.key}
            </Text>
          </Group>
        )}

        <Text fw={600} c="var(--fg-bright)">
          {title}
        </Text>

        {workflow.branch && (
          <Group gap="var(--space-1)" wrap="nowrap">
            <IconGitBranch size={14} color="var(--fg-faint)" />
            <Text
              component="a"
              href={workflow.branch.url}
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
              c="var(--fg-faint)"
            >
              {workflow.branch.name}
            </Text>
          </Group>
        )}

        {workflow.pr && (
          <Group gap="var(--space-1)" wrap="nowrap">
            <IconGitPullRequest size={14} color="var(--fg-faint)" />
            <Text
              component="a"
              href={workflow.pr.url}
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
              c="var(--fg-faint)"
            >
              #{workflow.pr.number}
            </Text>
          </Group>
        )}

        {workflow.attempt >= 2 && (
          <Badge color="dark" variant="light" c="var(--fg-faint)">
            #{workflow.attempt}
          </Badge>
        )}
      </Group>
    </Group>
  );
}
