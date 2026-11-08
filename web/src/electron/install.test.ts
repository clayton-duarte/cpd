// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { detectBrokenInstall } from './install';

describe('install', () => {
  it('reports a broken installation as a structured event', () => {
    const appDir = '/home/u/.local/share/cpd/app';
    const reported: unknown[] = [];
    const event = detectBrokenInstall({
      appDir,
      exists: () => false,
      report: (e) => void reported.push(e),
    });
    expect(event).not.toBeNull();
    expect(event).toMatchObject({ source: 'install', code: 'app_dir_missing' });
    expect(event?.details.appDir).toBe(appDir);
    expect(typeof event?.message).toBe('string');
    expect(event?.message.length).toBeGreaterThan(0);
    expect(reported).toEqual([event]);
  });
});
