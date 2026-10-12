# t2: How does CPD's Ledger map onto beads?

Date 2026-10-12 (18:27–18:40 EDT, ~13 min wall). Exercised: `bd` 1.3.1 (Homebrew), embedded Dolt (bundled), standalone `dolt` 2.4.2 (read-only inspection), beads docs from `/tmp/cpd-research/repos/steveyegge__beads` @ `a2d43e6792d1951f29d85c36aaf4f24229550da6`. Scratch: `/tmp/cpd-trials/t2/{repo,ledger/wf1,ledger/exp,env.sh,lib.sh,prove.sh,jobs.env}`. Machine: M1 / 8 GB.

## Verdict

- **Beads works as the Ledger, but only as a dumb store.** Every Ledger concept maps onto a native construct (table below). Beads enforces ordering and readiness. It enforces none of Locked, append-only, or Spec validity. `cpd` must enforce those itself, and must treat any `bd` write that does not come from `cpd` as tampering.
- **One beads workspace per Workflow, outside the repo:** `ledger/<wf-id>/` with `BEADS_DIR` set explicitly. Spawn every `bd` with `cwd = ledger/<wf-id>`, never the product repo. Product repo was byte-identical after every step, including `bd` writes run with cwd = the product repo (those only print a `beads.role` warning on stderr).
- **Init flags:** `bd init --prefix <wf> --skip-agents --skip-hooks --non-interactive`, run from the ledger dir with `BEADS_DIR` **unset**. Do not use `--stealth`. Then `bd metrics off`, and set `BD_DISABLE_METRICS=1 DO_NOT_TRACK=1` on every call (see risks: telemetry).
- **Job states:** `draft:frozen`, `queued:active`, `running:wip`, `in_review:wip`, `failed:frozen`, `cancelled:frozen`, and **Merged = built-in `closed`**. "Ready" is derived with `bd ready -t job -n 0`. A custom status in the `done` category does **not** unblock dependents, so Merged must use `closed`. A custom `cancelled:frozen` keeps dependents blocked, which satisfies the Cancelled-must-not-unblock invariant (demonstrated). The catch: `bd close` on a cancelled Job works and unblocks its dependents (demonstrated), so `cpd` must never call `bd close` except on a Job in `in_review`.
- **Attempts = child issues of the Job** (type `attempt`, id `<job>.<n>`), write-once metadata, one comment per feedback, plus `bd provenance` events (true append-only, idempotent) for branch, commit SHA and landing. Escalations are child issues of the Attempt. Metrics are flat numeric metadata keys.
- **Locked cannot be protected.** `update`, `edit`, `dep remove`, `delete`, `close` of a cancelled Job, `prune` and `flatten` all succeed on a Locked Job. `MIGRATION-FREEZE` is workspace-wide and blocks `cpd`'s own writes. `--readonly` / `BD_READONLY=true` is useful only for processes that must never write. `cpd` needs (a) a lock fingerprint stored outside beads, (b) a pre-write invariant check, and (c) an audit that compares `bd history --events` actors.
- **Useful primitives:**
  - `bd update --if-status <s>` is a compare-and-swap (exit 13 on mismatch); 10/10 concurrent races had exactly one winner.
  - `bd batch` gives atomic multi-Job status changes.
  - `bd close` refuses a Job with open children (attempts or escalations) and refuses a blocked Job without `--force`.
- **Concurrency and latency are fine:** 20 simultaneous writes produced no lock errors and no lost updates. Median command 0.40 s, worst 0.63 s.
- **Isolation caveat (my fault or unknown):** I may have touched the real home twice. See Risks "isolation incidents". Both are benign.

## Acceptance checklist

| # | item | result | evidence |
|---|---|---|---|
| 1 | Init recipe, repo provably untouched, global side effects listed | **pass** (with incidents noted) | F1 |
| 2 | Mapping table, each row demonstrated | **pass** | F2 |
| 3 | 5-Job ready semantics, cancelled keeps D blocked, appended F | **pass** | F3 |
| 4 | Three Attempts (S1,S2,S3), append-only, history in order | **pass** (append-only is by convention only) | F4 |
| 5 | Locked: what breaks it, freeze marker, conclusion | **pass** | F5 |
| 6 | History: `bd history`, `show --json`, audit, Dolt log | **pass** | F6 |
| 7 | Concurrency ×10 | **pass** | F7 |
| 8 | Latency, 20 commands | **pass** | F8 |
| 9 | JSON stability, noise | **pass** | F9 |
| extras | `cpd`↔`bd` contract | **pass** | Recipes |

## Findings

### F1. Init, isolation, global side effects

Setup. Scratch repo `/tmp/cpd-trials/t2/repo` (git init, one commit). `prove.sh` prints: `git status --porcelain`, a sha of `git config --list --local`, a sha of `ls -la .git/hooks`, a sha of all file contents (n=27 files), and the root listing. Baseline `proof-0.txt`:

```
status:[]  config:d9e5bf50fc11  hooks:72a1615ebd6d  files:4e9367555d81 n=27  ls-root:.git README.md
```

Every step group below (init, workflow, jobs, deps, lock, each ready step, attempts, escalation, tamper tests, freeze, history) ended with `chk` → `[repo unchanged]`. This is per step group, not literally after each single `bd` call. Also proven with writes issued from cwd = the product repo (`bd create`, `bd update` with `BEADS_DIR` pointing at the ledger):

```
$ cd repo; bd create "from product repo cwd" ...; bd update ... ; prove.sh | diff - proof-0.txt
REPO UNCHANGED after bd writes with cwd=product repo     # only stderr: "warning: beads.role not configured (GH#2950)"
```

Init that was used:

```
HOME=/tmp/cpd-trials/t2/home XDG_CONFIG_HOME=$HOME/.config
cd ledger/wf1; bd init --prefix wf1 --skip-agents --skip-hooks --non-interactive      # 4.5 s
  ✓ Initialized git repository      ← bd runs `git init` in the ledger dir (cwd) and commits its own files
  ✓ Committed beads files to git
  Backend: dolt / Mode: embedded / Database: wf1
```

What `bd init` created, in the ledger dir only:
- `.git/` (own repo, `beads.role=maintainer`, no hooks, no remotes).
- `.gitignore`.
- `.beads.gate.lock`.
- `.beads/{config.yaml, metadata.json, README.md, .gitignore, .local_version, embeddeddolt/<db>/.dolt}`.

No `AGENTS.md`, no hooks, no `.claude/` (because `--skip-agents --skip-hooks`).

Flag findings (`bd init --help`, docs `reference/worktrees.md`, `configuration.md`):
- `--skip-agents` and `--skip-hooks` are required to avoid `AGENTS.md`, agent integrations and git hooks.
- `--stealth` (tested in a second sandbox home `h2`): it did not write any global git file (no `~/.gitconfig` appeared). It does append to the **repo's** `.git/info/exclude`, so it would touch the product repo. The docs say "global gitattributes and gitignore", which I did not observe (disagreement; behaviour wins). Not needed with an external Ledger, so do not use it.
- Initialising with `BEADS_DIR` already set creates no git repo and no commit (worked, tested in `ledger/exp`), but then `bd` prints the `beads.role` warning on every call, because there is no git repo to hold `beads.role`. Preferred: init from the ledger dir with `BEADS_DIR` unset, and run later with cwd = ledger dir.
- `bd init --init-if-missing` is idempotent: "Skipping init: workspace already initialized.", rc=0.
- `bd context --json` fails with `BEADS_DIR points to unsafe location: /private/tmp/...`. `bd where` works. Not needed by `cpd`; it may be a `/tmp` artefact, **unverified** under a non-`/tmp` path.

Global side effects under the sandbox HOME (final listing, before cleanup):

| path | purpose |
|---|---|
| `.beads/machine-id` | random anonymous machine id for telemetry |
| `.beads/eventsData/*.evtq` (197 files, ~365 B each, +1 per `bd` call) and `.last-flush`, `eventkit.lock` | local telemetry queue (`cli_command` events with the command name); written even after `bd metrics off`. `BD_DISABLE_METRICS=1` or `DO_NOT_TRACK=1` stops the growth (197→198 without, 198→198 with) |
| `.config/bd/config.yaml` | user-global bd config (`metrics.disabled`, endpoint `https://gastownhall-eventsapi.com/mp/collect`, `notice_shown`) |
| `.dolt/config_global.json`, `.dolt/eventsData/*.devts`, `dolt.lock` | embedded Dolt's global config and its own usage events |
| `Library/Caches/com.apple.python/...*.pyc` | **mine**: python3 used by my timing scripts, not bd |

No `.gitconfig` and no `.config/git` was created in the sandbox HOME. Real `~/.gitconfig` sha is unchanged (`42583d03…`), and the real `~/.beads` files are unchanged (`machine-id`).

### F2. Mapping table (each row demonstrated in `ledger/wf1`)

Config applied once per ledger: `bd config set status.custom "draft:frozen,queued:active,running:wip,in_review:wip,failed:frozen,cancelled:frozen"` and `bd config set types.custom "workflow,job,attempt,escalation"`. Both are stored in the Dolt DB, not in `config.yaml`. `bd statuses` lists them with categories.

| Ledger concept | beads construct | Demonstrated by |
|---|---|---|
| **Workflow** | issue type `workflow`, id `wf1-oj9`, status `open` until PR merged then `closed`. Metadata `ticket, repo, lead_branch, phase (context\|plan\|dispatch\|pr), lead_session, dispatch_started_at`. One beads workspace per Workflow. | `bd create "..." -t workflow --metadata '{...}'`; `bd update $WF --set-metadata phase=dispatch` |
| **Job** | type `job`, `--parent <workflow>` (dep type `parent-child`), id `<wf>.<n>` | `bd show wf1-oj9.1 --json` → `"parent":"wf1-oj9"`, `dependency_type: parent-child` |
| Spec fields | goal → `description`; `context`, `tier`, `touches[]`, `acceptance[]`, `out_of_scope[]` → `metadata` (JSON arrays survive `create --metadata`). Do **not** use `--context`: it is concatenated into `description` as `## Context`. `--acceptance` is a native string field, unused because acceptance is an array. | `jq '.[0].metadata'` shows typed arrays |
| Tier | `metadata.tier` (string, `S1`); bd cannot validate it | `{"tier":"S1"}` |
| State draft | status `draft` (frozen) | not in `bd ready` |
| State queued | status `queued` (active) | in `bd ready` when unblocked |
| State **ready** (derived) | no stored value: `bd ready --json -t job -n 0` | F3 |
| State running / in_review | `running` / `in_review` (wip: out of `ready`) | A running → not ready |
| State **merged** | built-in `closed` + `close_reason=merged` + `metadata.merged_sha`. A custom `merged:done` status did **not** unblock dependents and left `closed_at` null (tested in `ledger/exp`). | `bd close $A --reason merged` → B,C become ready |
| State failed | `failed:frozen` (dependents stay blocked) | tested in `exp`: dependent stayed blocked |
| State **cancelled** | `cancelled:frozen` + `metadata.cancel_reason, cancelled_at`; dependents stay blocked | F3 step 3 |
| **Locked** | no beads construct. Derived by `cpd` as `status != draft`. Proposed extra: `metadata.lock_sha256` at `plan lock` (see F5) | F5 |
| **Dependency** | dep type `blocks`: `bd dep add <job> <dependency>` ("job depends on dependency"); cycle check on by default. `parent-child` does not block, and the Workflow's own status (`in_progress`, `deferred`) does not change child readiness (tested). | `bd dep add`; `bd blocked --json` → `blocked_by` |
| **Attempt** | type `attempt`, child of Job (`wf1-oj9.2.1`..`.3`). Status `in_progress` → `in_review` (submitted) → `closed` with `close_reason = outcome`. Metadata `number, tier, worktree, branch, started_at, submitted_at, ended_at, outcome, retry_cause`. | F4 |
| Attempt events | `bd provenance record --issue <attempt> --kind cut\|commit\|land --source cpd --ref <x> --ref-kind branch\|git-sha` (append-only, no update/delete, idempotent: second identical call → `"inserted":false`). `git-sha` must be 40 lowercase hex. | F4 |
| **Review verdict** | `bd comments add <attempt> "<feedback>"` (immutable rows with author and timestamp) + `metadata.outcome` (`approved\|rejected\|failed\|timed_out`) + `close_reason` | F4 |
| **Escalation** | type `escalation`, child of the Attempt (open while waiting). Question in `description`, `metadata.asker`; answer as a comment and `metadata.answered_by`; close with `--reason answered`. `bd close <attempt>` is refused while the escalation is open ("1 open child issue(s)"). | `wf1-oj9.5.1.1` |
| Gate | `bd gate create --blocks <id>` works, but it adds a `blocks` dependency to the target, so it **mutates a Locked Job's Dependencies**. Do not use gates for Jobs. | `gate create` on a scratch copy: `["parent-child","blocks"]`, Job disappeared from ready |
| **Metrics** | flat metadata keys `metrics_duration_s, metrics_turns, metrics_tokens_in, metrics_tokens_out, metrics_cost_usd`. `--set-metadata k=v` infers number/bool types (`"metrics_turns":"number"`). A JSON-looking value stays a **string**, so do not nest. | `jq 'map_values(type)'` |
| Append-only | cpd convention. In beads: Dolt history + `events` table (F6). | F5/F6 |
| Query | `bd list --all -n 0 -t job --parent <wf> --metadata-field tier=S3`; `bd children <job>` | worked |

Other validation: `bd update -s bogus` is rejected (status validated against built-ins + `status.custom`). Metadata keys and values are not validated. `--set-metadata` silently overwrites an existing key (`metrics_turns` 14 → 15).

### F3. Ready semantics (5-Job graph, real run)

Graph: A; B,C ← A; D ← B,C; E independent; all created as `draft`, deps added, then `plan lock` (`printf 'update <id> status=queued\n' … | bd batch` = one transaction, then `phase=dispatch`). Command used for every step: `bd ready --json -t job` (without `-t job` the Workflow issue itself is returned as ready).

```
all draft:                 []
step0 all queued:          [A, E]       blocked: D←[B,C], B←[A], C←[A]
A running / in_review:     [E]
step1 A merged (closed):   [B, C, E]
step2 B merged (closed):   [C, E]       blocked: D←[C]
step3 C cancelled:         [E]          blocked: D←[C]    ← D NOT ready
   bd ready --explain: "blocked_by":[{"id":"wf1-oj9.3","status":"cancelled","title":"Job C"}]
step4 F appended (draft → dep on B → queued): [E, F]    D still blocked
```

C's cancelled representation: `{"status":"cancelled","closed_at":null,"metadata":{"cancel_reason":"descoped by Lead","cancelled_at":"…"}}`.

**Trap found.** `bd close <C> --reason sneaky` on the cancelled Job succeeded and D became ready (`[D,E,F]`). I repaired it with `bd reopen` (which resets the status to `open`, losing `cancelled`) plus `bd update -s cancelled`. So `cpd` must check the current status before any `bd close`. If a Lead wants D to proceed without C, the allowed path is to cancel D and append a replacement Job (what step 4 does with F), or a `cpd`-level "supersede" op.

`bd list` hides `frozen` and `done`-category jobs by default, and defaults to `-n 50`; `bd ready` defaults to `-n 100`. Always pass `--all -n 0`.

### F4. Attempts (Job B: S1 rejected, S2 rejected, S3 approved)

Per attempt: `bd create "B attempt N" -t attempt --parent <B> -s in_progress --metadata '{"number":N,"tier":"S?","worktree":…,"branch":…,"started_at":…}'`; at submit `bd update -s in_review --set-metadata submitted_at=…`; verdict `bd comments add <attempt> "<feedback>"`; finish `bd update --set-metadata outcome=… ended_at=… retry_cause=… metrics_*=…` then `bd close <attempt> --reason <outcome>`. Read back, in order (`bd children $B --json | jq 'sort_by(.metadata.number)'`):

```
{"id":"wf1-oj9.2.1","n":1,"tier":"S1","status":"closed","outcome":"rejected","cause":"review_rejected","close_reason":"rejected"}
{"id":"wf1-oj9.2.2","n":2,"tier":"S2","status":"closed","outcome":"rejected","cause":"review_rejected","close_reason":"rejected"}
{"id":"wf1-oj9.2.3","n":3,"tier":"S3","status":"closed","outcome":"approved","cause":null,"close_reason":"approved"}
comments: .2.2 "REJECTED: still failing npm test -- B (2 failures in B.spec.ts)."  .2.3 "APPROVED: all acceptance commands pass; …"  (author cpd, created_at present)
```

Provenance on `.2.3`: `cut`(ref `cpd/b-a3`, branch, payload with tier and worktree), `commit` and `land` (40-hex SHA), `used` (ref-less, requires `--at`). `bd provenance log <attempt> --json`, `bd provenance by-ref <sha>` work.

Honest limits:
- "Append-only" here means `cpd` writes each metadata key at most once. Beads allows overwriting any key (`--set-metadata` overwrites silently), so the attempt row itself is mutable.
- Status transitions rewrite the row, but history is in `bd history` (F6).
- Provenance rows are the only structurally append-only construct. After `bd prune --pattern '*'`, `bd provenance log` still returned 2 of the 4 rows for a pruned Attempt (not investigated why).
- Closing the Job (`closed`/merged) while any Attempt or Escalation is still open is refused (`cannot close …: 1 open child issue(s)`). That is a free invariant.
- Moving a Job from blocked to `running` is **not** refused (`bd update $D -s running` succeeded on a blocked Job). Only `closed` is guarded.

Escalation (E attempt 1): `bd create "Escalation: …" -t escalation --parent <attempt> -d "<question>" --metadata '{"asker":…}'` → answer `bd comments add` + `--set-metadata answered_by=lead:user answered_at=…` → `bd close --reason answered`. `bd list -t escalation` shows only unanswered ones.

### F5. Locked: what beads allows

Job X was `queued` (Locked) with a dependency on E. All succeeded (exit 0), none refused:

```
bd update X -d "TAMPERED goal"                    # description rewritten
bd update X --set-metadata tier=O5                # Tier rewritten
bd update X --metadata '{...}'                    # replaces the WHOLE metadata object (touches/acceptance emptied)
bd dep remove X E ; bd dep add X F                # Dependencies changed
bd label add X tampered ; bd update X --title renamed -p 0
EDITOR=… bd edit X --description                  # works non-interactively with a scripted $EDITOR
bd close <cancelled C>                            # un-cancels, unblocks D (F3)
bd update <closed A> -d "edit after close"        # closed issues remain editable
bd update <pinned> -d …                           # pinned (frozen category) is not protected either
```

Destructive commands, on throwaway copies of the ledger:
- `bd delete <job> --force` deleted a Job. Deleting the **Workflow** succeeded and left its children behind (9 issues remained).
- `bd prune --pattern '*' --force` deleted 5–6 closed Jobs/Attempts, their dependencies and 27–30 audit events ("Events removed: 27").
- `bd compact --days 0` refuses without `--force` ("would squash 145 old commits into 1").
- `bd flatten --force`: "Flattened 145 commits → 1". After it, `bd history <id> --events` returned 0 rows for every issue: history is lost.
- `bd gc` (default): "DECAY — delete closed issues older than 90 days", then Dolt compaction. `bd admin compact` is lossy semantic summarisation ("original content is discarded"). Both are help-text reads; `gc`/`admin compact` not executed. Nothing runs them automatically as far as I saw (unverified).
- `bd sql` is unsupported in embedded mode (`'bd sql' is not yet supported in embedded mode`). The `dolt` CLI can still write directly to `.beads/embeddeddolt/<db>` (not tested for writes).

Protection features tested:

| feature | result |
|---|---|
| `MIGRATION-FREEZE` marker (`configuration.md` ~L523–545) | In `ledger/wf1/`: every write, even `comments add`, exits **14** with "workspace is frozen for migration"; reads still work. Workspace-wide only: no per-Job or per-Workflow scope, and it also blocks `cpd`'s own legitimate writes. Markers directly under `/tmp` are ignored (sticky dir), and `/tmp/cpd-trials/...` is fine. Only usable to **archive a finished Workflow**: touch the marker after PR merged, delete it to write. `BD_MIGRATION_FREEZE_FILE=<path>` works too, with payload `actor⇥timestamp⇥reason` echoed in the refusal. |
| `--readonly` / `BD_READONLY=true` | blocks writes (`operation 'update' is not allowed in read-only mode`, exit 1), reads work. Good for any non-`cpd` consumer of the Ledger. Does not distinguish Jobs. |
| `--if-status`, `--if-assignee` | per-call compare-and-swap, not a lock |
| `bd close`/`update -s closed` guards | refuse closing a blocked Job (`cannot close blocked issue … (use --force)`) and a Job with open children; `--force` bypasses. They protect **Merged** only |
| statuses `pinned`, `frozen` category | no edit protection |
| `bd lint`, `validation.*` config | template checks on description sections, not immutability |

**Conclusion: `cpd` must enforce Locked itself:**
1. Single writer: only `cpd` calls `bd`; Workers and the Reviewer get no Ledger write access (`BD_READONLY=true` if they need reads at all).
2. At `plan lock`, compute `sha256` over a canonical JSON of each Job's `{description, metadata spec fields, tier, sorted blocks-deps}` and store it **outside beads** (a file in the cpd state dir or a git commit on the Lead Branch). Optionally also `metadata.lock_sha256` in beads. Before `attempt start` and `job merged`, recompute from `bd show` and fail loud on mismatch (detects a hand-run `bd update`).
3. `cpd` never issues `update` on description, spec metadata or `dep` for a Job whose status ≠ `draft`; never `delete`, `prune`, `flatten`, `gc`, `compact`, `admin compact`, `reopen`; never `close` unless status is `in_review`.
4. Audit: `bd history <id> --events --json` records `actor` for each change. Run `cpd` with `BEADS_ACTOR=cpd` and flag any event whose actor is not `cpd` (a human `bd update` uses `git user.name` or `$USER`). This detects, not prevents. Dolt commit author is not the actor (always `beads`/`root`).
5. Optionally `touch MIGRATION-FREEZE` in a completed Workflow's ledger dir.

### F6. History and audit

`bd history <id>` (text) prints Dolt commit snapshots with only title/priority/status, so edits to the description are not visible there. Use JSON.
- `bd history <id> --json` returns an array of `{CommitHash, CommitDate, Committer, Issue{…full issue incl. description, metadata}}` (capitalised keys). Job X shows 17 snapshots: `"original goal"` (draft), then `"TAMPERED goal"`, then `"EDITED-BY-EDITOR goal"`. **Old values are recoverable.** Committer is `beads` or `root`, not the actor.
- `bd history <id> --events --json` returns audit events with `actor` (`cpd`), `event_type` (`updated`, `label_added`, `dependency_added`, `dependency_removed`, …), `old_value` (full prior issue JSON incl. description and metadata) and `new_value` (the changed fields). This preserves old values and the actor. It is deleted by `bd prune` (events removed) and lost by `bd flatten`.
- `bd show <id> --json` shows current state only, and `"comments_omitted": true`: comments need `bd comments <id> --json`.
- Standalone `dolt` works read-only against the store: `cd .beads/embeddeddolt/wf1 && dolt log --oneline` (157 commits; **one Dolt commit per `bd` write**, message `bd: update <id>`) and `dolt sql -r json -q "select id,description,json_extract(metadata,'$.tier') from dolt_history_issues where id='…' order by commit_date"` reproduces all old values.
- `bd vc status` works. `bd diff HEAD~3 HEAD` failed (`invalid ref format: HEAD~3`); use commit hashes.
- `bd events` needs `bd config set events-journal true` (default retention 7 days / 100k rows). Not enabled or tested. `bd audit` is a JSONL for agent interactions (disabled by default; not tested).
- Timestamp quirk: `bd provenance` `created_at` is local time with a `Z` suffix (`18:31:31Z` when UTC was 22:31), while other timestamps are real UTC. `cpd` should supply its own `--at`/metadata timestamps and not trust provenance `created_at`.

### F7. Concurrency (should)

All with embedded Dolt, no server, in a copy of the ledger.
- 10 rounds × 2 simultaneous `bd create` (`&` + `wait`): 20/20 exit 0, empty stderr, 33 issues = 13+20 expected, no duplicate ids.
- 10 rounds × 2 simultaneous `bd update <same issue> --set-metadata kNa/kNb`: all rc=0, 0 missing keys out of 20 (no lost update). A round of two takes 0.86–1.27 s vs ~0.4 s alone, so bd queues rather than errors.
- 10 CAS races (`--if-status queued -s running --set-metadata winner=x|y` fired together): every round returned rc `[0,13]`, exactly one winner.
- Not tested: many more than 2 writers, long-running ops (`flatten`, `gc`) concurrent with writes. Docs mention `BEADS_GATE_WAIT_TIMEOUT` (default 30 s) for maintenance gates.
- **Safe pattern:** no external retry needed for single commands. Use `--if-status` for state transitions. For read-check-write sequences (e.g. `attempt start` = check ready, then CAS), take one advisory lock per Workflow (`flock`/lockfile in the cpd state dir) so cpd instances do not interleave. Treat exit 13 as "someone else won", and exit 14 as "workspace frozen".

### F8. Latency (should)

20 commands in a copy of the wf1 ledger (~200 Dolt commits), wall time including process start:

```
create 0.57 0.55 0.58 | dep add 0.63 0.63 | update status 0.42 | update metadata 0.35 | comments add 0.39
ready 0.24 0.22 | show 0.37 0.40 | list 0.40 0.39 | blocked 0.14 | children 0.42 | history 0.22 | history --events 0.20
close 0.48 | provenance record 0.24
median 0.40 s · worst 0.63 s (dep add) · best 0.14 s · reads median 0.38 s · writes median 0.51 s
```

`ledger/wf1` init takes 4.5 s once. `bd --version` is 0.03 s.

### F9. JSON stability

- Every tested `--json` command printed one valid JSON document to stdout with empty stderr (when cwd = ledger repo). Types: `ready`, `show`, `list`, `history`, `history --events`, `blocked`, `children`, `dep list`, `comments`, `update`, `query` → **array** (even for one id); `create` (`--json`), `count`, `statuses`, `provenance record` → object; `ready --explain` → object `{blocked:[…]}`.
- Errors with `--json`: exit code 1, a JSON object `{"error","hint","schema_version"}` on **stdout** and human text on stderr. `cpd` must check the exit code first, then parse. Exit codes: 13 = `--if-*` mismatch, 14 = freeze.
- Noise: when cwd is not a git repo with `beads.role`, stderr gets a 3-line `warning: beads.role not configured`. `bd metrics` notices print on first run only. `--quiet` suppresses success lines. `create --silent` prints only the id.
- Fields `cpd` should parse (keys observed): issue = `id, title, description, status, issue_type, priority, parent, metadata{…}, created_at, updated_at, closed_at, close_reason, dependencies[{id,status,dependency_type,…}], dependency_count, dependent_count, comment_count, labels` (`labels` only appears in `list`/`show` when non-empty). `blocked` rows add `blocked_by[]` and `blocked_by_count`. `history` rows: `CommitHash, CommitDate, Committer, Issue`. Events: `id, issue_id, event_type, actor, old_value, new_value, comment, created_at`. Comments: `id, issue_id, author, text, created_at`.
- `bd schema` prints the JSON Schema for `issue` and `dependency` (generated from the Go structs) → generate the Zod models from it and pin `bd` version 1.3.1. `schema_version: 1` appears in error/provenance/comment objects.
- Defaults to override: `list -n 50`, `ready -n 100` → pass `-n 0`.

## Recipes

Environment for **every** `bd` call from `cpd`:

```sh
HOME=<cpd-state>/home            # sandbox only if cpd wants to keep bd's global files out of the real HOME; otherwise real HOME is fine
BEADS_DIR=<ledger-root>/<wf-id>/.beads   BEADS_ACTOR=cpd
BD_DISABLE_METRICS=1 DO_NOT_TRACK=1 BD_NON_INTERACTIVE=1
# spawn with cwd=<ledger-root>/<wf-id>, always `--json` (or --silent/--quiet) ; check exit code first
```

Init (idempotent): `mkdir -p L && cd L && env -u BEADS_DIR bd init --init-if-missing --prefix <wf> --skip-agents --skip-hooks --non-interactive && bd config set status.custom "draft:frozen,queued:active,running:wip,in_review:wip,failed:frozen,cancelled:frozen" && bd config set types.custom "workflow,job,attempt,escalation"`

`cpd` ↔ `bd` contract (`$W` = workflow id, `$J` = job id; every row is also preceded by the single-writer flock for the Workflow, and by Zod validation of inputs):

| cpd operation | bd command(s) | fields read | invariant `cpd` checks before writing |
|---|---|---|---|
| `workflow init` | init recipe above; `bd create "<ticket title>" -t workflow --silent --metadata '{"ticket":…,"repo":…,"lead_branch":…,"phase":"context","lead_session":…}'` | id | ledger dir doesn't already hold a workflow (`bd list -t workflow --all` is empty); repo path exists; product repo untouched |
| phase change | `bd update $W --set-metadata phase=plan\|dispatch\|pr` | `metadata.phase` | legal transition only (context→plan→dispatch→pr, no going back) |
| `job add` (draft) | `bd create "<title>" -t job --parent $W -s draft --silent -d "<goal>" --metadata '{"key":…,"tier":"S1","context":…,"touches":[…],"acceptance":[…],"out_of_scope":[…]}'` then `bd dep add $J $DEP` per dependency | new id | Spec passes schema; Tier is valid (`O1–O5, S0–S5, H0–H3`); each dep exists, is not `cancelled`/`failed`, and adds no cycle (bd also checks); Workflow phase is `plan`, or `dispatch` for an **appended** Job; Job still `draft`. After lock, only new Jobs may be added, never edits to existing ones |
| Job edit (draft only) | `bd update $J -d … --set-metadata …` / `bd dep add\|remove` | `status` | **status == draft**, else refuse (Locked) |
| `plan lock` | list `bd list --all -n 0 -t job --parent $W --json`; compute and store lock fingerprints (outside beads); `printf 'update %s status=queued\n' <ids> \| bd batch`; `bd update $W --set-metadata phase=dispatch --set-metadata dispatch_started_at=<ts>` | all job `status`, spec | all Jobs `draft`; at least one Job; DAG acyclic; every Tier valid; plan approved (plannotator) — all outside bd |
| `job ready` | `bd ready --json -t job -n 0 --parent $W` | `id, status==queued, metadata.*` | filter `status == "queued"` again client-side; `-t job` or the Workflow shows up; also `cpd` re-verifies the lock fingerprint |
| `attempt start` | `bd update $J --if-status queued -s running --quiet` (exit 13 → lost race); `bd create "$J attempt N" -t attempt --parent $J -s in_progress --silent --metadata '{"number":N,"tier":…,"worktree":…,"branch":…,"started_at":…}'`; `bd provenance record --issue $A --kind cut --source cpd --ref <branch> --ref-kind branch --payload '{…}'` | Job `status`, `bd children $J` count | Job is in the ready list (**bd does not stop a blocked Job entering `running`**); N = existing attempts + 1; lock fingerprint matches; no open attempt exists |
| `attempt submit` | `bd update $A -s in_review --set-metadata submitted_at=<ts>`; `bd update $J -s in_review`; `bd provenance record --kind commit --ref <40-hex sha> --ref-kind git-sha …` | `status` | Attempt is `in_progress`; `submitted_at` unset |
| `attempt verdict` | `bd comments add $A "<feedback>"`; `bd update $A --set-metadata outcome=approved\|rejected\|failed\|timed_out --set-metadata ended_at=<ts> [--set-metadata retry_cause=…] --set-metadata metrics_*=…`; `bd close $A --reason <outcome>`. On reject/retry: `bd update $J -s queued` (→ next attempt) or `-s failed` when retries exhausted | `metadata.outcome` | `outcome` key unset (write-once, bd would silently overwrite); Attempt is `in_review` (or `in_progress` for timed_out/failed); no open escalation child |
| `job merged` | `bd update $J --set-metadata merged_sha=<sha>`; `bd close $J --reason merged`; `bd provenance record --kind land --ref <sha> --ref-kind git-sha` | Job `status` | Job is `in_review`; last Attempt `approved`; rebase + fast-forward already succeeded; **never `close` a cancelled/failed Job** |
| `job cancel` | `bd update $J -s cancelled --set-metadata cancel_reason=… --set-metadata cancelled_at=…`; close any open attempt (`--reason failed`) first | `status` | Job is not `closed`; dependents remain blocked **by design**. To unblock, cancel the dependents and append replacement Jobs |
| `escalation ask` | `bd create "Escalation: <title>" -t escalation --parent $A -d "<question>" --silent --metadata '{"asker":"worker:<job>-<n>","asked_at":…}'` | id | Attempt is `in_progress` |
| `escalation answer` | `bd comments add $ES "<answer>"`; `bd update $ES --set-metadata answered_by=… answered_at=…`; `bd close $ES --reason answered` | `bd list -t escalation -n 0 --json` | escalation is `open`; `answered_by` unset; the notification to the Lead goes via the pi extension, not bd |
| `metrics record` | `bd update $A --set-metadata metrics_duration_s=… metrics_turns=… metrics_tokens_in=… metrics_tokens_out=… metrics_cost_usd=…` | – | keys unset (write-once); numbers only (no nested JSON) |
| `history` | `bd show $J --json`; `bd children $J --json`; `bd comments $A --json`; `bd provenance log $A --json`; `bd history $J --events --json` | all | read-only; compare event `actor` to `cpd` to flag tampering |
| archive | `touch <ledger>/MIGRATION-FREEZE` after PR merged | – | `cpd` removes it before any further write |

`jq` helpers: `bd children $J --json | jq 'sort_by(.metadata.number)'`; `bd list --all -n 0 -t job --parent $W --json | jq -r '.[]|"\(.metadata.key)\t\(.status)"'`; compare `bd history $J --events --json | jq '[.[]|select(.actor!="cpd")]'`.

## Risks and unknowns

- **Isolation incidents (benign, possibly mine).** (a) Real `~/.dolt/` was created at 18:27:09 (`config_global.json` = `{}`, `version_check.txt` = `2.4.2`, `eventsData/dolt.lock`): consistent with my `dolt version` call at that second with HOME unsandboxed (shell env did not carry over between my first calls). (b) Real `~/.config/bd/config.yaml` has mtime 18:27:57 and now reads `metrics.disabled: true`; this was likely a `bd metrics off` of mine run with the real HOME, or a parallel trial (unverified). Opting out of telemetry is the only change. The real `~/.beads/` (machine-id) and `~/.gitconfig` are unchanged (sha checked). I did not delete the real files, since I can't prove ownership. Lead: decide whether to remove `~/.dolt`.
- **Telemetry:** bd queues a `cli_command` event per call (`.evtq`) even with `metrics.disabled: true` in my run, and the endpoint is a third-party URL. Use `BD_DISABLE_METRICS=1`/`DO_NOT_TRACK=1` (verified: no new queue file). Whether queued files were ever uploaded was not observed.
- Locked, append-only and Spec validity are convention only; any process with filesystem access can `bd delete`, `bd prune`, `bd flatten`, or run `dolt` directly on `.beads/embeddeddolt`. A hand-run `bd` leaves an event with a different actor, but `flatten`/`prune` remove it.
- `bd reopen` resets status to `open`, so a custom status is lost; Dolt history keeps it.
- A closed issue is un-closable and editable; `--force` bypasses every close guard.
- Dolt commit per write: 157 commits after ~120 writes; `bd gc`/`compact` exist for long-lived stores (not run). Workflows are short, so fine; unverified at 1000+ writes.
- The beads CLI surface changes quickly (1.3.1 was the only version used). `bd schema` gives a typed contract. Pin the version and run a contract test in CI.
- Provenance `created_at` timezone bug (local time labelled `Z`), and it survived `prune` only partially (2/4 rows, unexplained).
- Not tested: `events-journal`, `bd audit`, `--dolt-auto-commit batch`, Dolt server mode, `types.custom` rejection of unknown `-t` values, `bd context` under a non-`/tmp` path, `bd gc` and `admin compact` actually executed, `bd close --force` effect on cancelled Jobs' children.

## Escalations

None. Open question for the Lead: should `cpd` also keep its own lock fingerprints and a state file outside beads (my recommendation, F5), given that beads cannot protect Locked? And may I remove the real `~/.dolt/` directory created at 18:27:09?
