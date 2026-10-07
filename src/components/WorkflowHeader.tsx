import { Badge, Group, Text } from '@mantine/core';
import { IconGitPullRequest } from '@tabler/icons-react';
import type { Workflow } from '../model/types';
import { w } from '../theme';

export function WorkflowHeader({ workflow }: { workflow: Workflow }) {
  return (
    <Group gap="md" wrap="nowrap">
      <Group gap={6} wrap="nowrap">
        <IconGitPullRequest size={18} color={w(0.6)} />
        <Text fw={600}>{workflow.title}</Text>
        {workflow.pr != null && (
          <Text size="sm" c={w(0.45)}>
            #{workflow.pr}
          </Text>
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
