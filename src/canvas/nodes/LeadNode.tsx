import type { Session } from '../../model/types';
import { LeadCard } from '../../components/LeadCard';

export interface LeadNodeData {
  session: Session;
  [key: string]: unknown;
}

/** Wraps the unmodified LeadCard for placement inside the session frame. */
export function LeadNode({ data }: { data: LeadNodeData }) {
  return <LeadCard session={data.session} />;
}
