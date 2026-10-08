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

- `CPD_DAEMON_PORT` — port the daemon listens on.
- `CPD_DB` — path to the SQLite transcript DB (defaults under `.cpd/`, which
  is gitignored — it holds conversation content and must never be committed).
- `CPD_PI_AUTH` — path to the `pi` auth file (defaults to
  `$HOME/.pi/agent/auth.json`).
- `VITE_CPD_API` — override the API base used by the frontend.
- `CPD_DAEMON_URL` — override the daemon URL the Vite dev proxy targets
  (defaults to `http://localhost:$CPD_DAEMON_PORT`).

## Reviewing a branch (`review.sh`)

```bash
./review.sh                      # serve the current branch
./review.sh integration/phase-b  # serve a specific ref
```

Starts the daemon, waits for it to answer `/api/health`, then starts Vite.
Both processes are torn down together on exit or Ctrl-C. If the daemon never
comes up, the script prints an error and exits non-zero instead of silently
serving a backend-less app.

| Env var           | Default                              | Meaning                              |
| ------------------ | ------------------------------------ | ------------------------------------- |
| `CPD_REVIEW_PORT`  | `4000`                                | Port `review.sh` serves Vite on       |
| `CPD_DAEMON_PORT`  | `4317`                                | Port the daemon listens on            |
| `CPD_DEV_PORT`     | `5173`                                | Port plain `pnpm dev` serves on       |
| `CPD_DAEMON_URL`   | `http://localhost:$CPD_DAEMON_PORT`  | Proxy target for the `/api` dev proxy |

## License

MIT
