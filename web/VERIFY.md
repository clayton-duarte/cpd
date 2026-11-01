# Web `verify:web` end-to-end verification

Attempt: `verify-web-a1`
Worktree: `/home/clayton/projects/cpd.web-client.client`
Date: 2026-10-31 22:01 EDT

This file records every command run for the `verify-web` Job, its exit status,
and the confirmations required by the acceptance criteria. All commands were run
from the repo root (the worktree root). No product code was changed.

## Commands and exit statuses

### 1. Frozen install

```
$ timeout 300 pnpm install --frozen-lockfile
Scope: all 2 workspace projects
✓ Lockfile passes supply-chain policies (verified 3m ago)
Lockfile is up to date, resolution step is skipped
Done in 203ms using pnpm v12.6.0
EXIT: 0
```

Confirmation: **frozen install ok** (exit 0, lockfile up to date, no changes).

### 2. Root `verify:web` (typecheck + build + vitest)

```
$ timeout 600 pnpm run verify:web
$ pnpm --filter cpd-web-client run typecheck && pnpm --filter cpd-web-client run build && pnpm --filter cpd-web-client run test
$ tsc --noEmit
$ vite build
vite v8.3.5 building client environment for production...
transforming...
✓ 298 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                   0.39 kB │ gzip:   0.26 kB
dist/assets/index-BBjTRrDy.css   27.37 kB │ gzip:   5.68 kB
dist/assets/index-Bpmb2cVP.js   423.44 kB │ gzip: 131.63 kB
✓ built in 1.69s
$ vitest run

 RUN  v5.0.5 /home/clayton/projects/cpd.web-client.client

 ✓ web/src/api/mock.test.ts (2 tests) 5ms
 ✓ web/src/api/wire.test.ts (3 tests) 9ms
 ✓ web/src/parity/parity.test.ts (1 test) 3ms
 ✓ web/src/api/actions.test.ts (1 test) 4ms
 ✓ web/src/fold/fold.test.ts (3 tests) 4ms
 ✓ web/src/views/views.test.tsx (3 tests) 40ms

 Test Files  6 passed (6)
      Tests  13 passed (13)
   Duration  736ms
EXIT: 0
```

Confirmation: **verify:web ok** (exit 0; typecheck clean, build produced
`dist/index.html` plus hashed JS/CSS assets, 13/13 vitest tests passed).

### 3. Reachability — preview serves the built app over HTTP

Build (fresh, standalone):

```
$ timeout 300 pnpm --filter cpd-web-client run build
$ vite build
vite v8.3.5 building client environment for production...
✓ 298 modules transformed.
dist/index.html                   0.39 kB │ gzip:   0.26 kB
dist/assets/index-BBjTRrDy.css   27.37 kB │ gzip:   5.68 kB
dist/assets/index-Bpmb2cVP.js   423.44 kB │ gzip: 131.63 kB
✓ built in 4.05s
EXIT: 0
```

Start the preview server in the background, wait, then curl it:

```
$ setsid pnpm --filter cpd-web-client exec vite preview --port 4173 &   # background
$ sleep 3
$ timeout 10 curl -sf -o /tmp/verify-web-a1-index.html -w "HTTP_CODE=%{http_code}\n" http://localhost:4173/
HTTP_CODE=200
EXIT: 0
```

Served index HTML (HTTP 200):

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>cpd web client</title>
    <script type="module" crossorigin src="/assets/index-Bpmb2cVP.js"></script>
    <link rel="stylesheet" crossorigin href="/assets/index-BBjTRrDy.css">
  </head>
  <body>
    <div id="root"></div>
  </body>
</html>
```

The served HTML references the built assets by their hashed names
(`/assets/index-Bpmb2cVP.js` and `/assets/index-BBjTRrDy.css`), matching the
names emitted by the build in step 2/3.

Confirmation: **preview served index HTML** that references the built assets.

Server teardown:

```
$ fuser -k 4173/tcp
$ pgrep -af "vite preview"        -> NONE
$ ss -ltnp | grep 4173            -> PORT_4173_FREE
```

Confirmation: **no background process left running** (port 4173 free, no
`vite preview` process remaining).

## Confirmations summary

- frozen install ok — exit 0
- verify:web ok — exit 0 (typecheck clean, build ok, 13/13 vitest tests pass)
- preview served index HTML — HTTP 200, HTML references built assets
  `/assets/index-Bpmb2cVP.js` and `/assets/index-BBjTRrDy.css`
- no background process left running (port 4173 free)
- no file outside `web/VERIFY.md` touched (docs-only change)

## Result

All acceptance criteria hold. Every check passed; no failure to escalate.
