import { Badge, Card, Group, Text } from '@mantine/core';
import { IconMessage } from '@tabler/icons-react';
import type { Session } from '../model/types';


export interface LeadCardProps {
  session: Session;
}

/** A lead is not a job: no status icon, no attempt. */
export function LeadCard({ session }: LeadCardProps) {
  return (
    <Card
      withBorder
      padding="sm"
      w={220}
      style={{
        backgroundColor: 'var(--bg-panel)',
        borderColor: 'var(--line-strong)',
        borderStyle: 'solid',
        borderWidth: 1,
      }}
    >
      <Group justify="space-between" wrap="nowrap">
        <Group gap="xs" wrap="nowrap">
          <IconMessage size={16} color="var(--fg-bright)" />
          <Text size="sm">Opus</Text>
        </Group>
        <Badge color="dark" variant="light" c="var(--fg-bright)">
          {session.leadTier}
        </Badge>
      </Group>
      <Text size="xs" c="var(--fg-muted)" mt="xs">
        {session.name}
      </Text>
    </Card>
  );
}
