import { describe, expect, it } from 'vitest';
import { fakeExec, fakeFs } from './fakes';
import { buildPreviewView, PREVIEW_BRANCH } from './preview';
import { BUILT_SHA_FILE, PREVIEW_VIEW_DIR } from './views';

// The preview view is the prod build of the preview branch, not a dev server (air-d96): with a git
// checkout on branch `preview`, the builder runs the same command prod is built with, against that
// checkout, into the preview build dir.
const PREVIEW = '/preview';
const SHA = 'abc123';
const BUILD = 'pnpm --filter cpd-web-client run build:electron';

describe('buildPreviewView', () => {
  it('builds the preview view from the preview branch', async () => {
    const fx = fakeExec({
      'git rev-parse --abbrev-ref HEAD': { stdout: `${PREVIEW_BRANCH}\n` },
      'git rev-parse HEAD': { stdout: `${SHA}\n` },
      [BUILD]: {},
    });
    const fs = fakeFs({ [`${PREVIEW}/.git/HEAD`]: '' });

    const result = await buildPreviewView({ exec: fx.exec, fs }, PREVIEW);

    expect(result).toEqual({ ok: true, outDir: PREVIEW_VIEW_DIR, sha: SHA });
    const build = fx.calls.find((c) => c.cmd === 'pnpm');
    expect(build?.cwd).toBe(PREVIEW);
    expect(build?.env?.['CPD_ELECTRON_OUT']).toBe(PREVIEW_VIEW_DIR);
    expect(fs.readText(`${PREVIEW}/web/${PREVIEW_VIEW_DIR}/${BUILT_SHA_FILE}`)).toBe(SHA);
  });

  it('refuses to build the preview view from a checkout that is not on the preview branch', async () => {
    const fx = fakeExec({
      'git rev-parse --abbrev-ref HEAD': { stdout: 'main\n' },
      'git rev-parse HEAD': { stdout: `${SHA}\n` },
      [BUILD]: {},
    });
    const fs = fakeFs({ [`${PREVIEW}/.git/HEAD`]: '' });

    const result = await buildPreviewView({ exec: fx.exec, fs }, PREVIEW);

    expect(result.ok).toBe(false);
    expect(fx.calls.some((c) => c.cmd === 'pnpm')).toBe(false);
  });
});
