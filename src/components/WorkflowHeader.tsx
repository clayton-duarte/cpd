import { Badge, Group, Text } from '@mantine/core';
import { IconGitPullRequest } from '@tabler/icons-react';
import type { Workflow } from '../model/types';
import { FLOW_ORDER, flowLabel } from '../model/derive';
const OWNER_COLOR: Record<Workflow['owner'], string> = {
  you: 'var(--red)',
  agents: 'var(--blue)',
  neutral: 'var(--fg-bright)',
};

export function WorkflowHeader({ workflow }: { workflow: Workflow }) {
  const current = flowLabel(workflow.phase);
  const currentIndex = FLOW_ORDER.indexOf(current);
  const ownerColor = OWNER_COLOR[workflow.owner];

  return (
    <Group gap="md" wrap="nowrap">
      <Group gap={6} wrap="nowrap">
        <IconGitPullRequest size={18} color="var(--fg-muted)" />
        <Text fw={600}>{workflow.title}</Text>
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

      <Group gap={6} wrap="nowrap">
        {FLOW_ORDER.map((label, i) => {
          const isCurrent = i === currentIndex;
          const textColor = i < currentIndex
            ? 'var(--fg-faint)'
            : i > currentIndex
              ? 'var(--line-strong)'
              : 'var(--fg-bright)';
          return (
            <Group key={label} gap={6} wrap="nowrap">
              {i > 0 && (
                <Text size="sm" c="var(--line-strong)">
                  →
                </Text>
              )}
              <Group gap={4} wrap="nowrap" data-testid={isCurrent ? 'flow-label-current' : undefined}>
                {isCurrent && (
                  <span
                    style={{
                      display: 'inline-block',
                      width: 6,
                      height: 6,
                      borderRadius: '50%',
                      backgroundColor: ownerColor,
                    }}
                  />
                )}
                <Text
                  size="sm"
                  c={textColor}
                  style={
                    isCurrent
                      ? { borderBottom: `2px solid ${ownerColor}`, paddingBottom: 2 }
                      : undefined
                  }
                >
                  {label}
                </Text>
              </Group>
            </Group>
          );
        })}
      </Group>
    </Group>
  );
}
