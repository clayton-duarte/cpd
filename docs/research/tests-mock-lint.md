# tests-mock: audit of `cargo fmt --check` and `cargo clippy -D warnings` (Job `lint-audit-job2`, attempt `lint-audit-job2-a1`)

Measured on `origin/main` 5c992a8d (rust 1.99.1, clippy 0.1.99, edition 2024, Apple M4 Pro, 14 cores, sccache on through `scripts/cargo.sh`), in a scratch worktree removed afterwards. Nothing in the repo changed. Every number is from a command I ran; a claim I did not run is marked `unverified`. 489 `.rs` files, 159k lines.
Test code = a file under `tests/`, named `*_tests.rs` or `tests.rs`, or after a `#[cfg(test)] mod x {` line of its file (clippy counts only; fmt counts by file name). `cli/src/fakes/` counts as product (a library module).

## Summary

| | fmt | clippy `-D warnings` |
|---|---|---|
| today | fails: 10,055 hunks in 489 files, 103,745 changed lines | fails: 224 diagnostics, 41 lints. With `-D` the build stops at the first one (`cpd-core`'s build script) |
| machine-fixable | all (`cargo fmt`) | 114 of 224 (51 %), 421 changed lines; 110 left, about 96 by hand |
| time | 3.6-4.4 s | warm 3-4 s, fresh target dir 15-26 s |
| config | no `rustfmt.toml` today; 3 keys remove 89 % of the diff (section 4) | none needed |

Decision for the User, found by the audit: the fmt diff is 104k lines because the code is not in rustfmt's default style (lines up to 320 characters; 4,604 lines of `core/src` pass 100). Take the default and one 104k-line mechanical PR, or add `rustfmt.toml` (`max_width = 160`, `use_small_heuristics = "Max"`) and a 11.7k-line one (section 4).

## 1. `cargo fmt --all --check`

`scripts/cargo.sh fmt --all --check -- --color never` exits 1. There is no `rustfmt.toml`, no `.rustfmt.toml`, no `[lints]` table. The edition is 2024 from `[workspace.package]` (`Cargo.toml:7`) and rustfmt follows it (the first hunk is the 2024 import order, `{Invocation, run}`). So nothing in settings explains the diff: it is the default style against code formatted by hand.

| crate | files that differ (of all `.rs`) | changed lines, product | changed lines, test files | total |
|---|---|---|---|---|
| core | 183 of 189 | 23,175 | 4,191 | 27,366 |
| engine | 30 of 31 | 2,146 | 28 | 2,174 |
| tui | 110 of 113 | 16,510 | 2,975 | 19,485 |
| cli | 166 (total `.rs` count not taken per crate) | 1,058 | 53,662 | 54,720 |
| total | 489 | 42,889 | 60,856 | 103,745 |

- "Changed lines" is the `+` and `-` lines of `fmt --check` output. Plus is about 4x minus (86k against 18k): rustfmt breaks long lines. Sample: `cli/build.rs:5` a 120-character chain becomes 5 lines. Test files are split by name; the test modules inside product files count as product.
- `cli/tests` alone is 53k lines (51 %). The migrations rewrite those files (section 5).
- `cargo fmt` is the only fix. No hand edit.

## 2. `cargo clippy --workspace --all-targets`

Run as `scripts/cargo.sh clippy --workspace --all-targets --keep-going --message-format=json`, without `-D warnings`, so the build goes on and every crate reports (a script counted the JSON, one diagnostic per lint, file and line: the lib and test targets report the same line twice). With `-- -D warnings` (run, exit 101): it stops at `core/build.rs:80` (`write_with_newline`); `cpd-core`'s build script fails, so nothing that depends on it is checked. That is why the audit runs without it, and why the first Gate run would show one error at a time until `core/build.rs` is fixed.

Counts per crate and product-vs-test (before any fix):

| crate | product | test | total |
|---|---|---|---|
| core | 52 | 13 | 65 |
| cli | 29 | 81 | 110 |
| tui | 23 | 23 | 46 |
| engine | 2 | 0 | 2 |
| one hit inside a std macro expansion (`cloned_ref_to_slice_refs`) | 1 | 0 | 1 |
| total | 107 | 117 | 224 |

41 distinct lints. The 20 most frequent (`F` = `--fix` applies it, `H` = by hand):

| # | lint | n | test | fix | example |
|---|---|---|---|---|---|
| 1 | `clippy::collapsible_if` | 33 | 4 | F | `core/src/output.rs:67` (let chains, edition 2024) |
| 2 | `private_interfaces` (rustc) | 24 | 0 | H | `cli/src/fakes/tmux/cmds.rs:45` `Cx<'_>` more private than `kill_*` |
| 3 | `clippy::type_complexity` | 21 | 13 | H | `core/src/context/distill.rs:256` |
| 4 | `clippy::needless_question_mark` | 17 | 16 | F | `core/src/context/manager.rs:42` |
| 5 | `clippy::cloned_ref_to_slice_refs` | 13 | 12 | F | `tui/src/widgets/column.rs:529` |
| 6 | `clippy::too_many_arguments` | 11 | 4 | H | `core/src/attempt/kind.rs:74` (8/7) |
| 7 | `dead_code` (rustc) | 10 | 10 | H | `core/src/host/tmux/pill_tests.rs:6` const `LEFT` |
| 8 | `clippy::single_element_loop` | 9 | 9 | F | `core/src/inbox/rules.rs:1309` |
| 9 | `unused_imports` (rustc) | 8 | 8 | F | `tui/src/tree/fixing_tests.rs:8` |
| 10 | `clippy::useless_format` | 8 | 8 | F | `tui/src/tree/rows.rs:934` |
| 11 | `clippy::needless_borrow` | 6 | 1 | F | `core/src/job/edit.rs:205` |
| 12 | `clippy::bool_assert_comparison` | 5 | 5 | F | `tui/src/tree/another_owner_tests.rs:94` |
| 13 | `clippy::large_enum_variant` | 4 | 0 | H | `core/src/context/contexts.rs:85` |
| 14 | `clippy::needless_borrows_for_generic_args` | 4 | 0 | F | `core/src/context/legacy.rs:110` |
| 15 | `clippy::redundant_closure` | 4 | 1 | F | `core/src/driver/run.rs:219` |
| 16 | `clippy::needless_range_loop` | 4 | 3 | H | `tui/src/plan/flow.rs:355` |
| 17 | `clippy::doc_lazy_continuation` | 3 | 0 | H | `core/src/config/tier.rs:242` |
| 18 | `clippy::io_other_error` | 3 | 3 | F | `tui/src/background_tests.rs:14` |
| 19 | `clippy::field_reassign_with_default` | 3 | 3 | H | `tui/src/plan/mod.rs:918` |
| 20 | `clippy::ptr_arg` | 3 | 3 | H | `cli/tests/pr/pr_crud.rs:92` `&PathBuf` |

The other 21 lints have 31 hits (1-2 each), among them `write_with_newline` (`core/build.rs:80`, the one that stops `-D`), `print_with_newline` 2, `unit_arg` 2, `non_snake_case` 2 (`tui/src/palette/colours.rs:30`, a test name with `__`), `zombie_processes` (`core/src/session/run.rs:384`), `enum_variant_names` (`core/src/pr/status.rs:126`), `wrong_self_convention` (`tui/src/logs/view.rs`), `duplicate_mod` (`cli/tests/tooling/install_script.rs`).

### Machine-fixable

`scripts/cargo.sh clippy --fix --allow-dirty --workspace --all-targets` in the scratch worktree, 63 s. A second pass changed nothing more (same 71 files, +198 -223).

| crate | files | + | - | changed | of which test files (by name) |
|---|---|---|---|---|---|
| core | 28 | 90 | 103 | 193 | 3 files, 12 lines |
| engine | 2 | 3 | 4 | 7 | 0 |
| tui | 20 | 44 | 61 | 105 | 7 files, 28 lines |
| cli | 21 | 61 | 55 | 116 | 18 files, 96 lines |
| total | 71 | 198 | 223 | 421 | 28 files, 136 lines |

So 114 of 224 diagnostics (51 %) are fixed by `--fix`; 110 remain: cli 64 (39 test), core 30 (8 test), tui 15 (8 test), 1 in a std macro. Of the 110, 14 carry a `MachineApplicable` suggestion that `--fix` did not apply (`unused_imports` 4, `needless_question_mark` 3, `useless_format` 3, `bool_assert_comparison` 2, in `cli/tests/flow/*`): I infer these files are shared by `#[path]` among test targets (`duplicate_mod` is reported in `install_script.rs`), unverified. They are one-line hand edits, so about 96 are real hand edits.

### Hand edits and design changes

| lint | n left | what it asks | design change? |
|---|---|---|---|
| `private_interfaces` | 24 (3 files, one cause) | `Cx` in `cli/src/fakes/tmux/{cmds,ctl,hooks}.rs` is less visible than the `pub` fns that take it | no: visibility, one root cause |
| `type_complexity` | 21 (17 files, 13 test) | `type` aliases | no |
| `too_many_arguments` | 11 (9 files) | a parameter struct per fn | yes: signatures. Four other fns in core already carry `#[allow(clippy::too_many_arguments)]` (`events/typed.rs:42`, `attempt/kind.rs:106`, `attempt/submit.rs:145`, `git/worktree.rs:470`) |
| `dead_code` | 10 (test) | delete the item, after checking why it exists (pill-test helpers) | no |
| `large_enum_variant` | 4 (`context/contexts.rs`, `job/edit.rs`, `job/retry.rs`, `pr/edit.rs`) | `Box` the big variant | yes: constructors and matches |
| `needless_range_loop` 4, `field_reassign_with_default` 3, `ptr_arg` 3, `doc_lazy_continuation` 3 | 13 | local rewrites | no |
| `enum_variant_names` | 1 (`pr/status.rs:126`) | renames a variant | yes if the name is stored or printed (unverified) |
| `wrong_self_convention` 1, `zombie_processes` 1 | 2 | rename a method; `wait()` a child (`session/run.rs:384`, a real process: a migration likely rewrites it) | yes (public name) / no |

## 3. Time

All through `scripts/cargo.sh`, `timeout` on each, wall time.

| run | wall |
|---|---|
| `cargo fmt --all --check` | 3.6 s, 4.4 s |
| `clippy --workspace --all-targets`, fresh scratch `target/` (sccache has the dependencies) | 14.9 s (17.0 s in my own worktree, earlier base) |
| after `cargo clean -p cpd-core` (cold for the product crates) | 16.0 s |
| warm, nothing changed | 3.1-4.4 s |
| `clippy ... -- -D warnings` today (stops at `cpd-core`'s build script) | 7.2 s |
| `build --workspace` after the clippy runs (for scale) | 25.9 s |

What it adds to the Gate: fmt about 4 s and clippy 15-26 s, so **about 20-30 s** before the nextest step, against a Gate of 518-690 s on pro and a limit of 30 minutes (pro-d16: not raised, not needed). Clippy checks, nextest builds: they share no artefact, so clippy does not warm the test build and the cost is additive. Unverified: the same on pro. This Mac is not pro; clippy is mostly CPU, so my guess is 2-3x (60-90 s at worst); the first Gate run with the step prints the real number. The nextest Summary target (133 s) is not affected: the step is outside nextest.

## 4. Config

- **`clippy.toml`: none.** No lint needs a threshold. `too_many_arguments` (11) and `type_complexity` (21) are fixed by a struct and an alias; a raised threshold would hide a smell and leave upstream's default.
- **`allow`: none proposed, 0 deviations.** Every lint above has a fix. The 4 existing `#[allow(clippy::too_many_arguments)]` stay as they are (they pass today); I did not check whether their commit messages give a reason (unverified).
- **`[workspace.lints]`: not needed with zero deviations.** It would be the place for the first one (a `[workspace.lints.clippy]` table, each key with a comment holding reason and count, and `[lints] workspace = true` in the four crates). Adding it empty is speculative.
- **`rustfmt.toml`: the one deviation to decide.** Same tree, `fmt --check` with a temporary `rustfmt.toml` each time, then removed:

| rustfmt.toml | hunks | changed lines |
|---|---|---|
| none (default) | 10,055 | 103,745 |
| `max_width = 120` | 8,009 | 70,908 |
| `max_width = 120`, `use_small_heuristics = "Max"` | 3,668 | 24,318 |
| `max_width = 140`, `Max` | 2,001 | 12,464 |
| `max_width = 160`, `Max` | 1,900 | 11,692 |
| `max_width = 200`, `Max` | 2,434 | 15,601 |

  For deviating: 160 + `Max` is the style the code has, and it turns a 104k-line rewrite (which breaks `git blame` for most of the code and conflicts with every open PR) into 11.7k. Against: Convention, upstream's default wins; both keys are stable (they ran on 1.99.1). Recommendation, for the User: **the default** if the fmt PRs land after the last migration, because the migrations rewrite the 53k lines of `cli/tests` and every migration PR can leave its files fmt-clean (unverified that Workers will), so the diff shrinks by itself; **160 + `Max`** if fmt must land before them. Either way, list the fmt commits in `.git-blame-ignore-revs` (GitHub honours that file).

## 5. The plan

### Order against the migrations

The migration PRs (docs/research/tests-mock-design.md §7; ADR 0013 is not on main: `docs/adr` ends at 0012) rewrite: `cli/tests/{session,engine,store,tui,flow,pr,tooling}/*` and `cli/tests/common` (`m-*`: 53k fmt lines, 81 clippy hits), `core/src/{store,host,driver,session,slot,ask,plan,context,pr,git}/` (`ck`), `core/src/host/tmux/{junctions,palette}.rs` and 13 files with real waits (`m-units`), `tui/src/*/` (loop seams), `cli/src/fakes/` (`ft`, `fg`). A lint or fmt PR before them conflicts with them. The fix lands after `del`, the last migration (the User said late), in this order:

1. clippy machine fix, then clippy hand fix (few lines);
2. `cargo fmt`, after the clippy PRs, because `--fix` output is not rustfmt-clean;
3. the Gate step PR, last: it is red until 1 and 2 have landed.

Re-run this audit's three commands at `del` before cutting the PRs: the numbers below are against today's tree, and the migrations remove most test-code lints.

### PRs of at most about 400 changed lines

| PR | content | changed lines | needs |
|---|---|---|---|
| L1 | `clippy --fix`: core + engine (30 files) | 200 | `del` |
| L2 | `clippy --fix`: tui + cli (41 files) | 221 | L1 |
| L3 | by hand, core: `core/build.rs:80` first (it blocks `-D`), then `too_many_arguments`, `large_enum_variant`, `doc_lazy_continuation`, `enum_variant_names`, the rest (30 diagnostics) | about 150 (guess) | L1 |
| L4 | by hand, tui (15 diagnostics) | about 60 (guess) | L2 |
| L5 | by hand, `cli/src/fakes/tmux` visibility (24 diagnostics, one cause) | under 20 (guess) | L2 |
| L6 | by hand, cli: the 14 un-fixed, `type_complexity` aliases, `dead_code`, `ptr_arg`; one PR per `cli/tests/*` directory if over 400 | about 150-250 (guess) | L2 |
| F1 | `cargo fmt`: core | 27.4k | L1-L6 |
| F2 | `cargo fmt`: engine | 2.2k | F1 |
| F3 | `cargo fmt`: tui | 19.5k | F1 |
| F4 | `cargo fmt`: cli | 54.7k | F1 |
| G | the Gate step | 3 lines + docs | F1-F4 |

L1-L6 fit the ceiling (if a guess is wrong, split by crate). F1-F4 cannot: `cargo fmt` is one command whose output is 104k lines (11.7k with the `rustfmt.toml` of section 4, then one PR). Splitting it into hundreds of file PRs buys nothing: each is as mechanical as the whole and conflicts worse with open PRs. The User said "however large" (pro-d28). Each F PR is one `cargo fmt` commit and nothing else, and the Gate's own `fmt --check` is its proof.

### The Gate step PR (`G`)

- `package.json`, `verify:steps` (today, read: `check-migrations-frozen.sh && check-portable.sh && npm run typecheck && (npm test in a scratch TMPDIR) && pi-check.sh … && export CPD_COMMIT=$(git rev-parse --short HEAD) && PATH="$PWD/scripts/tripwire:$PATH" scripts/cargo.sh nextest run --workspace`). Insert after the `export CPD_COMMIT=…` step and before the nextest step (cheap steps first; `cli/build.rs` reads `CPD_COMMIT`, so clippy sees the build's environment):

      scripts/cargo.sh fmt --all --check && scripts/cargo.sh clippy --workspace --all-targets -- -D warnings

  Unverified: whether clippy wants the tripwire `PATH` too. It runs the two build scripts (`core/build.rs`, `cli/build.rs`); I read their start and saw no tmux or git call.
- `scripts/gate-run.sh`: **no change.** It takes the whole `verify:steps` as its argument and checks for leftovers after it (read: its header and `main` flow); fmt and clippy start no process and write nothing under `TMPDIR`.
- The pin: `pi/test/gate.test.ts`, test "the verify script runs the portability check and pi-check, in the documented order". It asserts `pkg.scripts["verify:steps"]` equals the array joined by ` && `. It fails first when only `package.json` changes (the exact-equality assertion) and passes when the two strings above are added to the array at the same place; rename the test to mention fmt and clippy. Also update the hs-d355 prose in `docs/implementing.md` (Gate order, near line 145).
- `scripts/cargo.sh` needs no change: `fmt` and `clippy` are not `test` or `nextest`, so they take no test slot (read: `scripts/cargo.sh`, the `case` on `${1:-}`). `hooks.pre-commit` (quick check) stays `typecheck` and `build`.

## Unverified, in one place

Gate-step time on pro (guess 2-3x the Mac); line counts of L3-L6; why 14 fixable hits were not applied; whether `enum_variant_names` hits a stored name; whether Workers leave touched files fmt-clean; the reason behind the 4 existing `allow`s.
