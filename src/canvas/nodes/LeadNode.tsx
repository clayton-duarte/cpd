import { LeadCard } from '../../components/LeadCard';
import type { LeadCardData } from '../../model/levels';

export interface LeadNodeData {
  lead: LeadCardData;
  [key: string]: unknown;
}

/** React Flow node wrapping the existing LeadCard unchanged -- no handles, no edges at this level. */
export function LeadNode({ data }: { data: LeadNodeData }) {
  return <LeadCard lead={data.lead} />;
}
