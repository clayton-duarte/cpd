# Axioms audit 2: where each axiom is enforced

Audit 2 fixes nothing. For each of the 29 axioms this document records where the axiom is enforced (a guard, a type, a check, a test, the Gate), where only prose carries it, and a proposal. It merges the five part findings files (`craft`, `types`, `ops`, `cpd`, `hygiene`); the full entries in section 6 are those parts' entries, moved verbatim and in axiom order. Every measurement was read-only and wrapped in `timeout`.

## Summary

- 6 of the 29 axioms have a mechanism that enforces their core: 1, 5, 6, 7, 13, 25.
- 21 are enforced in part, with a named gap: 3, 8-12, 14-24 and 26-29.
- 2 are prose only, with no mechanism at all: 2 (inference is not evidence) and 4 (KISS).
- Largest gap 1 (axiom 29): tests still run real git (735 call sites in 39 Rust test files) and real tmux (213 `env.tmux(` sites in 46 files); no fake git exists.
- Largest gap 2 (axiom 27): the primary-checkout guard reads only the `bash` tool, so `edit`/`write` by absolute path are unguarded (hs-d606, two incidents).
- Largest gap 3 (axiom 28): no flake/leak metric; `/tmp` holds 83G, 378 `cpd-test-*` dirs and 284 tmux sockets that no run checks.
- Largest gap 4 (axiom 22): no guard names the evidence that justified it; noblock, testguard and secretguard cite 0 record ids.
- Largest gap 5 (axiom 2): no mechanism requires a claim to be labelled or verified; the Worker Spec template has no `Verified:` rule.
- Axioms 10, 11, 12, 13, 15, 19, 25 and 27 need no new guard: no evidence, or the accepted design already covers it.

## Table

| # | Axiom | Enforced by | Gap in one line | Proposal in one line | Size | Evidence |
|---|---|---|---|---|---|---|
| 1 | Convention over configuration | test + script + check: `layout.rs`, `npm-fresh.sh`, `check-portable.sh`, config refusal | a crate can add a directly versioned dependency and nothing fails | manifest test refusing a non-`.workspace = true` entry, plus `--locked` in the Gate | S | measured |
| 2 | Inference is not evidence | none | nothing requires a claim to be labelled or verified | add a `Verified:` rule to the Worker Spec template and pin it | S | measured |
| 3 | YAGNI | test: `dead_code.rs` | a speculative stub passes while its `allow(dead_code)` reason is non-empty | refuse speculative markers in an `allow(dead_code)` reason | S | measured |
| 4 | KISS | none | no lint, complexity check or size limit | clippy `too_many_lines`/`cognitive_complexity` as a Gate step | M | measured |
| 5 | DRY | test: policy, interface, pill, stale-phrase and box tests | `docs/commands.md` is hand-written, not pinned to clap | parse every `cpd …` synopsis of `commands.md` against the interface `TABLE` | M | measured |
| 6 | Delete first, but check the fence | script + test: frozen migrations, `dead_code.rs`, stale phrases | only migrations are fenced; event/record kind names are not | frozen check over event-kind and record-kind names | S | measured |
| 7 | Explicit over implicit | type + check: `config/file.rs finish()`, `deny_unknown_fields`, `check-portable.sh` | `deny_unknown_fields` is opt-in and remembered by hand | scan file-parsed `Deserialize` structs for the attribute | M | measured |
| 8 | Make illegal states unrepresentable | type + check: ids, `Draft`, `ErrorCode`, 349 `CHECK` clauses | `jobs.kind` and `attempts.status` are bare `String` inside the core | parse them into enums at load, `bad_row` on an unknown value | S | measured |
| 9 | Validate at the boundary, fail loud | check + type: id `parse`, coded errors, config/policy parse, unknown fields | values read back from the Ledger are not re-validated | same enum parse-at-load as entry 8; one change closes both | S | measured |
| 10 | Tests are the proof | check + gate: not-red/failed/tampered/files-missing, nextest no fail-fast | "for the right reason" is unenforced; any exit 1 counts as red | none needed now (no evidence) | - | measured |
| 11 | Ask, don't assume | script + check + type: `cpd ask`, escalation row, `WORKER_EXCLUDED_TOOLS` | nothing forces a Worker to ask; 11 escalations expired unanswered | none needed; surface `escalation-expired` through the inbox | - | measured |
| 12 | A question is not an instruction | check: Plan review gate, question record shape, askguard | nothing separates a chat interrogative from an instruction | none needed now (no evidence) | - | measured |
| 13 | Reversibility | check: attempt worktrees, frozen migrations, append-only triggers | nothing records a decision's reversibility cost outside migrations | none needed (YAGNI); the store one-way door is already fenced | - | measured |
| 14 | Idempotence | test: 58 rerun tests, per command | nothing checks an `Idempotent: yes` label is true; one is already false | docs test tying each `Idempotent: yes` to a named rerun test | M | measured |
| 15 | Composition over inheritance | type: Rust has no inheritance; `erasableSyntaxOnly` | a `class X extends Y` in cpd-authored `pi/` is not refused | none needed; at most a test for `pi/lib` | - | read in code |
| 16 | Small, atomic commits | check + gate: `no_commits`, squash merge, Spec prose | nothing measures a commit's size, count or message | record commit count and diff size at submit over a threshold | S | measured |
| 17 | Prose is not enforcement | guard + check + type: secretguard, `kind_reserved`, noblock, askguard, mainguard, loop-guard | most of the 32 skill rules are prose; the edit/write half of hs-d606 is absent | extend the primary-checkout guard to `edit`/`write` (hs-d606) | M | measured |
| 18 | Tools over scraping | guard (dependency absence) + test: `DENIED_TOOLS` | no production guard on `curl`/`wget`; only test-harness deny shims | a test pinning that no HTTP-client crate is declared | S | measured |
| 19 | Brevity is not vagueness | check: askguard 1500, label visibility, line/title/label caps | nothing measures preamble, recap or closing offers | none needed; the caps already cover the enforceable part | - | measured |
| 20 | Make it work, make it fast, make it good | check + test + gate: shape checks, TDD template, nextest timeouts | the docs-first step is not required of a `tdd` PR | add `docs` to the required `tdd` kinds, or declare it prose | S | measured |
| 21 | Fix the cause, not the symptom | check + test + gate: `test_files_missing`, `tests_not_red`, `guard_tests`, leak check | no metric records flakes/leaks/sleeps; a symptom patch is invisible | require a `fixes` field in the Spec; record a flake/leak metric | S/M | measured |
| 22 | Guards only for proven bad behaviour | guard: mainguard, secretguard, noblock, testguard, git wrapper | no guard names its justifying evidence; 3 shell guards cite 0 record ids | `pi/test/guards.test.ts` requiring a record id per shell/git guard | S | measured |
| 23 | Blocked means it needs the human | type + test: adapter `Kind`, state tests, session `legal` table | `cpd session state blocked` accepts any label, including agent work | add `--kind human|agent`, refuse `blocked` with `agent` | M | read in code |
| 24 | No planned work only in chat | check: reminder phrase list, `turns.ts`, tests | the reminder is invisible; no rate is recorded and synonyms escape | append a `reminder: due` log line and assert it | S | measured |
| 25 | Recency wins | check + gate + test: `supersedes` links, distill `seq`, immutable records | contradiction detection was removed by decision; two in-force records can conflict | none needed; the curating distiller is the accepted replacement | - | measured |
| 26 | Decided work skips review | check + test: `--draft`/`queued`, `check_needs_not_draft`, skill pins | a queued Job need not cite a decision | refuse a queued Job whose Spec context cites no record id | S | measured |
| 27 | A primary checkout is a mirror of origin/main | guard + test: mainguard (bash), git wrapper, skill test | `edit`/`write` by absolute path are unguarded (hs-d606) | none here; Plan `main-mirror` PR `guard` owns the extension | - | read in code |
| 28 | No flakes, no leaks, no orphans, no sleep, no errors | gate + script + hook + check + test: `gate-run.sh`, nextest timeouts, sleep ratchet, harness | flakes have no metric; leaked `/tmp` files and dead sockets are invisible | flake Event from the Gate failure diff + machine-wide `/tmp` sweep in doctor | M | measured |
| 29 | Only authored code is tested | guard + test + gate: tmux tripwire, `DENIED_TOOLS`, allowlist ratchet | real git (735 sites) and real tmux (213 sites) still run; no git fake exists | stateful `fake-git` + `scripts/tripwire/git`, then migrate the areas | L | measured |

## Measurements

Every number of the zero-tolerance (28) and mocks (29) entries, with the command that produced it. The command output is quoted after the arrow.

#### Axiom 28: No flakes, no leaks, no orphans, no sleep, no errors

- `timeout 30 node --test pi/test/no-fixed-sleeps.test.ts` → `# tests 3`, `# pass 3`, `# fail 0`.
- `timeout 60 rg -c 'thread::sleep|tokio::time::sleep|sleep ' cli/tests core engine tui | awk -F: '{s+=$NF} END {print s}'` → `319` (per dir `246/52/9/12`); `timeout 60 rg -c 'thread::sleep' cli/tests core engine tui | awk -F: '{s+=$NF} END {print s}'` → `111`; `timeout 60 rg -n 'sleep [0-9]' cli/tests core engine tui --glob '*.rs' | wc -l` → `76`.
- `timeout 60 rg -c 'setTimeout' pi/test | awk -F: '{s+=$NF} END {print s}'` → `15`; `timeout 60 rg -l 'setTimeout' pi/test | wc -l` → `6`.
- `timeout 60 grep -cv '^#\|^$' pi/test/sleep-allow.txt` → `79`; `... | awk '{s+=$2} END{print s}'` → `469` (pi `287`, Rust `182`).
- `timeout 120 du -sh /tmp` → `83G`.
- `timeout 60 find /tmp -maxdepth 1 -name 'cpd-test-*' | wc -l` → `378`; `timeout 120 du -sch /tmp/cpd-test-* | tail -1` → `394M`.
- `timeout 60 find /tmp -maxdepth 1 -name 'tmp.*' | wc -l` → `99`; `-name '.tmp*'` → `1`; `timeout 120 du -sch /tmp/tmp.* | tail -1` → `227M`.
- `timeout 60 ls /tmp/tmux-1000 | wc -l` → `284`; `timeout 60 find /tmp -maxdepth 1 -name 'pi-check*' | wc -l` → `9`.
- `timeout 120 du -sch /tmp/sp* /tmp/dc* /tmp/hn* | tail -1` → `63G`.
- `timeout 60 rg -n 'flak' core cli engine scripts docs/research/hygiene.md | wc -l` → `6` (source hit `cli/tests/session/session_reload.rs:497`); `timeout 60 rg -n 'retries' .config/nextest.toml` → exit 1, `0` lines.
- `timeout 60 rg -n 'flak|leak|sleep' core/src/events | wc -l` → `6` (`kind.rs:114,189`, `typed.rs:369,370,545`, `mod.rs:181`; none is a leak or flake counter).
- `timeout 60 cpd events list --kind error --limit 500 | jq -r '.events | length'` → `206`; by code `80 distill_invalid, 60 gh_failed, 15 cleanup_failed, 9 store_busy, 9 tmux_failed, 7 spawn_failed, 7 tier_invalid, 5 title_failed, 4 store_too_new, 3 adapter_missing, 3 push_failed, 1 config_invalid, 1 plan_required, 1 pr_start_failed, 1 tier_not_allowed`; oldest `2026-10-15`, newest `2026-10-25`.
- `timeout 60 rg -n 'fail-fast|leak-timeout|slow-timeout' .config/nextest.toml` → `:19`, `:24`, `:33`, `:79`; `timeout 60 rg -n 'GATE_RUN|run_pids|^finish|tripwire.log' scripts/gate-run.sh` → `:27`, `:68`, `:88`, `:96`, `:117`.

#### Axiom 29: Only authored code is tested

- `timeout 60 rg -l '\.git\(|git_repo\(|keyed_repo\(|local_origin\(|github_origin\(|Command::new\("git"\)' cli/tests core/tests | wc -l` → `39`.
- `timeout 60 rg -c '\.git\(' cli/tests core/tests | awk -F: '{s+=$NF}END{print s}'` → `735`; `timeout 60 rg -l '\.git\(' cli/tests core/tests | wc -l` → `33`.
- `timeout 60 rg -n 'Command::new\("git"\)' cli/tests core/tests` → 4 lines (`pr/git_guard.rs:20`, `core/tests/{hooks_idle.rs:24,migrations.rs:462,repin.rs:96}`); `... core/src engine/src tui/src cli/src` → `core/src/git/run.rs:57`, `core/src/policy/origin.rs:108`, `core/src/context/distill.rs:434`.
- `timeout 60 rg -n 'execFileSync\("git"' pi/test` → `pi/test/mainguard.test.ts:26`; `timeout 60 rg -n '\bgit\(' pi/test/mainguard.test.ts | wc -l` → `4`.
- `timeout 60 rg -l '\bgit::' core/src engine/src tui/src cli/src | wc -l` → `25`; `timeout 60 rg -c '\bgit::' core/src engine/src tui/src cli/src | awk -F: '{s+=$NF}END{print s}'` → `130`.
- `timeout 60 rg -l 'env\.tmux\(|real_tmux' cli/tests | wc -l` → `46`; `timeout 60 rg -c 'env\.tmux\(' cli/tests | awk -F: '{s+=$NF}END{print s}'` → `213`; `timeout 60 rg -n 'real_tmux' cli/tests core engine tui | wc -l` → `19` (4 definitions, 1 import, 14 call sites).
- `timeout 60 rg -l 'fake_tmux\(\)|tmux_state\(\)' cli/tests` → 5 files; `timeout 60 rg -n 'fake_tmux\(\)|tmux_state\(\)' cli/tests | wc -l` → `9`.
- `timeout 60 grep -cv '^#\|^$' scripts/tripwire/allow.txt` → `124`.
- `timeout 60 rg -n 'DENIED_TOOLS' cli/tests/common/mod.rs` → `307`, `751`; `timeout 60 rg -n 'fn fake\(' cli/tests/common/mod.rs` → `1090`.
- `timeout 60 rg -n 'Command::new\("(gh|pi)"\)' cli/tests core/tests` → exit 1, `0` lines; `timeout 60 rg -n 'TcpStream|TcpListener|UdpSocket|UnixStream' cli/tests core/tests engine` → exit 1, `0` lines.
- `TMPDIR=/tmp/audit-hyg-trip NEXTEST_BINARY_ID=cpd::foo NEXTEST_TEST_NAME=bar::baz timeout 15 scripts/tripwire/tmux list-sessions` → `tripwire: real tmux called by cpd::foo bar::baz: tmux list-sessions`, exit `127` (no tmux started); the dir was removed after.
- `timeout 60 grep -c '#\[test\]' cli/tests/tooling/tripwire_script.rs` → `10`; `timeout 60 ls cli/src/fakes` → `mod.rs`, `tmux`.
- `timeout 60 rg -l 'fake-git|CPD_TEST_GIT' --glob '!node_modules' --glob '!target' .` → `docs/research/tests-mock-design.md`, `docs/research/tests-mock-flow-b.md` (2 docs, no code).

## Mocking git and tmux

The sizing and the order of work are the hygiene part's (axiom 29). External by hs-d613: git is not mocked today; tmux has a fake (`cli/src/fakes/tmux/`) that only 5 test files use.

- git: 39 Rust test files invoke real git, 735 `.git(` call sites in `cli/tests` and `core/tests` plus 4 `Command::new("git")`; `pi/test/mainguard.test.ts:26` runs real git (4 call sites). The product spawns git at `core/src/git/run.rs:57` and `core/src/context/distill.rs:434`. No fake git exists; the only mentions of `fake-git`/`CPD_TEST_GIT` are 2 design docs. Size: **L overall; the git fake alone is M.**
- tmux: 46 Rust test files call `env.tmux(`/`real_tmux`; 213 `env.tmux(` + 14 `real_tmux()` call sites + 2 `Command::new("tmux")`. The fake exists but only 5 test files use it (9 sites); the 124-line allowlist lists every real-tmux area, so the tripwire denies nothing that already exists. Size: part of the same L migration.
- Proposal: a stateful `fake-git` (the design's Option A) reached through a `CPD_TEST_GIT` read in `core/src/git/run.rs:57`, and a `scripts/tripwire/git` with `allow-git.txt`; the 12 real `contract::` tests stay as permanent lines.

Order of work (≤ 8 steps), from the hygiene part:

1. `fg-spike`: `cli/src/fakes/git/` state model plus the 12 fixture subcommands (`init add commit clone tag checkout merge for-each-ref config remote rev-parse update-ref`) and `worktree add`; abort if `merge-tree`/`rebase` do not model in 3 Jobs.
2. `fg-read`: read-only subcommands (`status diff cat-file ls-tree show log rev-list rev-parse config branch`).
3. `fg-merge`: `merge-base merge-tree rebase read-tree write-tree`.
4. `fg-wt`: `worktree branch remote push fetch ls-remote`.
5. `fg-env`: the `CPD_TEST_GIT` seam, a nextest setup script that exports it, `scripts/tripwire/git` + `allow-git.txt`, and the 12 contract tests as permanent lines.
6. Migrate the core/engine/flow areas (`mu-git`, `en-t2`, `fl-g`, `fl-b`) and, for tmux, `m-session`, `m-engine`, `m-tui` against the existing `cli/src/fakes/tmux/`.
7. Migrate `pr` and `tooling` (12 `pa-*`/`pb-*` Jobs, `tl-install`, `tl-scripts`) and, for tmux, `m-flow`, `m-tooling`.
8. `del-1`: delete `allow-git.txt` and the `git` branch, `scripts/tripwire/allow.txt`, `real_tmux()` and the `tmux-restart` group; the tripwire stays an unconditional deny.

## Proposals, ranked

At most 15, by value over size. Each line: what, where, size, evidence. A guard on a raw tool appears only where there is evidence.

1. Extend the primary-checkout guard to the `edit` and `write` tool calls — `pi/lib/adapter.ts` + `pi/lib/mainguard.ts` — M — evidence hs-d606 (two named incidents) plus `rg -c 'toolName === "bash"' pi/lib/adapter.ts` → 1.
2. Build the stateful `fake-git` behind `CPD_TEST_GIT` and `scripts/tripwire/git`, then migrate the 735 git sites and the 213 tmux sites — `cli/src/fakes/git/`, `core/src/git/run.rs:57` — L — measured: 39 git files, 46 tmux files, no fake git.
3. Record a `flake` Event from the Gate's failure diff and add a machine-wide `/tmp` sweep to the doctor — `scripts/gate-run.sh`, `core/src/events/kind.rs`, engine doctor — M — measured: `/tmp` 83G, 378 `cpd-test-*`, 284 sockets, no flake counter.
4. `pi/test/guards.test.ts` fails a shell/git guard whose header and docs section name no record id — `pi/lib/*.ts`, `docs/commands.md` — S — measured: `rg -o 'hs-d[0-9]+' pi/lib` → 0; noblock/testguard/secretguard cite 0.
5. Parse `jobs.kind` and `attempts.status` into enums at load — `core/src/plan/jobs.rs:47`, `core/src/attempt/ledger.rs:30` — S — measured: `pub kind: String|pub status: String` → 4 sites.
6. Add a `Verified:` rule to the Worker Spec template and a test that the rendered template carries it — `core/src/spec/worker.md`, `core/src/spec/mod.rs` — S — measured: `rg -n 'unverified|Verified' core/src/spec/worker.md` → 0.
7. Add a manifest test refusing any dependency that is neither `path` nor `.workspace = true`, and `--locked` to the Gate's cargo commands — `cli/tests/tooling/layout.rs`, `package.json` — S — measured: `.workspace = true` counts 16/11/13/13, `--locked` only in `install-cpd.sh`/`preview-tui.sh`.
8. Extend `cli/tests/tooling/dead_code.rs` to refuse a speculative `allow(dead_code)` reason (`stub`, `not written yet`, `later`, `future`, `TODO`) — S — measured: 12 sites in 8 files, `tui/src/tree/actions.rs:149` is a stub.
9. Parse every `cpd …` synopsis of `docs/commands.md` and check each command path and flag exists in the binary — `cli/tests/tooling/` — M — measured: only `logs/mod.rs:141` and `skill.test.ts:6` read `commands.md`.
10. Scan file-parsed `Deserialize` structs for `deny_unknown_fields` or add a derive helper that defaults it — `cli/tests/tooling/` — M — measured: 12 files carry it, 21 mention `Deserialize`, 11 `#[serde(default)]` sites.
11. A docs test tying each `Idempotent: yes` label of `docs/commands.md` to a named rerun test — M — measured: 95 labels, 58 rerun tests, `quality-commands.md:116` records one false label.
12. Require `docs` among the kinds of a `tdd` PR, or declare the docs-first step prose-only — `core/src/plan/shape.rs:22` — S — measured: required kinds are `["test", "impl"]`.
13. Refuse a non-draft Job whose Spec `context` cites no Context record id — `core/src/job/add.rs`, `core/src/job/spec.rs` — S — measured: `rg -o 'decision|hs-' core/src/job/spec.rs` → 0.
14. Record the commit count and diff size at submit and refuse over a documented threshold — `core/src/attempt/submit.rs` — S — measured: submit runs `git rev-list --count` once, pins no size.
15. Add `[workspace.lints.clippy]` with `too_many_lines` and `cognitive_complexity` and a `scripts/check-lint.sh` Gate step — M — measured: 0 clippy matches across the five manifests.

## The full entries

### 1. Convention over configuration

- **Enforced by:**
  - `test` `cli/tests/tooling/layout.rs:204`: pins the four workspace members, their directories, `cpd-engine` owning `watch`/`doctor`, `cpd-core` not, and `cpd-tui` not depending on `cpd-engine` (the one-way dependency direction). Blind spot: it does not require each dependency line to be `.workspace = true`; a crate can add a directly versioned dependency and this still passes.
  - `test` `cli/tests/tooling/layout.rs:294`: `-p <member>` must resolve the same features as `--workspace`. Blind spot: feature unification only, never versions or provenance.
  - `script` `scripts/npm-fresh.sh:23` (`exec npm ci --no-audit --no-fund`): installs exactly `package-lock.json`, so a Node deviation is pinned; pinned by `test` `cli/tests/tooling/npm_fresh.rs:45,50,56,61`. Blind spot: Node only. The Rust build is not run `--locked` in the Gate: `scripts/install-cpd.sh:76` and `scripts/preview-tui.sh:69` are the only `--locked` users.
  - `check` `scripts/check-portable.sh:87`: a file of `scripts/` in neither the covered nor the Linux-only list fails, each entry carrying a written reason, so a new script cannot skip the decision. Blind spot: portability, not dependency convention.
  - `test` `cli/tests/flow/config.rs:265`: every unknown `config.toml` key is refused with its key path; a deviation must be named, never silently accepted. Blind spot: cpd config only.
- **Prose only:** `docs/implementing.md:153`: "Every dependency is declared once ... **Later Jobs may not add, remove or change dependencies or features**" — no test reads a crate `Cargo.toml` for this, and the axiom's "search for an existing `pi-package` and take it pinned" step has no mechanism: the keyword is declared in `pi/package.json:7` but nothing searches npm for it.
- **Gap:** a crate can add `foo = "1.2"` in its own `[dependencies]` instead of `foo.workspace = true` and no check fails; the four manifests use `.workspace = true` 16/11/13/13 times (see Verified) but nothing pins that. The Gate's test build also re-resolves `Cargo.lock`, since `scripts/cargo.sh` passes no `--locked`, so a manifest edit can move the lock inside the Gate. No incident evidence: `no evidence`.
- **Proposal:** add one `test` to `cli/tests/tooling/layout.rs` (beside `:294`) that reads the four crate manifests and fails any `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]` entry that is neither a `path` dependency nor `.workspace = true`, and add `--locked` to the Gate's cargo commands in `package.json` `verify:steps`. Size S.
- **Verified:**
  - `timeout 60 rg -n '\[workspace.dependencies\]' Cargo.toml` → `12:[workspace.dependencies]`.
  - `timeout 60 rg -c 'workspace = true' core/Cargo.toml engine/Cargo.toml cli/Cargo.toml tui/Cargo.toml` → `core/Cargo.toml:16`, `engine/Cargo.toml:11`, `cli/Cargo.toml:13`, `tui/Cargo.toml:13`.
  - `timeout 60 rg -n -- '--locked' scripts/*.sh` → `scripts/install-cpd.sh:76`, `scripts/preview-tui.sh:69` (2 lines).
  - `timeout 60 rg -n 'fn the_four_crates|fn every_member_resolves' cli/tests/tooling/layout.rs` → `204`, `294`.
  - `timeout 60 rg -n 'exec npm ci' scripts/npm-fresh.sh` → `23`.
  - `timeout 60 rg -n 'not classified' scripts/check-portable.sh` → `87`.
  - `timeout 60 rg -n 'fn config_refuses_each_bad_file' cli/tests/flow/config.rs` → `265`.
  - `timeout 60 rg -l 'pi-package' --glob '!node_modules' --glob '!target' | wc -l` → `13` files (only `pi/package.json:7` declares the keyword; the rest are docs); `timeout 60 rg -n 'pi-package' pi/package.json` → `7`.

### 2. Inference is not evidence

- **Enforced by:**
  - `none`. No type, guard, check, test, Gate or hook requires a claim to be labelled or verified. The generic Worker Spec template (`core/src/spec/worker.md`) has no `## Verified` and no `## Not verified` section; the TDD red check (`tests_not_red` at `core/src/spec/worker.md:193`, `tests_failed` at `:200`) proves a test fails first, which is a different rule (10, out of scope).
  - `check` the quick check the Spec embeds (`core/src/spec/worker.md:40`) runs `npm run typecheck` and `scripts/cargo.sh build --workspace`, which prove the artefact compiles, not that a claim is true; blind spot: compilation is not evidence of behaviour.
- **Prose only:** `docs/contract.md:7`: "**unverified** means a design assumption the implementation Job must check"; `docs/research/tests-mock-units.md:1`: "A claim I did not read in the source is marked `unverified`". Both are text the reader must honour; nothing fails when it is ignored. `rg -n 'unverified|Verified|not verified' core/src/spec/worker.md` prints nothing.
- **Gap:** a Worker, Lead or Distiller can submit an unlabelled inference and nothing refuses it. Evidence: `core/src/spec/worker.md` contains 0 matches for `unverified|Verified`; the two in-tree uses of the label are comments (`core/src/events/mod.rs:214`, `core/src/host/tmux.rs:657`); the Distiller prompt `core/src/context/distill_prompt.md` has 0 matches for `unverified|evidence|verify`. No `error` Event code records an unlabelled claim: `no evidence`.
- **Proposal:** add a `Verified:` line (and an explicit "label anything you did not verify `unverified`" rule) to the Worker Spec template `core/src/spec/worker.md`, and a `test` in `core/src/spec/mod.rs` that the rendered template contains that rule; a research/audit acceptance then cannot omit it. Size S.
- **Verified:**
  - `timeout 60 rg -n 'unverified|Verified|not verified' core/src/spec/worker.md` → exit 1, 0 lines.
  - `timeout 60 rg -n 'unverified' core/src/events/mod.rs core/src/host/tmux.rs` → `core/src/events/mod.rs:214`, `core/src/host/tmux.rs:657` (2 lines).
  - `timeout 60 rg -n 'unverified|evidence|verify' core/src/context/distill_prompt.md` → exit 1, 0 lines.
  - `timeout 60 rg -c 'unverified' --glob '*.md' docs | wc -l` → `111`.
  - `timeout 60 rg -n 'pub fn worker|pub fn distiller|no-context-files' core/src/host/pi/launch.rs` → `163`, `193`, `195` (the Worker argv has none; the distiller at `:195` passes `--no-context-files`).

### 3. YAGNI

- **Enforced by:**
  - `test` `cli/tests/tooling/dead_code.rs:19`: every `#[allow(dead_code)]` needs a live reason on its own line and may not cite a finished Job, so unused code must be justified rather than silenced. Blind spot: it fires only on an explicit `allow(dead_code)`; a `pub` item with no caller needs no attribute and is never seen, and a reason of "stub" or "not written yet" passes.
  - `check` `core/src/spec/worker.md:123` (`### Out of scope`): each Job's Spec carries the scope, and the acceptance is checked at submit; blind spot: it is text the Worker must obey, and the submit check tests the named acceptance, not that no extra file was touched (the diff may touch any file not guarded or protected).
- **Prose only:** `docs/implementing.md:153`: "Later Jobs may not add, remove or change dependencies or features ... If you need something not listed, ask the Lead" — a scope limit with no guard; `docs/implementing.md:177`: "Deliberately absent: a regex crate ... an error-derive crate ..." — the decisions are prose, and nothing fails when a new dependency is added.
- **Gap:** speculative code passes. `tui/src/tree/actions.rs:149` is `#[allow(dead_code)] // stub: the click that builds it is not written yet` — a stub kept alive by a reason that says it is not used, exactly what YAGNI forbids, and `dead_code.rs` accepts it because the reason is non-empty and names no Job. Evidence: `rg -n 'allow\(dead_code\)' --glob '*.rs'` counts 12 sites in 8 files (see Verified). No dependency guard exists: `cli/tests/tooling/layout.rs` reads `Cargo.toml` only for crates/features (2 matches), never to refuse a new dependency. No `error` Event counts scope creep: `no evidence`.
- **Proposal:** extend `cli/tests/tooling/dead_code.rs:19` to fail an `allow(dead_code)` whose reason contains a speculative marker (`stub`, `not written yet`, `later`, `future`, `TODO`), so the stub at `tui/src/tree/actions.rs:149` must be deleted or justified as live. Size S.
- **Verified:**
  - `timeout 60 rg -n 'allow\(dead_code\)' tui/src/tree/actions.rs` → `149`.
  - `timeout 60 rg -c 'allow\(dead_code\)' --glob '*.rs' | awk -F: '{s+=$2} END {print s}'` → `12`; `timeout 60 rg -l 'allow\(dead_code\)' --glob '*.rs' | wc -l` → `8`.
  - `timeout 60 rg -n 'fn every_allow_dead_code' cli/tests/tooling/dead_code.rs` → `19`.
  - `timeout 60 rg -n 'Later Jobs may not add|Deliberately absent' docs/implementing.md` → `153`, `177`.
  - `timeout 60 rg -n 'Out of scope' core/src/spec/worker.md` → `123`.
  - `timeout 60 rg -n 'Cargo.toml' cli/tests/tooling/layout.rs | wc -l` → `2`.

### 4. KISS

- **Enforced by:**
  - `none`. There is no lint, complexity check or size limit. `rg -n 'clippy' Cargo.toml core/Cargo.toml engine/Cargo.toml cli/Cargo.toml tui/Cargo.toml` exits 1 (0 matches), there is no `[workspace.lints]` and no `.cargo/config.toml`, so nothing in the Gate can fail a needlessly complex change.
- **Prose only:** `docs/implementing.md:177` ("Deliberately absent ...") records decisions against new machinery; `docs/implementing.md:13` pins the crate count and dependency direction as a design, but a test at `cli/tests/tooling/layout.rs:204` only checks that shape, not complexity inside a crate.
- **Gap:** a very long function, a deep generic tower or a needless abstraction passes every check. Evidence: 0 clippy matches across the five manifests; `timeout 60 rg -n 'too_many_lines|cognitive_complexity|complexity' Cargo.toml core/Cargo.toml tui/Cargo.toml cli/tests/tooling/*.rs` prints nothing. No `error` Event code measures complexity: `no evidence`.
- **Proposal:** add `[workspace.lints.clippy]` with `too_many_lines` and `cognitive_complexity` and a `scripts/check-lint.sh` (`scripts/cargo.sh clippy --workspace --all-targets -- -D warnings`) as a Gate step in `package.json` `verify:steps`; a `test` in `cli/tests/tooling/` pins that the Gate runs it. Size M (clippy is a toolchain component, no new crate).
- **Verified:**
  - `timeout 60 rg -n 'clippy' Cargo.toml core/Cargo.toml engine/Cargo.toml cli/Cargo.toml tui/Cargo.toml` → exit 1, 0 lines.
  - `timeout 60 rg -n 'too_many_lines|cognitive_complexity|complexity' Cargo.toml core/Cargo.toml tui/Cargo.toml cli/tests/tooling/*.rs` → exit 1, 0 lines.
  - `timeout 60 rg -n 'fn the_four_crates' cli/tests/tooling/layout.rs` → `204`.
  - `timeout 60 rg -n 'Deliberately absent' docs/implementing.md` → `177`.

### 5. DRY

- **Enforced by:**
  - `test` `core/src/policy/mod.rs:565` (`the_contract_domains_table_is_the_code`): the domains table of `docs/contract.md` equals `Domain::ALL`, the `Policy` fields and the `default.toml` tables, and every documented typed key/default/allowed value is accepted by the loader. Blind spot: the policy table alone.
  - `test` `core/src/policy/exposure_tests.rs:105`: the table's Exposure column equals the code's `exposure(domain)`. Blind spot: one column.
  - `test` `cli/tests/tooling/interface.rs:65`: `docs/interface.md` is generated from `cpd --help` and byte-compared. Blind spot: one generated page.
  - `test` `core/src/pill_tests.rs:10` and `cli/tests/tooling/pill_sites.rs:90,107`: `pi/lib/pill-spec.json` is the one pill source (caps, colours, clickable rule), and the cap glyphs and reverse-video escapes are refused in non-test source outside the spec and the three adapters. Blind spot: pill data only.
  - `test` `tui/src/logs/mod.rs:146` (`no_stale_statement_is_left`): a fixed list of stale phrases must not reappear in `docs/contract.md`, `docs/commands.md`, `logs/mod.rs` or `plan/mod.rs`. Blind spot: a phrase list, not a general check.
  - `test` `tui/src/plan/box_tests.rs:33`: `boxes.rs` must draw through `BoxWidget` and must not hand-build a box. Blind spot: the Plan boxes only.
- **Prose only:** `docs/implementing.md:104`: the area table is "the one source of truth" for test layout — enforced by `cli/tests/tooling/layout.rs:144`; `docs/implementing.md:62` and `docs/contract.md:2408` describe the pill's one source, enforced above.
- **Gap:** `docs/commands.md` is hand-written and is not generated or pinned to the binary. Only the `cpd …` commands the two skills name are checked to exist in it (`pi/skills/skill.test.ts:6`); nothing checks that a synopsis in `commands.md` still matches `cpd --help`, so it can restate a flag the binary no longer has. Evidence: `cli/tests/tooling/interface.rs:65` reads the binary and writes `docs/interface.md`; the only readers of `docs/commands.md` are `tui/src/logs/mod.rs:141` (a stale-phrase list) and `pi/skills/skill.test.ts:6` (skill commands exist in it), neither a synopsis-to-clap match. `no evidence` of a drift incident.
- **Proposal:** add a `test` in `cli/tests/tooling/` that parses every `cpd …` synopsis of `docs/commands.md` and checks each command path exists in `cli/src/interface.rs`'s `TABLE` (and each named flag in clap's `--help` for that path), the reverse of the skill check. Size M.
- **Verified:**
  - `timeout 60 rg -n 'fn the_contract_domains_table_is_the_code' core/src/policy/mod.rs` → `565`.
  - `timeout 60 rg -n 'fn the_contract_table_exposure_column_is_the_code' core/src/policy/exposure_tests.rs` → `105`.
  - `timeout 60 rg -n 'fn interface_page_is_generated_from_the_binary' cli/tests/tooling/interface.rs` → `65`.
  - `timeout 60 rg -n 'fn pill_spec_shipped_file' core/src/pill_tests.rs` → `10`.
  - `timeout 60 rg -n 'fn no_pill_cap_glyph|fn no_reverse_video_pill' cli/tests/tooling/pill_sites.rs` → `90`, `107`.
  - `timeout 60 rg -n 'fn no_stale_statement_is_left' tui/src/logs/mod.rs` → `146`.
  - `timeout 60 rg -n 'fn the_pr_boxes_are_drawn_by_the_box_widget' tui/src/plan/box_tests.rs` → `33`.
  - `timeout 60 rg -n 'docs/commands\.md' --glob '*.rs' | rg -i 'read_to_string|include_str'` → `tui/src/logs/mod.rs:141` (`include_str!`); `timeout 60 rg -n 'readFileSync.*commands\.md' --glob '*.ts' --glob '!node_modules'` → `pi/skills/skill.test.ts:6`.

### 6. Delete first, but check the fence

- **Enforced by:**
  - `script` `scripts/check-migrations-frozen.sh:22` (`migrations frozen check failed`): a file under `core/src/store/migrations/` that exists on the base is never modified, renamed or deleted; the Gate runs it (`package.json` `verify:steps`). Blind spot: migrations only.
  - `test` `core/tests/migrations.rs:521` (`the_frozen_check_fails_on_an_edited_deleted_or_renamed_migration`) and `:512` (`..._passes_on_no_change_and_on_a_new_migration`): pin the script's two directions. Blind spot: the script's own behaviour.
  - `test` `cli/tests/tooling/dead_code.rs:19`: deleting dead code is preferred; silencing it with `allow(dead_code)` needs a live reason. Blind spot: explicit `allow(dead_code)` only.
  - `test` `tui/src/logs/mod.rs:146`: stale statements are deleted, not left beside the code. Blind spot: a fixed phrase list.
  - `test` `tui/src/tree/scroll_tests.rs:207` (`the_tree_view_never_renders_the_widget_or_a_scratch_buffer`) and `tui/src/plan/box_tests.rs:33`: a deleted hand-made renderer must not come back. Blind spot: two views.
- **Prose only:** `scripts/repin.sh:1-2` describes the schema-hash fence in its header (the script is the enforcement); `docs/implementing.md` "Deliberately absent" and the "Checked and kept" pattern in the research notes carry the "find the reason it exists" practice in text.
- **Gap:** the fence exists only for migrations (and the `SCHEMA_HASH` pin `scripts/repin.sh` rewrites). A deletion or rename of any other append-only data (an Event kind name, a record kind name) has no fence: the migration tests (`core/tests/migrations.rs`) guard new migrations, not the removal of an existing name from code. Evidence: `rg -n 'frozen' scripts/ core/tests/migrations.rs` finds only `scripts/check-migrations-frozen.sh` and its tests; `no evidence` of a lost Event-kind incident.
- **Proposal:** add a `check` `scripts/check-frozen.sh` (or extend `scripts/check-migrations-frozen.sh`) that fails when a name present on the base in `core/src/events/kind.rs` or `core/src/store/migrations/` disappears from HEAD, and a `test` in `core/tests/migrations.rs` pinning both directions. Size S.
- **Verified:**
  - `timeout 60 rg -n 'frozen check failed' scripts/check-migrations-frozen.sh` → `22`.
  - `timeout 60 rg -n 'fn the_frozen_check_fails_on_an_edited|fn the_frozen_check_passes' core/tests/migrations.rs` → `521`, `512`.
  - `timeout 60 rg -n 'fn every_allow_dead_code' cli/tests/tooling/dead_code.rs` → `19`.
  - `timeout 60 rg -n 'fn no_stale_statement_is_left' tui/src/logs/mod.rs` → `146`.
  - `timeout 60 rg -n 'fn the_tree_view_never_renders' tui/src/tree/scroll_tests.rs` → `207`.
  - `timeout 60 rg -n 'frozen' scripts/ core/tests/migrations.rs` → only `scripts/check-migrations-frozen.sh` and `core/tests/migrations.rs`.

### 7. Explicit over implicit

- **Enforced by:**
  - `type`+`check` `core/src/config/file.rs:479` (`finish()`): every leaf key is named once in `Config::parse`; whatever the walk does not consume is refused as "not a known key" with its dotted key path. `test` `cli/tests/flow/config.rs:265` pins the refusal for an unknown key at the top and in every table. Blind spot: `$CPD_HOME/config.toml` only.
  - `type` `#[serde(deny_unknown_fields)]`: 34 occurrences in 12 files (e.g. `core/src/config/repo.rs:11`, `core/src/policy/domain.rs:35`, `core/src/context/record.rs:17`, `core/src/pill.rs:5`) make an unknown field of a parsed file an error. Blind spot: it is opt-in per struct; nothing forces a new `Deserialize` type to carry it.
  - `check` `scripts/check-portable.sh:87`: a script must be explicitly classified (covered or Linux-only, with a reason); nothing is implicit. Blind spot: scripts only.
  - `test` `cli/tests/tooling/interface.rs:65`: `docs/interface.md` is generated from `cpd --help`, so the page is derived, never hand-kept. Blind spot: one page.
- **Prose only:** `docs/contract.md:95`: "Unknown flags and unknown config keys are errors. Nothing falls back silently." — the rule in text; its enforcement is clap plus `core/src/config/file.rs:479`. `pi/skills/cpd-lead/SKILL.md` names the guard reasons (e.g. G2's 1500-character limit) beside the tool, but the guard is code (`pi/lib/askguard.ts`).
- **Gap:** `deny_unknown_fields` is remembered by hand. Evidence: 12 files carry it (`rg -l`), while 21 `.rs` files mention `Deserialize`; `#[serde(default)]` supplies an implicit value at 11 sites (e.g. `core/src/context/record.rs:29`), and no test ties such a default to a documented value outside the policy table. A new file parsed from disk can accept an unknown key silently if its author forgets the attribute. `no evidence` of an incident.
- **Proposal:** add a `test` in `cli/tests/tooling/` that scans `core/src` for `#[derive(... Deserialize ...)]` structs whose input is a file cpd parses (`config`, `policy`, `context`, `pill`, `plan/outline`, `secret/grants`) and fails when `deny_unknown_fields` is absent, or introduce a small derive helper that adds it by default. Size M.
- **Verified:**
  - `timeout 60 rg -n 'fn finish' core/src/config/file.rs` → `479`.
  - `timeout 60 rg -n 'deny_unknown_fields' --glob '*.rs' | wc -l` → `34`; `timeout 60 rg -l 'deny_unknown_fields' --glob '*.rs' | wc -l` → `12`; `timeout 60 rg -l 'Deserialize' --glob '*.rs' | wc -l` → `21`.
  - `timeout 60 rg -c 'serde\(default' --glob '*.rs' | awk -F: '{s+=$2} END {print s}'` → `11`; `timeout 60 rg -n 'serde\(default' core/src/context/record.rs` → `29`.
  - `timeout 60 rg -n 'fn config_refuses_each_bad_file' cli/tests/flow/config.rs` → `265`.
  - `timeout 60 rg -n 'not classified' scripts/check-portable.sh` → `87`.
  - `timeout 60 rg -n 'fn interface_page_is_generated_from_the_binary' cli/tests/tooling/interface.rs` → `65`.

### 8. Make illegal states unrepresentable

- **Enforced by:**
  - `type` `core/src/ids.rs:89` (`simple_id!`) and `core/src/ids.rs:97` (`parse`): each id is a newtype whose only constructor validates contract §2.1; `Deserialize` at `core/src/ids.rs:82` runs `parse`, so a malformed id cannot enter from JSON or the CLI. Blind spot: it covers ids only, not other stored strings.
  - `type` `core/src/context/record.rs:36` (`enum Draft`): each record kind carries exactly the links it may have (`:33-34`), so a decision cannot name a question. Blind spot: the record text is free.
  - `type` `core/src/error.rs:41` (`enum ErrorCode`) with the macro at `core/src/error.rs:36`: each code maps to exactly one `Exit`, so a code cannot be printed with the wrong exit (`core/src/error.rs:4`). Blind spot: the message text is free.
  - `type` `core/src/ids.rs:181` (`AttemptId` over `NonZeroU32`): an Attempt number of zero is unrepresentable.
  - `check` `core/src/store/migrations/*.sql`: 349 `CHECK` clauses across 45 files constrain statuses, shapes and nullability; `core/src/store/migrations/0001-initial.sql:302` (`jobs_status`) allows only the declared status transitions.
- **Prose only:** none (the invariant lives in the types and the CHECK constraints; `docs/contract.md` §2.1 restates the grammar the code enforces).
- **Gap:** `jobs.kind` and `attempts.status` are plain `String` inside the core (`core/src/plan/jobs.rs:47`, `core/src/attempt/ledger.rs:30`), so a kind a later migration adds to the CHECK but no `matches!` is silently treated as "neither": `core/src/attempt/submit.rs:62` (`open|update`) and `core/src/attempt/kind.rs:21` (`test|impl`). No evidence of a wrong value reaching such a match (the CHECK guards writes).
- **Proposal:** parse `jobs.kind` and `attempts.status` into enums at load (`core/src/plan/jobs.rs`, `core/src/attempt/ledger.rs`), as the ids are parsed, so the matches are exhaustive and an unknown stored value is `bad_row`; pin it with a unit test that loads an unknown kind. Size S.
- **Verified:** `rg -c '^simple_id!' core/src/ids.rs` -> 7; `grep -h CHECK core/src/store/migrations/*.sql | grep -c CHECK` -> 349; `ls core/src/store/migrations/*.sql | wc -l` -> 45; `rg -n 'pub kind: String|pub status: String' core/src/plan/jobs.rs core/src/attempt/ledger.rs` -> 4; `sed -n '36p' core/src/context/record.rs` -> `pub enum Draft {`; `rg -c 'draft>queued' core/src/store/migrations/*.sql | awk -F: '{s+=$2} END{print s}'` -> 3.

### 9. Validate at the boundary, fail loud

- **Enforced by:**
  - `check` `core/src/ids.rs:82`: serde `Deserialize` for every id calls `parse`; bad input is `invalid_*_id` (exit 2), never a default.
  - `type` `core/src/error.rs:41`: every failure is a coded `CpdError`; the process exit is fixed by the code (`core/src/error.rs:58`), so no failure can be swallowed into exit 0.
  - `check` `core/src/config/file.rs:1-10`: `config.toml` is parsed key by key and every unknown key is refused with its dotted path (contract §8.1); a bad leaf (`PortRange::parse` at `core/src/config/file.rs:81`) is an error, not a fallback.
  - `check` `core/src/policy/load.rs:29` (`parse_scope`): a scope file is parsed once into `Scope`; a bad key is `policy_invalid` with the key (`core/src/policy/load.rs:22`); `/test-commands` is validated (`core/src/policy/domain.rs:161`).
  - `check` `serde(deny_unknown_fields)` on 34 structs (`core/src/policy/domain.rs`, `core/src/context/record.rs`, `core/src/config/repo.rs`, `tui/`): an unknown field is refused.
  - `check` `core/src/error.rs:71-73`: a store too new, a changed migration or a wrong migration order refuses (`store_too_new`, `migration_changed`, `migration_order`, contract §4.5).
- **Prose only:** none.
- **Gap:** values read back from the Ledger are not re-validated: `core/src/plan/jobs.rs:168` reads `kind` as a bare `String` and `core/src/attempt/ledger.rs:40` reads `kind`/`status` the same, so "inside, the types are already true" does not hold for a hand-edited or older database; the CHECK constraints guard writes, not reads. No evidence of a bad row today.
- **Proposal:** the same parse-at-load as entry 8 (one enum per `kind`/`status`, `bad_row` on an unknown value); one change closes both gaps. Size S. A test in `core/src/plan/jobs.rs` (or `cli/tests/store`) pins that an unknown `kind` is refused.
- **Verified:** `rg -c 'deny_unknown_fields' -g '*.rs' --glob '!node_modules' | awk -F: '{s+=$2} END{print s}'` -> 34; `rg -c '= "[a-z_]+" => (Failure|Usage|Refused|NotFound|Timeout),' core/src/error.rs` -> 190; `rg -n 'pub fn parse_scope' core/src/policy/load.rs` -> 1 line; `sed -n '168p' core/src/plan/jobs.rs` -> the tuple read.

### 10. Tests are the proof

- **Enforced by:**
  - `check` `core/src/attempt/kind.rs:200` (`("test", HookOutcome::Passed) => TestsNotRed`): a `test` Job's named tests must FAIL at submit. Blind spot: only the exit status is read, so a compile error or a wrong panic counts as red.
  - `check` `core/src/attempt/kind.rs:206` (`("impl", HookOutcome::Failed { .. }) => TestsFailed`): an `impl` Job's named tests must pass.
  - `check` `core/src/attempt/kind.rs:34` (`guard_tests`) with `core/src/git/tamper.rs:95` (`touched_guarded`): an `impl` Job's diff may not change the `test_files` of the `test` Jobs it needs unless the Lead approved the path (`tests_tampered`).
  - `check` `core/src/attempt/kind.rs:83` (`TestFilesMissing`): a `test` Job must change a path its `test_files` names.
  - `test` `cli/tests/flow/attempt.rs:559` (not-red), `:598` (files-missing), `:694` (failed), `:715` (tampered): the contract of the checks is pinned.
  - `gate` `package.json:14` (`scripts/gate-run.sh npm run verify:steps`) with `.config/nextest.toml:19` (`fail-fast = false`): the full suite runs once per PR and lists every failure.
- **Prose only:** `core/src/spec/worker.md:36` ("A test that is wrong is a question, not an edit"); the Spec asks the red test to fail for the right reason, which no check can see.
- **Gap:** "for the right reason" is unenforced: a compile error or a bare `panic!()` is accepted as a red test because the check only compares the exit (`core/src/attempt/kind.rs:200-206`), and `[test-commands]` `run` has no way to assert the failure's shape. No evidence of a wrong-reason red (no Event, no metric).
- **Proposal:** none needed now (no evidence). If a wrong-reason red is ever seen, S: the test Job's Spec names an expected failure marker and `[test-commands] run` must surface it; do not build it speculatively.
- **Verified:** `rg -c 'tests_not_red|tests_failed|tests_tampered|test_files_missing' cli/tests/flow/attempt.rs` -> 9; `rg -n 'TestsNotRed|TestsFailed|TestsTampered' core/src/error.rs` -> 3 codes; `sed -n '19p' .config/nextest.toml` -> `fail-fast = false`; `sed -n '200p' core/src/attempt/kind.rs` -> the not-red arm.

### 11. Ask, don't assume

- **Enforced by:**
  - `script` `core/src/ask/question.rs:39` (`ask`): a Worker's `cpd ask` opens an Escalation and blocks in 500 ms polls (`core/src/ask/question.rs:23`) until an answer or the timeout (`ask_timeout`, exit 5); only a `running` Attempt may ask (`core/src/ask/question.rs:84`).
  - `check` `core/src/ask/question.rs:124` (`wait`): the Worker Session goes `waiting` under the label `cpd: question to the Lead`; a `stopped` Session is `illegal_transition`.
  - `check` `core/src/ask/question.rs:68` (`open`): the Escalation row and its `escalation` Event are written in the transaction that starts the wait.
  - `check` `core/src/ask/reply.rs:14` (`answer`): only `open -> answered`; a second answer is `escalation_answered`, an expired one is `escalation_expired`.
  - `type` `core/src/host/pi/launch.rs:14` (`WORKER_EXCLUDED_TOOLS`, applied at `core/src/host/pi/launch.rs:166`): the Worker pi is launched with `--exclude-tools ask_user_question,...`, so its only question channel is `cpd ask`.
- **Prose only:** `core/src/spec/worker.md:54` ("If continuing would require guessing, ask the Lead and wait"); `pi/skills/cpd-lead/SKILL.md:39` ("Resolve first, ask second").
- **Gap:** nothing forces a Worker to ask: `attempt submit` checks the tests and the diff, not that a question was asked, so a Worker can guess and submit. Measured: 358 `escalation` Events, 347 `escalation-answered`, 11 `escalation-expired`; the 11 expiries are Workers that followed the rule and got no answer in `ask_minutes`. No Event counts a Worker that guessed.
- **Proposal:** none needed for the channel (it exists and blocks until answered). The 11 expiries are the one signal; the engine already emits `escalation-expired` and `core/src/inbox/rules.rs` is the surface that should make them seen, a follow-up for the Manager, not a new guard.
- **Verified:** `timeout 60 cpd events list --kind escalation --limit 5000` -> 358; `--kind escalation-answered` -> 347; `--kind escalation-expired` -> 11; `rg -c 'WORKER_EXCLUDED_TOOLS' core/src/host/pi/launch.rs` -> 2.

### 12. A question is not an instruction

- **Enforced by:**
  - `check` `core/src/plan/review.rs:379-384`: a Plan's draft Jobs become `queued` (work) only when `cpd_plan_review` is approved; presenting the plan starts nothing. The transition is pinned by the `jobs_status` trigger (`core/src/store/migrations/0001-initial.sql:302`, only `draft>queued`) and by `is_draft`/`is_under_review` (`core/src/plan/jobs.rs:68`, `core/src/plan/jobs.rs:73`). Blind spot: it gates Plan drafts only.
  - `check` `core/src/store/migrations/20261018T0828-kind-tables.sql:90`: a `question` record carries no links and only an `answer` names its `question`, so a question cannot be stored as a decision.
  - `check` `core/src/context/mod.rs:285` (`context ask`): records a question that asks nobody; `cpd context answer` is a separate act.
  - `check` `pi/lib/adapter.ts:64` with `pi/lib/askguard.ts:4` (1500-character limit) and `pi/lib/askguard.ts:86` (a cited label must appear in the visible chat): an `ask_user_question` that is not a short, grounded question is refused. Blind spot: it shapes the ask, it does not stop work started elsewhere.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:174` ("While a review is pending, don't ask the User in chat..."); `pi/skills/cpd-lead/SKILL.md:29` ("Never use `cpd context ask`: it records an open question ... and asks nobody").
- **Gap:** nothing separates a User's interrogative chat message from an instruction to an agent: a Lead or Worker can act on "should we X?" and no check refuses it. No evidence of the misbehaviour (no Event, no record); the draft gate covers the Plan, not chat.
- **Proposal:** none needed now: no evidence, and a guard on how an agent reads chat is not a guard on a raw tool (A2/hs-d248 do not force one). If evidence appears, the smallest useful step is S: make the Lead skill's "resolve first, ask second" a check only where cpd already sees the ask (the Plan review).
- **Verified:** `timeout 60 cpd events list --kind plan-review --limit 5000` -> 349 (178 pending, 168 approved, 3 rejected); `rg -n 'a decision cannot name a question' core/src/context/record.rs` -> 1; `sed -n '90p' core/src/store/migrations/20261018T0828-kind-tables.sql` -> the question CHECK; `rg -c 'draft>queued' core/src/store/migrations/*.sql | awk -F: '{s+=$2} END{print s}'` -> 3.

### 13. Reversibility

- **Enforced by:**
  - `check` `core/src/driver/run.rs:342` with `core/src/git/worktree.rs:578` (`create_attempt_worktree_with`): every Attempt runs in a worktree cut from the Lead Branch (`cpd/<plan>--<attempt>`), so a Job's work is discarded by removing the worktree, and the merge rebases/fast-forwards.
  - `check` `scripts/check-migrations-frozen.sh:1` (diff filter at `scripts/check-migrations-frozen.sh:20`, `MDR`): an applied migration is never edited, renamed or deleted. The Gate runs it first (`package.json:15`) and `scripts/install-cpd.sh:42-62` refuses an install whose migrations are not on `origin/main`, so the schema one-way door is fenced.
  - `check` `core/src/store/schema.rs:551` (generated `_no_delete` triggers) with `core/src/store/schema/tests.rs:1581` (`events_and_context_records_stay_append_only`): `txns`, `history` and `events` are append-only and `context_records` refuses UPDATE and DELETE, so a decision is superseded, never rewritten (contract §4.4).
  - `check` `core/src/git/tamper.rs:120` (`check_over`, called at `core/src/attempt/submit.rs:163`): a protected path in an Attempt's diff is `gate_tampered`, so a merge cannot silently rewrite a guarded file.
  - `check` `core/src/plan/review.rs:379`: an approval queues only the drafts it reviewed (fingerprint `current`), so a post-review edit is not silently applied.
- **Prose only:** the `AGENTS.md` axiom 13; `docs/contract.md` §4.5 "N5" states the migration rule the script enforces.
- **Gap:** nothing records the reversibility cost of a decision or names a one-way door outside the migrations (the design docs do it by hand, for example `docs/research/migrations.md:272`). No evidence of a loss from it.
- **Proposal:** none needed (YAGNI): the one class of one-way door cpd owns, the store schema, is already fenced by the frozen check and the install refusal.
- **Verified:** `sed -n '1p' scripts/check-migrations-frozen.sh` -> the usage comment; `rg -c 'MDR' scripts/check-migrations-frozen.sh` -> 1; `ls core/src/store/migrations/*.sql | wc -l` -> 45; `sed -n '1581p' core/src/store/schema/tests.rs` -> the append-only test; `rg -n 'CREATE TRIGGER \{table\}_no_delete' core/src/store/schema.rs` -> 1 (the DELETE trigger is generated, not written in a migration).

### 14. Idempotence

- **Enforced by:**
  - `test` `cli/tests/flow/context.rs:106` (`new_is_idempotent_for_the_same_title_and_jira`): `cpd context new` twice writes nothing the second time and answers `already`; blind spot: one command.
  - `test` `core/src/store/schema/tests.rs:1195` (`the_import_of_migration_7_is_idempotent_on_its_own`): a migration import run again changes no row; blind spot: migration 7 only.
  - `test` `cli/tests/flow/plan_cancel.rs:275` (a rerun writes nothing), `:290`, `:312` (a rerun finishes a half-finished cancel); `core/src/plan/cancel.rs:1` documents "every step is idempotent"; blind spot: one command.
  - `test` `cli/tests/pr/pr.rs:818` (`open_again_is_already_and_touches_nothing`), `cli/tests/pr/pr_crud.rs:412` (`add_again…`), `cli/tests/flow/plan.rs:982` (a rerun writes nothing), `cli/tests/tooling/harness.rs:685` (`teardown_is_idempotent_and_spares_the_test_process`), `core/src/host/git_guard.rs:161` (`install_twice_leaves_one_identical_file`); blind spot: each is per-command and per-author.
  - `test` the whole family: 58 test functions named `*_again`/`*_twice`/`*idempotent*` across `cli/tests` and `core/src`; blind spot: there is no check that every idempotent command has one.
- **Prose only:** `docs/commands.md` marks 95 command sections `Idempotent:`; no code parses the label (`rg` finds 16 lines with `Idempotent` outside `docs/`, all comments in tests), so a label is a claim, not a check. Axiom: `/home/clayton/.pi/agent/AGENTS.md:55`.
- **Gap:** nothing checks that a command labelled `Idempotent: yes` is actually idempotent, and one label is already false: `docs/research/quality-commands.md:116` records "the idempotence statement ("Idempotent: yes") is false for it" for `plan new` into an archived Context.
- **Proposal:** `check`: a docs test that parses every `Idempotent:` line of `docs/commands.md` and requires a named rerun test for each `yes`, so the label cannot drift, plus a rerun test for `plan new` into an archived Context. Size M.
- **Verified:** `timeout 60 rg -c "Idempotent:" docs/commands.md` → 95; `timeout 120 rg -n "fn [a-z0-9_]*(idempotent|twice|_again)" cli/tests core/src --glob '*.rs' | wc -l` → 58; `timeout 60 rg -n "Idempotent" --glob '!node_modules' --glob '!target' --glob '!docs/**' . | wc -l` → 16; `timeout 60 sed -n '106p' cli/tests/flow/context.rs` → `fn new_is_idempotent_for_the_same_title_and_jira() {`; `timeout 60 sed -n '1195p' core/src/store/schema/tests.rs` → `fn the_import_of_migration_7_is_idempotent_on_its_own() {`.

### 15. Composition over inheritance

- **Enforced by:**
  - `type` Rust (`core/src/`): the language has no inheritance, so the core is 231 `impl` blocks over 2 traits — parts, never a hierarchy; blind spot: a deep trait-object or `Box<dyn>` stack is still writable and unchecked.
  - `type` `tsconfig.json:5` (`"erasableSyntaxOnly": true`): the pi extension cannot use TS-only runtime constructs (enums, namespaces, parameter properties), so its parts stay plain functions and objects; blind spot: it does not ban `class … extends`.
  - `test` none: no test bans an inheritance chain.
- **Prose only:** the axiom is `/home/clayton/.pi/agent/AGENTS.md:59`; no repo doc or skill restates it (`rg -ni "composition|inherit" docs/implementing.md docs/contract.md pi/skills core/src --glob '*.md' --glob '*.rs'` finds no statement of the rule, only unrelated uses of "inherit" for processes and locks).
- **Gap:** no check refuses a new `class X extends Y` in cpd-authored `pi/`; today 2 authored classes extend `Error` (`pi/lib/cpd.ts:6`, `pi/lib/slack-channel.ts:26`) and 4 upstream-derived `pi/compact-tools/` classes extend framework classes (`pi/compact-tools/extensions/compact-tools-layout.ts:98,150`, `.../compact-tools.ts:199`, `.../compact-tools-grouping.ts:201`); a `DataStore extends BaseStore extends Store` chain would pass. Evidence: 6 `class … extends` in `pi/` outside `pi/test/` — 2 of them `Error` subclasses, 4 framework component subclasses, all composition with a library rather than a domain hierarchy.
- **Proposal:** `none needed`: Rust cannot inherit and `erasableSyntaxOnly` already strips TS-only runtime class syntax, and the six `extends` are an `Error` subclass and framework component subclasses. If a guard is wanted, S: a test refusing a `class … extends` in cpd-authored `pi/lib/` or `pi/*.ts` (never in an upstream fork).
- **Verified:** `timeout 60 rg -c "^impl " core/src --glob '*.rs' | awk -F: '{s+=$2} END{print s}'` → 231; `timeout 60 rg -c "^pub trait |^trait " core/src --glob '*.rs' | awk -F: '{s+=$2} END{print s}'` → 2; `timeout 60 rg -n "class [A-Za-z0-9_]+ extends" pi --glob '*.ts' | grep -v "pi/test/" | wc -l` → 6; `timeout 60 rg -n "class [A-Za-z0-9_]+ extends" pi/lib pi/*.ts --glob '*.ts'` → `pi/lib/cpd.ts:6`, `pi/lib/slack-channel.ts:26`; `timeout 60 rg -n "erasableSyntaxOnly" tsconfig.json` → `tsconfig.json:5`.

### 16. Small, atomic commits

- **Enforced by:**
  - `check` `core/src/attempt/submit.rs:53,66` (`no_commits`): submit runs `git rev-list --count <base>..HEAD` and refuses when it is 0, so at least one commit is required; blind spot: it checks presence, never size, count or message.
  - `gate` `core/src/pr/gh.rs:324`: the PR is squash-merged (`gh pr merge … --squash`), so the PR title becomes the one commit on `main`; blind spot: it collapses Job commits after the fact and constrains none of them.
  - `test` `cli/tests/flow/spec.rs:70`: the rendered Worker Spec must contain `Commit a skeleton within 10 minutes`; `pi/skills/skill.test.ts:242` pins the same wording in `spec-files.md`; blind spot: both assert the prose exists, not that commits happen.
  - `gate`/`hook` `.config/wt.toml:5` (`pre-commit` = `npm run typecheck` + `cargo build`) and `cliff.toml:18` (`conventional_commits = false`): no hook or changelog rule reads a commit's size or message.
- **Prose only:** `core/src/spec/worker.md:59` ("Commit a skeleton within 10 minutes of starting, then commit at least every 20 minutes") and `pi/skills/cpd-lead/references/spec-files.md:46`. Axiom: `/home/clayton/.pi/agent/AGENTS.md:63`.
- **Gap:** a Worker can push one 4000-line commit and pass every check; nothing measures commit size, count or message, and no metric records commit shape. Evidence: `rg -n "rev-list" core/src/attempt/submit.rs` = 1 line, and the cadence is pinned only as text (`cli/tests/flow/spec.rs:70`).
- **Proposal:** `check` S: record the commit count and the diff size of the Attempt at submit (`git rev-list --count` already runs) and refuse, or write them into the Job's `metrics`, over a documented threshold, pinned by a test in `cli/tests/flow/attempt.rs`. This is a submit check, not a guard on `git`.
- **Verified:** `timeout 60 rg -n "rev-list" core/src/attempt/submit.rs` → `53:`; `timeout 60 rg -n "NoCommits|no_commits" core/src --glob '*.rs'` → `submit.rs:66`, `error.rs:178`; `timeout 60 sed -n '59p' core/src/spec/worker.md` → `- Commit a skeleton within 10 minutes of starting, then commit at least every 20 minutes.`; `timeout 60 sed -n '70p' cli/tests/flow/spec.rs` → `assert!(spec.contains("Commit a skeleton within 10 minutes"));`; `timeout 60 sed -n '18p' cliff.toml` → `conventional_commits = false`; `timeout 60 sed -n '5p' .config/wt.toml` → `[pre-commit]`.

### 17. Prose is not enforcement

- **Enforced by:** the rules of this repo that DO have a guard (each is a `## Rules` bullet of a skill, cited by `SKILL.md:line`):
  - `guard` `pi/lib/secretguard.ts:5` (`secretBypass`) applied at `pi/lib/adapter.ts:61`: lead `SKILL.md:31` / manager `SKILL.md:32` "Agents never call `op` directly" — refuses `op` and `cpd secret approve` in any Session; blind spot: only those two words.
  - `guard` `core/src/job/mod.rs:266` (`kind_reserved`): lead `SKILL.md:33` "never `cpd job add --kind open`" — refuses the reserved kinds; blind spot: only those two kinds.
  - `guard` `pi/lib/noblock.ts:6,70` (`BOUND_SECONDS`, `blockingReason`) applied at `pi/lib/adapter.ts:75`: manager `SKILL.md:30` "Never block … 30 seconds" — refuses a `bash` `sleep`/`timeout`/`watch`/`tail -f`/`cpd events follow`/`gh … --watch` over 30 s; blind spot: Manager only, and only the listed words.
  - `guard` `pi/lib/askguard.ts:4,29,35,86` (`LIMIT`, `tooLong`, `unseenReason`, `unseen`) applied at `pi/lib/adapter.ts:65,68`: lead `SKILL.md:29` / manager `SKILL.md:33` "a few short choices" — refuses a question over 1500 chars and (Lead/Manager) a label (A4, D1–D5) stated nowhere; blind spot: dialog shape, not "ask the User".
  - `guard` `pi/lib/mainguard.ts:65` (`primaryCheckoutMove`) applied at `pi/lib/adapter.ts:59`: lead `SKILL.md:30` "Never move HEAD or change the files of a repo's primary checkout" (the `git` half) — refuses the seven git subcommands in a primary checkout; blind spot: not `edit`/`write`, not the other tools.
  - `guard` `pi/lib/loop-guard.ts:47,58`: lead `SKILL.md:40` "Never poll, sleep or run a foreground loop" — trips a breaker after more turn starts than `loop_guard_turns`; blind spot: counts turns, never sleeps, and does not block the call.
  - `guard` `core/src/attempt/submit.rs:40,66` and `core/src/attempt/kind.rs:34`: the Worker prompt rules "Commit your work", "Never edit the protected paths" and the red test; blind spot: the Worker's raw tools are otherwise unguarded.
  - `check` `pi/lib/turns.ts:83,92` + `pi/lib/reminder.ts:61`: lead `SKILL.md:41` "Register planned work at once" — a hidden `agent_end` reminder when the last message speaks of future work and names no Job/PR key; blind spot: it warns, it never blocks.
  - `check` `core/src/job/edit.rs:97,106` (`job_locked`): lead `SKILL.md:43` "Append-only once locked"; blind spot: Locked Jobs only.
  - `type` `core/src/output.rs`: lead `SKILL.md:37` / manager `SKILL.md:40` "Every `cpd` command prints one JSON value" — the `Output` enum is the only printer; blind spot: a type, not a runtime check.
  - `guard` `core/src/host/git_guard.rs::WRAPPER`: `git stash` is refused in every Session by the `$CPD_HOME/bin/git` PATH wrapper (hs-d310); blind spot: only `stash`.
- **Prose only:** the sampled rules with NO guard: lead `SKILL.md:28` "Never write product code, edit the repo checkout, or commit product changes" (the decided `edit`/`write` half of hs-d606 is absent from the code); lead `SKILL.md:30` "never run `tmux` or `wt`, never force-push, never `git push`"; lead `SKILL.md:32` "Never run `cpd plan decide`/`watch`/`attempt run`/`job dispatch`/`session run`/`tui`/`events follow`" (noblock covers a Manager, not a Lead); lead `SKILL.md:34,35,36` (pane env, `session rename`, session id forms); lead `SKILL.md:38` "Ask the User directly, never through the Manager"; lead `SKILL.md:39` "Resolve first, ask second"; lead `SKILL.md:42` "Decided work is queued, undecided design is a draft"; lead `SKILL.md:44` "Never review inside your own session"; lead `SKILL.md:45` debug commands; manager `SKILL.md:27` "Records first"; manager `SKILL.md:28` "Never answer for the User"; manager `SKILL.md:29` "Act only when the User talks to you"; manager `SKILL.md:31` "Never … commit"; manager `SKILL.md:34` "Never run `tmux` or `wt`, never type into a pane"; manager `SKILL.md:36` "Never run `cpd plan decide`/`watch`/… `context manager`"; manager `SKILL.md:38` "Keep the work moving". Axiom: `/home/clayton/.pi/agent/AGENTS.md:67`.
- **Gap:** of the 32 `## Rules` bullets the two skills hold (18 + 14), most have only prose; the weakest is the one with evidence: lead `SKILL.md:28` forbids editing a repo checkout, but the adapter guards only seven git subcommands. hs-d606 (2026-10-24) records two Workers writing `docs/*.md` into `/home/clayton/projects/cpd` with the `edit` tool by absolute path (`anim-perf stutter-docs-a1`, `sync engine-tick-docs-a1`) and decides fix (1): "the adapter refuses an agent's `edit`/`write` (and the file-writing forms of bash it can detect) to a path in a primary checkout, extending 15.16". `rg -n "edit/write|the edit tool|file-writing" docs/commands.md docs/contract.md` returns nothing and §15.16 documents only the seven git subcommands, so the decided guard is not in the code, tests or docs of this worktree.
- **Proposal:** `guard` M: implement hs-d606 fix (1) — extend the primary-checkout guard to the `edit` and `write` tool calls in `pi/lib/adapter.ts` (reuse `primaryRoot`/`resolveDir` from `pi/lib/mainguard.ts`), with its test in `pi/test/mainguard.test.ts`. Evidence: hs-d606 (two named incidents), so it satisfies "a guard only for proven bad behaviour"; no other proposal is recommended without evidence.
- **Verified:** `timeout 60 rg -n "export function secretBypass|export function blockingReason|export function primaryCheckoutMove|export function tooLong|export function unseen" pi/lib/secretguard.ts pi/lib/noblock.ts pi/lib/mainguard.ts pi/lib/askguard.ts` → `secretguard.ts:5`, `noblock.ts:70`, `mainguard.ts:65`, `askguard.ts:29`, `askguard.ts:86`; `timeout 60 awk 'NR>=26 && NR<=45 && /^- /' pi/skills/cpd-lead/SKILL.md | wc -l` → 18; `timeout 60 awk 'NR>=25 && NR<=40 && /^- /' pi/skills/cpd-manager/SKILL.md | wc -l` → 14; `timeout 60 rg -n "toolName ===|toolName !==" pi/lib/adapter.ts` → only `bash`, `ask_user_question`, `plannotator`; `timeout 60 rg -n "primary|checkout" pi/lib --glob '*.ts' | grep -v mainguard | wc -l` → 0; `timeout 60 rg -n "edit/write|the edit tool|file-writing" docs/commands.md docs/contract.md` → 0 lines; `timeout 60 rg -c "^### 15\." docs/commands.md` → 18 sections, none named for `edit`/`write`.

### 18. Tools over scraping

- **Enforced by:**
  - `guard` (dependency absence) the five manifests declare no HTTP client: GitHub is reached by spawning `gh` (`core/src/pr/gh.rs:148`, `core/src/inbox/team.rs:194`), git by `git`, tmux by `tmux`; blind spot: `docs/implementing.md:153` states "Later Jobs may not add, remove or change dependencies" but no test pins the dependency list, so a new HTTP crate would pass every check.
  - `test` `cli/tests/common/mod.rs:307` (`DENIED_TOOLS = ["gh","pi","curl","wget","ssh"]`) and `:751` installs a deny shim (exit 97) into every `TestEnv`'s `bin/`; a test that made cpd fetch a page would exit 97. Blind spot: the harness only, and only `gh` is exercised — 74 `fake_gh`/`fake("gh")` call sites install a purpose-built fake, none a scraper.
  - `check` none: no production guard refuses a Session's `bash` `curl`/`wget` (0 occurrences of `curl`/`wget` in product code).
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:30` ("`git` and `gh` are allowed for reading") and `docs/commands.md:1737` (the `gh api graphql` poll); `docs/implementing.md:177` lists crates "deliberately absent". Axiom: `/home/clayton/.pi/agent/AGENTS.md:71`.
- **Gap:** an agent or a future dependency could fetch a page; the only enforcement is the absence of an HTTP crate in the workspace and the test harness's deny shims. Evidence: 0 `curl`/`wget` in product code, 0 HTTP-client dependencies, and no record of scraping. A guard on the raw `bash` `curl`/`wget` therefore has `no evidence: do not guard`.
- **Proposal:** `check` S: a test (in `cli/tests/tooling/` or `pi/skills/skill.test.ts`) that the five manifests declare no HTTP-client crate, pinning `docs/implementing.md:153`; no guard on a raw tool.
- **Verified:** `timeout 60 rg -c "reqwest|ureq|isahc|attohttpc|hyper|surf" Cargo.toml core/Cargo.toml cli/Cargo.toml engine/Cargo.toml tui/Cargo.toml` → 0 (no output); `timeout 60 rg -n "Command::new\(\"gh\"\)" core/src --glob '*.rs'` → `pr/gh.rs:148`, `inbox/team.rs:194`; `timeout 60 rg -n "DENIED_TOOLS" cli/tests/common/mod.rs` → `307`, `751`; `timeout 60 rg -n "fake_gh|fake\(\"gh\"" cli/tests --glob '*.rs' | wc -l` → 74; `timeout 60 rg -c "\bcurl\b|\bwget\b" core/src cli/src engine/src tui/src pi/lib pi/*.ts` → 0 (no output).

### 19. Brevity is not vagueness

- **Enforced by:**
  - `check` `pi/lib/askguard.ts:4` (`LIMIT = 1500`) with `:29` (`tooLong`) applied at `pi/lib/adapter.ts:65`: an `ask_user_question` call whose text sums over 1500 characters is refused, so a dialog cannot ramble; blind spot: dialogs only, and it is a ceiling, not a style.
  - `check` `pi/lib/askguard.ts:35,86` (`unseenReason`, `unseen`) applied at `pi/lib/adapter.ts:68`: a cited label must be stated in the question or the visible chat, forcing the point into the text; blind spot: Lead/Manager only.
  - `check` `core/src/plan/outline.rs:18` (`LINE_MAX = 200`), `core/src/args.rs:217` (`MAX_CHARS = 72`), `pi/ask-user-question/tool/types.ts:8,9` (`MAX_HEADER_LENGTH = 16`, `MAX_LABEL_LENGTH = 60`): outline lines, PR titles and dialog labels are capped; blind spot: each is a length cap on one artefact.
  - `test` `pi/lib/render.ts:49`: a long summary is truncated with `…` for display; blind spot: display only.
- **Prose only:** the axiom is `/home/clayton/.pi/agent/AGENTS.md:75`; no repo doc or skill restates it — `rg -ni "brevity|preamble|closing offer|lead with the answer" pi core/src cli/src --glob '*.ts' --glob '*.rs'` gives 13 lines, all in `core/src/session/retitle.rs` tests about the title parser's preamble and in `pi/test/policy.test.ts`, none an enforcement.
- **Gap:** nothing measures preamble, recap, closing offers, or the "failures first" ordering; the caps only stop extreme length. Evidence: 13 hits for the axiom's words, none a check; the 206 `error` Events (top `distill_invalid` 80, `gh_failed` 60, `cleanup_failed` 15) carry none about message style.
- **Proposal:** `none needed`: the enforceable parts (dialog length, label visibility, title/outline/label caps) already are; a content lint for "no preamble" would be brittle and would itself need evidence. If wanted, S: a `check` in `pi/lib/askguard.ts` refusing a Lead/Manager message that ends with a closing-offer phrase, pinned by a test — not recommended.
- **Verified:** `timeout 60 rg -n "LIMIT = 1500" pi/lib/askguard.ts` → `4`; `timeout 60 rg -n "unseenReason" pi/lib/askguard.ts` → `35`; `timeout 60 rg -n "LINE_MAX|MAX_CHARS|MAX_HEADER_LENGTH|MAX_LABEL_LENGTH" core/src/plan/outline.rs core/src/args.rs pi/ask-user-question/tool/types.ts` → `outline.rs:18`, `args.rs:217`, `types.ts:8`, `types.ts:9`; `timeout 60 rg -ni "brevity|preamble|closing offer|lead with the answer" pi core/src cli/src --glob '*.ts' --glob '*.rs' | wc -l` → 13; `timeout 60 cpd events list --kind error --limit 500 | python3 -c "import json,sys; print(len(json.load(sys.stdin)['events']))"` → 206.

### 20. Make it work, make it fast, make it good

- **Enforced by:**
  - `check` `core/src/plan/shape.rs:44` (`check`) with `core/src/job/mod.rs:310` (`check_test_job_required`): every draft `impl` Job must need a `test` Job of its PR, so tests come before implementation ("make it work" first); blind spot: it checks the dependency, not the order of the Jobs' content.
  - `check` `core/src/plan/shape.rs:16,22`: a `tdd` PR that has not opened must keep a `test` and an `impl` Job; the required kinds are exactly `["test", "impl"]`, so a `tdd` PR with no `docs` Job passes; blind spot: the contract-amended-first (`docs`) step is not required.
  - `check` `core/src/attempt/kind.rs:194,200,206`: a `test` Job's named tests must build and fail at submit, an `impl` Job's must pass; blind spot: it pins red/green, not that the red test came from the docs.
  - `test` `cli/tests/pr/pr_crud.rs:288,291`: pins that `pr add --shape tdd` lays out `docs`, `test` (needs `api-docs`) and `impl` (needs `api-tests`) — the template in `core/src/pr/add.rs:52-54`; blind spot: a Job added by hand need not follow the template.
  - `gate` `.config/nextest.toml:33` (`slow-timeout = 30s`, terminate after 3) fails a slow test and `:24` (`leak-timeout … fail`) fails a leaking one, so "make it fast"/"make it good" have tripwires; blind spot: per-test limits only, no performance budget for a feature.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:158` states the order docs → red tests → impl; `pi/skills/skill.test.ts:263` pins that this sentence exists (it asserts the order inside the prose), not that a Plan follows it. Axiom: the global AGENTS.md (in my system prompt; not on the on-disk file).
- **Gap:** the `docs`-first step is not enforced; a `tdd` PR can be reviewed and locked with only `test` + `impl`. Evidence: `core/src/plan/shape.rs:22` lists `["test", "impl"]`, and `rg -n "docs" core/src/plan/shape.rs core/src/job/mod.rs` finds 0 — `docs` appears in neither `missing()` nor `check_test_job_required`.
- **Proposal:** `check` S: either add `docs` to the required kinds of a `tdd` PR in `missing()` (`core/src/plan/shape.rs:22`), with its test in `cli/tests/flow/plan.rs`, or declare the docs step prose-only; the tests-before-impl half already holds.
- **Verified:** `timeout 60 sed -n '22p' core/src/plan/shape.rs` → `["test", "impl"]`; `timeout 60 rg -n "fn check_test_job_required" core/src/job/mod.rs` → `310`; `timeout 60 rg -n "docs" core/src/plan/shape.rs core/src/job/mod.rs | wc -l` → 0; `timeout 60 sed -n '52,54p' core/src/pr/add.rs` → `stub("docs", …)`, `stub("tests", "test", … Some("docs"))`, `stub("impl", "impl", … Some("tests"))`; `timeout 60 sed -n '288p;291p' cli/tests/pr/pr_crud.rs` → the kinds and the `needs: ["api-docs"]`; `timeout 60 sed -n '24p;33p' .config/nextest.toml` → the leak-timeout and slow-timeout lines.

### 21. Fix the cause, not the symptom

- **Enforced by:**
  - `check` `core/src/attempt/kind.rs:80,83` (`test_files_missing`): a `test` Job's named files must appear in its diff, so a fix cannot claim a reproduction it did not write; blind spot: the file, not its content.
  - `check` `core/src/attempt/kind.rs:194,200` + `core/src/attempt/submit.rs`: the `test` Job's named tests must build and then **fail** at submit (`tests_not_red`), so a fix starts from a red reproduction; blind spot: it cannot tell a reproduction of the reported cause from any other failing test.
  - `check` `core/src/attempt/kind.rs:34` (`guard_tests`, `tests_tampered`): the `impl` Job may not edit the red test files without the Lead's approval, so it cannot pass by weakening the reproduction; blind spot: the Lead may approve a change that hides the symptom.
  - `test` `cli/tests/flow/attempt.rs:559` (`a_test_job_whose_named_tests_pass_is_refused_tests_not_red…`), `:681` (`an_impl_job_is_run_against_the_union…`): pins the red/green contract; blind spot: behaviour-change Jobs, not bug fixes as such.
  - `test` `pi/skills/skill.test.ts:265` pins the sentence "A bug or flake fix starts with a test that reproduces it" in `SKILL.md:158`; blind spot: it enforces the prose, not the practice.
  - `gate` `scripts/gate-run.sh` (the Gate leak check, hs-d519) and `.config/nextest.toml:24` (leak-timeout fails a leaking test) make a class of flakes fail their own run; blind spot: leaks only, not flakes generally.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:158` (reproduce first); axiom: the global AGENTS.md (in my system prompt; not on the on-disk file).
- **Gap:** nothing distinguishes a bug/flake-fix Job from a feature Job — any `tdd` PR's red test satisfies the guard — and no metric records flakes, leaks or sleeps, so a symptom patch is invisible. Evidence: the Attempt `metrics` are `turns, tokens_*, cache_*, cost_usd, duration_s, model, thinking_level, stop_reason` only (`core/src/metrics/mod.rs:29-38`), and the 206 `error` Events carry no `flake`/`leak`/`sleep` code.
- **Proposal:** `check` S: require a fix Job's Spec to name the failing Event id or Context record it fixes (a `fixes` field validated in `core/src/spec/`), pinned by a test in `cli/tests/flow/spec.rs`, so "cause not symptom" is reviewable; the red-test guard already enforces the reproduction step. For flakes, M: record a `flake`/`leak` metric at the Gate's leak check so the count is measurable.
- **Verified:** `timeout 60 rg -n "let missing|TestFilesMissing|TestsNotRed|fn guard_tests" core/src/attempt/kind.rs` → `34`, `80`, `83`, `201`; `timeout 60 sed -n '194p;200p' core/src/attempt/kind.rs` → the `run` comment and the `("test", HookOutcome::Passed)` arm; `timeout 60 rg -n "fn a_test_job_whose_named_tests_pass_is_refused_tests_not_red|fn an_impl_job_is_run_against" cli/tests/flow/attempt.rs` → `559`, `681`; `timeout 60 rg -n "reproduces it" pi/skills/skill.test.ts pi/skills/cpd-lead/SKILL.md` → `skill.test.ts:265`, `SKILL.md:158`; `timeout 60 sed -n '29,38p' core/src/metrics/mod.rs` → the metric fields; `timeout 60 cpd events list --kind error --limit 500 | python3 -c "import json,sys; print(len(json.load(sys.stdin)['events']))"` → 206.

### 22. Guards only for proven bad behaviour

- **Enforced by:**
  - `guard` `pi/lib/mainguard.ts:65` (`primaryCheckoutMove`, applied in the one handler at `pi/lib/adapter.ts:59`): refuses `checkout switch reset rebase merge pull stash` run in a primary checkout, for every role; evidence hs-n436 and hs-d248 at `docs/commands.md:3744`. Blind spot: only those seven git subcommands, only through the `bash` tool (Plan `main-mirror` PR `guard` extends it to `edit`, `write` and bash writes), and not judged through `sh -c`, `xargs` or `GIT_DIR`.
  - `guard` `pi/lib/secretguard.ts:5` (`secretBypass`, applied at `pi/lib/adapter.ts:61`): refuses `op` and `cpd secret approve`; its docs section `docs/commands.md:3772` cites **0** record ids. Blind spot: only the command word of a simple command; `sh -c`, an alias or a script passes.
  - `guard` `pi/lib/noblock.ts:70` (`blockingReason`, applied at `pi/lib/adapter.ts:75`): refuses a Manager's `bash` wait over 30 s; `docs/commands.md:3624` cites **0** record ids. Blind spot: literal durations only (`sleep $n` passes), Manager role only.
  - `guard` `pi/lib/testguard.ts:77` (`unfilteredTestRun`, applied at `pi/lib/adapter.ts:78`): refuses a Worker's unfiltered `cargo test`, `cargo nextest run`, `npm test` and `npm run verify`; `docs/commands.md:3656` cites **0** record ids. Blind spot: a filter heuristic; a run the heuristic does not read passes.
  - `guard` `core/src/host/git_guard.rs:30` (the `WRAPPER`, put first on the Session's `PATH` by `core/src/host/git_guard.rs:88`): refuses `git stash` in every Session; evidence hs-d310 at `core/src/host/git_guard.rs:19` and `docs/contract.md:2879`. Blind spot: `stash` only; `/usr/bin/git` by absolute path bypasses it.
  - `guard` `pi/lib/loop-guard.ts:47`, `pi/lib/askguard.ts:29` and `:86` (applied at `pi/lib/adapter.ts:66`), `pi/lib/pnguard.ts:23`: guards on adapter deliveries, on the `ask_user_question` tool and on the `plannotator` tool, not on a raw tool; evidence hs-d552/hs-d571 (`docs/commands.md:3730`) and hs-d502/hs-d548/hs-d553/hs-n425 (`docs/commands.md:3682`). Blind spot: they bound tools cpd adds, not git, gh, edit or the shell.
  - `none`: no mechanism requires a guard to be justified by evidence; the rule is not itself enforced.
- **Prose only:** `docs/commands.md:3746` (the adapter guard of §15.16 applies "hs-d248, a guard only for behaviour seen in practice") and `docs/research/repo-policy.md:288` (typed keys land "only with evidence (hs-d248)").
- **Gap:** no guard code names the record or Event that justified it (`rg -o 'hs-d[0-9]+' pi/lib | wc -l` → 0), and three raw-tool guards (on the shell) cite no record id in their docs section: `noblock` (0), `testguard` (0), `secretguard` (0), where `mainguard` cites 4 and the loop guard 2. A new guard on `git`, `gh`, `edit` or the shell can therefore be added today with prose and no evidence, and three were.
- **Proposal:** `check`: a test `pi/test/guards.test.ts` that reads every guard module's header comment and its docs section and fails when a shell/git/edit guard names no record id or Event code; before it can pass, `noblock` (§15.11), `testguard` (§15.12) and `secretguard` (§15.17) must record the evidence that justified them, or be removed (`no evidence: do not guard`). Size S.
- **Verified:**
  - `timeout 60 bash -c "rg -o 'hs-d[0-9]+' pi/lib | wc -l"` → 0; `ls pi/lib | grep -E 'guard|noblock' | wc -l` → 7.
  - `timeout 60 bash -c 'for r in "3624 3655 15.11" "3656 3681 15.12" "3682 3719 15.13" "3730 3743 15.15" "3744 3771 15.16" "3772 3823 15.17"; do set -- $r; sed -n "$1,$2p" docs/commands.md | rg -o "hs-[dn][0-9]+" | sort -u | wc -l; done'` → 0, 0, 8, 2, 4, 0.

### 23. Blocked means it needs the human

- **Enforced by:**
  - `type` `pi/lib/state.ts:7` (`Kind = "human" | "agent"`) with `pi/lib/state.ts:59` (`wanted()`): `blocked` is reported only for the latest open `human` reason, `waiting` for the latest open `agent` one; an agent reason can never yield `blocked`.
  - `test` `pi/test/state.test.ts` (16 tests): "a Worker question alone reports waiting with its label"; "a Worker question opened during a dialog does not hide it … still blocked `dialog`"; "the dialog closed with a Worker question still open reports waiting".
  - `check` `core/src/session/state.rs:54` (`legal`): the transition table; `core/src/session/state.rs:19` (`Label`): a `waiting`/`blocked` label is 1–80 characters, no controls.
  - `test` `core/src/inbox/rules.rs:703` asserts the Inbox treats any `blocked` as a dialog "whatever its label".
  - `none`: nothing in the core enforces *which* kind of work may be `blocked`; the distinction lives only in the adapter.
- **Prose only:** `docs/commands.md:3359` (§15.4), the comments `core/src/session/row.rs:38` ("`blocked` needs the User, `waiting` needs another agent") and `core/src/inbox/rules.rs:25` ("a Worker's question goes to its Lead, and the Lead that waits for the Worker is `waiting`, not `blocked`").
- **Gap:** `session state` accepts `blocked` with any label, including work another agent clears. Real Events: across all `session-state` Events (15 026), 905 transitions were to `blocked` and 200 of them carried the agent-work label `cpd: Worker question` (203 more `cpd: question to the Lead`), against 298 to `waiting` (`cpd: Worker question` 148). Those 403 `blocked` Events with agent labels are exactly the mislabels hs-d303 fixed in the adapter; the core still permits them, so the invariant can regress without any check failing.
- **Proposal:** `type`: give `cpd session state` an explicit `--kind human|agent`, store it, and refuse `blocked` with `agent` (and `waiting` with `human`) in `core/src/session/state.rs`, so the invariant is a refused state and not adapter discipline; the adapter already holds the kind (`pi/lib/state.ts:7`) and would pass it. Pinned by a `cli/tests/flow/session.rs` test and a `core/src/inbox/rules.rs` test; Size M.
- **Verified:**
  - `timeout 120 cpd events list --kind session-state --limit 5000`, then paged with `--since <next>` (5 pages): 15 026 Events; 905 to `blocked` (`dialog` 317, `cpd: question to the Lead` 203, `cpd: Worker question` 200, `cpd: plan review` 185); 298 to `waiting` (`cpd: question to the Lead` 150, `cpd: Worker question` 148).
  - `timeout 60 bash -c "rg -c '^test\(' pi/test/state.test.ts; rg -c '#\[test\]' core/src/inbox/rules.rs"` → 16; 58.

### 24. No planned work only in chat

- **Enforced by:**
  - `check` `pi/lib/reminder.ts:61` (`needsReminder`) with `pi/lib/turns.ts:71` and `:83`: at a Lead's run end, if its last assistant text matches the fixed phrase list and names no Job or PR key of its Plan, the next run carries one hidden (`display: false`) `cpd-reminder` message; 12 tests in `pi/test/reminder.test.ts`.
  - `check` `pi/lib/reminder.ts:11` (`PHRASES`): the whole definition is a fixed list of 19 phrases plus `after … merges`; extending it is a change to `docs/commands.md:3540`.
  - `test` `pi/skills/skill.test.ts:502` pins the skill prose ("Register planned work at once", "Check each Plan's registered work").
  - `check` `docs/commands.md:1263` and `docs/commands.md:1270`: a stub draft needs only `--title` (`cpd job add <k> --draft --title "…"`), so registration is one command at any time.
  - `none`: nothing blocks a Lead that plans in chat without registering.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:41` ("No planned work lives only in your chat"), `pi/skills/cpd-manager/SKILL.md` (the Manager checks registered work), `docs/commands.md:3542`.
- **Gap:** the reminder is a heuristic and invisible: it fires only for role `lead`, only after a `cpd plan show` that succeeds, only on the 19 fixed substrings, and a firing is not recorded (`rg -l 'reminder' ~/.cpd/run/adapter/*.log | wc -l` → 0 of 1327 logs; only a failed check is logged), so there is no measurement of how often planned work stays in chat, and any synonym outside the list escapes.
- **Proposal:** `check`: append one line to the Lead's adapter log when the reminder is due (for example `reminder: due`) and assert it in `pi/test/reminder.test.ts`, so the mechanism's rate is measurable; keep the phrase list. Size S.
- **Verified:**
  - `timeout 60 bash -c "awk '/export const PHRASES/,/\\];/' pi/lib/reminder.ts | grep -c '  \"'"` → 19.
  - `timeout 60 bash -c "rg -l 'reminder' ~/.cpd/run/adapter/*.log 2>/dev/null | wc -l; ls ~/.cpd/run/adapter/*.log | wc -l"` → 0; 1327.
  - `timeout 60 bash -c "rg -c '^test\(' pi/test/reminder.test.ts"` → 12.

### 25. Recency wins

- **Enforced by:**
  - `check` `core/src/context/contexts.rs:254` (`check_links`): `--supersedes` must name an existing record whose kind the writer may supersede (`supersedes_invalid`, `record_not_found`); `core/src/context/record.rs:276` (`Draft::from_parts`) applies the same pairing. Because every write appends the next `seq`, the target is always older, so the later record can never be removed by an earlier one.
  - `check` `core/src/context/distill.rs:334`: a `superseded` retirement is refused unless its target is in force, is not the record itself and has a **greater** `seq` ("a later record supersedes").
  - `check` `core/src/context/inforce.rs:31`: a record named by any later record's `supersedes` leaves force; `context show` labels it with its successor (`docs/contract.md:229`, `docs/commands.md:324`).
  - `gate` `core/src/store/migrations/20261018T0828-kind-tables.sql:106` (`context_records_immutable`, `BEFORE UPDATE`): a record is never edited, so the earlier record is "superseded, never edited".
  - `test` `core/src/context/inforce.rs` (9 `#[test]`s: a chain of supersessions leaves its last link; as of a `seq` a later supersede does not count) and `cli/tests/flow/context.rs` (4 `supersedes_invalid` assertions).
  - `none`: no check detects that two in-force records contradict when neither names the other.
- **Prose only:** `docs/contract.md:229` ("The User's rule is **recency wins**"), `docs/commands.md:320` (`--supersedes`, "recency wins").
- **Gap:** contradiction *detection* is gone by decision: hs-d549 and hs-d551 removed the `## Contradictions` section (`docs/contract.md:2029`, `docs/commands.md:521`) and `cli/tests/flow/context_distill.rs:442` now asserts the distiller prompt contains no "Contradictions"; the distiller only *proposes* a `superseded` retirement from its own model output, which the User approves. Two contradicting records in force therefore both stay active until someone supersedes them by hand. That is the accepted design, not a defect, but it is where recency is not enforced.
- **Proposal:** `none needed`: detecting a semantic contradiction needs a model, and the accepted replacement (the curating distiller proposing a User-approved `superseded` retirement) is already the smallest version of that; the write-time mechanisms above already make the direction of a supersession impossible to invert.
- **Verified:**
  - `timeout 60 bash -c "rg -c '#\[test\]' core/src/context/inforce.rs; rg -c 'supersedes_invalid' cli/tests/flow/context.rs"` → 9; 4.
  - `timeout 60 bash -c "rg -c 'Contradiction' docs/commands.md docs/contract.md cli/tests/flow/context_distill.rs"` → 1; 2; 1 (all saying it was removed and must not return).

### 26. Decided work skips review

- **Enforced by:**
  - `check` `core/src/job/add.rs:127` and `:158`: `--tier` and `--spec` are required unless `--draft`, and in phase `plan` every Job is a draft; `core/src/job/add.rs:167` stores a non-draft Job directly as `queued` with its fingerprint, so a decided Job runs without a Plan review.
  - `check` `core/src/job/add.rs:214` (`check_needs_not_draft`): a `queued` Job may not depend on a draft (`needs_unavailable`), so decided work and undecided design cannot be mixed silently.
  - `check` `docs/commands.md:945` and `:963` (`no_draft_jobs`, `draft_incomplete`): a Plan review covers exactly the drafts, so it can never be used to re-approve queued work.
  - `test` `pi/skills/skill.test.ts:574` (and `:581`): pins the skill text "added queued", "Spec that cites the decision", "When unsure, a draft".
  - `none`: nothing checks that a Job added without `--draft` actually carries out a recorded decision.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:42` and `:200` ("Work that only carries out a decision the User already recorded … is added queued … with a Spec that cites the decision"), `docs/commands.md:1270`.
- **Gap:** the `--draft` flag is the only switch: a Lead can add undecided design `queued` and it runs with no Plan review, and nothing ties a queued Job to a decision. Evidence: `core/src/job/spec.rs` has **0** references to `decision` or `hs-`; the preconditions of `job add` (`docs/commands.md:1280`–§5.1 table) contain no decision check; the only pin is the skill-prose test `pi/skills/skill.test.ts:574`.
- **Proposal:** `check`: `cpd job add` and `cpd job edit --spec` refuse a Job that is not a draft (and not a `--review` Job) whose Spec `context` cites no Context record id (`\bhs-[dn][0-9]+\b`) with `spec_invalid` and the JSON path `$.context`; a stub draft and a Job already stored are not re-checked. Pinned by a new `cli/tests/flow/job.rs` test; Size S.
- **Verified:**
  - `timeout 60 bash -c "rg -o 'decision|hs-' core/src/job/spec.rs | wc -l; rg -n 'decision' core/src/job/add.rs"` → 0; lines 85–86 only (the `decide(conn, …)` add/no-op helper, not a decision citation).
  - `timeout 60 bash -c "rg -n 'added queued|Spec that cites the decision' pi/skills/cpd-lead/SKILL.md | wc -l"` → 2.

### 27. A primary checkout is a mirror of origin/main

- **Enforced by:**
  - `guard` `pi/lib/mainguard.ts:65` (`primaryCheckoutMove`), applied for every role at `pi/lib/adapter.ts:59`: refuses `checkout switch reset rebase merge pull stash` run in the nearest `.git`-directory root, resolving `cd`, `~`, `$HOME` and `git -C`; evidence hs-n436/hs-d573/hs-d248 at `docs/commands.md:3744`; 21 tests in `pi/test/mainguard.test.ts` (including "refuses the incident command with a tilde and the home directory resolved").
  - `guard` `core/src/host/git_guard.rs:30`, `:88`: the `$CPD_HOME/bin/git` wrapper is first on every Session's `PATH` and refuses `git stash` everywhere; evidence hs-d310 (`core/src/host/git_guard.rs:19`).
  - `test` `pi/test/mainguard.test.ts:262` ("no skill file names a git command that moves HEAD") and `:258`/`:260` (both skills name only the allowed read forms); `docs/commands.md:3768` says the tests read the skill files.
  - `check` (in flight, Plan `main-mirror`, not to be duplicated): PR `guard` (dispatch; `guard-bash-tests` running, `guard-impl` queued) extends §15.16 to the `edit` and `write` tools and to the bash write forms; PR `spec` refuses a Job Spec that names a Plan repo's primary checkout (`cli/tests/flow/job.rs`); PR `install` makes a skipped `cpd-install.sh` fail loud.
  - `none`: the `edit` and `write` tools by absolute path are not guarded yet.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:30` and `pi/skills/cpd-manager/SKILL.md:31` ("Never move HEAD or change the files of a repo's primary checkout … the adapter refuses it"), `docs/commands.md:3744`.
- **Gap:** the guard reads only the `bash` tool (`rg -c 'toolName === "bash"' pi/lib/adapter.ts` → 1, the guard handler); `pi/lib/mainguard.ts` names no `edit`/`write` (0 matches), so a Worker that edits a primary checkout with the `edit`/`write` tool by absolute path is not refused. That is the 2026-10-24 incident (Plan `main-mirror`'s context: two Workers changed `docs/contract.md` and `docs/commands.md` of cpd's primary checkout with `edit`, records hs-d606/hs-d608/hs-d609), and PR `guard` of `main-mirror` owns the extension.
- **Proposal:** `none needed` here: Plan `main-mirror` PRs `guard`, `spec` and `install` are the smallest enforcement and are already in flight; duplicating them would be YAGNI.
- **Verified:**
  - `timeout 60 bash -c "rg -o 'edit|write' pi/lib/mainguard.ts | wc -l; rg -c 'toolName === \"bash\"' pi/lib/adapter.ts; rg -c '^test\(' pi/test/mainguard.test.ts"` → 0; 1; 21.
  - `timeout 60 cpd plan show --plan main-mirror` → phase `dispatch`; PR `guard` dispatch (`guard-bash-tests` running, `guard-impl` queued), PR `spec` dispatch, PR `install` dispatch.

### 28. No flakes, no leaks, no orphans, no sleep, no errors

- **Enforced by:**
  - `gate` `package.json:14` (`verify` is `scripts/gate-run.sh npm run verify:steps`, pinned by `test` `pi/test/gate.test.ts:8`): the whole Gate runs under one random run id. Blind spot: it checks only this run.
  - `script` `scripts/gate-run.sh:27` (`GATE_RUN` + private `TMPDIR=$RUN`) and `finish()` `:88`: at the end it fails the run when the run left a process carrying `GATE_RUN` (Linux `/proc/<pid>/environ` `:73`; macOS `ps -axww -E` `:76`), a tmux socket named `*-<id>-*` (`:117`) or any path under `$RUN`, then kills/removes only what it listed. Blind spot: a run killed before `finish()` leaves its leftovers, and a machine-wide leftover from any other run is invisible by design.
  - `hook` `scripts/gate-run.sh:96`: `finish()` reads `$TMPDIR/tripwire.log` and fails the run on any line, so a test that swallowed the tripwire's exit 127 still fails. Blind spot: only real-`tmux` calls made under the tripwire (axiom 29's guard), no file/socket leak.
  - `check` `.config/nextest.toml:19` (`fail-fast = false`): a red run lists every failing test, never only the first, so a flake cannot hide behind an earlier failure. Blind spot: reporting, not detection.
  - `check` `.config/nextest.toml:24` (`leak-timeout = { period = "100ms", result = "fail" }`, the ms-leak incident): a test that leaves a process holding its stdout/stderr pipes fails. Blind spot: a detached process that closes its pipes, a leaked file or a leaked socket.
  - `check` `.config/nextest.toml:33` (`slow-timeout = { period = "30s", terminate-after = 3 }`, `:79` for the tmux-restart group): a test that hangs is killed at 90 s and fails. Blind spot: "not a hang", not "no sleep"; a 20 s sleep passes.
  - `test` static sleep ratchet `pi/test/no-fixed-sleeps.test.ts:8` (no `settle`/`sleep(` in a pi test), `:61` (no `setTimeout`/`setInterval`/`Date.now()`/`spawnSync("sleep"`/`waitFor(` outside `pi/test/fake.ts` and the allowlist) and `:80` (no `thread::sleep(`/`Instant::now(`/`SystemTime::now(` in Rust test code outside the allowlist). Blind spot: it counts text, not behaviour, and it allows the listed debt (`pi/test/sleep-allow.txt`: 79 entries, 469 waits).
  - `test` `cli/tests/tooling/harness.rs:149` (`no_fake_sleeps_longer_than_sleep_seconds`, `SLEEP_SECONDS` = 120 at `cli/tests/common/mod.rs:75`): a fake's shell `sleep N` is bounded. Blind spot: a shim's `sleep` is allowed because the harness reaps it.
  - `test` `cli/tests/tooling/harness.rs:208` (`no_test_sleeps_for_a_result_or_bounds_the_time_a_result_takes`: a `sleep >= 250 ms` or an elapsed upper bound needs a `// time-claim:` line) and `:268` (`every_wait_polls_through_the_harness`). Blind spot: a sleep under 250 ms, and a loop that polls without sleeping, pass.
  - `test` leak behaviour: `cli/tests/tooling/harness.rs:449` (a killed test leaves no tmux server, process, dir or reaper), `:488` (a dropped env), `:579` (no git wrapper), `:656` (a process that leaves the process group and is not recorded), `:667`, `:685`; `cli/tests/tooling/gate_run.rs:106,132,148,161,176,195,226,237,248` pin every direction of the Gate check (clean run, process/dir/socket left, another run's leftovers, a failing command's status, the tripwire log).
  - `check` `scripts/cargo.sh:101` runs a full test run inside `cpd slot --` (and `package.json` `test` does too): full runs serialise through the machine's slots, so two Gates do not contend. Blind spot: full runs only; a slot does not bound a sleep or a leak.
- **Prose only:** `docs/implementing.md:138` ("never a fixed sleep to get a result; waits poll a positive signal"); `:133-137` (the tripwire/allowlist/sleep-check prose, whose mechanisms are code); `:143-149` (the Gate leak check prose); `docs/research/hygiene.md:1` documents the leaks in prose and nothing enforces its findings. The axiom's "never retried, ignored or bypassed" has no mechanism and no other text: `rg -n 'retries' .config/nextest.toml` prints nothing.
- **Gap:** (a) **flakes** have no metric: `rg -n 'flak' core cli engine scripts docs/research/hygiene.md` → 6 lines, one in source (`cli/tests/session/session_reload.rs:497`, a comment naming a fixed flake), and `rg -n 'flak|leak|sleep' core/src/events` → 6 lines, all `SessionAsleep` plus the `events follow` loop's `thread::sleep` (`core/src/events/mod.rs:181`), none a flake/leak counter. A flake is fixed only when a human sees it. (b) **leaks/orphans** outside a live run: `/tmp` is 83G; 378 `cpd-test-*` = 394M, 99 `tmp.*` + 1 `.tmp*` = 227M, 284 sockets in `/tmp/tmux-1000`, 9 `pi-check*`, and `/tmp/sp-* /tmp/dc-* /tmp/hn-*` = 63G; the Gate check cannot see any of them. Evidence that a killed run does leak: `docs/research/hygiene.md` R5 (310 → 320 `.tmp*` under `timeout -s KILL`), not reproduced here. (c) **sleeps**: the ratchet allows 79 files / 469 waits (182 Rust, 287 pi); the raw text is larger — `rg -c 'thread::sleep|tokio::time::sleep|sleep ' cli/tests core engine tui` sums to 319, `sleep [0-9]` in Rust sources to 76, and `setTimeout` in `pi/test` to 15 in 6 files. (d) **errors**: `cpd events list --kind error` returns 206 in the last 30 days, top `distill_invalid` 80, `gh_failed` 60, `cleanup_failed` 15; these are product errors, and no mechanism counts a test "ending in an error" beyond nextest's exit code.
- **Proposal:** record the one metric that is missing — a `flake` Event (`Kind::Flake` in `core/src/events/kind.rs`) written by the Gate's open Job when a test that failed in the previous run passes in this one (`scripts/gate-run.sh` writes its failed test ids to `$CPD_HOME/run/test-failures.json`; the next run diffs), pinned by a new `cli/tests/tooling/gate_run.rs` test — and extend `scripts/gate-run.sh` with a machine-wide sweep (`/tmp/cpd-test-*`, `/tmp/tmp.*` and `/tmp/tmux-<uid>/*` sockets whose server is gone and that are older than a day), run by the engine's doctor, pinned in `cli/tests/tooling/harness.rs`. Size M. The sleep and error parts need `none needed`: the static ratchet already fails a new sleep and bounds the debt, and nextest's exit code already fails an erroring test.
- **Verified:**
  - `timeout 30 node --test pi/test/no-fixed-sleeps.test.ts` → `# tests 3`, `# pass 3`, `# fail 0`.
  - `timeout 60 rg -c 'thread::sleep|tokio::time::sleep|sleep ' cli/tests core engine tui | awk -F: '{s+=$NF} END {print s}'` → `319` (per dir `246/52/9/12`); `timeout 60 rg -c 'thread::sleep' cli/tests core engine tui | awk -F: '{s+=$NF} END {print s}'` → `111`; `timeout 60 rg -n 'sleep [0-9]' cli/tests core engine tui --glob '*.rs' | wc -l` → `76`.
  - `timeout 60 rg -c 'setTimeout' pi/test | awk -F: '{s+=$NF} END {print s}'` → `15`; `timeout 60 rg -l 'setTimeout' pi/test | wc -l` → `6`.
  - `timeout 60 grep -cv '^#\|^$' pi/test/sleep-allow.txt` → `79`; `... | awk '{s+=$2} END{print s}'` → `469` (pi `287`, Rust `182`).
  - `timeout 120 du -sh /tmp` → `83G`.
  - `timeout 60 find /tmp -maxdepth 1 -name 'cpd-test-*' | wc -l` → `378`; `timeout 120 du -sch /tmp/cpd-test-* | tail -1` → `394M`.
  - `timeout 60 find /tmp -maxdepth 1 -name 'tmp.*' | wc -l` → `99`; `-name '.tmp*'` → `1`; `timeout 120 du -sch /tmp/tmp.* | tail -1` → `227M`.
  - `timeout 60 ls /tmp/tmux-1000 | wc -l` → `284`; `timeout 60 find /tmp -maxdepth 1 -name 'pi-check*' | wc -l` → `9`.
  - `timeout 120 du -sch /tmp/sp* /tmp/dc* /tmp/hn* | tail -1` → `63G`.
  - `timeout 60 rg -n 'flak' core cli engine scripts docs/research/hygiene.md | wc -l` → `6` (source hit `cli/tests/session/session_reload.rs:497`); `timeout 60 rg -n 'retries' .config/nextest.toml` → exit 1, `0` lines.
  - `timeout 60 rg -n 'flak|leak|sleep' core/src/events | wc -l` → `6` (`kind.rs:114,189`, `typed.rs:369,370,545`, `mod.rs:181`; none is a leak or flake counter).
  - `timeout 60 cpd events list --kind error --limit 500 | jq -r '.events | length'` → `206`; by code `80 distill_invalid, 60 gh_failed, 15 cleanup_failed, 9 store_busy, 9 tmux_failed, 7 spawn_failed, 7 tier_invalid, 5 title_failed, 4 store_too_new, 3 adapter_missing, 3 push_failed, 1 config_invalid, 1 plan_required, 1 pr_start_failed, 1 tier_not_allowed`; oldest `2026-10-15`, newest `2026-10-25`.
  - `timeout 60 rg -n 'fail-fast|leak-timeout|slow-timeout' .config/nextest.toml` → `:19`, `:24`, `:33`, `:79`; `timeout 60 rg -n 'GATE_RUN|run_pids|^finish|tripwire.log' scripts/gate-run.sh` → `:27`, `:68`, `:88`, `:96`, `:117`.

### 29. Only authored code is tested

- **Enforced by:**
  - `guard` `scripts/tripwire/tmux:31-47` with the allowlist `scripts/tripwire/allow.txt` (124 entries, 9 tags): the Gate's last step runs with the tripwire first on `PATH` (`package.json:15`); a test not matching an allowlist prefix that runs real `tmux` gets `exit 127` and a `tripwire.log` line, and `scripts/gate-run.sh:96` fails the run. Verified live. Blind spot: `tmux` only — there is no `scripts/tripwire/git` and no `allow-git.txt`.
  - `guard` `cli/tests/common/mod.rs:307` (`DENIED_TOOLS = ["gh","pi","curl","wget","ssh"]`, installed per env at `:751`, exit `97`): a test that reaches real `gh`, `pi`, `curl`, `wget` or `ssh` without installing a fake fails. Blind spot: `git` and `tmux` are absent from the list, and `env.fake(name, body)` (`:1090`) can `exec` the real program.
  - `test` the allowlist's own ratchet `pi/test/tripwire-allow.test.ts:17` (each line well formed, uniquely tagged with a live PR key) and `:27` (each prefix names an existing test module, checked against `cargo nextest list --workspace`): a debt line cannot be added without a tag, and the tag leaves with its migration. Blind spot: it ratifies the debt, it does not shrink it.
  - `test` `cli/tests/tooling/tripwire_script.rs` (10 tests): allow/deny, prefix and binary-id matching, comment/blank lines, the log, the "no tmux behind the tripwire" case, the PATH lookup.
  - `test` `pi/test/no-fixed-sleeps.test.ts:80` (the Rust half of the static check): the only static check that scans test code for a mock (time), listed here because time is one of the axiom's externals. Blind spot: time only.
  - `gate` `scripts/pi-check.sh` (Gate step in `package.json:15`): the real pinned `pi`, the one named real external left in the Gate (`docs/implementing.md:182`).
  - `none` for a real `git` spawn: no guard denies one; `git` is not in `DENIED_TOOLS`; there is no git tripwire and no fake git.
- **Prose only:** `docs/implementing.md:130` ("Never touch the real `~/.cpd`, the tmux server `-L cpd`, pi, or the network."); `scripts/tripwire/allow.txt:1-3` (a migration Job deletes its lines, "Never add a line for a hit"); `docs/research/tests-mock-design.md` §3 (the git one-way door, hs-d613: real git IS external and must be mocked) and §5 (the guard).
- **Gap:** (a) **git** (external by hs-d613): 39 Rust test files invoke real git (`rg -l '\.git\(|git_repo\(|…'`), 735 `.git(` call sites in `cli/tests core/tests` plus 4 `Command::new("git")`, and `pi/test/mainguard.test.ts:26` runs real git (4 call sites). The product spawns it at `core/src/git/run.rs:57` and `core/src/context/distill.rs:434`; `core/src/policy/origin.rs:108` is test-only. No fake git exists (`ls cli/src/fakes` → `mod.rs`, `tmux`; `rg -l 'fake-git|CPD_TEST_GIT'` → 2 design docs). No mechanism stops the spawn. (b) **tmux**: 46 Rust test files call `env.tmux(`/`real_tmux`; 213 `env.tmux(` + 14 `real_tmux()` call sites + 2 `Command::new("tmux")`. The fake exists (`cli/src/fakes/tmux/`) but only 5 test files use it (`fake_tmux()`/`tmux_state()`, 9 sites); the 124-line allowlist still lists every real-tmux area, so today the tripwire denies nothing that already exists. (c) **gh/pi/network**: only the default deny shim; no test spawns `gh` or `pi` directly (`rg -n 'Command::new\("(gh|pi)"\)' cli/tests core/tests` → 0) and no test opens a socket (`rg -n 'TcpStream|TcpListener|UdpSocket|UnixStream' cli/tests core/tests engine` → 0). No evidence of a real network call.
- **Proposal:** build a stateful `fake-git` (the design's Option A) reached through a `CPD_TEST_GIT` read in `core/src/git/run.rs:57`, and a `scripts/tripwire/git` with `allow-git.txt`; the 12 real `contract::` tests stay as permanent lines. Order of work (≤ 8 steps): 1. `fg-spike`: `cli/src/fakes/git/` state model plus the 12 fixture subcommands (`init add commit clone tag checkout merge for-each-ref config remote rev-parse update-ref`) and `worktree add`; abort if `merge-tree`/`rebase` do not model in 3 Jobs. 2. `fg-read`: read-only subcommands (`status diff cat-file ls-tree show log rev-list rev-parse config branch`). 3. `fg-merge`: `merge-base merge-tree rebase read-tree write-tree`. 4. `fg-wt`: `worktree branch remote push fetch ls-remote`. 5. `fg-env`: the `CPD_TEST_GIT` seam, a nextest setup script that exports it, `scripts/tripwire/git` + `allow-git.txt`, and the 12 contract tests as permanent lines. 6. migrate the core/engine/flow areas (`mu-git`, `en-t2`, `fl-g`, `fl-b`) and, for tmux, `m-session`, `m-engine`, `m-tui` against the existing `cli/src/fakes/tmux/`. 7. migrate `pr` and `tooling` (12 `pa-*`/`pb-*` Jobs, `tl-install`, `tl-scripts`) and, for tmux, `m-flow`, `m-tooling`. 8. `del-1`: delete `allow-git.txt` and the `git` branch, `scripts/tripwire/allow.txt`, `real_tmux()` and the `tmux-restart` group — the tripwire stays an unconditional deny. Size L overall; the git fake alone is M.
- **Verified:**
  - `timeout 60 rg -l '\.git\(|git_repo\(|keyed_repo\(|local_origin\(|github_origin\(|Command::new\("git"\)' cli/tests core/tests | wc -l` → `39`.
  - `timeout 60 rg -c '\.git\(' cli/tests core/tests | awk -F: '{s+=$NF}END{print s}'` → `735`; `timeout 60 rg -l '\.git\(' cli/tests core/tests | wc -l` → `33`.
  - `timeout 60 rg -n 'Command::new\("git"\)' cli/tests core/tests` → 4 lines (`pr/git_guard.rs:20`, `core/tests/{hooks_idle.rs:24,migrations.rs:462,repin.rs:96}`); `... core/src engine/src tui/src cli/src` → `core/src/git/run.rs:57`, `core/src/policy/origin.rs:108`, `core/src/context/distill.rs:434`.
  - `timeout 60 rg -n 'execFileSync\("git"' pi/test` → `pi/test/mainguard.test.ts:26`; `timeout 60 rg -n '\bgit\(' pi/test/mainguard.test.ts | wc -l` → `4`.
  - `timeout 60 rg -l '\bgit::' core/src engine/src tui/src cli/src | wc -l` → `25`; `timeout 60 rg -c '\bgit::' core/src engine/src tui/src cli/src | awk -F: '{s+=$NF}END{print s}'` → `130`.
  - `timeout 60 rg -l 'env\.tmux\(|real_tmux' cli/tests | wc -l` → `46`; `timeout 60 rg -c 'env\.tmux\(' cli/tests | awk -F: '{s+=$NF}END{print s}'` → `213`; `timeout 60 rg -n 'real_tmux' cli/tests core engine tui | wc -l` → `19` (4 definitions, 1 import, 14 call sites).
  - `timeout 60 rg -l 'fake_tmux\(\)|tmux_state\(\)' cli/tests` → 5 files; `timeout 60 rg -n 'fake_tmux\(\)|tmux_state\(\)' cli/tests | wc -l` → `9`.
  - `timeout 60 grep -cv '^#\|^$' scripts/tripwire/allow.txt` → `124`.
  - `timeout 60 rg -n 'DENIED_TOOLS' cli/tests/common/mod.rs` → `307`, `751`; `timeout 60 rg -n 'fn fake\(' cli/tests/common/mod.rs` → `1090`.
  - `timeout 60 rg -n 'Command::new\("(gh|pi)"\)' cli/tests core/tests` → exit 1, `0` lines; `timeout 60 rg -n 'TcpStream|TcpListener|UdpSocket|UnixStream' cli/tests core/tests engine` → exit 1, `0` lines.
  - `TMPDIR=/tmp/audit-hyg-trip NEXTEST_BINARY_ID=cpd::foo NEXTEST_TEST_NAME=bar::baz timeout 15 scripts/tripwire/tmux list-sessions` → `tripwire: real tmux called by cpd::foo bar::baz: tmux list-sessions`, exit `127` (no tmux started); the dir was removed after.
  - `timeout 60 grep -c '#\[test\]' cli/tests/tooling/tripwire_script.rs` → `10`; `timeout 60 ls cli/src/fakes` → `mod.rs`, `tmux`.
  - `timeout 60 rg -l 'fake-git|CPD_TEST_GIT' --glob '!node_modules' --glob '!target' .` → `docs/research/tests-mock-design.md`, `docs/research/tests-mock-flow-b.md` (2 docs, no code).

## Not verified

Merged from the five parts. A claim the part did not verify is labelled here.

- Axiom 1: I did not run the proposed manifest test; that a directly versioned dependency is absent today is unverified (I only counted `.workspace = true` uses, 16/11/13/13).
- Axiom 2: the claim that a live Worker prompt carries `AGENTS.md` is verified for this Attempt only (I read my own context); whether every role and every pi version does is unverified. The Distiller's live prompt was not read.
- Axiom 4: whether clippy is installed on the Gate's machine and which lints its toolchain supports is unverified.
- Axiom 5: whether `docs/commands.md` currently drifts from `cpd --help` is unverified; I did not diff them.
- Axiom 6: whether an Event-kind or record-kind name has ever been removed from the code is unverified (`no evidence` found).
- Axiom 7: which of the 21 `Deserialize`-mentioning files parse untrusted on-disk input (and so need `deny_unknown_fields`) is unverified; I read the 12 that have it, not the 9 that do not.
- The instruction "the Worker's prompt gets `AGENTS.md` by its argv" (A9): I read the Worker argv (`core/src/host/pi/launch.rs:166`, no `--no-context-files`) and the live file (`/home/clayton/.pi/agent/AGENTS.md`, 19 axioms including 8-13); I did not dump my own prompt to a file, and I infer from `PI_SESSION_FILE` (`.pi/agent/sessions/...`) that the loaded file is the 19-axiom one rather than the 29-axiom `agents` worktree.
- The store counts are the live machine's `$CPD_HOME` database at this time; they are point-in-time, not a rate.
- Whether any production Worker has actually guessed instead of asking: no command measures it; the "no evidence" labels stand until one does.
- The emit of a wrong-reason red test: `rg` found no marker mechanism; I did not run the Gate or the full suite (out of scope for a Worker).
- My own system prompt: I read that it carries axioms 1–21 (all of 14–21) while `/home/clayton/.pi/agent/AGENTS.md` on disk has 19 headings. Which source the harness injected is unverified: I infer the prompt was built from a newer AGENTS.md than the file, or the harness merges them.
- The `class … extends` count of 6 is a keyword grep; `interface … extends` and generic constraints are not in it. That 2 are `Error` subclasses and 4 framework component subclasses is by eye, not a parser.
- Axiom 15 "no check bans inheritance": I grepped for class inheritance and found none; an unknown check under another name could exist. Unverified.
- The Gate's leak check (`scripts/gate-run.sh`, hs-d519) and nextest's leak-timeout were read, not run (a Worker runs only its own tests): unverified that they fail a leak on the current head.
- The 206 `error` Events are the whole Context's and are not time-bounded or attributed to a Plan or Job; the counts are a snapshot at the time of this audit, not a per-Plan figure.
- The idempotence test functions named at `cli/tests/flow/context.rs:106`, `core/src/store/schema/tests.rs:1195` and the rest were read by name, not run; the Gate proves them.
- The command that would close the axiom 14 gap (a test over the `Idempotent:` labels) is a proposal, not written; its size M is a guess.
- I did not open a live pi pane or read the raw argv of a running Worker; I confirmed only that my own system prompt carries the 19-axiom AGENTS.md and that the `lead`/`worker` argv builders in `core/src/host/pi/launch.rs` pass no `--no-context-files` (the flag is only in `distiller`, `core/src/host/pi/launch.rs:195`). That the running Worker argv equals that code is "should", not read.
- The 200 `blocked` `cpd: Worker question` and 203 `blocked` `cpd: question to the Lead` Events are counted without timestamps: I infer they predate hs-d303's switch to `waiting` (which now shows 148 and 150). I did not date them against hs-d303.
- I did not run any test, the full suite or the Gate; the test counts above are the number of `test(`/`#[test]` sites and `rg` occurrences, not passing runs.
- The proposal sizes (S/M) are my estimates of code change, not measured.
- `docs/commands.md` line numbers were read from this worktree's base commit; a later merge may move them.
- Whether a live Worker prompt carries axioms 28 and 29: I read my own context and the source file (19 axioms); I did not read the pi loader, and I did not read another role's prompt.
- The `/tmp` sizes, counts and socket counts are one host at one time and include leftovers of past audits and Jobs, not only tests; that any particular `/tmp/cpd-test-*` is a test's leak is unverified.
- The 469 allowed waits and the 124 allowlist entries are the machine's own counts; whether each is genuinely a sleep (rather than `Instant::now` used for a legitimate time claim) is unverified.
- The design's job counts (58 Jobs, 18 git-dependent Jobs, about 963 TypeScript tests, 12 contract tests) are claims of `docs/research/tests-mock-design.md`, not re-measured here.
- The Manager's "about 658 real git test spawns": I measured 735 `.git(` call sites over 33 files plus 4 direct spawns; which pattern is the "spawns" count is unverified, and a call site inside a loop spawns more than once.
- Whether every `thread::sleep` counted is in test code: the raw grep counts production code too (the ratchet's 469 is the machine's own, allowlisted count).
- The `pi/test/sleep-allow.txt` one-time raise on `tui/src/plan/mod.rs` (15) and the two "new debt" lines: I read the reason text, I did not check whether the scan now limits itself to test modules.
