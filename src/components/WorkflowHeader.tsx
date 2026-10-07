import { Badge, Group, Text } from '@mantine/core';
import { IconGitPullRequest } from '@tabler/icons-react';
import type { Workflow } from '../model/types';

export function WorkflowHeader({ workflow }: { workflow: Workflow }) {
  return (
    <Group gap="md" wrap="nowrap">
      <Group gap={6} wrap="nowrap">
        <IconGitPullRequest size={18} color="var(--fg-muted)" />
        <Text fw={600} c="var(--fg-bright)">
          {workflow.title}
        </Text>
        {workflow.pr != null && (
          <Text size="sm" c="var(--fg-faint)">
            #{workflow.pr}
          </Text>
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
