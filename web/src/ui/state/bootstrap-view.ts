import type { BootstrapStatus } from '../../electron/update/types';
export function bootstrapView(_status: BootstrapStatus): { visible: boolean; title: string; detail: string } {
  throw new Error('not implemented');
}
