import { describe, expect, it } from 'vitest';
import { bootstrapView } from './bootstrap-view';

describe('bootstrapView', () => {
  it('bootstrapView hides when ready', () => {
    expect(bootstrapView({ state: 'ready' }).visible).toBe(false);
  });
  it('bootstrapView announces the install while installing', () => {
    expect(bootstrapView({ state: 'installing' })).toEqual({
      visible: true,
      title: 'Installing the cpd command tool',
      detail: 'Runs once in the background; the cockpit starts when it finishes.',
    });
  });
  it('bootstrapView shows the error and the log path on failure', () => {
    expect(bootstrapView({ state: 'failed', error: 'boom', logPath: '/tmp/cli-bootstrap.log' })).toEqual({
      visible: true,
      title: 'cpd command tool install failed',
      detail: 'boom (log: /tmp/cli-bootstrap.log)',
    });
  });
});
