# Session cost in the cockpit — design note (hs-d689)

Status: research note. It recommends nothing; it lays out the options and their costs.
Every claim below carries a citation to a file:line or a doc section. Claims I could not
run or read are labelled `unverified` or `guess:`.

Frames already fixed by the User (hs-d691), not decided here:

1. **All levels are shown**: per Session row, per Plan row (sum), per Context (total).
2. **The value is refreshed at the tree's tick.**
3. **This note is an option matrix** (each option with its cost, dependency and failure
   mode), not one recommendation, so the User chooses the build afterwards.

Method: the pi session format was read from the installed pi docs
(`~/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent/docs/`);
cpd's parser and read models were read from this repo; pi's own totals and cpd's parser
output were measured on real session files; codeburn 0.9.27 was read and run. The exact
commands and numbers are in [§9 Measurements](#9-measurements).

One premise in the brief was wrong: **codeburn does read pi sessions.** See [§7](#7-codeburn-verdict).

---

## 1. Data path

The three ways cpd could get a per-Session dollar figure. Each is an option for the whole
feature; [§8](#8-follow-up-build-plan) picks one to build first.

### 1.1 What the number actually is

pi writes JSONL session files (pi docs `session-format.md`, sections `UsageEntry`,
`CompactionEntry`, `BranchSummaryEntry`):

- an assistant `message` carries `message.usage.cost = {input, output, cacheRead,
  cacheWrite, total}` (pi docs `session-format.md` ~line 91);
- a standalone `{"type":"usage"}` entry carries the same `usage.cost` and
  "contribute[s] to session token and cost totals" (`session-format.md:113-121`, example
  at line 118; the `kind` is arbitrary, e.g. `cache_warm`);
- a `compaction` entry carries an optional `usage` "included in session token and cost
  totals" (`session-format.md:135`);
- a `branch_summary` entry carries an optional `usage`, same words
  (`session-format.md:160`).

pi's own session-stats RPC returns `cost` and says it "include[s] assistant messages,
usage reported by tools, and compaction/branch-summary generation across the full
session" (pi docs `rpc-commands.md:526-565`, the note at line 565). So the authoritative
pi number is **assistant messages + standalone `usage` + `compaction.usage` +
`branch_summary.usage`**.

cpd's parser does **not** use the RPC. No file under `core/src` or `pi/` calls
`get_session_stats` (grep of `.rs`/`.ts`, no match); the only `--mode rpc` use is the
extension-load probe in `pi/lib/reload.ts:92-102` (`unverified`: a future adapter could
add it). cpd instead parses the file itself.

### 1.2 cpd's own parser (already in the repo)

`core/src/metrics/mod.rs` parses the file named by a Session's `file`
(`core/src/host/pi/session_file.rs:12`, contract §6.5) into the Attempt `metrics`
`{turns, tokens_in, tokens_out, cache_read, cache_write, cost_usd, duration_s, model,
thinking_level, stop_reason}` (contract §2.2 line 400):

- it sums `usage.cost.total` for every **assistant `message`** entry and nothing else
  (`core/src/metrics/mod.rs:173-181`);
- it skips `usage`, `compaction` and `branch_summary` entries: the match arm is
  `_ => {}` (`core/src/metrics/mod.rs:183`);
- for a fork it skips the copied parent entries by timestamp: `forked` is set when the
  header has a string `parentSession` (`core/src/metrics/mod.rs:159`), and each subsequent
  entry with `at < first` (its RFC 3339 timestamp earlier than the fork header's) sets
  `inherited = true` (`core/src/metrics/mod.rs:163`); the `message` arm is guarded by
  `!inherited` (`core/src/metrics/mod.rs:173`), and `last`/`duration_s` ignore inherited
  entries (`core/src/metrics/mod.rs:164-166`). `model_change` and
  `thinking_level_change` are **not** guarded, so a fork keeps its parent's starting model
  and thinking level (the module comment, `core/src/metrics/mod.rs:9-14`).

The measured gaps this creates are in [§9.2](#92-fork-and-compaction-deltas): the parser
under-counts pi's own total by every `compaction`/`branch_summary`/standalone `usage`
entry, but it is the only path already in the repo and the only one that is fork-safe.

### 1.3 Where cpd stores cost today

The parsed value is stored only under `attempts.metrics`, written in the transaction that
**ends** an Attempt (`core/src/driver/worker.rs:440` `metrics()`, called from
`core/src/driver/run.rs:673,708,789`; contract A9 line 815). `sessions` has no cost column
and `SESSION_SELECT` (the tree's query) selects no cost (`tui/src/read/shared.rs:267`).
Lead and Manager Sessions are not Attempts, so their cost is stored **nowhere** today
(verified: schema column `attempts.metrics` is the only cost field, contract §2.2 line 400;
`cpd session show` on this Session prints `file` and `attempt` and no cost — exact output in
[§9.1](#91-session-show-fields)).

### 1.4 The matrix

| Option | Cost | Dependency | Failure mode |
|---|---|---|---|
| **(a) cpd parses the pi session file itself** | Parser exists (`core/src/metrics/mod.rs:109`); needs a read-model join + per-tick file reads (worst measured 30.6 ms for a 4.5 k-entry file, [§9.3](#93-read-cost-of-one-parse)); no new dep | pi's file format (`session-format.md`) and the `Session.file` path (`session_file.rs:12`); no external process | format drift (unknown entry kinds; the parser already rejects any line it cannot deserialise, `core/src/metrics/mod.rs:117-135`); current parse **omits** compaction/branch/cache-warm cost (`:183`); a file mid-write can be malformed (the parser's own error path) |
| **(b) cpd reads codeburn's store/export** | One subprocess or file read; codeburn parses **all** pi sessions in ~2.16 s on this machine ([§9.4](#94-codeburn-read-cost)); needs codeburn installed and its cache fresh | codeburn 0.9.27 (`~/.local/lib/node_modules/codeburn`), its `~/.cache/codeburn` store, and its `sessions --format json` shape; no cpd dep in the repo today (grep `codeburn`, no match) | codeburn not installed / different version → no number; codeburn **counts inherited fork entries** (measured, [§9.2](#92-fork-and-compaction-deltas)) → double counting; codeburn **omits compaction/branch-summary** (parser `if (entry.type !== "message") continue;`, `parse-worker.js:14174`) → under-count; cache staleness |
| **(c) reuse only codeburn's pricing catalog, compute cost in cpd** | Needs a copy of / a call into `~/.cache/codeburn/litellm-pricing.json` (1.4 MB) and codeburn's `calculateCost` (`parse-worker.js:832`); cpd already has the tokens | codeburn's pricing file and its model-alias / price-override state; pi already reports a cost, so this duplicates it | catalog drift vs pi's reported `cost.total`; missing/unknown model → codeburn falls back to its own table (`parse-worker.js:14217-14218`); cpd would silently disagree with pi and with the stored `attempts.metrics` |

Recommendation folded into [§6](#6-source-of-truth) and [§7](#7-codeburn-verdict): (a), with
the parser gap closed, is the smallest correct path; (b) is disqualified by the fork double
count; (c) attacks a problem cpd does not have (pi already prices).

---

## 2. Levels

How each fixed level (hs-d691) is summed, and where its data comes from. "Own cost" below
always means the fork-excluded figure (cpd's parser, `core/src/metrics/mod.rs:163`).

| Level | Summed how | Cost | Dependency | Failure mode |
|---|---|---|---|---|
| **Per Session** | that file's own cost; for a Worker Attempt, `attempts.metrics.cost_usd` once ended (`core/src/driver/worker.rs:440`; contract §2.2 line 400) | one parse per Session per read (0.6–30.6 ms, [§9.3](#93-read-cost-of-one-parse)) | `Session.file` exists (`session_file.rs:12`); live leads/managers have no stored metrics, so a parse is required | file missing/unreadable → no value (not zero: distinguish "unknown" from "$0.00"); mid-write malformed file |
| **Per Plan** | sum of the Plan's Sessions' own costs: the Lead plus its side forks (`tui/src/read/tree.rs:80-86` `ContextNode::forks`, `PlanNode.sessions` `read/tree.rs:59-63`) | N parses, N = the Plan's Sessions; pure addition | the same as per Session; **must** use own cost, not the file total | a fork file's copied rows double-count if the sum uses file totals (cpd's parser already excludes them, `metrics/mod.rs:163`); a Session with no `file` (`cpd session list` shows `file: null` for some, e.g. id `30b263f6…`) contributes nothing |
| **Per Context** | sum over its Plans, **plus** its live Manager Session (the Manager is Context-level: `read/tree.rs:53-57, 203-210`); archived Manager Sessions are listed separately (`archived_managers`, `read/tree.rs:48-49`) | N parses over the whole Context | the same; a Context row presently has only a dot and id (`tui/src/tree/rows.rs:336-340`), so a new field is needed | double counting a fork under both a Plan and the Context if the fork is added at both levels; an archived Session's cost if archived rows are summed |
| **Per Job / Attempt** | sum of that Job's `attempts.metrics.cost_usd` (`tui/src/read/flow.rs:212-223` `AttemptNode` has no cost field today) | one JSON read per Attempt already in the store; no file I/O | `attempts.metrics` is written only when the Attempt ends (contract A9 line 815) | a **live** Attempt shows nothing until it ends; retries sum their Attempts; a cancelled/never-started Attempt has no metrics |

Two Session roles are not Attempts and so have no stored cost: `lead` and `manager`
(contract §2.2; only `attempts.worker_session` links a worker Session, `SESSION_SELECT`
selects `s.attempt`). For per-Plan and per-Context levels the number therefore **must**
come from parsing the file, not from `attempts.metrics`. This is the strongest argument for
option (a) in [§1.4](#14-the-matrix).

Forks: a fork is a Session drawn under its Context (`read/tree.rs:76-83`), and its file
copies the source's entries (pi `SessionManager.forkFrom`, quoted in
`core/src/metrics/mod.rs:9-13`). The measured fork delta is in
[§9.2](#92-fork-and-compaction-deltas): cpd's own parse excludes the copied rows; a naive
file total (or codeburn) includes them and double-counts.

---

## 3. Render spot

Four concrete surfaces. Mock-ups use the cockpit's real glyphs from the palette
(`tui/src/palette.rs`; separators are `⋅`, `tui/src/tree/rows.rs:278-291`).

### 3.1 The tree row

The tree is the natural home: per Session row is fixed by hs-d691, and a Plan row already
draws right-aligned Job counters. Mock of a live tree (Context → Plan → Sessions), with the
proposed cost at the far right of each Plan row and each Session row:

```text
╭ 󰋙 building-cpd                                  ← Context (id + dot; no numbers today)
├─ ● cost-spike                      $0.05         ← Plan row: Lead's id + counters + cost
├─ ● cost-spike cost-docs a1         $0.02         ← Attempt Worker Session row
╰󰬬 collapse
   ╰󰬪 (nothing)                                   ← a fork would be another ├─ row here
```

Today `Plan`/`Fork`/`Manager` rows are one `Line` of `dot + name`
(`tui/src/tree/rows.rs:320-328` `row_text`, `:342` `plan_row`), with the Plan's five Job
counters drawn right-aligned afterwards (`tui/src/tree/rows.rs:493-538`, `counters` at
`:278`, `line.right_aligned()` at `:538`).
The row is truncated so the name never reaches the counters (`:325`). A cost would either
be appended to the counters line (`$0.05` after `2⋅2⋅5`) or be a second right-aligned
column; the widget itself "has no right-aligned column"
(`tui/src/tree/rows.rs:4-6`), so either way cpd draws it into the row's `Rect` as the
counters already do.

### 3.2 The Plan view

The centre-top pane is one box per PR, title on the top border
(`tui/src/plan/boxes.rs:1-9`, `title` at `:130`, `layout` at `:179`). The PR title is the
cheapest place for a Plan total:

```text
╭─ owner/repo → main · cost-spike · $1.234 ──────╮
│   ● job-impl   ● job-tests   ● job-open        │
╰────────────────────────────────────────────────╯
```

`AttemptNode` (`tui/src/read/flow.rs:212`) carries no cost today, so the Plan view would
read it from the new field. The per-Job/pill spot is also possible (`flow` pills,
`tui/src/plan/boxes.rs:1-9`).

### 3.3 A status-line pill

The status line is tmux's `STATUS_FORMAT` (`core/src/host/tmux.rs:319`) with a workers pill
(`@cpd_workers` / `@cpd_workers_short`) and an incident pill
(`@cpd_incident`); the engine sets those options **each tick** and the status line cannot
read the store (contract §6.1.1 line 2411: "the status line cannot read the store", "the
value is up to one tick late, and stale while the engine is stopped"). The same mechanism
gives a machine total:

```text
─ [workers 2/6 · 3 waiting · 1 queued] ─ [cost $12.34] ────────────────
```

`@cpd_workers` is set only when the value changed (contract §6.1.1 line 2411;
`set -g @cpd_workers '<text>'`). A cost pill would need a new `@cpd_cost` set the same way,
read by `STATUS_FORMAT` and sized like the incident pill
(`core/src/host/tmux.rs:236-247, 319-336`).

### 3.4 The legend

The legend already documents every colour, dot and counter
(`tui/src/widgets/legend.rs:1-9`, `sections()` at `:107`), including the counters order
"planned ⋅ active ⋅ failed ⋅ done" (`legend.rs:150-166`). A cost needs one row so its
glyph and rounding are defined:

```text
Costs → $0.12 per Session row · $1.2k per Plan/Context · $0.001 floor
```

The Pulse legend is a separate list (`tui/src/plan/pulse.rs:103-119`); it would only need a
row if cost is drawn in Pulse.

### 3.5 The matrix

| Render spot | Cost | Dependency | Failure mode |
|---|---|---|---|
| **Tree Session/Plan row** | one `Line` addition per row; reuses the counters' right-aligned `Rect` draw (`tui/src/tree/rows.rs:493-538`) | the read model carries the value (new field on `SessionNode`/`PlanNode`, `tui/src/read/shared.rs:220`, `read/tree.rs:59`) | a long name + counters + cost overflows the row; the existing truncation (`rows.rs:325`) must reserve the cost width too, or the number is cut |
| **Plan view title/box** | one `Span` on the title (`tui/src/plan/boxes.rs:130`) | `AttemptNode`/`PlanBoxes` need a cost field (`tui/src/read/flow.rs:212`) | a Plan total that quietly excludes live Attempts (they have no metrics yet) reads as $0.00 and misleads |
| **Status-line pill** | a new `@cpd_cost` option + a `STATUS_FORMAT` range/size (`core/src/host/tmux.rs:319-336`); engine sets it each tick | the engine can compute the machine total; the pill **cannot** read the store (contract §6.1.1 line 2411) | stale while the engine is stopped (accepted for `@cpd_workers`, same cost); one more tmux global to clear on a server restart |
| **Legend** | one static row (`tui/src/widgets/legend.rs:107`) | none | none (display only) |

---

## 4. Format

Exact glyphs and rounding. pi reports a float (`usage.cost.total`, e.g.
`3.7978488` measured). Rules the options must pick:

- **currency glyph**: the User's chosen display currency is codeburn's (`codeburn currency`,
  `codeburn --help`); cpd has no currency concept today. `$` is assumed here
  (`guess:` the User keeps USD).
- **sub-cent**: an Attempt can cost `$0.0006` (`guess:` a short tier-H0 run); under a cent
  it must not read as `$0.00`. Either a floor of `<1¢` or three decimals.
- **thousands**: a Context can exceed `$1000` (the measured `04598f9d` file alone is
  `$502.29`, [§9.2](#92-fork-and-compaction-deltas)); `$1.2k` keeps the row narrow.
- **per day vs per month**: hs-d691 fixes *per Session / per Plan / per Context*, not a
  time period. A day/month figure is a separate aggregate and is **not** one of these
  levels.

| Format option | Cost | Dependency | Failure mode |
|---|---|---|---|
| `$0.123` (3 decimals, `$` prefix, always) | trivial; one `format!` | the raw f64 | a `$502.293` row is 8 cells wide; `$0.001` for a `$0.0006` run overstates by ~40% |
| `$0.12` (2 decimals) + floor `<1¢` under a cent | trivial; one branch | the raw f64 | `$0.004` and `$0.0006` both read `<1¢`; no ranking among cheap rows |
| `$0.12` (2), `$1.2k` (1 sig-fig thousands), `$1.2M` | one `format_quantity` helper; must be shared by tree and status pill | the raw f64 | rounding hides the exact contract value; two surfaces can round the same value differently unless the helper is the one source (DRY) |
| per-day / per-month aggregate | a window over timestamps plus a period field | Event/Attempt timestamps; not in `SessionNode` today | not a level hs-d691 asked for; adds a period dimension to every row |

The 3.5 matrix's **recommended first build** (not a User decision): `$0.12` with a `<1¢`
floor for rows, and the same helper for the status pill, so the two cannot disagree.

---

## 5. Freshness

The tick the value refreshes at, and what stale means.

The tree loop reads the store on a `Schedule` (`tui/src/tree/mod.rs:194`). In production
the watcher is up, so `Schedule::new(now, pump::SAFETY)` uses `SAFETY = 2 s`
(`tui/src/pump.rs:19`). A read also happens when the store's `cpd.db`/`cpd.db-wal` is
written (the commit watcher, `tui/src/read/watch.rs:11-19`) and after a background command
(`schedule.force()`, `tui/src/tree/mod.rs:224`). `Schedule::take` gates a wanted read to at
most one per `FRAME` = 50 ms (`pump.rs:17`, `palette.rs:456`). The snapshot is built by
`read::tree_at` (`tui/src/tree/mod.rs:279`); a redraw also happens on the 50 ms spinner
frame and the 1 s Inbox-age second (`tui/src/tree/mod.rs:295-306`). So: **the tree
re-reads about every 2 s, or sooner on a store commit**; this is the "tick" hs-d691 names.
The read model's module comment states the same ("refreshes a view about every second",
`tui/src/read/mod.rs:17-19`).

A cost from a **file** parse has a second freshness question: the file is not the store, so
the commit watcher does not see it. A still-open Session's file grows between ticks and the
value must be re-parsed on each read (the parser re-reads the whole file, `parse_file`
`core/src/metrics/mod.rs:109`).

| Freshness option | Cost | Dependency | Failure mode |
|---|---|---|---|
| **Parse session files on each tree read** (2 s / on commit) | re-parses every live file each read; measured 0.6–30.6 ms per file ([§9.3](#93-read-cost-of-one-parse)) | `Session.file`; the file mtime is *not* what triggers the read, the store commit is | a long-running Session's file grows and is re-read whole; a file that changed with no store write waits up to 2 s; the value is only as fresh as the last tick |
| **Parse once at Attempt end, store in `attempts.metrics`** | free at read time (JSON already in the row) | the end transaction (contract A9 line 815) | a **live** Attempt, a Lead and a Manager show nothing for their whole life; exactly the gap [§2](#2-levels) calls out |
| **Cache parsed cost in the store, refreshed by the engine each `watch.tick_seconds`** | a migration + an engine step + a per-file mtime/size check | contract change (out of scope here, §Out of scope), engine running | the engine must run for any value; while it is stopped the value is stale (same accepted cost as `@cpd_workers`, contract §6.1.1 line 2411) |
| **Read codeburn's cache** | no cpd parse | codeburn must have run; its cache is daily (`~/.cache/codeburn/daily-cache.v74.json`) | up to a day stale, plus codeburn's fork double count ([§7](#7-codeburn-verdict)) |

---

## 6. Source of truth

Is pi's `usage.cost` the right number? **Yes, for what pi does** — but cpd must present it
with a stated boundary, because two of pi's three "totals" sources differ from what cpd's
parser records.

- pi's reported `usage.cost.total` is the provider's own price for that call (pi docs
  `session-format.md:~91`; the RPC returns the same `cost`, `rpc-commands.md:555-565`).
- pi's session total includes compaction and branch-summary generation and any
  tool-reported/standalone usage (`session-format.md:121,135,160`; `rpc-commands.md:565`).
- cpd's current parser records only the assistant-message part
  (`core/src/metrics/mod.rs:173-183`), so `attempts.metrics.cost_usd` is **lower** than
  pi's own session total by exactly the compaction/branch/standalone cost. Measured:
  `0.0337815` on the `768f6500` file (~0.9%), [§9.2](#92-fork-and-compaction-deltas).

How to present without double counting:

1. **Use the reported cost, never a re-priced one.** pi already includes cache reads/writes
   in `cost.total`; recomputing with codeburn's catalog (option (c)) can only disagree.
2. **Close the parser gap** so `cost_usd` equals pi's own total: also add
   `usage.cost.total` from standalone `usage`, `compaction.usage` and
   `branch_summary.usage` (the three arms missing at `core/src/metrics/mod.rs:183`). This
   is a one-function change and makes cpd agree with pi's RPC
   (`rpc-commands.md:565`).
3. **Exclude inherited fork entries** on every level. cpd's parser does
   (`metrics/mod.rs:159,163`); a fork's copied parent rows must never be added twice. This
   is the single guard that makes per-Session/Plan/Context summing correct.
4. **Distinguish "no data" from "$0.00".** A Session with no `file` (e.g. `30b263f6…` in
   `cpd session list`) or an unreadable file must render as unknown, not free; a legitimately
   free run (a flat-rate model) is a real `$0.00`.

`guess:` with those four rules the tree can show a per-Session number equal to pi's own
footer total for that Session; that equality is a good acceptance test
([§8](#8-follow-up-build-plan)).

---

## 7. codeburn verdict

**The brief's premise is wrong: codeburn does read pi sessions.** codeburn 0.9.27 is
installed at `~/.local/lib/node_modules/codeburn` (bin `~/.local/bin/codeburn` →
`dist/cli.js`; `codeburn --version` → `0.9.27`, Node v22.23.2 ≥ 22.13). Its worker
contains a pi provider:

- `getPiSessionsDir` defaults to `~/.pi/agent/sessions`
  (`dist/parse-worker.js:14050-14052`);
- `createPiProvider` registers it under the name `pi`
  (`dist/parse-worker.js:14252-14299`), and `createParser19` is its parser
  (`dist/parse-worker.js:14135`);
- the provider is wired into the registry at `dist/parse-worker.js:14325`.

What `createParser19` reads, precisely (`dist/parse-worker.js:14135-14250`):

- **reads** `session` (header), `model_change`, `model_usage`, and `message` entries; every
  other entry type is skipped by `if (entry.type !== "message") continue;`
  (`dist/parse-worker.js:14174`);
- for an assistant `message` it takes `msg.usage`, skips it when
  `input === 0 && output === 0` (`dist/parse-worker.js:14193`), prefers the reported
  `msg.usage.cost.total` when it is a finite non-zero number
  (`reportedCost2`/`isReported`, `dist/parse-worker.js:14216-14217`), and otherwise falls
  back to its own `calculateCost` pricing table (`dist/parse-worker.js:832`, called at
  `:14218`);
- it does **not** read standalone `usage`, `compaction` or `branch_summary` entries, so it
  **undercounts** against pi's own total by those amounts (measured 0.0337815 on
  `768f6500`, [§9.2](#92-fork-and-compaction-deltas));
- it does **not** check the header's `parentSession` or entry timestamps, so it **counts a
  fork's copied parent rows** (measured: `de18371d` reported `15.723482` where cpd's own
  parse is `13.926127`; difference `1.797355`, [§9.2](#92-fork-and-compaction-deltas));
- `model_change` sets `resolvedModel` from `entry.model`
  (`dist/parse-worker.js:14165-14166`), but pi's `model_change` entry carries `modelId`,
  not `model` (pi `session-format.md` section `ModelChangeEntry`; a real entry:
  `"modelId":"deepseek/deepseek-v4.1-flash"`), so that read is dead for pi `unverified` as
  a bug — cost still comes out right because the assistant message itself carries `model`
  and the reported `cost.total` is used.

`codeburn --help` commands that matter here: `report`, `overview`, `export --format json`,
`status`, `budget`, `price-override`, `model-alias`, `model-flat-rate`, `context
[session]`, `sessions`. The local store is `~/.cache/codeburn` (`daily-cache.v74.json`,
`session-cache.v10`, `litellm-pricing.json`; `ls ~/.cache/codeburn`). A per-pi-session
total usable by cpd **does** exist:

```console
$ codeburn sessions --provider pi --period all --format json
[ { "sessionId": "768f6500-…", "provider": "pi", "project": "clayton",
    "cost": 3.7640673, "calls": 95, "turns": 95,
    "startedAt": "2026-10-25T00:26:23.154Z", "endedAt": "2026-10-25T13:06:25.919Z" }, … ]
```

`sessionId` is pi's header `id` (`sessionId = entry.id ?? sessionId`,
`dist/parse-worker.js:14166`), which is cpd's `sessions.id`, so it is joinable. But note
the default `sessions --period` is `30days` (`codeburn sessions --help`) and the period is
**local-time**: `--period today` returned only `0.531038` for `768f6500` because its first
calls fall on the previous local day — `--period all` returns the full `3.764067`. A cpd
integration must pass `--period all` or an explicit `--from/--to`, or it will silently
show a partial session.

**Verdict: do not read codeburn for the value; ignore it for display.** Reasons, all
measured or cited above: (1) it double-counts forks, which the per-Plan/Context sums
require to be exact; (2) it undercounts pi's own total by compaction/branch cost; (3) it
adds a dependency (version, cache, install) cpd does not have (grep `codeburn` in the repo:
no match) for a number cpd can already read from the file pi wrote. **Reusing its pricing
catalog** (option (c)) is also not worth it: pi already reports a priced `cost.total`
(`session-format.md:~91`), so cpd would be replacing an authoritative number with a local
estimate. The one thing codeburn is useful for here is as a **cross-check** in this note's
measurements, which is how it is used.

---

## 8. Follow-up build plan

**Which option**: data path **(a)** — cpd parses the pi session file itself
([§1.4](#14-the-matrix)) — refreshed at the tree tick (freshness option 1, [§5](#5-freshness)).
It is the only path that is fork-safe and has no new dependency, and it is already the
shape of `core/src/metrics/mod.rs`.

**The minimal change set** (a draft for the follow-up Plan, not implemented here):

1. **Command or field to add.** A per-Session field `cost_usd: Option<f64>` on
   `SessionNode` (`tui/src/read/shared.rs:220`) populated in `sessions()`
   (`tui/src/read/shared.rs:277`, over `SESSION_SELECT` at `:267`), plus `cost_usd` on
   `PlanNode` and `ContextNode` (sums, `tui/src/read/tree.rs:59,35`). No new CLI command is strictly needed; if the
   follow-up wants one, `cpd session show` already emits the row and could add the same
   field (its current output is in [§9.1](#91-session-show-fields)).
2. **Close the parser gap first.** Add the standalone `usage`, `compaction.usage` and
   `branch_summary.usage` arms to `core/src/metrics/mod.rs:183` so `cost_usd` matches pi's
   own total (`rpc-commands.md:565`). This is inside the existing `metrics` module, no
   migration.
3. **View and files to change.** Tree row rendering in `tui/src/tree/rows.rs` (append the
   formatted cost to the counters line, `counters` at `:278`, draw at `:493-538`, reserve
   its width in `row_text`'s truncation at `:323`); the read model in
   `tui/src/read/shared.rs`/`read/tree.rs` computes the per-Session values and the Plan and
   Context sums. A shared format helper (the "make it work, make it fast, make it good"
   order) used by the row and, later, the status-line pill
   (`core/src/host/tmux.rs:319-336`). The Plan view (`tui/src/plan/boxes.rs:130`) and the
   legend (`tui/src/widgets/legend.rs:107`) are the next two spots, in that order.
4. **Tests that would pin it** (red first, per the repo's test rules):
   - a unit test in the `metrics` module that a file with a `compaction`/`usage`/
     `branch_summary` entry sums all three (fails today: `core/src/metrics/mod.rs:183`);
   - a `tui/src/read/` test that a fork's `SessionNode.cost_usd` excludes the copied parent
     rows (mirrors [`9.2`](#92-fork-and-compaction-deltas)) and that `PlanNode`/`ContextNode`
     sums equal the sum of their Sessions' own costs, not the file totals;
   - a `tui/src/tree/rows.rs` drawing test (its existing `row_of`/`fg` helpers,
     `rows.rs:680-682,902`) that a Plan row's cost is right-aligned and never collides with
     the counters;
   - a test that a Session with no `file` renders unknown, not `$0.00`.
5. **Freshness**: compute in the existing `tree_at` read (`tui/src/tree/mod.rs:279`), which
   already runs on the 2 s `SAFETY` schedule (`pump.rs:19`); no new timer.

**Which options it implements**: data path (a); all three levels (per Session, per Plan,
per Context); the tree render spot; the 2-decimal + `<1¢` format; the tick freshness. It
deliberately defers codeburn (option ignored, [§7](#7-codeburn-verdict)), the status-line
pill, the Plan view and the store-cached variant. It touches **no** contract change and no
migration, which the Spec's Out-of-scope excludes from this note.

---

## 9. Measurements

All commands ran under `timeout`; Python 3 and `jq` are the measuring tools, the Rust
parser is this repo's `cpd_core::metrics::parse_file` compiled from the same worktree.

### 9.1 `cpd session show` fields

```console
$ timeout 30 cpd session show
{"ok":true,"session":{"archived_at":null,"asleep_at":null,"attempt":"cost-docs-a1",
 "by_user":false,"context":"building-cpd","cwd":"…/cpd.cost-spike--cost-docs-a1",
 "file":"/home/clayton/.pi/agent/sessions/--home-clayton-projects-cpd.cost-spike--cost-docs-a1--/
     2026-10-25T23-08-08-488Z_87fd1ef0-9915-4448-9f7e-bbad6b856c5c.jsonl",
 "forked_from":null,"id":"87fd1ef0-…","model":"openrouter/deepseek/deepseek-v4.1-flash",
 "name":"cost-spike cost-docs a1","role":"worker","state":"working","tier":"S3", …}}
```

Fields present: `file` and `attempt` (the link to the Attempt) but **no cost field**. This
is the raw material a cost field would extend.

### 9.2 Fork and compaction deltas

Files (paths under `~/.pi/agent/sessions`; each with its `sessions.id`):

| Session | File (suffix) | `(a)` own assistant cost | `(b)` own + standalone + compaction + branch | cpd stored/parse | codeburn `--period all` | Interpretation |
|---|---|---:|---:|---:|---:|---|
| `79a98ac9…` (cost-spike Lead) | `--home-clayton--/2026-10-25T22-46-10-932Z_79a98ac9…jsonl` | 0.04721030 | 0.04721030 | not stored (no Attempt) | 0.04721030 | live Lead; all three agree; the Lead's cost is stored nowhere (contract §2.2) |
| `e8848bc4…` (focus-tests2 Worker) | `--home-clayton-projects-cpd.cursor-idle-blink--focus-tests2-a1--/2026-10-25T23-01-29-882Z_e8848bc4…jsonl` | 0.01894386 | 0.01894386 | `attempts.metrics.cost_usd = 0.018943859999999996` | 0.018943859999999996 | cpd's parser and codeburn exactly equal pi's assistant total |
| `768f6500…` (planning-dot Lead, has a `compaction` with usage) | `--home-clayton--/2026-10-25T00-26-20-586Z_768f6500…jsonl` | 3.76406730 | **3.79784880** | not stored | 3.76406730 | codeburn omits the `compaction.usage` **0.0337815** (~0.9%); cpd's parser omits it too (`metrics/mod.rs:183`) |
| `de18371d…` (a fork) | `--home-clayton-projects-cpd--/2026-10-15T18-26-14-659Z_de18371d…jsonl` | 13.92612700 (own, post-header) | 13.92612700 | inherited rows **1.79735520** skipped by cpd | **15.72348200** | codeburn counts the 1.797355 inherited rows cpd skips → double counts if summed with the parent |
| `04598f9d…` (a large fork) | `--home-clayton-projects-cpd--/2026-10-16T13-37-41-799Z_04598f9d…jsonl` | 502.29216820 | 502.93665745 (compaction 0.64448925) | parse `cost_usd=502.2921682`, `turns=1383` | 504.94545300 | codeburn = own 502.2921682 + inherited 2.6532848, and misses compaction 0.64448925 |

Commands (all under `timeout`):

```console
# (a)+(b) per file: sum assistant usage.cost.total, and add usage/compaction/branch_summary
$ timeout 60 python3 /tmp/cb-measure.py \
    "$HOME/.pi/agent/sessions/--home-clayton--/2026-10-25T00-26-20-586Z_768f6500-9f16-4203-b7df-3da1a4306e17.jsonl"
# (c) cpd's own parse, compiled from this worktree against core/src/metrics/mod.rs
$ timeout 60 /tmp/measure_rust "$HOME/.pi/agent/sessions/--home-clayton--/2026-10-25T00-26-20-586Z_768f6500-9f16-4203-b7df-3da1a4306e17.jsonl" 50
# (d) codeburn per pi session
$ timeout 300 codeburn sessions --provider pi --period all --format json > /tmp/cb-sessions-all.json
$ timeout 60 jq -c --arg id 768f6500-9f16-4203-b7df-3da1a4306e17 '.[]|select(.sessionId==$id)' /tmp/cb-sessions-all.json
```

Fork mechanism, exactly as `core/src/metrics/mod.rs` reads it: `forked` is the header's
`parentSession` being a string (`:159`); an entry is `inherited` when `forked && at < first`
(`:163`), i.e. its own RFC 3339 timestamp is earlier than the fork header's; only the
`message` arm is guarded by `!inherited` (`:173`), so `model_change` /
`thinking_level_change` still seed `model`/`thinking_level` from the copied rows. codeburn
has no such test (`parse-worker.js:14135-14250`), which is the double count above.

`grep -rEl '"type":"usage"' ~/.pi/agent/sessions` and
`grep -rEl '"kind":"cache_warm"' ~/.pi/agent/sessions` returned **no file** on this machine
(2178 session files), so the standalone-`usage` gap is real in code but unexercised here;
the `compaction` gap **is** measured above. `branch_summary` with usage: none found.

### 9.3 Read cost of one parse

`cpd_core::metrics::parse_file` compiled `-O` from this worktree (`/tmp/measure_rust`),
best of 50 runs, all under `timeout 60`:

| File (entries) | cpd `cost_usd` | cpd `turns` | parse best |
|---|---:|---:|---:|
| `e8848bc4…` (61 entries) | 0.01894386 | 25 | **0.61 ms** |
| `79a98ac9…` (82 entries) | 0.04721030 | 31 | **0.86 ms** |
| `768f6500…` (234 entries) | 3.76406730 | 95 | **2.21 ms** |
| `04598f9d…` (4512 entries) | 502.29216820 | 1383 | **30.57 ms** |

```console
$ timeout 60 /tmp/measure_rust "<file>" 50     # prints cost_usd, turns, best_ms
```

A typical live Session is ~1–2 ms; the worst large file is ~30 ms. The tree would parse
only the Sessions it draws, once per ~2 s tick; a few dozen live Sessions is tens of
milliseconds per tick (`guess:`, since the live count varies). This is the number the
store-cached freshness option ([§5](#5-freshness)) exists to avoid.

### 9.4 codeburn read cost

```console
$ cd /tmp && /usr/bin/time -f "wall=%e s" timeout 300 codeburn sessions --provider pi --period today --format json > /dev/null
codeburn sessions all wall=2.16 s
```

2.16 s for 212 pi Sessions on this machine. Reading codeburn per tree tick would blow the
2 s tick budget; it is only viable cached.

### 9.5 codeburn source facts

```console
$ grep -n "getPiSessionsDir\|createPiProvider\|createParser19" \
    ~/.local/lib/node_modules/codeburn/dist/parse-worker.js
14050:function getPiSessionsDir(override) {
14135:function createParser19(source, seenKeys) {
14252:function createPiProvider(sessionsDir) {
$ codeburn --version              # 0.9.27
$ codeburn --help                 # report, overview, export, status, budget, price-override,
                                  # model-alias, model-flat-rate, context, sessions, …
$ ls ~/.cache/codeburn            # daily-cache.v74.json  session-cache.v10  litellm-pricing.json
```
