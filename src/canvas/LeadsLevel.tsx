import { Group } from '@mantine/core';
import { LeadCard } from '../components/LeadCard';
import type { LeadCardData } from '../model/levels';
import { SPACE_CARDS } from '../layout/spacing';

export interface LeadsLevelProps {
  leads: LeadCardData[];
  onSelect: (leadId: string) => void;
}

/**
 * Leads level: one card per lead session. Leads have no dependency edges
 * between them -- they are a set, not a graph -- so a plain wrapping flow is
 * correct; gaps come from the CSS flex gap (browser-computed), never a
 * hand-written position (D26).
 */
export function LeadsLevel({ leads, onSelect }: LeadsLevelProps) {
  return (
    <Group gap={`${SPACE_CARDS}px`} align="flex-start" p={`${SPACE_CARDS}px`} wrap="wrap">
      {leads.map((lead) => (
        <LeadCard key={lead.id} lead={lead} onClick={() => onSelect(lead.id)} />
      ))}
    </Group>
  );
}
