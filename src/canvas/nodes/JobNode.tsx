import { Handle, Position } from '@xyflow/react';
import { JobCard } from '../../components/JobCard';
import type { Job } from '../../model/types';

export interface JobNodeData {
  job: Job;
  colorDisabled?: boolean;
  [key: string]: unknown;
}

/**
 * React Flow custom node wrapping the existing JobCard unchanged, with
 * target/source handles for dependency edges.
 */
export function JobNode({ data }: { data: JobNodeData }) {
  return (
    <>
      <Handle type="target" position={Position.Left} />
      <JobCard job={data.job} colorDisabled={data.colorDisabled} />
      <Handle type="source" position={Position.Right} />
    </>
  );
}
