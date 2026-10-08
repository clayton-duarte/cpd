import { PlanCard } from '../../components/PlanCard';
import type { PlanCardData } from '../../model/levels';

export interface PlanNodeData {
  plan: PlanCardData;
  [key: string]: unknown;
}

/** React Flow node wrapping the existing PlanCard unchanged -- no handles, no edges at this level. */
export function PlanNode({ data }: { data: PlanNodeData }) {
  return <PlanCard plan={data.plan} />;
}
