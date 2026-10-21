# Hygiene sweep on hs (Job `audit-sweep`, attempt `audit-sweep-a1`)

One-off clean-up of the leaks listed in `docs/research/hygiene.md`, run 2026-10-21 about 11:38–11:45 EDT on `homeserver` (hs). Only the leaks of that report were touched; nothing of the live cpd (`~/.cpd`, `tmux -L cpd`, any pi), no herdr process, no test run younger than 1 h.

## Measures

| | before | after |
|---|---|---|
| processes (`ps -e`) | 968 | 888 (864 right after the kills; other work started since) |
| load average (1/5/15 min) | 21.30 18.73 13.40 | 13.97 16.89 13.12 (a busy machine; sample at the end) |
| files in `/tmp/tmux-1000` | 1205 | 24 |
| `pi-check-*` sockets | 1143 | 0 |
| `cpdt-*` sockets | 38 | 0 |
| `/tmp/.tmp*` | 310 | 0 |
| `/dev/shm/cpd-test-*` | 66 | 14 (all younger than 1 h, made by tests running now) |
| `/tmp/cpd-test-*` | 378 | 378 (not in the Job's list of removals; left) |
| free disk on `/` | 303 G | 315 G |
| worktrees of `~/projects/cpd` | 33 | 26 |

## Processes killed (SIGTERM, all gone after 3 s)

All had ppid 1 and age over 1 h.

### 32 tmux servers `tmux [-u] -L cpdt-<n>-0` (bug 3)

Each had `CPD_HOME=/dev/shm/cpd-test-*/cpd-home` (a test home) and a dead `cpd watch` pane. `<pid> <age s> <socket>`:

```
208543   73424 -L cpdt-203175-0
274595   78364 -L cpdt-269763-0
624481   85264 -L cpdt-614925-0
717577   85247 -L cpdt-706952-0
816331   79908 -L cpdt-811328-0
820853   73149 -L cpdt-809774-0
967913   86867 -L cpdt-961296-0
988456   76315 -L cpdt-978372-0
1191206   68364 -L cpdt-1183636-0
1277575   79565 -L cpdt-1272121-0
1424948   90837 -L cpdt-1418138-0
1654377   76055 -L cpdt-1649773-0
1959241   68180 -L cpdt-1952938-0
1990086   70924 -L cpdt-1984393-0
2090028   75870 -L cpdt-2084503-0
2184025   69069 -L cpdt-2175367-0
2208372   84726 -L cpdt-2200585-0
2300253   75780 -L cpdt-2295137-0
2562448   81427 -L cpdt-2557289-0
2694879   74093 -L cpdt-2688909-0
2722336   84577 -L cpdt-2716289-0
2734336   72341 -L cpdt-2729262-0
2920224   83551 -L cpdt-2913436-0
3263589   84453 -L cpdt-3257558-0
3506184   88781 -L cpdt-3500736-0
3550413   78635 -L cpdt-3543567-0
3641976   75200 -L cpdt-3636315-0
3643488   87370 -L cpdt-3637845-0
3797822   75152 -L cpdt-3791812-0
3847738   68650 -L cpdt-3838520-0
3919235   80584 -L cpdt-3909641-0
3986605   80561 -L cpdt-3980704-0
```

### 5 `cpd watch` with a deleted exe (bug 2)

Exe `/home/clayton/projects/cpd.quality--fl-teardown-a1/target/debug/cpd (deleted)`, `CPD_HOME=/tmp/cpd-test-*/cpd-home`, cwd a deleted `/tmp/cpd-test-*`, ppid 1. Pids 2695656, 2695723, 2777372, 3092648, 3422585.

### 2 driver shells of the same prototype (bug 2)

`sh -c "while :; do sleep 1; done" /tmp/cpd-test-<x>/driver`, cwd `/`, `CPD_HOME=/tmp/cpd-test-GLRH40/cpd-home` and `.../cpd-test-MiYYeN/...`, ppid 1. Pids 2729844, 2730083.

### Not found / not touched

- Bug 1 git-wrapper `/bin/sh .../cpd-home/bin/git` shells: none was running (no `sh` with a deleted cwd, none at 100 % CPU).
- Left running: the live `tmux -L cpd` server (pid 1641702) and its `cpd watch`; the `cpdt-*` servers younger than 1 h (running tests, e.g. `tmux -L cpdt-1631784-0`, `-1631788-0`, `-1788583-0`, which had a live parent test or a recent start); `herdr server` and `herdr --session cpd-test server`; `cpd attempt run ...` of Plan hygiene; every pi (including the User's orphan pi under herdr).

## Sockets removed

1181 sockets in `/tmp/tmux-1000`, each only after `tmux -L <name> ls` failed (no server): 1143 `pi-check-*`, 37 `cpdt-*`, 1 `cpdt-probe`. (The 38 `cpdt-*` sockets include the servers killed above, whose sockets stayed.) `pi-check-*` younger than 2 min were not touched (none were live; 0 left at the end). The other probe sockets in that directory (`cpdchk-*`, `cpdx-*`, `pt*`, `probe*`, ...) were not in the list and are left.

## Directories removed (older than 1 h, nothing open in `lsof` or `/proc/*/cwd`)

- 310 `/tmp/.tmp*` (all)
- 46 `/dev/shm/cpd-test-*` (all of Oct 20)

Total 356, 0 skipped as in use. Left: 14 `/dev/shm/cpd-test-*` younger than 1 h; 378 `/tmp/cpd-test-*` (not in the Job's list; prototype-harness leftovers per hygiene.md).

## Worktrees

Removed (clean, 0 commits outside `origin/*`; `git -C ~/projects/cpd worktree remove <path>` without `--force`, then `git branch -D`; PR status from `cpd plan show`):

| worktree | branch | why |
|---|---|---|
| `../cpd.inbox.prrow` | `cpd/inbox/prrow` | PR `prrow` cancelled |
| `../cpd.inbox.team` | `cpd/inbox/team` | PR `team` cancelled |
| `../cpd.pi-pin.autoinstall` | `cpd/pi-pin/autoinstall` | PR `autoinstall` cancelled |
| `../cpd.quality.flakes3` | `cpd/quality/flakes3` | PR `flakes3` cancelled |
| `../cpd.quality.docsplit` | `cpd/quality/docsplit` | PR `docsplit` cancelled |
| `../cpd.quality.mergebg` | `cpd/quality/mergebg` | PR `mergebg` cancelled |
| `../cpd.quality--fl-bare-a1` | `cpd/quality--fl-bare-a1` | ended (cancelled) Attempt of cancelled PR `flakes3` |

Left, with the reason:

| worktree | reason |
|---|---|
| `../cpd.quality.harness` (PR merged) | clean but unpushed: `HEAD` has commits outside `origin/*` (42 counted by `rev-list HEAD --not --remotes=origin` now; hygiene.md: a local rule-S merge after the recorded head) |
| `../cpd.quality.flakes4` (PR merged) | clean but 1 commit outside `origin/*` (local rule-S merge) |
| `../cpd.release--setup-lock-a1` (cancelled PR `busyfix`) | 2 commits of the Attempt never pushed |
| `../cpd.release.busyfix` (cancelled) | 2 commits not on `origin` |
| `../cpd.manager.open-now` (cancelled) | 2 commits not on `origin` |
| `/tmp/cpd-mainfix` | not a Plan's worktree (hygiene.md); not touched |
| live Plans' worktrees (`agent-comms`, `hygiene`, `manager.interim`, `mergefix`, `plan-ux`, `ui-design`, `yield`) and the locked `...gate-1533882` | in use |

## Live cpd after the sweep

`cpd doctor` (13 checks, `ok: true`; the engine check is `watch`):

```
config ok, store ok, tamper ok, git ok, tmux ok, pi ok, gh ok, gh_auth ok, pi_node ok, tiers ok, pi_adapter ok, lead_packages ok, watch ok
```

`tmux -L cpd ls`:

```
cpd: 27 windows (created Mon Oct 19 16:07:49 2026)
cpd-work: 5 windows (created Wed Oct 21 11:08:12 2026)
```

`ps -eo ppid,args` after: the only ppid-1 `cpdt`/`cpdchk`/`cpd watch`/`while :` entry is the live `tmux -L cpd`.
