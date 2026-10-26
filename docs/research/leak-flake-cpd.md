# LEAK-FAIL on cpd's real tests: findings (rs-cpd-a1)

## Verdict

(in progress: skeleton)

## Evidence

### Step 0: the six saved Gate logs (verified: `sed 's/\[[0-9;]*m//g' /tmp/cpd-leak-flake/leak-logs/*.log | grep -E 'Starting|^ +LEAK-FAIL'`)

| log | tests / binaries | victims (position, nextest time) |
|---|---|---|
| bifrost_pr-spike | 5137 / 17 | config::runner_tests::read_with_maps_... (2013, 0.217 s) |
| bifrost_pr-spike2 | 5137 / 17 | jobs fingerprint (3539, 0.902 s), exposure (3550, 0.228), poll_run (4017, 0.122), pulse (4484, 0.215), counter (4697, 0.189), view_ports (4984, 0.178) |
| mcp-policy_pr-mcp | 3757 / 19 | policy::tests::a_symlinked_file... (2649, 0.207) |
| no-slots_pr-slots | 3633 / 19 | ensure_starts_the_engine_for_a_caller_without_a_utf8_locale (266, 85.043 s), curation_lists_pending_retirements_and_doc_candidates (449, 21.876 s) |
| tests-mock_pr-mf-a1 | 3530 / 20 | tree::rows (3274, 0.159), tree::view (3344, 0.136) |
| tests-mock_pr-mf-b1 | 3530 / 20 | store::schema (2553, 0.128) |

What the logs show (verified): every victim's captured output is the normal `running 1 test ... ok`, `exited with code 0, but leaked handles`; nothing from a holder is in the output. No thread count is printed. The Gate's `Starting N tests across M binaries` line is the only run header.
For the two long victims the test's own `finished in` (84.85 s, 21.12 s) is 0.19 s and 0.76 s short of nextest's time (85.043, 21.876): the leak wait follows the test's end.
What the logs cannot show: no timestamps, so which tests ran together, and the load, are unknown.
Spawning victims: both run the `cpd` binary through the test fixture (`cli/tests/common/fx.rs` `Fx::cpd`/`cpd_with`, `cli/tests/common/mod.rs:1003` `TestEnv::command`, stdin null, stdout/stderr piped by `output()`-style capture: see Step 4 for the file:line of what is inherited).
