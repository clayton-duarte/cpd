import type { UpdateState } from './types';

export function updateMenuItem(state: UpdateState, onRestart: () => void): { label: string; enabled: boolean; click?: () => void } {
  switch (state.phase) {
    case 'restart-needed':
      return { label: 'Restart to Apply Update', enabled: true, click: onRestart };
    case 'syncing':
    case 'building':
    case 'waiting':
      return { label: 'Updating…', enabled: false };
    case 'error':
      return { label: 'Update Failed (see log)', enabled: false };
    case 'idle':
      return { label: 'Up to Date', enabled: false };
  }
}
