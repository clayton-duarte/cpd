import { memo } from 'react';
import { Handle, Position } from '@xyflow/react';
import { JobCard } from '../../components/JobCard';
import type { Job } from '../../model/types';

export interface JobNodeData {
  job: Job;
  colorDisabled?: boolean;
  onRun?: (jobId: string) => void;
  [key: string]: unknown;
}

/**
 * React Flow custom node wrapping the existing JobCard unchanged, with
 * target/source handles for dependency edges.
 *
 * `selected` comes from our own Canvas-level `selectedJobId` state (React
 * Flow's internal node.selected flag), not from React Flow's built-in
 * click-to-select — elementsSelectable is off, so this prop is the single
 * source of truth for the selected look.
 */
function JobNodeImpl({ data, selected }: { data: JobNodeData; selected?: boolean }) {
  return (
    <div
      aria-selected={selected ?? false}
      data-selected={selected ?? false}
      style={
        selected
          ? {
              border: '2px solid var(--blue-edge)',
              backgroundColor: 'var(--blue-tint)',
              borderRadius: 'var(--radius, 4px)',
            }
          : undefined
      }
    >
      <Handle type="target" position={Position.Left} />
      <JobCard job={data.job} colorDisabled={data.colorDisabled} onRun={data.onRun} />
      <Handle type="source" position={Position.Right} />
    </div>
  );
}

/** L6: memoized on exactly the fields JobCard renders, plus `selected` -- an `onRun` callback
 * that changes identity every render (the common case, since callers tend to inline it) must
 * NOT force a re-render on its own. */
function jobFieldsEqual(a: Job, b: Job): boolean {
  return (
    a.id === b.id &&
    a.title === b.title &&
    a.status === b.status &&
    a.owner === b.owner &&
    a.profile === b.profile &&
    a.tier === b.tier &&
    a.attempt === b.attempt &&
    a.artifactCount === b.artifactCount &&
    a.steeringPending === b.steeringPending
  );
}

export const JobNode = memo(JobNodeImpl, (prev, next) => {
  return (
    prev.selected === next.selected &&
    prev.data.colorDisabled === next.data.colorDisabled &&
    jobFieldsEqual(prev.data.job, next.data.job)
  );
});
