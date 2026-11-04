import type { BootstrapStatus } from '../../electron/update/types';
export function bootstrapView(status: BootstrapStatus): { visible: boolean; title: string; detail: string } {
  switch (status.state) {
    case 'ready':
      return { visible: false, title: '', detail: '' };
    case 'installing':
      return {
        visible: true,
        title: 'Installing the cpd command tool',
        detail: 'Runs once in the background; the cockpit starts when it finishes.',
      };
    case 'failed':
      return {
        visible: true,
        title: 'cpd command tool install failed',
        detail: `${status.error} (log: ${status.logPath})`,
      };
  }
}
