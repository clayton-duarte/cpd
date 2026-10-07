import { Badge, Group, Text } from '@mantine/core';
import { IconGitBranch, IconGitPullRequest, IconTicket } from '@tabler/icons-react';
import type { Workflow } from '../model/types';
import { w } from '../theme';

export function WorkflowHeader({ workflow }: { workflow: Workflow }) {
  const title = workflow.ticket ? workflow.ticket.title : workflow.title;

  return (
    <Group gap="md" wrap="nowrap">
      <Group gap={6} wrap="nowrap">
        {workflow.ticket && (
          <Group gap={4} wrap="nowrap">
            <IconTicket size={14} color={w(0.45)} />
            <Text
              component="a"
              href={workflow.ticket.url}
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
              c={w(0.45)}
            >
              {workflow.ticket.key}
            </Text>
          </Group>
        )}

        <Text fw={600} c="var(--fg-bright)">
          {title}
        </Text>

        {workflow.branch && (
          <Group gap={4} wrap="nowrap">
            <IconGitBranch size={14} color={w(0.45)} />
            <Text
              component="a"
              href={workflow.branch.url}
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
              c={w(0.45)}
            >
              {workflow.branch.name}
            </Text>
          </Group>
        )}

        {workflow.pr && (
          <Group gap={4} wrap="nowrap">
            <IconGitPullRequest size={14} color={w(0.45)} />
            <Text
              component="a"
              href={workflow.pr.url}
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
              c={w(0.45)}
            >
              #{workflow.pr.number}
            </Text>
          </Group>
        )}

        {workflow.attempt >= 2 && (
          <Badge color="dark" variant="light" c={w(0.45)}>
            #{workflow.attempt}
          </Badge>
        )}
      </Group>
    </Group>
  );
}
