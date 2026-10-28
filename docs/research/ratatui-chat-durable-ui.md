# ratatui chat: pi-durable's API for an external UI (evidence file `durable-ui`)

SKELETON, 2026-10-28 15:20 EDT. Findings follow in later commits.

Read at: `@earendil-works/pi-durable@1.4.0` (npm latest, ran `npm view @earendil-works/pi-durable version dist-tags` 2026-10-28 EDT:
`version = '1.4.0'`, `dist-tags = { latest: '1.4.0' }`); repo `earendil-works/pi` `main` at `33385f9572303e79bfb3811cc92cc8dd65e22261`
(committed 2026-10-28 14:53 EDT, ran `git rev-parse HEAD`), tag `v1.4.0`.

1. Client API
2. Streaming and cross-process visibility
3. Steering and interruption
4. Human-in-the-loop
5. Hosting
6. IPC options from Rust
