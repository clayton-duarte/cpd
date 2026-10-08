import { Badge, Group, Text } from '@mantine/core';
import { CardShell } from './CardShell';
import type { LeadCardData } from '../model/levels';

export interface LeadCardProps {
  lead: LeadCardData;
  onClick?: () => void;
}

/** Leads-level card: name, tier code, and a count of its plans (counts only). */
export function LeadCard({ lead, onClick }: LeadCardProps) {
  return (
    <div onClick={onClick} style={{ cursor: onClick ? 'pointer' : undefined }}>
      <CardShell borderColor="var(--line-strong)">
        <Group justify="space-between" wrap="nowrap" gap="var(--gap)">
          <Text size="sm" fw={600} c="var(--fg-bright)" truncate style={{ minWidth: 0 }}>
            {lead.name}
          </Text>
          <Badge color="dark" variant="light" c="var(--fg-bright)">
            {lead.leadTier}
          </Badge>
        </Group>
        <Text size="xs" c="var(--fg-faint)" mt="var(--gap)">
          {lead.planCount} {lead.planCount === 1 ? 'plan' : 'plans'}
        </Text>
      </CardShell>
    </div>
  );
}
