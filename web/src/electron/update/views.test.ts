import { expect, it } from 'vitest';
import { fakeExec, fakeFs } from './fakes';
import { BUILT_SHA_FILE, buildProdView, PROD_VIEW_DIR } from './views';

// The prod view is the default build of the latest commit on main (air-d89 point 4): the builder
// reads main HEAD in its checkout and writes the prod build dir with the command prod is built with.
const ROOT = '/repo';
const SHA = 'abc123';
const BUILD = 'pnpm --filter cpd-web-client run build:electron';

it('builds the prod view from the main checkout', async () => {
  const fx = fakeExec({
    'git rev-parse HEAD': { stdout: `${SHA}\n` },
    [BUILD]: {},
  });
  const fs = fakeFs({ [`${ROOT}/.git/HEAD`]: '' });

  const result = await buildProdView({ exec: fx.exec, fs }, ROOT);

  expect(result).toEqual({ ok: true, outDir: PROD_VIEW_DIR, sha: SHA });
  const build = fx.calls.find((c) => c.cmd === 'pnpm');
  expect(build?.cwd).toBe(ROOT);
  expect(build?.env?.['CPD_ELECTRON_OUT']).toBe(PROD_VIEW_DIR);
  expect(fs.readText(`${ROOT}/${PROD_VIEW_DIR}/${BUILT_SHA_FILE}`)).toBe(SHA);
});
