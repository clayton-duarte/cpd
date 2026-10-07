import { Badge, Group, Text } from '@mantine/core';
import { IconGitPullRequest } from '@tabler/icons-react';
import type { Workflow } from '../model/types';
import { FLOW_ORDER, flowLabel } from '../model/derive';
import { TOKENS, w } from '../theme';

const OWNER_COLOR: Record<Workflow['owner'], string> = {
  you: TOKENS.red,
  agents: TOKENS.blue,
  neutral: TOKENS.white,
};

export function WorkflowHeader({ workflow }: { workflow: Workflow }) {
  const current = flowLabel(workflow.phase);
  const currentIndex = FLOW_ORDER.indexOf(current);
  const ownerColor = OWNER_COLOR[workflow.owner];

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

      <Group gap={6} wrap="nowrap">
        {FLOW_ORDER.map((label, i) => {
          const isCurrent = i === currentIndex;
          const opacity = i < currentIndex ? 0.45 : i > currentIndex ? 0.25 : 1;
          return (
            <Group key={label} gap={6} wrap="nowrap">
              {i > 0 && (
                <Text size="sm" c={w(0.25)}>
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
                  c={w(opacity)}
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
