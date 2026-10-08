import { memo } from 'react';
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
function WorkflowHeaderNodeImpl({ data }: { data: WorkflowHeaderNodeData }) {
  if (!data.workflow) return null;
  return (
    <div style={{ position: 'absolute', top: 0, left: 0, pointerEvents: 'none' }}>
      <WorkflowHeader workflow={data.workflow} />
    </div>
  );
}

/** L6: memoized on the workflow fields WorkflowHeader actually renders. */
function workflowFieldsEqual(a?: Workflow, b?: Workflow): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.id === b.id &&
    a.title === b.title &&
    a.attempt === b.attempt &&
    a.ticket?.key === b.ticket?.key &&
    a.ticket?.title === b.ticket?.title &&
    a.branch?.name === b.branch?.name &&
    a.pr?.number === b.pr?.number
  );
}

export const WorkflowHeaderNode = memo(WorkflowHeaderNodeImpl, (prev, next) =>
  workflowFieldsEqual(prev.data.workflow, next.data.workflow),
);
