# Leak-flake: does cpd follow the industry standards for testing Rust? (Q5)

## Verdict

Q5 answer: **partly**. cpd matches the standard on the runner model (nextest, process per test) and on test groups, and breaks it on three items: one test run per host, leak strictness, and thread count under concurrency. Counts are over 16 projects read (table below), `verified` unless marked.
- Runner model: yes. 6 of 16 run nextest in CI (uv, omicron, tokio, rust-analyzer, nextest, reth); 9 run `cargo test`; tikv defaults to `cargo test` with nextest opt-in.
- Thread count: cpd uses the default (cores) like 4 of the 6 nextest users; the 2 that cap it (uv `test-threads` 20/12, omicron `--test-threads -2`) do it for their own runner size. omicron's comment (build-and-test.sh:124-128) gives the same reason as this Plan: too many tests at once make tests slow "where the test would have worked but the system was too overloaded". tikv caps libtest to 2 threads on macOS (Makefile:83).
- Leak strictness: cpd is alone. 0 of 16 set `leak-timeout`; 2 of 2 extra projects found by `gh search code` (polkadot-sdk, prisma-engines) set `leak-timeout = "100ms"` (the default period) with the default result `pass`. 0 of 18 set `result = "fail"` (the search is a sample, not a census: inference).
- Retries: cpd has none; 4 of 16 do (omicron 2, reth 2, tikv 2, nextest 3 on macOS); 12 do not. Retry is common, not universal; the repo's rule against retries is a deviation, a harmless one for LEAK-FAIL's cause.
- Concurrency on one host: 0 of 16 share a host between several full test runs (GitHub-hosted: a new VM per job, docs below; cloud runners: inference; buildomat and incredibuild: unverified). cpd's about 5 Gates at once on one 14-core machine has no counterpart found. No project with a shared host was found, so no data on how they cap threads.
- Which deviation explains LEAK-FAIL: concurrency on one host with default threads (inference, consistent with the Plan's data: 3 Gates = 42 processes had 0 LEAK; 4-5 Gates at load 55-100 had 1-8). `result = "fail"` makes a late nextest visible, it does not cause it; no retries leaves it unabsorbed. Process-per-test and test groups are harmless.

## Evidence

### 1. The two models (primary documentation, read for this Job)

| | `cargo test` | nextest |
|---|---|---|
| Unit of execution | a test binary: binaries run serially; "binaries are responsible for running individual tests in parallel" | a test: each in a process of its own, all binaries in parallel (<https://nexte.st/docs/design/how-it-works/>) |
| Parallelism inside | libtest threads of one process; `--test-threads` default = `available_parallelism` (<https://doc.rust-lang.org/rustc/tests/index.html>) | `test-threads` default `"num-cpus"` (<https://nexte.st/docs/configuration/reference/>) |
| Why | simplest interface: only the exit code | per-test status and time, long-pole tests overlap, per-test isolation: "process-per-test model is powerful because it serves as the Schelling point" (<https://nexte.st/docs/design/why-process-per-test/>) |
| Leaks | not detected | `leak-timeout`: waits after the test exits for stdout/stderr to close; default 100 ms; default result: leaky tests "are considered to be successful"; `result = "fail"` (0.9.95) gives `LEAK-FAIL` (<https://nexte.st/docs/features/leaky-tests/>) |
| Retries | none | `--retries`, a retried pass is `FLAKY` (<https://nexte.st/docs/features/retries/>) |
| Slow tests | none | `slow-timeout` default 60 s, only reports unless `terminate-after` (<https://nexte.st/docs/features/slow-tests/>) |
| Limits per set of tests | none | test groups (`max-threads`), `threads-required` for "heavy tests" that "might overwhelm" the CPU (<https://nexte.st/docs/configuration/test-groups/>, <https://nexte.st/docs/configuration/threads-required/>) |

What the leak timer is for (leaky-tests page): a test that starts a subprocess which inherits stdout/stderr and does not clean it up; nextest "can detect some, but not all" such cases. It was designed to spot a real child, with default result `pass`, because the signal is advisory. The page does not mention a late nextest or a loaded host (verified: read the page; the word load does not appear).

What nextest says about CI and shared machines: it documents CI features (`--partition` sharding across machines: <https://nexte.st/docs/ci-features/partitioning/>; archives) for runs "that take too long on a single machine". I found no page that discusses several nextest runs on one host (unverified: I read the pages above, not the whole site). `-j/--test-threads` accepts a negative integer, "relative to CPU count" (running page), the mechanism omicron uses.

GitHub-hosted runners: "each GitHub-hosted runner is a new virtual machine (VM)", except single-CPU runners (container on a shared VM) (<https://docs.github.com/en/actions/concepts/runners/about-github-hosted-runners>). So one test run per VM and no neighbour run on the same cores. This is Q1's likely axis, not answered here.

### 2. Projects (read at HEAD during this Job; `gh api`, 76 calls of the 120 allowed, see Reproduction)

| # | project | test runner (file:line) | nextest config keys | runs-on | shared host? | files read |
|---|---|---|---|---|---|---|
| 1 | astral-sh/uv | nextest (test.yml:122,217,316) | `.config/nextest.toml`: `test-threads=20` (ci), `12` (ci-macos); groups `serial`,`io-bound` (max 1); no leak, no retries | depot-ubuntu-24.04-16; namespace-profile-macos-15; namespace-profile-windows-2022-x86-64-16x32 (test.yml:32,153,247) | cloud runner vendors; one VM per job (inference) | <https://github.com/astral-sh/uv/blob/HEAD/.github/workflows/test.yml> <https://github.com/astral-sh/uv/blob/HEAD/.config/nextest.toml> |
| 2 | oxidecomputer/omicron | nextest (buildomat/build-and-test.sh:148) | `ci`: `retries=2`, `flaky-result="fail"`; groups; `threads-required=4`; CLI `--test-threads -2`; no leak | buildomat `ubuntu-22.04-large` (jobs/build-and-test-linux.sh:8); rust.yml checks on ubuntu-22.04, macos-26 | own CI service; host sharing unverified | <https://github.com/oxidecomputer/omicron/blob/HEAD/.github/buildomat/build-and-test.sh> <https://github.com/oxidecomputer/omicron/blob/HEAD/.github/buildomat/jobs/build-and-test-linux.sh> <https://github.com/oxidecomputer/omicron/blob/HEAD/.config/nextest.toml> |
| 3 | tokio-rs/tokio | nextest (ci.yml:89,122,126,164) + `cargo test --doc`, `cargo hack test` | no file (contents API: not found) | matrix windows/ubuntu/macos-latest (ci.yml:63-67) | GitHub-hosted: no | <https://github.com/tokio-rs/tokio/blob/HEAD/.github/workflows/ci.yml> |
| 4 | rust-lang/cargo | cargo test (main.yml:243,255) | no file | matrix ubuntu-latest, ubuntu-24.04-arm, macos-15, windows-latest, windows-11-arm (main.yml:142-175,205-230) | GitHub-hosted: no | <https://github.com/rust-lang/cargo/blob/HEAD/.github/workflows/main.yml> |
| 5 | rust-lang/rust-analyzer | nextest (ci.yaml:129); `cargo test` for proc-macro (ci.yaml:76) | only `slow-timeout` 60s x5 | matrix ubuntu/windows/macos-latest (ci.yaml:90) | GitHub-hosted: no | <https://github.com/rust-lang/rust-analyzer/blob/HEAD/.github/workflows/ci.yaml> <https://github.com/rust-lang/rust-analyzer/blob/HEAD/.config/nextest.toml> |
| 6 | BurntSushi/ripgrep | cargo test (ci.yml:186) | no file | ubuntu-latest etc. matrix (ci.yml:55-95) | GitHub-hosted: no | <https://github.com/BurntSushi/ripgrep/blob/HEAD/.github/workflows/ci.yml> |
| 7 | bevyengine/bevy | cargo test (tools/ci/src/commands/test.rs:26; `cargo run -p ci -- test`, ci.yml:56); optional `-- --test-threads` argument (test.rs:14) | no file | matrix windows/ubuntu/macos-latest (ci.yml:30) | GitHub-hosted: no | <https://github.com/bevyengine/bevy/blob/HEAD/.github/workflows/ci.yml> <https://github.com/bevyengine/bevy/blob/HEAD/tools/ci/src/commands/test.rs> |
| 8 | nushell/nushell | cargo test (ci.yml:109-111) | no file | incredibuild-ubuntu-2204, incredibuild-windows-2022, macos-latest (ci.yml:45-52) | third-party hosted runners; sharing unverified | <https://github.com/nushell/nushell/blob/HEAD/.github/workflows/ci.yml> |
| 9 | helix-editor/helix | cargo test --workspace (build.yml:60) | no file | matrix 5 OSes, GitHub-hosted (build.yml:67) | no | <https://github.com/helix-editor/helix/blob/HEAD/.github/workflows/build.yml> |
| 10 | bytecodealliance/wasmtime | cargo test (ci/run-tests.py:26; main.yml:516,789) | no file | ubuntu-latest / ubuntu-26.04 / matrix (main.yml:41,69,619) | GitHub-hosted: no | <https://github.com/bytecodealliance/wasmtime/blob/HEAD/.github/workflows/main.yml> <https://github.com/bytecodealliance/wasmtime/blob/HEAD/ci/run-tests.py> |
| 11 | nextest-rs/nextest | nextest on itself (ci.yml:227 `cargo local-nt run --profile ci`) | `ci` profile: `retries` count 3 on macOS for integration tests; group `my-group` max 16; `threads-required=4`; no test-threads, no leak | ubuntu-latest, ubuntu-24.04-arm, macos-latest, windows-latest (ci.yml:166-172) | GitHub-hosted: no | <https://github.com/nextest-rs/nextest/blob/HEAD/.github/workflows/ci.yml> <https://github.com/nextest-rs/nextest/blob/HEAD/.config/nextest.toml> |
| 12 | clap-rs/clap | cargo test via make (ci.yml:82) | no file | ubuntu/windows/macos-latest (ci.yml:36-62) | GitHub-hosted: no | <https://github.com/clap-rs/clap/blob/HEAD/.github/workflows/ci.yml> |
| 13 | tauri-apps/tauri | cargo test (test-core.yml:126-139) | no file | windows/ubuntu/macos-latest, macos-14 (test-core.yml:44-66) | GitHub-hosted: no | <https://github.com/tauri-apps/tauri/blob/HEAD/.github/workflows/test-core.yml> |
| 14 | meilisearch/meilisearch | cargo test (test-suite.yml:43,61,79,98) | no file | ubuntu-22.04, ubuntu-22.04-arm (test-suite.yml:22) | GitHub-hosted: no | <https://github.com/meilisearch/meilisearch/blob/HEAD/.github/workflows/test-suite.yml> |
| 15 | paradigmxyz/reth | nextest (unit.yml:72,79,139) + `cargo test --doc` | default profile: `retries` 2 exponential; slow-timeout overrides; no test-threads, no leak | depot-ubuntu-latest-4 / -8 (unit.yml:31,83) | cloud runner vendor; one VM per job (inference) | <https://github.com/paradigmxyz/reth/blob/HEAD/.github/workflows/unit.yml> <https://github.com/paradigmxyz/reth/blob/HEAD/.config/nextest.toml> |
| 16 | tikv/tikv | `make test` = cargo test (Makefile:336 -> scripts/test-all); nextest opt-in `test_with_nextest` (Makefile:339) | `ci` profile: `retries=2`, slow-timeout 60s x2; `RUST_TEST_THREADS ?= 2` on macOS (Makefile:83) | its GitHub workflow list holds one file (tikv-clippy-darwin.yml); the test CI runs elsewhere (unverified) | unverified | <https://github.com/tikv/tikv/blob/HEAD/Makefile> <https://github.com/tikv/tikv/blob/HEAD/.config/nextest.toml> |

"Files read" are HEAD URLs; line numbers are of the file at the time read. tokio, cargo, ripgrep, bevy, nushell, helix, wasmtime, clap, tauri, meilisearch had no `.config/nextest.toml` (the contents API returned not found). Not among the 16 and not read: deno, rust-lang/rust, swc (I stopped at 16; the Job needs at least 12). deno's `ci.generate.ts` was too large for the contents API and is not used.

Counts (over the 16 rows):
- use nextest in CI: **6** (uv, omicron, tokio, rust-analyzer, nextest, reth); `cargo test` only: 9; tikv: `cargo test` default, nextest opt-in, `ci` profile exists (unverified which CI uses it).
- set `test-threads`: nextest users **2 of 6** (uv in the file; omicron on the command line); libtest cap: tikv (macOS, `RUST_TEST_THREADS` 2), bevy (optional argument). **4 of 6** nextest users use the default.
- set a `leak-timeout`: **0 of 16**. Extra, from `gh search code 'leak-timeout filename:nextest.toml'` then read: polkadot-sdk (`.config/nextest.toml:87`) and prisma-engines (`:83`) set `"100ms"` with a comment copied from nextest's docs template, result left at default (`pass`). With `result = "fail"`: **0 of 18**.
- use retries: **4 of 16** (omicron `profile.ci` 2 + `flaky-result="fail"`; reth default profile 2; tikv `ci` 2; nextest `ci` 3 on macOS only); and reth's retries apply locally too. 12 of 16: none.
- use test groups / `threads-required`: uv, omicron, nextest (groups); omicron, nextest (`threads-required`). cpd has a `tmux-restart` group (`.config/nextest.toml`), so cpd follows this.
- self-hosted shared hosts: **0 verified**. omicron on buildomat and nushell on incredibuild are third-party/own CI services (unverified whether a host runs several jobs); uv/reth use depot/namespace cloud runners (inference: one VM per job).

### 3. Comparison with cpd (about 5 full Gates at once on one 14-core developer machine; default threads; `leak-timeout.result = "fail"`; no retries)

| item | the standard (this table) | cpd | standard or cpd's own |
|---|---|---|---|
| one test run per VM | 16 of 16 on CI VMs (GitHub-hosted: new VM per job) | about 5 runs on one host, plus Workers' builds | **cpd's own** |
| default threads | 4 of 6 nextest users default; 2 cap it for their runner (uv, omicron) | default (14), x about 5 concurrent runs = about 70 | default is standard; the default under 5 concurrent runs is **cpd's own** |
| thread cap when the host is loaded | omicron: `-2` because the system was "too overloaded" and tests "take longer than their default timeouts" (build-and-test.sh:124-128); tikv: libtest 2 on macOS | none (PR `threads` pending: 4) | the standard does cap, case by case |
| leak result `pass` | 18 of 18 that set a timeout or none: `pass` | `fail` | **cpd's own** |
| leak period | default 100 ms (2 of 2 that set it keep 100 ms) | 100 ms | standard |
| retries on CI | 4 of 16 | none (repo rule) | minority practice; cpd's own stricter choice |
| process per test (nextest) | 6 of 16, a large share of the big ones | yes | standard |
| test groups | uv, omicron, nextest | `tmux-restart` | standard |
| shared host with threads cap, `--partition`, `threads-required` or `max-threads` group | none found | one group (4) for tmux tests only | no standard to copy; omicron's `-2` is the nearest |

### 4. Verdict per item

| item | follows the standard? | explains LEAK-FAIL? |
|---|---|---|
| runner model (nextest, process per test) | yes, 6 of 16 | harmless (the Plan's data: 0 of 27,772 processes had a stray descriptor) |
| nextest `test-threads` default | yes in itself, 4 of 6 | only in combination with the next row |
| concurrency on one host | **no**, 0 of 16 | **the deviation the Plan's evidence points to**: nextest is scheduled late (spawn to shell 1-11 s) so its 100 ms timer runs while it is not running (inference; H3a not proven) |
| leak strictness `fail` | **no**, 0 of 18 | it exposes the late timer as a failure; the standard `pass` would hide it. It is not the cause. The repo's rule says never to set it back (the repo's rule, not this Job's) |
| retries | no (4 of 16 use them) | not a cause; absence leaves a LEAK-FAIL unabsorbed |
| test groups | yes | harmless |

## Reproduction

Needs `gh` authenticated; read-only; about 76 calls. Save as a script in a temp dir and run with `bash`.

```bash
set -u
T=$(mktemp -d); cd "$T"
g(){ timeout 30 gh api "$1" 2>/dev/null; }
dec(){ python3 -c "import sys,json,base64
try: print(base64.b64decode(json.load(sys.stdin)['content']).decode())
except Exception: print('NOTFOUND')"; }
# 1. nextest config and workflow list of each project (2 calls each)
for r in astral-sh/uv oxidecomputer/omicron tokio-rs/tokio rust-lang/cargo rust-lang/rust-analyzer \
         BurntSushi/ripgrep bevyengine/bevy nushell/nushell helix-editor/helix bytecodealliance/wasmtime \
         nextest-rs/nextest clap-rs/clap tauri-apps/tauri meilisearch/meilisearch paradigmxyz/reth tikv/tikv; do
  n=${r//\//_}
  g "repos/$r/contents/.config/nextest.toml" | dec > "$n.toml"
  g "repos/$r/contents/.github/workflows" | python3 -c "import sys,json
try: print(' '.join(x['name'] for x in json.load(sys.stdin)))
except Exception: print('NOLIST')" > "$n.list"
done
# 2. the workflow that runs the tests (one call each)
wf(){ g "repos/$1/contents/$2" | dec > "$3"; }
wf astral-sh/uv .github/workflows/test.yml uv.wf
wf bevyengine/bevy .github/workflows/ci.yml bevy.wf
wf BurntSushi/ripgrep .github/workflows/ci.yml rg.wf
wf bytecodealliance/wasmtime .github/workflows/main.yml wasmtime.wf
wf clap-rs/clap .github/workflows/ci.yml clap.wf
wf helix-editor/helix .github/workflows/build.yml helix.wf
wf meilisearch/meilisearch .github/workflows/test-suite.yml meili.wf
wf nextest-rs/nextest .github/workflows/ci.yml nextest.wf
wf nushell/nushell .github/workflows/ci.yml nu.wf
wf oxidecomputer/omicron .github/buildomat/build-and-test.sh omicron.sh
wf oxidecomputer/omicron .github/buildomat/jobs/build-and-test-linux.sh omicron-job.sh
wf paradigmxyz/reth .github/workflows/unit.yml reth.wf
wf rust-lang/cargo .github/workflows/main.yml cargo.wf
wf rust-lang/rust-analyzer .github/workflows/ci.yaml ra.wf
wf tauri-apps/tauri .github/workflows/test-core.yml tauri.wf
wf tokio-rs/tokio .github/workflows/ci.yml tokio.wf
wf tikv/tikv Makefile tikv.mk
wf bytecodealliance/wasmtime ci/run-tests.py wasmtime.py
wf bevyengine/bevy tools/ci/src/commands/test.rs bevy.rs
# 3. leak-timeout users outside the list (1 search + 1 call each)
timeout 30 gh search code 'leak-timeout filename:nextest.toml' --limit 30 --json repository,path
wf paritytech/polkadot-sdk .config/nextest.toml polkadot.toml
wf prisma/prisma-engines .config/nextest.toml prisma.toml
# 4. the keys
grep -HnE 'test-threads|leak-timeout|retries|slow-timeout|threads-required|test-group' *.toml
grep -HnE 'runs-on|nextest|cargo test|--test-threads|RUST_TEST_THREADS' *.wf *.sh *.mk *.py *.rs
echo "$T"   # remove it when done: rm -rf "$T"
```

Docs (curl, not gh): the nextest pages in section 1 (note `design/how-it-works`, `features/leaky-tests`, `features/retries`, `features/slow-tests`, `configuration/reference`, `configuration/threads-required`, `configuration/test-groups`, `ci-features/partitioning`, `design/why-process-per-test`) and the rustc book `tests/index.html`.

## What this means for the fix

No code change in this Job. Options the data supports, with cost:
- Cap `test-threads` below the core count (PR `threads`, 4 provisional): this is what omicron does for the same symptom (`-2`, build-and-test.sh:124-128) and what tikv does on macOS; cost: one line, longer single-Gate wall time. The standard supports the idea of a cap, not the number 4 (verified: no project documents how it chose one beyond omicron's reason).
- Leaving `result = "fail"` and `period = 100ms`: a strictness the 18 projects do not share; it is the repo's rule and the standard does not argue against it, but the standard (default `pass`) is where a loaded host would not have failed. Keep it, as the Plan's rules say; the finding is that cpd is the strict outlier, not that it is wrong.
- Retries: 4 of 16 projects use them on CI; the repo's rule forbids them. Not needed to explain the data.
- Running fewer Gates per host (Q3/Q4's axis): matches the standard (one run per VM); cost: Worker cap.
- Nothing here shows process-per-test, nextest, or test groups to be at fault.
