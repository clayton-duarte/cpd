# Backlog

Work for later.

- `run/<plan>/` retention: everything (logs, Worker Specs, plannotator data) is kept until the User says it is too big; only then set a policy (hs-d63).
- Move CI from `ubuntu-24.04` to `ubuntu-26.04` deliberately, in its own change (hs-d195).
- Host widget over SSH: show **<client> → homeserver** (User, 2026-10-22). The User's Macs (mac-pro, mac-air) are clients that SSH into homeserver, where the cockpit's tmux runs, and every client sees the same panes. Find the client in use: tmux's `client-focus-in` / `client-active` hooks (`focus-events` is already on, and Ghostty reports focus) record the client's `SSH_CONNECTION` IP, mapped to its ssh alias. The Host section shows that machine and homeserver, and nothing about any other attached client; the status line, drawn per client, can name each client exactly. The client's numbers come from its own installed cpd sampler, GPU included (it already works on the Mac), fetched by homeserver over one persistent SSH connection (e.g. `ssh <alias> cpd host sample --json`); the alternative is the Mac pushing them, which needs a service on each Mac. Unverified: the focus hooks with Ghostty over SSH. No SSH: the local machine alone, as now.
