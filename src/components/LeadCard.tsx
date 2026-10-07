import { Badge, Card, Group, Text } from '@mantine/core';
import { IconMessage } from '@tabler/icons-react';
import type { Session } from '../model/types';
import { TOKENS, w } from '../theme';

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
        backgroundColor: 'rgba(230,230,230,.03)',
        borderColor: w(0.25),
        borderStyle: 'solid',
        borderWidth: 1,
      }}
    >
      <Group justify="space-between" wrap="nowrap">
        <Group gap="xs" wrap="nowrap">
          <IconMessage size={16} color={TOKENS.white} />
          <Text size="sm">Opus</Text>
        </Group>
        <Badge color="dark" variant="light" c={TOKENS.white}>
          {session.leadTier}
        </Badge>
      </Group>
      <Text size="xs" c={w(0.6)} mt="xs">
        {session.name}
      </Text>
    </Card>
  );
}
