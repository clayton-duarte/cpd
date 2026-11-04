import type { UpdateState } from './types';

export function updateMenuItem(_state: UpdateState, _onRestart: () => void): { label: string; enabled: boolean; click?: () => void } {
  throw new Error('not implemented');
}
