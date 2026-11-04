import { describe, expect, it } from 'vitest';
import { updateMenuItem } from './menu';

describe('updateMenuItem', () => {
  it('offers a restart when restart-needed', () => {
    let clicks = 0;
    const item = updateMenuItem({ phase: 'restart-needed' }, () => void clicks++);
    expect(item.label).toBe('Restart to Apply Update');
    expect(item.enabled).toBe(true);
    item.click?.();
    expect(clicks).toBe(1);
  });

  it('is disabled while building', () => {
    for (const phase of ['syncing', 'building', 'waiting'] as const) {
      const item = updateMenuItem({ phase }, () => {});
      expect(item).toEqual({ label: 'Updating…', enabled: false });
    }
  });

  it('shows failure on error', () => {
    expect(updateMenuItem({ phase: 'error', message: 'boom' }, () => {})).toEqual({ label: 'Update Failed (see log)', enabled: false });
  });

  it('shows up to date when idle', () => {
    expect(updateMenuItem({ phase: 'idle' }, () => {})).toEqual({ label: 'Up to Date', enabled: false });
  });
});
