import type { Workflow } from '../../model/types';
import { WorkflowHeader } from '../../components/WorkflowHeader';

export interface WorkflowHeaderNodeData {
  workflow?: Workflow;
  [key: string]: unknown;
}

/**
 * Sits at the top of the workflow's region inside the session frame, above
 * the waves. The node itself spans the full workflow bounding box (sized by
 * ELK); only the top padding reserved in toGraph.ts is actually drawn into.
 */
export function WorkflowHeaderNode({ data }: { data: WorkflowHeaderNodeData }) {
  if (!data.workflow) return null;
  return (
    <div style={{ position: 'absolute', top: 0, left: 0, pointerEvents: 'none' }}>
      <WorkflowHeader workflow={data.workflow} />
    </div>
  );
}
