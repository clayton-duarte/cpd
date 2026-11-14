# Remediation of the 24-hour merged-PR feedback audit

The User explicitly requested all 44 outstanding review concerns in one PR, on 2026-11-14. This is a bounded maintenance request, not a restart of the paused continuous QA run. The one-area Job restriction does not describe this cross-area maintenance branch; the User still reviews and merges. No deployment or live-machine access is authorized.

Base: `8a8c67d2761a22b4b76af637b20dd430b8d83552`. The original audit covered 88 merged PRs, 662 timeline entries, and 189 distinct concerns. Preserve previously merged behavior and regression coverage; resolving a thread is not evidence of a fix.

## Evidence and design decisions

- SQLite child rows are removed before parent rows, and repair refuses unknown additive columns rather than dropping data: https://www.sqlite.org/foreignkeys.html . Frozen migrations are never edited; any new schema goes in a forward migration.
- GitHub thread metadata is authoritative only after a complete selected connection. `pageInfo` distinguishes completeness from truncation: https://docs.github.com/en/graphql/guides/using-pagination-in-the-graphql-api . Partial or omitted reads cannot clear a merge gate.
- A mutation rechecks its prerequisites at execution; cached or enqueue-time permission is not sufficient. Re-review claims and outbox entries remain atomic and requests revalidate current head/state.
- curl reads private headers from stdin using `--header @-`; credentials never enter process argv: https://curl.se/docs/manpage.html . Header values reject CR/LF injection.
- Parser alternative: extending the custom line scanner has already produced nine independently reproduced language failures. Adopt maintained Tree-sitter Rust/TSX grammars, using named syntax nodes rather than quotes/braces as language boundaries. https://tree-sitter.github.io/tree-sitter/using-parsers/2-basic-parsing.html ; https://docs.rs/tree-sitter/ ; https://vitest.dev/api/test ; https://doc.rust-lang.org/reference/attributes/testing.html . Record exact dependency versions in Cargo.lock and the implementing guide. Runtime-generated test names remain a static-analysis limitation, not a claim of complete semantic test coverage.
- WebSocket Origin checking and client authentication are separate boundaries: https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html . The User chose loopback-only by default, with authenticated LAN access as an opt-in, on 2026-11-14. Apply the same boundary to production and preview; configure each of the three independent backends separately.
- State labels normalize whitespace/control characters and truncate Unicode characters; original dialog identity/title stays intact. Busy-session Escape chooses abort before any dialog cancellation; the existing backend reports unsupported aborts.

Official documentation accessed 2026-11-14. These sources support the adopted mechanics; they are not evidence of unanimous community agreement about product policy. Research and acceptance criteria also remain linked to the original review below.

## Review-to-fix ledger

Every row must name implementation and validation before this PR is ready. Duplicate review reports may share a root fix; none are silently dropped.

| Concern | Review | Implementation / validation |
|---|---|---|
| 4570597165 | [#1256: Preserve unknown thread state when the poll omits reviewThreads](https://github.com/clayton-duarte/cpd/pull/1256#discussion_r4570597165) | [#1287](https://github.com/clayton-duarte/cpd/issues/1287) — GraphQL selects `pageInfo`; a forward migration adds explicit `review_threads_complete`, initially false for legacy rows. Omitted/truncated reads retain IDs but clear trust. `incomplete_metadata_never_clears_the_merge_gate_and_unchanged_reads_backfill` covers unknown, complete-open, complete-empty and metadata recovery. |
| 4570597171 | [#1256: Detect head-only changes before deciding whether to re-wake](https://github.com/clayton-duarte/cpd/pull/1256#discussion_r4570597171) | [#1302](https://github.com/clayton-duarte/cpd/issues/1302) — Poll retains the changed-head set before refreshing persisted heads, and routes a head-only owned change through feedback wake-up once. The SWR lifecycle regression keeps `updatedAt` constant across the push and asserts re-wake followed by an unchanged quiet poll. |
| 4570821892 | [#1256: Discard queued re-review requests after the PR closes](https://github.com/clayton-duarte/cpd/pull/1256#discussion_r4570821892) | [#1303](https://github.com/clayton-duarte/cpd/issues/1303) — Queued re-review comments, including legacy entries with `head_sha`, revalidate open state, exact head and outstanding feedback. `queued_rereview_is_discarded_after_close_or_a_newer_push` proves obsolete requests make no GitHub calls. |
| 4570978758 | [#1256: Initialize the review baseline in the poll that discovers feedback](https://github.com/clayton-duarte/cpd/pull/1256#discussion_r4570978758) | [#1304](https://github.com/clayton-duarte/cpd/issues/1304) — Metadata initializes the baseline on first discovery; quiet polls advance it without posting. Metadata/backfill tests and `quiet_polls_advance_the_baseline_before_feedback_reappears` prove that feedback on an unchanged head does not request a stale re-review. |
| 4570978764 | [#1256: Verify thread-list completeness before clearing the merge gate](https://github.com/clayton-duarte/cpd/pull/1256#discussion_r4570978764) | [#1287](https://github.com/clayton-duarte/cpd/issues/1287) — GraphQL selects `pageInfo`; a forward migration adds explicit `review_threads_complete`, initially false for legacy rows. Omitted/truncated reads retain IDs but clear trust. `incomplete_metadata_never_clears_the_merge_gate_and_unchanged_reads_backfill` covers unknown, complete-open, complete-empty and metadata recovery. |
| 4571012112 | [#1256: Apply the unresolved-thread gate to auto-merge](https://github.com/clayton-duarte/cpd/pull/1256#discussion_r4571012112) | [#1288](https://github.com/clayton-duarte/cpd/issues/1288) — The auto-merge executor shares the unresolved/unknown-thread gate. Polling an owned armed PR queues cancellation when blocked; cancellation preserves desired auto-merge as pending for later revalidation. Tests `auto_merge_waits_for_complete_resolved_thread_metadata` and `already_armed_auto_merge_is_disabled_when_threads_are_unknown` cover the external-write boundary. |
| 4571012117 | [#1256: Advance the review baseline while feedback is empty](https://github.com/clayton-duarte/cpd/pull/1256#discussion_r4571012117) | [#1304](https://github.com/clayton-duarte/cpd/issues/1304) — Metadata initializes the baseline on first discovery; quiet polls advance it without posting. Metadata/backfill tests and `quiet_polls_advance_the_baseline_before_feedback_reappears` prove that feedback on an unchanged head does not request a stale re-review. |
| 4571058228 | [#1256: Backfill review metadata when the SWR fingerprint is unchanged](https://github.com/clayton-duarte/cpd/pull/1256#discussion_r4571058228) | [#1305](https://github.com/clayton-duarte/cpd/issues/1305) — Review metadata has its own conditional equality check and backfill after cache commit. A matching old SWR fingerprint cannot suppress absent columns. The metadata recovery regression and retained `gh_poll_unchanged_pr_opens_no_write_txn` cover recovery and unchanged-read idempotence. |
| 4570571795 | [#1279: Skip whitespace before reading JavaScript test titles](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4570571795) | [#1310](https://github.com/clayton-duarte/cpd/issues/1310) — Tree-sitter Rust/TSX grammars replace the custom scanner; production selects grammar from the actual path. Parser regressions cover all eight syntax findings plus fixture-language isolation. All existing preservation/submit regressions are retained. |
| 4570571804 | [#1279: Decode escaped quotes when constructing test identities](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4570571804) | [#1310](https://github.com/clayton-duarte/cpd/issues/1310) — Tree-sitter Rust/TSX grammars replace the custom scanner; production selects grammar from the actual path. Parser regressions cover all eight syntax findings plus fixture-language isolation. All existing preservation/submit regressions are retained. |
| 4570571809 | [#1279: Count assertions made through expect methods](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4570571809) | [#1310](https://github.com/clayton-duarte/cpd/issues/1310) — Tree-sitter Rust/TSX grammars replace the custom scanner; production selects grammar from the actual path. Parser regressions cover all eight syntax findings plus fixture-language isolation. All existing preservation/submit regressions are retained. |
| 4570762472 | [#1279: Recognize restricted visibility on Rust test declarations](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4570762472) | [#1310](https://github.com/clayton-duarte/cpd/issues/1310) — Tree-sitter Rust/TSX grammars replace the custom scanner; production selects grammar from the actual path. Parser regressions cover all eight syntax findings plus fixture-language isolation. All existing preservation/submit regressions are retained. |
| 4570762477 | [#1279: Distinguish JSX text from quoted JavaScript literals](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4570762477) | [#1310](https://github.com/clayton-duarte/cpd/issues/1310) — Tree-sitter Rust/TSX grammars replace the custom scanner; production selects grammar from the actual path. Parser regressions cover all eight syntax findings plus fixture-language isolation. All existing preservation/submit regressions are retained. |
| 4570837391 | [#1279: Recognize multiline Rust test attributes](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4570837391) | [#1310](https://github.com/clayton-duarte/cpd/issues/1310) — Tree-sitter Rust/TSX grammars replace the custom scanner; production selects grammar from the actual path. Parser regressions cover all eight syntax findings plus fixture-language isolation. All existing preservation/submit regressions are retained. |
| 4570837396 | [#1279: Preserve significant whitespace in approved test names](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4570837396) | [#1311](https://github.com/clayton-duarte/cpd/issues/1311) — Approval validates blankness without trimming the stored identity. `significant_title_whitespace_survives_approval` proves trailing title whitespace remains significant. |
| 4571007340 | [#1279: Recognize parameterized JavaScript test declarations](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4571007340) | [#1310](https://github.com/clayton-duarte/cpd/issues/1310) — Tree-sitter Rust/TSX grammars replace the custom scanner; production selects grammar from the actual path. Parser regressions cover all eight syntax findings plus fixture-language isolation. All existing preservation/submit regressions are retained. |
| 4571151543 | [#1279: Allow whitespace between assertion names and call parentheses](https://github.com/clayton-duarte/cpd/pull/1279#discussion_r4571151543) | [#1310](https://github.com/clayton-duarte/cpd/issues/1310) — Tree-sitter Rust/TSX grammars replace the custom scanner; production selects grammar from the actual path. Parser regressions cover all eight syntax findings plus fixture-language isolation. All existing preservation/submit regressions are retained. |
| 4570891394 | [#1264: Implement the WebUI command before gating engine restarts on it](https://github.com/clayton-duarte/cpd/pull/1264#discussion_r4570891394) | [#1289](https://github.com/clayton-duarte/cpd/issues/1289) — Real internal `webui --prepare/--ready/--activate` commands dispatch through the CLI. A ready loopback standby promotes after retirement; runtime publishes only the listening PID, and legacy token records supply the old PID. Supervisor respects staging. Real CLI test `real_webui_commands_prepare_validate_activate_and_reject_dead_records`, real TCP `runtime.test.ts`, supervisor and install-order tests cover this handoff. |
| 4570911858 | [#1264: Consolidate the outage display with the connection-aware banner](https://github.com/clayton-duarte/cpd/pull/1264#discussion_r4570911858) | [#1306](https://github.com/clayton-duarte/cpd/issues/1306) — App renders the existing connection-aware EngineBanner in the full-width banner row. The obsolete cached-state component and its duplicate tests are removed; equivalent down/hidden/restart coverage lives in the central EngineBanner tests. `shows_one_connection_aware_banner_when_an_engine_down_snapshot_becomes_stale` proves one notice and connection-state precedence. |
| 4569267650 | [#1278: Implement the WebUI commands before invoking them during install](https://github.com/clayton-duarte/cpd/pull/1278#discussion_r4569267650) | [#1289](https://github.com/clayton-duarte/cpd/issues/1289) — Real internal `webui --prepare/--ready/--activate` commands dispatch through the CLI. A ready loopback standby promotes after retirement; runtime publishes only the listening PID, and legacy token records supply the old PID. Supervisor respects staging. Real CLI test `real_webui_commands_prepare_validate_activate_and_reject_dead_records`, real TCP `runtime.test.ts`, supervisor and install-order tests cover this handoff. |
| 4569759722 | [#1278: Discover the old WebUI PID from an existing runtime source](https://github.com/clayton-duarte/cpd/pull/1278#discussion_r4569759722) | [#1289](https://github.com/clayton-duarte/cpd/issues/1289) — Real internal `webui --prepare/--ready/--activate` commands dispatch through the CLI. A ready loopback standby promotes after retirement; runtime publishes only the listening PID, and legacy token records supply the old PID. Supervisor respects staging. Real CLI test `real_webui_commands_prepare_validate_activate_and_reject_dead_records`, real TCP `runtime.test.ts`, supervisor and install-order tests cover this handoff. |
| 4570566984 | [#1277: Check comment targets for accepted replies before retrying](https://github.com/clayton-duarte/cpd/pull/1277#discussion_r4570566984) | [#1308](https://github.com/clayton-duarte/cpd/issues/1308) — Uncached review-comment replies inspect every REST comment page before retrying a potentially accepted write. `accepted_comment_reply_timeout_is_deduplicated_across_rest_pages` proves one POST after accepted-then-timeout, alongside the retained thread retry regression. |
| 4570566991 | [#1277: Require a terminal marker before treating a thread as answered](https://github.com/clayton-duarte/cpd/pull/1277#discussion_r4570566991) | [#1309](https://github.com/clayton-duarte/cpd/issues/1309) — Uncached thread reply detection reuses the terminal-line marker predicate instead of substring matching. `a_quoted_nonterminal_marker_does_not_suppress_an_uncached_reply` proves quoted text cannot suppress a needed reply. |
| 4564631911 | [#1114: Validate the reconciled commit before publishing it](https://github.com/clayton-duarte/cpd/pull/1114#discussion_r4564631911) | [#1298](https://github.com/clayton-duarte/cpd/issues/1298) — `pr push` measures the immutable reconciled candidate against the refreshed baseline before moving the Lead ref or publishing. `direct_pr_push_validates_reconciled_coverage_before_moving_either_ref` passes through the real CLI and preserves both refs on rejection. |
| 4569233587 | [#1274: Cancel pending preparation before clearing submission accounting](https://github.com/clayton-duarte/cpd/pull/1274#discussion_r4569233587) | [#1307](https://github.com/clayton-duarte/cpd/issues/1307) — Restored the missing preparation epoch checks and expected `TurnPreparationAborted` handling from the omitted merge commit. All four durable-host abort tests passed, including cancellation while event preparation is pending and continued host usability. |
| 4569563630 | [#1274: Handle aborted event preparation without shutting down the host](https://github.com/clayton-duarte/cpd/pull/1274#discussion_r4569563630) | [#1307](https://github.com/clayton-duarte/cpd/issues/1307) — Restored the missing preparation epoch checks and expected `TurnPreparationAborted` handling from the omitted merge commit. All four durable-host abort tests passed, including cancellation while event preparation is pending and continued host usability. |
| 4568951308 | [#1183: Bound the reseal search instead of materializing every combination](https://github.com/clayton-duarte/cpd/pull/1183#discussion_r4568951308) | [#1301](https://github.com/clayton-duarte/cpd/issues/1301) — Reseal walks completed redirects backwards in recorded completion order, checking one historical state per redirect, rather than materializing combinations. Existing intermediate-seal/refusal tests and `reseal_handles_many_replaced_dependencies_without_a_cartesian_search` cover 24 independent redirected dependencies. |
| 4566335920 | [#1177: Resolve busy-session Escape before cancelling the dialog](https://github.com/clayton-duarte/cpd/pull/1177#discussion_r4566335920) | [#1300](https://github.com/clayton-duarte/cpd/issues/1300) — Busy AskCard Escape bubbles to the parent’s abort handler without cancelling the dialog, including unsupported aborts (the backend returns its capability refusal). The new AskCard regression proves bubbling and no dialog response; idle cancellation remains covered by the retained tests. |
| 4565490145 | [#1151: Isolate repository policy errors from the whole GitHub poll](https://github.com/clayton-duarte/cpd/pull/1151#discussion_r4565490145) | [#1299](https://github.com/clayton-duarte/cpd/issues/1299) — Invalid repo/owner policies suppress only local policy excuses and appear in per-repo `policy_errors`; valid raw GitHub reads still commit. `an_invalid_repository_policy_does_not_fail_the_shared_github_endpoint` covers two repos and confirms no endpoint failure/pause. |
| 4561203844 | [#1130: Security: Authenticate the newly activated preview server](https://github.com/clayton-duarte/cpd/pull/1130#discussion_r4561203844) | [#1282](https://github.com/clayton-duarte/cpd/issues/1282) — Loopback defaults and a separate per-backend HTTP Basic credential protect production, preview, CPD sockets and HMR. `access.test.ts`, `serve-browser.test.ts`, `ws.test.ts` and `plugin.test.ts` exercise missing/wrong/valid credentials and refuse LAN startup before cockpit work. |
| 4556999231 | [#1113: Include merged workflows when gating deployment approval](https://github.com/clayton-duarte/cpd/pull/1113#discussion_r4556999231) | [#1284](https://github.com/clayton-duarte/cpd/issues/1284) — `workflow::prereq::deployment_owners` joins actual run PRs to all owning Workflows, including merged owners. CLI regressions `a_merged_workflow_still_gates_its_deployment` and `an_unrelated_open_workflow_does_not_own_a_deployment` passed. |
| 4556999238 | [#1113: Filter deployment ownership instead of merely ranking matches](https://github.com/clayton-duarte/cpd/pull/1113#discussion_r4556999238) | [#1284](https://github.com/clayton-duarte/cpd/issues/1284) — `workflow::prereq::deployment_owners` joins actual run PRs to all owning Workflows, including merged owners. CLI regressions `a_merged_workflow_still_gates_its_deployment` and `an_unrelated_open_workflow_does_not_own_a_deployment` passed. |
| 4556999244 | [#1113: Delete prerequisite rows before deleting their workflows](https://github.com/clayton-duarte/cpd/pull/1113#discussion_r4556999244) | [#1295](https://github.com/clayton-duarte/cpd/issues/1295) — `plan/delete.rs` deletes prerequisite and re-review rows before parent Workflows. CLI `deleting_a_plan_removes_its_prerequisites_before_workflows` passes through the actual delete command. |
| 4556999250 | [#1113: Keep the LaunchDarkly token out of process arguments](https://github.com/clayton-duarte/cpd/pull/1113#discussion_r4556999250) | [#1296](https://github.com/clayton-duarte/cpd/issues/1296) — LaunchDarkly authorization headers use curl stdin (`-H @-`), with CR/LF rejection. `private_headers_use_stdin_and_reject_injected_lines` checks credential absence from argv and injection refusal. |
| 4557145977 | [#1113: Move live prerequisite checks off the cockpit request loop](https://github.com/clayton-duarte/cpd/pull/1113#discussion_r4557145977) | [#1297](https://github.com/clayton-duarte/cpd/issues/1297) — `plan show` reads local prerequisite state, reporting external checks as `not_checked`; live evaluation remains at execution gates. `snapshot_marks_external_checks_unknown_without_running_processes` verifies the read-only boundary. This is an explicit unknown result, not a fabricated cached success. |
| 4557145991 | [#1113: Enforce prerequisites when an armed merge actually executes](https://github.com/clayton-duarte/cpd/pull/1113#discussion_r4557145991) | [#1285](https://github.com/clayton-duarte/cpd/issues/1285) — The outbox rechecks every owning Workflow before merge/auto-merge and deployment writes. `an_added_prerequisite_blocks_an_already_queued_merge` proves a prerequisite added after enqueue blocks GitHub execution; prerequisite waits remain retryable. |
| 4557172760 | [#1113: Security: Reject agent callers before confirming prerequisites](https://github.com/clayton-duarte/cpd/pull/1113#discussion_r4557172760) | [#1286](https://github.com/clayton-duarte/cpd/issues/1286) — `workflow/prereq.rs` refuses nonempty agent Session/Attempt identity before Env/store access. `agent_sessions_and_workers_cannot_confirm_manual_prerequisites` covers both identities. |
| 4556845029 | [#1112: Sanitize approval titles before using them as state labels](https://github.com/clayton-duarte/cpd/pull/1112#discussion_r4556845029) | [#1294](https://github.com/clayton-duarte/cpd/issues/1294) — `pi/lib/state.ts` collapses control/whitespace and caps state labels at 80 Unicode characters while preserving identity/title. The state regression covers multiline/control input and emoji boundaries. |
| 4556513503 | [#1109: Update the Electron E2E expectation for the new workers text](https://github.com/clayton-duarte/cpd/pull/1109#discussion_r4556513503) | [#1293](https://github.com/clayton-duarte/cpd/issues/1293) — `StatusBar.tsx` uses readable queued/waiting/running labels. The component regression and Electron E2E expectation agree on that text. StatusBar component tests passed; Electron E2E was not run locally. |
| 4556439703 | [#1108: Reject rebuilds that would discard unknown additive columns](https://github.com/clayton-duarte/cpd/pull/1108#discussion_r4556439703) | [#1283](https://github.com/clayton-duarte/cpd/issues/1283) — `store/repair.rs` refuses unknown columns before rebuilding jobs or incidents; `rebuild_refuses_unknown_columns_before_touching_data` verifies both refusal and preserved future-column values. |
| 4556119095 | [#1105: Exercise the engine's SWR pass in the lifecycle scenario](https://github.com/clayton-duarte/cpd/pull/1105#discussion_r4556119095) | [#1292](https://github.com/clayton-duarte/cpd/issues/1292) — The lifecycle scenario now additionally calls `GithubSource::pass_with_effects`, the E5 production entry. Heavy feedback uses the existing complete-connection parser and incomplete feedback is reported rather than erased. `engine_swr_lifecycle_carries_feedback_head_changes_resolution_and_merge` passes alongside all nine retained CLI transitions. |
| 4556089054 | [#1104: Avoid running the environment-mutating test twice in parallel](https://github.com/clayton-duarte/cpd/pull/1104#discussion_r4556089054) | [#1291](https://github.com/clayton-duarte/cpd/issues/1291) — `engine/tests/watch/main.rs` registers the original HOME-mutating scenario only through its test-marked module, removing the duplicate wrapper. The engine watch binary runs that scenario once; the new staging test uses its own home and does not mutate HOME. |
| 4556128207 | [#1104: Security: Require authentication before supervising the LAN WebUI](https://github.com/clayton-duarte/cpd/pull/1104#discussion_r4556128207) | [#1282](https://github.com/clayton-duarte/cpd/issues/1282) — Loopback defaults and a separate per-backend HTTP Basic credential protect production, preview, CPD sockets and HMR. `access.test.ts`, `serve-browser.test.ts`, `ws.test.ts` and `plugin.test.ts` exercise missing/wrong/valid credentials and refuse LAN startup before cockpit work. |
| 4556031549 | [#1103: Honor explicit owner escalation before the mitigation deadline](https://github.com/clayton-duarte/cpd/pull/1103#discussion_r4556031549) | [#1290](https://github.com/clayton-duarte/cpd/issues/1290) — `protocol bug escalate <id> --reason` persists a bug-specific owner request; the Inbox bypasses only that bug’s deadline. `an_explicit_bug_escalation_opens_only_that_bug_inside_the_window` proves immediate Yellow visibility and closure on mitigation; CLI tests cover caller ownership and persistence. |

### LAN access decision (owner clarification)

Production and preview bind `127.0.0.1` by default. A non-loopback
`CPD_WEBUI_HOST` is an explicit opt-in and requires an independent
`CPD_WEBUI_ACCESS_TOKEN` of at least 32 characters on that backend. HTTP Basic
authentication uses username `cpd`; the browser's authenticated same-origin
WebSocket upgrade carries the same credential. Both static requests and socket
upgrades are checked, including preview HMR. This client credential is separate
from the durable-host process credential. Credentials never enter URLs or logs.

Generate a different random token on each machine (for example `openssl rand
-hex 32`), supply it to that machine's WebUI service environment, and configure
its LAN bind address explicitly. Browser authentication is per backend. Use
HTTPS through a trusted reverse proxy for LAN access: HTTP Basic credentials are
encoded, not encrypted. The reverse proxy must forward the Authorization header
and support WebSocket upgrades; keep the backend inaccessible from untrusted
networks. This change does not connect to or deploy onto any of the three hosts.

The parser dependencies are Tree-sitter and the upstream Rust and TypeScript
grammars (MIT). They replace the hand-built lexical scanner while retaining the
existing static preservation interface. Dynamic test generation remains outside
that interface; the runtime test gate remains authoritative for execution.

## Batch boundaries and review

The 30 root-cause issues cover all 44 comments; their normal attribution markers
remain available to future accounting. These are direct writes for the owner’s
explicit bounded maintenance request, not a new discovery loop or a reset of
the paused allowance-gated run. The global QA configuration remains unchanged.
The single PR links every issue with a closing keyword; issues remain open
until the User merges it. No original review thread is marked resolved merely
because a replacement PR exists.

Staging is owner-only runtime state, distinct from active readiness. Both engine
generations leave the serving WebUI alone while staging is alive, including
when a planned-restart marker exists. An expired/failed standby does not remove
the old listener’s adoption record. Promotion can have a brief listener gap; this
is not a zero-downtime deployment claim. Each host retains its own configuration
and state.

Browser credentials alone are insufficient against cross-site WebSocket requests:
the access boundary permits same-origin browser clients or an explicitly
configured exact `CPD_WEBUI_ALLOWED_ORIGINS` entry. An explicitly authenticated
cross-origin client must also list its trusted UI origins per backend. Private-address matching alone does not
establish browser trust. This follows OWASP’s explicit Origin allowlist guidance;
non-browser clients still require the LAN credential.

The thread-completeness column follows the existing migration ownership
rule: the frozen, hand-written PR delete-history trigger is not rewritten. Its
additive-column exception and the SQL contract are updated; generated insert
and update history still includes the new column. Readiness test fixtures now
state that their empty thread list is complete, rather than relying on feedback
being empty. The default remains unknown for a real legacy row.

The forward migration is **breaking after 144**, although its SQL only adds
columns. An older writer can update `open_threads` without invalidating the
new completeness flag, so allowing it to keep writing would violate the trust
invariant. The compatibility declaration follows the repository’s §4.5 writer
safety contract; schema shape alone does not establish safe compatibility.
Legacy thread arrays remain in place but start untrusted. The reseal completion
lookup compares the canonical history primary key against `json_array(plan,
source)`, using `history_tbl_pk` instead of scanning every history row for each
replacement. Candidate states remain bounded by the number of completed
redirects.

### Browser authentication experiment

A fresh headless Chromium profile, two isolated loopback HTTP origins, synthetic
credentials, and the production `webAccess` policy verified same-origin HTTP
Basic plus WebSocket authentication. After authenticating B, a socket to B from
UI A was refused because Chromium did not send the cached credential. The
initial cross-backend cache instructions were removed. The existing browser
entry point is same-origin; this PR does not implement the requested future
three-backend selector or claim that an Origin allowlist authenticates one.
See [WebUI access](../architecture/webui-access.md) for the supported setup and
the required authentication boundary for that future selector.

## Validation results

- Rust workspace: 7,218 unique tests verified passing; four configured skips.
  The final full run executed 7,218 tests: 7,214 passed and four failed. Two
  historical bug-issue fixtures needed true chronological migration snapshots;
  two engine stand-down tests overlapped the temporary browser experiment's
  ports. After the fixture correction and releasing those ports, all six tests
  in those two modules passed, including every failed test. No production code
  changed after that full run. Earlier focused runs also covered the parser,
  prerequisite/deployment gates, reconciliation, SWR lifecycle and handoff.
- Web: all 231 Vitest files passed, 1,710 tests; production build passed. Root
  and web TypeScript checks passed. The build retains its chunk-size advisory.
- Node: 22 focused state/abort tests and nine CLI grammar/skill tests passed.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `git diff --check`, the frozen-migration check against `origin/main`, and the
  portability check (22 files, no exemptions) passed. Cargo still reports the
  existing unused workspace dependency advisories for `notify` and
  `tui-tree-widget`.
- Offline pi adapter load/activation check passed against pi 1.5.2. Its first
  attempt was blocked by the sandbox denying a local Unix socket; the isolated
  check passed with local socket permission.
- The generated CLI interface and usage grammar were regenerated through their
  two binary-backed tests. Removed duplicate banner tests are represented in
  the central EngineBanner and App connection-precedence regressions.
- Chromium verified the same-origin/cross-origin authentication distinction
  described above. Electron E2E was not run locally; its worker-text expectation
  was updated, and the corresponding component regression passed.

Local execution used Node 24.22.1, Rust 1.100 and pnpm 12.6.0. The container
needed an isolated tmux binary, a CLI on the test PATH, local socket permission,
and a subreaper wrapper for process-cleanup tests because its PID 1 does not
reap orphaned children. These are local tooling accommodations, not committed
runtime changes. Hosted CI has not been polled or claimed green.
