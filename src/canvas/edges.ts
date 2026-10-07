import type { Job } from '../model/types';

/** Edge style helper: dashed when either endpoint is a draft, solid otherwise. */
export function edgeStyle(sourceJob: Job, targetJob: Job): 'dashed' | 'solid' {
  return sourceJob.status === 'draft' || targetJob.status === 'draft' ? 'dashed' : 'solid';
}
