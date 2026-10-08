# CPD

Continuous Parallel Delivery — a personal workflow GUI for orchestrating AI coding agents.

A canvas shows projects, sessions, workflows and jobs in a GitHub-Actions style; an inbox
shows where attention is needed. Work flows Context → Plan → Dispatch → Review → Ship.

Status: early prototype. Fake data only, no backend.

## Stack

Vite · React · TypeScript · [React Flow](https://reactflow.dev) · [ELK](https://eclipse.dev/elk/) · [Mantine](https://mantine.dev) · Tabler icons

## Develop

```bash
pnpm install
pnpm dev
```

## Running locally

Prerequisite: logged into GitHub Copilot via `pi` (the daemon reads
`~/.pi/agent/auth.json`).

```bash
pnpm install
pnpm dev:all
```

This starts the daemon and the Vite dev server together. The app is served on
Vite's port; the daemon listens on port 4317. The browser talks to `/api`,
which Vite proxies to the daemon, so there is a single origin and no CORS
configuration anywhere.

### Env knobs

- `CPD_DEV_PORT` — port the Vite dev server serves the app on (default `8888`).
- `CPD_DAEMON_PORT` — port the daemon listens on.
- `CPD_DB` — path to the SQLite transcript DB (defaults under `.cpd/`, which
  is gitignored — it holds conversation content and must never be committed).
- `CPD_PI_AUTH` — path to the `pi` auth file (defaults to
  `$HOME/.pi/agent/auth.json`).
- `VITE_CPD_API` — override the API base used by the frontend.
- `CPD_DAEMON_URL` — override the daemon URL the Vite dev proxy targets
  (defaults to `http://localhost:$CPD_DAEMON_PORT`).

### Reviewing a specific branch

Check it out and run the same command — there is no separate review tool:

```bash
git checkout some-branch
pnpm dev:all
```

| Env var          | Default                             | Meaning                               |
| ---------------- | ----------------------------------- | ------------------------------------- |
| `CPD_DAEMON_PORT` | `4317`                             | Port the daemon listens on            |
| `CPD_DAEMON_URL`  | `http://localhost:$CPD_DAEMON_PORT` | Proxy target for the `/api` dev proxy |

## License

MIT