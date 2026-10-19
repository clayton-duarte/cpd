#!/bin/sh
# Measures one Rust test run (hs-d284: at most 30 s on the clock, at most 300 s
# of test time added up). Builds the tests first (`nextest run --no-run`, not
# timed), then runs `scripts/cargo.sh nextest run --workspace --profile timing`
# with the given extra arguments (none for the full run) and prints, from the
# JUnit report of the `timing` profile (.config/nextest.toml):
#   - the load before and after (`uptime`),
#   - wall: nextest's run time (its Summary line, build excluded) and the
#     clock time of the whole `nextest run` command,
#   - sum: every test's time added up, and the test count,
#   - the 30 slowest tests,
#   - per test binary: the summed time, the count and the slowest test.
# Exits with nextest's exit status, after printing (a red run is measured too).
#
#   scripts/test-time.sh                 # the full run
#   scripts/test-time.sh -E 'binary_id(cpd::plan)'   # a subset, same report
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# nextest 0.9.148 writes its reports under <workspace>/target/nextest/<profile>/,
# also with CARGO_TARGET_DIR set (observed).
junit="$root/target/nextest/timing/junit.xml"

scripts/cargo.sh nextest run --workspace --profile timing --no-run "$@" >&2

rm -f "$junit"
load_before=$(uptime)
start=$(date +%s.%N)
status=0
scripts/cargo.sh nextest run --workspace --profile timing "$@" >&2 || status=$?
end=$(date +%s.%N)
load_after=$(uptime)

if [ ! -f "$junit" ]; then
  echo "scripts/test-time.sh: nextest wrote no JUnit report at $junit (exit $status)" >&2
  [ "$status" -ne 0 ] || status=1
  exit "$status"
fi

node - "$junit" "$start" "$end" "$load_before" "$load_after" <<'EOF'
const fs = require("node:fs");
const [junit, start, end, loadBefore, loadAfter] = process.argv.slice(2);
const xml = fs.readFileSync(junit, "utf8");
const unescape = (s) =>
  s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, "&");
const attrs = (tag) => {
  const out = {};
  for (const m of tag.matchAll(/([\w-]+)="([^"]*)"/g)) out[m[1]] = unescape(m[2]);
  return out;
};
const suites = xml.match(/<testsuites\b[^>]*>/);
if (!suites) throw new Error(`no <testsuites> in ${junit}`);
const run = attrs(suites[0]);
const tests = [];
for (const m of xml.matchAll(/<testcase\b[^>]*>/g)) {
  const a = attrs(m[0]);
  if (a.time === undefined) throw new Error(`testcase without time: ${m[0]}`);
  tests.push({ binary: a.classname, name: a.name, time: Number(a.time) });
}
const sum = tests.reduce((s, t) => s + t.time, 0);
const f = (x) => x.toFixed(3).padStart(9);
console.log(`load before: ${loadBefore.trim()}`);
console.log(`load after:  ${loadAfter.trim()}`);
console.log(`wall:  ${Number(run.time).toFixed(3)} s (nextest run time); ${(Number(end) - Number(start)).toFixed(3)} s (the whole nextest run command)`);
console.log(`sum:   ${sum.toFixed(3)} s (every test's time added up)`);
console.log(`count: ${tests.length} tests (${run.failures ?? "?"} failed, ${run.errors ?? "?"} errors)`);
console.log("\n30 slowest tests (s):");
for (const t of [...tests].sort((a, b) => b.time - a.time).slice(0, 30)) console.log(`${f(t.time)}  ${t.binary} ${t.name}`);
const bins = new Map();
for (const t of tests) {
  const b = bins.get(t.binary) ?? { sum: 0, count: 0, max: null };
  b.sum += t.time;
  b.count += 1;
  if (!b.max || t.time > b.max.time) b.max = t;
  bins.set(t.binary, b);
}
console.log("\nper binary (sum s, count, slowest s):");
for (const [name, b] of [...bins].sort((a, b) => b[1].sum - a[1].sum))
  console.log(`${f(b.sum)}  ${String(b.count).padStart(4)}  ${f(b.max.time)}  ${name}`);
EOF

exit "$status"
