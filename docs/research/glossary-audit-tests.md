# Glossary audit: tests

Part: `cli/tests/**`, every `*_tests.rs` under `core/src` and `engine/src`, `pi/test/**`, `pi/skills/skill.test.ts`, `scripts/tests.sh` and the golden or bless files. Audit only: no file other than this one changed. Sense codes as in the Spec: `object` (the cpd object, a Workflow after the rename), `pull request`, `phase`, `step`, `mixed`, `other`. Measured on the base of Attempt aud-tests-a1 (worktree `cpd.glassary--aud-tests-a1`); other Plans may merge changes: a count may move by up to 2%.

## Coverage

### Paths of the part, and what was read

| Path | Files | Files with a match | Occurrences | Read in full | Skimmed | Not read |
|---|---|---|---|---|---|---|
| `cli/tests/common` | 14 | 5 | 78 | none | the matching lines, in aggregate and by sample (see Method) | the 9 files with no match: searched, not opened |
| `cli/tests/engine` | 16 | 11 | 294 | none | the matching lines, in aggregate and by sample (see Method) | the 5 files with no match: searched, not opened |
| `cli/tests/flow` | 70 | 51 | 1534 | none | the matching lines, in aggregate and by sample (see Method) | the 19 files with no match: searched, not opened |
| `cli/tests/pr` | 34 | 34 | 2752 | none | the matching lines, in aggregate and by sample (see Method) | the 0 files with no match: searched, not opened |
| `cli/tests/session` | 18 | 3 | 8 | none | the matching lines, in aggregate and by sample (see Method) | the 15 files with no match: searched, not opened |
| `cli/tests/store` | 9 | 5 | 103 | none | the matching lines, in aggregate and by sample (see Method) | the 4 files with no match: searched, not opened |
| `cli/tests/tooling` | 26 | 6 | 68 | none | the matching lines, in aggregate and by sample (see Method) | the 20 files with no match: searched, not opened |
| `cli/tests/tui` | 38 | 20 | 458 | none | the matching lines, in aggregate and by sample (see Method) | the 18 files with no match: searched, not opened |
| `*_tests.rs` under core/src | 90 | 39 | 581 | none | the matching lines, in aggregate and by sample (see Method) | the 51 files with no match: searched, not opened |
| `*_tests.rs` under engine/src | 20 | 12 | 232 | none | the matching lines, in aggregate and by sample (see Method) | the 8 files with no match: searched, not opened |
| pi/test/** (incl. compact-tools, fixtures, golden) | 164 | 20 | 102 | none | the matching lines, in aggregate and by sample (see Method) | the 144 files with no match: searched, not opened |
| pi/skills/skill.test.ts | 1 | 1 | 116 | none | the matching lines, in aggregate and by sample (see Method) | the 0 files with no match: searched, not opened |
| scripts/tests.sh | 1 | 0 | 0 | none | the matching lines, in aggregate and by sample (see Method) | the 1 files with no match: searched, not opened |
| **Total** | **501** | **207** | **6326** | | | |

No file was read in full. What was opened: the lines around the matches of the families below (sampled, never every line of a file), lines 1 to 40 of `cli/tests/tooling/interface.rs`, lines 60 to 120 of `cli/tests/tooling/pill_sites.rs`, the head of `scripts/tests.sh`, `cli/tests/tooling/guards.rs`, `dead_code.rs`, `pi/test/no-fixed-sleeps.test.ts`, and the lines of `pi/skills/skill.test.ts` (86, 104 to 135, 395 to 433, 463 to 469, 490 to 548, 586 to 763) that the matches point to. Files with no match were not opened; they are unread, so a sense that a regex cannot see (a test about a Workflow written without any `pr` word) is not in this audit.

Out of the part, seen in passing and not audited: the inline `#[cfg(test)] mod tests` of core and engine files that are not named `*_tests.rs` (for example `core/src/store/schema/tests.rs`, `core/src/job/open.rs`, `core/src/pr/open.rs`); the golden files `pi/test/compact-tools/golden/*.json` are in the part and have 0 matches. Who covers the inline test modules is unverified.

Golden and bless files: `rg -l CPD_BLESS .` prints `docs/interface.md`, `docs/commands.md` (docs, another part) and `cli/tests/tooling/interface.rs` (in the part). `interface.rs` writes `docs/interface.md` from the `cpd --help` tree and names no command itself: 0 matches of the pattern.

### Method and commands

Every command runs from the repo root. `S` searches exactly the part. `PAT` is the acceptance pattern (`\bprs?\b`, `Pr[A-Z]`, `pr_`, `_pr\b`, `pull_request`, `PullRequest`, plus the upper-case `PR_`, `_PR`, and the prose `pull request`).

```sh
S() { rg "$@" cli/tests pi/test pi/skills/skill.test.ts scripts/tests.sh $(rg --files core/src engine/src -g '*_tests.rs'); }
PAT='(?i:\bprs?\b)|Pr[A-Z]|[Pp]r_|PR_|_pr\b|_PR\b|(?i:pull_?request)|(?i:pull request)'
S -P -l "$PAT" | wc -l                      # 207 files with a match
S -P -o -I "$PAT" | wc -l                   # 6326 occurrences
S -P -c "$PAT" | awk -F: '{s+=$NF}END{print s}'   # 4973 matching lines
```

Occurrences per directory (`<dir>` as in the table above; for core and engine pass `$(rg --files core/src -g '*_tests.rs')`): `rg -P -o -I "$PAT" <dir> | wc -l`. Files per directory: `find <dir> -type f | wc -l` and `rg -P -l "$PAT" <dir> | wc -l`.

The patterns of the Identifier families, one per line (line N is family N). Each Count below is `S -P -o -I -f <(sed -n Np re.txt) | wc -l` run in the repo, with this block saved as `re.txt`:

```
\["pr"
(?<!"gh", )"pr", "[a-z]
"--pr"|--pr\b
until[-_][Pp][Rr]\b
(?<!"gh", )"pr"(?!, "[a-z])(?<!\["pr")
"prs"
"(base_pr|from_pr|to_pr|needs_other_pr)"
\b(INTO|FROM|UPDATE|JOIN|TABLE|EXISTS|ON) prs\b
pr_needs
pr_posts
pull_requests
\b(pull_request|PULL_REQUEST)\b
pull_request_\w+|\w+_pull_request\b
pullRequest|PullRequest
\bpr_(number|url|opened_at)\b
\bpr_status\b
\bpr_poll_\w+
pr_open_escalate_minutes
\bpr_(in_use|exists|locked|ended|not_found|not_ready|not_open|not_closed|required|adopted|empty|start_failed|other_plan|title_required|needs_required)\b
\bpr-(started|opened|merged|added|edited|described|cancelled|changed|base-changed|ready|failing|poll-failing|comments|gate-ran|ticket|key|repo|base|changes|closed|feedback|stacked)\b
\bPrKey\b
\bPr(Ready|NotReady|NotFound|Opened|Failing|Row|State)\b
Pr[A-Z]\w*
\bPr\b
(?<![\w"'.\-/])pr(?![\w"'\-])
\.pr\b
\bpr_(add|start|edit|describe|cancel|view|list|data|row|fx|cmd|crud|local_needs|lead_branch|key|base|repo|set|state|item|ended|log|calls|values|rows|sql|empty|jira|merge|push|rebase|poll|post|review|reply|checks|comments|reaction_queue|stacked_ready|adopt\w*|needs\w*|open\w*|gate|body|post_flow|post_auto_merge|lead_worktree_path|in)\b
\bpr_\w+
\w+_pr\b
_prs?_
\bPRs?\b
(?i:pull request)
\bPR_\w*
PRRT_\w+|PRR_\w+
pullrequestreview-\d+
"gh", "pr"|gh pr [a-z-]+
CPD_TEST_PR_POLL|PR_POLL_EVERY_TICK
tui_widgets_pr_\w+
\b[a-z0-9]+-pr(-[a-z0-9]+)?\b|\bpr-[a-z0-9]+
(?<![\w"'.\-])prs(?![\w"'])|\.prs\b
\w*_PR_\w*|\bPRS\b|\w+_PR\b
\[pr(\.\w+)?\]|\bpr\.(gate|post|jira_required|feedback|aside|checks|lead|key)\b
\\npr_\w+
\bpr:[a-z]
\bpr (poll|merge|list|view|open|add|start|edit|push|rebase|describe|cancel|reply|post|checks|create|close)\b
'pr\{?\w*\}?'|\bpr\d+
pr cancelled
\\"prs?\\"
(core/src/pr/|pr_open_job)
/pr\{n\}
(?<![\w.\-/])prs?(?![\w\-])
```

Matches of `PAT` that no row covers: none. The check is a match-level one (a match is covered when a family regex matches a span that overlaps it), run with Python `re` over the 207 files and the 51 patterns above; it printed `6326 0`. Python `re` and PCRE agree on these patterns except that I did not run the check under PCRE: unverified. The residual of the earlier token-level pass was the words `pull` and `Pr` cut by the tokenizer (from `pull request` and `PrKey`), which the prose and `Pr[A-Z]` rows cover.

Counts in the Files column are matching lines per file (`S -P -c -f <(sed -n Np re.txt)`, top five), not occurrences. Families overlap (a line `cpd.pr(&["pr", "add"])` hits five); their counts must not be summed.

### Files per class (what the test does with the word)

A file takes the first class it matches, in this order. The commands (`PAT` and `S` as above; `comm` keeps only files with a match):

```sh
C1='"pr",|\["pr"|"--pr"|cpd pr |--pr\b|\bpr (add|start|edit|open|view|poll|merge|push|rebase|describe|list|cancel|reply|post)\b'
C2='"(pr|prs|pull_request|base_pr|until_pr|pr_\w+|to_pr|from_pr)"|\b(INTO|FROM|UPDATE|JOIN|TABLE|EXISTS) (prs|pull_requests|pr_posts|pr_needs)\b|\bpr_(status|number|url|opened_at)\b|\.pr\b|\bpr\.[a-z]'
C3='(assert\w*!?|contains|match\w*|predicate|starts_with|includes|equal)\(.*(\bPRs?\b|pull request|\bpr\b)|(refused|error)\(.*(PR|pull request)'
S -P -l "$PAT" | sort > /tmp/hit.txt
S -P -l "$C1" | sort | comm -12 - /tmp/hit.txt > /tmp/class1.txt
S -P -l "$C2" | sort | comm -12 - /tmp/hit.txt | comm -23 - /tmp/class1.txt > /tmp/class2.txt
S -P -l "$C3" | sort | comm -12 - /tmp/hit.txt | comm -23 - /tmp/class1.txt | comm -23 - /tmp/class2.txt > /tmp/class3.txt
cat /tmp/class1.txt /tmp/class2.txt /tmp/class3.txt | sort | comm -23 /tmp/hit.txt - > /tmp/class5.txt
```

| Class | What | Files |
|---|---|---|
| 1 | invokes the CLI (`["pr", "add"]`, `--pr`, `cpd pr ...`) | 97 |
| 2 | reads a JSON key or a table (`"pr"`, `"prs"`, `INTO prs`, `pr_status`, `.pr`) | 53 |
| 3 | asserts text (help, error, Event text, skill text, docs) only | 18 |
| 4 | pins a name in a generated file (docs/interface.md, a snapshot) | 0 |
| 5 | PR only in a fixture name or a comment | 39 |
| | total | 207 |

Class 4 is empty: no file of the part holds a name that a generated file pins. `cli/tests/tooling/interface.rs` generates `docs/interface.md` from the command tree and has no match. `pi/test/compact-tools/golden/*.json` have no match. The regex classes are approximate: class 1 counts a file that has one `pr add` among many JSON reads; 72 files match all three regexes (flags `111`).

**Class 1: invokes the CLI (97 files).** `cli/tests/common/engine.rs`, `cli/tests/common/fx.rs`, `cli/tests/common/mod.rs`, `cli/tests/engine/doctor.rs`, `cli/tests/engine/watch_auto_merge.rs`, `cli/tests/engine/watch_react.rs`, `cli/tests/engine/watch_sleep.rs`, `cli/tests/engine/watch_titles_width.rs`, `cli/tests/engine/watch.rs`, `cli/tests/flow/driver_fix.rs`, `cli/tests/flow/driver_open.rs`, `cli/tests/flow/driver_verify.rs`, `cli/tests/flow/driver.rs`, `cli/tests/flow/feedback_surface.rs`, `cli/tests/flow/gate.rs`, `cli/tests/flow/job_chain.rs`, `cli/tests/flow/job_edit_locked.rs`, `cli/tests/flow/job_fix.rs`, `cli/tests/flow/job_open_description.rs`, `cli/tests/flow/job_open_jira.rs`, `cli/tests/flow/job_resources.rs`, `cli/tests/flow/job_verify.rs`, `cli/tests/flow/job.rs`, `cli/tests/flow/plan_cancel.rs`, `cli/tests/flow/plan_delete.rs`, `cli/tests/flow/plan_overlap.rs`, `cli/tests/flow/plan_render_cancelled.rs`, `cli/tests/flow/plan_show.rs`, `cli/tests/flow/plan.rs`, `cli/tests/flow/policy_fallback.rs`, `cli/tests/flow/policy_layout.rs`, `cli/tests/flow/policy_machine.rs`, `cli/tests/flow/policy_model_use.rs`, `cli/tests/flow/policy_model.rs`, `cli/tests/flow/policy_post.rs`, `cli/tests/flow/policy_resources.rs`, `cli/tests/flow/policy_scopes.rs`, `cli/tests/flow/policy.rs`, `cli/tests/flow/repo_cmds.rs`, `cli/tests/flow/repo_config.rs`, `cli/tests/flow/review_close.rs`, `cli/tests/flow/review_job.rs`, `cli/tests/pr/merge.rs`, `cli/tests/pr/pr_adopt_add.rs`, `cli/tests/pr/pr_adopt_life.rs`, `cli/tests/pr/pr_adopt_pub.rs`, `cli/tests/pr/pr_adopt_rebase.rs`, `cli/tests/pr/pr_checks.rs`, `cli/tests/pr/pr_comments.rs`, `cli/tests/pr/pr_crud.rs`, `cli/tests/pr/pr_describe.rs`, `cli/tests/pr/pr_jira.rs`, `cli/tests/pr/pr_lead_branch.rs`, `cli/tests/pr/pr_local_needs.rs`, `cli/tests/pr/pr_merge.rs`, `cli/tests/pr/pr_needs_required.rs`, `cli/tests/pr/pr_open_jira.rs`, `cli/tests/pr/pr_open.rs`, `cli/tests/pr/pr_post_auto_merge.rs`, `cli/tests/pr/pr_post_flow.rs`, `cli/tests/pr/pr_post.rs`, `cli/tests/pr/pr_push.rs`, `cli/tests/pr/pr_reaction_queue.rs`, `cli/tests/pr/pr_rebase_running.rs`, `cli/tests/pr/pr_reply.rs`, `cli/tests/pr/pr_review.rs`, `cli/tests/pr/pr_start.rs`, `cli/tests/pr/pr_view_feedback.rs`, `cli/tests/pr/pr_view.rs`, `cli/tests/pr/pr.rs`, `cli/tests/pr/repo_hooks.rs`, `cli/tests/store/events.rs`, `cli/tests/tooling/harness.rs`, `cli/tests/tooling/no_repo_lock.rs`, `cli/tests/tooling/testkit_hosts.rs`, `cli/tests/tooling/testkit.rs`, `cli/tests/tui/tui_merge.rs`, `cli/tests/tui/tui_plan_cancelled.rs`, `cli/tests/tui/tui_plan.rs`, `cli/tests/tui/tui_read.rs`, `cli/tests/tui/tui_widgets_pr_menu.rs`, `core/src/janitor_tests.rs`, `core/src/job/incident_tests.rs`, `core/src/loops_ports_tests.rs`, `core/src/policy/exposure_tests.rs`, `core/src/pr/deploys_tests.rs`, `core/src/pr/gh_runner_tests.rs`, `core/src/pr/incident_tests.rs`, `core/src/pr/poll_tests.rs`, `core/src/store/schema/reaction_refusal_tests.rs`, `engine/src/watch/open_description_tests.rs`, `engine/src/watch/open_jira_tests.rs`, `engine/src/watch/open_tests.rs`, `engine/src/watch/ports_tests.rs`, `engine/src/watch/runner_tests.rs`, `pi/skills/skill.test.ts`, `pi/test/skill-retry.test.ts`

**Class 2: reads a JSON key or a table (53 files).** `cli/tests/common/inbox.rs`, `cli/tests/common/session.rs`, `cli/tests/engine/watch_resources.rs`, `cli/tests/engine/watch_worker_archive.rs`, `cli/tests/flow/ask.rs`, `cli/tests/flow/attempt.rs`, `cli/tests/flow/config.rs`, `cli/tests/flow/context_distill.rs`, `cli/tests/flow/context_pending_waiting.rs`, `cli/tests/flow/context_pending.rs`, `cli/tests/flow/context.rs`, `cli/tests/flow/driver_desc_gate.rs`, `cli/tests/flow/plan_outline.rs`, `cli/tests/flow/plan_render_waiting.rs`, `cli/tests/flow/policy_jira.rs`, `cli/tests/flow/secret_op.rs`, `cli/tests/flow/secret.rs`, `cli/tests/flow/spec.rs`, `cli/tests/pr/git_guard.rs`, `cli/tests/pr/pr_stacked_ready.rs`, `cli/tests/session/session_titles.rs`, `cli/tests/store/store_local_needs.rs`, `cli/tests/store/store_pr_posts.rs`, `cli/tests/store/store.rs`, `cli/tests/tui/bare.rs`, `cli/tests/tui/inbox_rules.rs`, `cli/tests/tui/tui_cause.rs`, `cli/tests/tui/tui_idle.rs`, `cli/tests/tui/tui_tree_delete.rs`, `cli/tests/tui/tui_tree.rs`, `cli/tests/tui/tui_widgets.rs`, `cli/tests/tui/tui.rs`, `core/src/attempt/cancel_tests.rs`, `core/src/attempt/incident_tests.rs`, `core/src/attempt/submit_tests.rs`, `core/src/driver/ports_tests.rs`, `core/src/inbox/follow_tests.rs`, `core/src/inbox/pr_tests.rs`, `core/src/inbox/runner_tests.rs`, `core/src/incident_tests.rs`, `core/src/job/fix_retry_tests.rs`, `core/src/job/worked_on_tests.rs`, `core/src/merge/linear_tests.rs`, `core/src/plan/display_tier_tests.rs`, `core/src/store/schema/job_edit_tests.rs`, `core/src/store/schema/job_verify_tests.rs`, `core/src/store/schema/lead_branch_tests.rs`, `engine/src/watch/inbox_tests.rs`, `engine/src/watch/incident_tests.rs`, `engine/src/watch/provider_hold_e3_tests.rs`, `engine/src/watch/schedule_tests.rs`, `engine/src/watch/stacked_ready_tests.rs`, `pi/test/updates.test.ts`

**Class 3: asserts text (18 files).** `cli/tests/engine/watch_models.rs`, `cli/tests/engine/watch_team.rs`, `cli/tests/session/board.rs`, `cli/tests/session/session_send.rs`, `cli/tests/tooling/layout.rs`, `cli/tests/tui/inbox_claim.rs`, `cli/tests/tui/tui_widgets_column.rs`, `cli/tests/tui/tui_widgets_inbox.rs`, `core/src/fakes/tmux/conformance_tests.rs`, `core/src/inbox/blue_tests.rs`, `core/src/inbox/claim_tests.rs`, `core/src/install/migration_tests.rs`, `engine/src/watch/local_time_tests.rs`, `pi/test/protocol.test.ts`, `pi/test/reminder.test.ts`, `pi/test/retier.test.ts`, `pi/test/review-chat.test.ts`, `pi/test/testguard.test.ts`

**Class 5: fixture name or comment only (39 files).** `cli/tests/engine/watch_inbox_models.rs`, `cli/tests/flow/context_curation.rs`, `cli/tests/flow/context_inbox_cyan.rs`, `cli/tests/flow/driver_resources.rs`, `cli/tests/flow/resource.rs`, `cli/tests/pr/git_lead_branch.rs`, `cli/tests/pr/git.rs`, `cli/tests/pr/main.rs`, `cli/tests/store/main.rs`, `cli/tests/tooling/guards.rs`, `cli/tests/tui/main.rs`, `cli/tests/tui/tui_plan_waiting.rs`, `cli/tests/tui/tui_widgets_inbox_actions.rs`, `cli/tests/tui/tui_widgets_pr_click.rs`, `core/src/config/catalog_cache_tests.rs`, `core/src/context/distill_git_tests.rs`, `core/src/fakes/git/rebase_tests.rs`, `core/src/fakes/git/remote_tests.rs`, `core/src/policy/machine_tests.rs`, `core/src/pr/comments_tests.rs`, `core/src/prune/lead_branch_tests.rs`, `core/src/store/schema/blue_tests.rs`, `core/src/store/schema/claim_tests.rs`, `core/src/store/schema/models_notice_tests.rs`, `core/src/store/schema/tier_catalog_tests.rs`, `engine/src/watch/archive_tests.rs`, `pi/test/answers.ts`, `pi/test/askguard.test.ts`, `pi/test/delivery.test.ts`, `pi/test/first-message.test.ts`, `pi/test/no-fixed-sleeps.test.ts`, `pi/test/noblock.test.ts`, `pi/test/pi-check.test.ts`, `pi/test/policy.test.ts`, `pi/test/render.test.ts`, `pi/test/slack-channel.test.ts`, `pi/test/slack-parse.test.ts`, `pi/test/sleep-allow.txt`, `pi/test/tripwire-allow.test.ts`

## Identifier families

Sense is by what the family names, not by each use: a `mixed` family is one whose uses split or cannot be told from the text. The After column proposes; the User decides.

| # | Family | Surface | Sense | Count | Files (top 5, matching lines) | After the rename | Hazard |
|---|---|---|---|---|---|---|---|
| 1 | argv noun `["pr"` (`cpd pr <verb>`) | argv element, CLI noun | mixed | 392 (`S -P -o -I -f <(sed -n 1p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (45); cli/tests/pr/pr_start.rs (36); cli/tests/pr/pr.rs (34); cli/tests/pr/pr_merge.rs (26); cli/tests/flow/job.rs (21) | decision | Why mixed: `pr add\|start\|edit\|describe\|list\|view\|cancel` act on the object, `pr poll\|post\|reply\|merge\|checks` on the pull request, `pr open\|push\|rebase` on the object and its pull request. CLI tree; docs/interface.md (regenerated); skill text; the verbs split: add, start, edit, describe, list, view, cancel act on the object; poll, post, reply, merge, checks on the pull request; open, push, rebase on both |
| 2 | argv noun `"pr", "<verb>"` (not gh) | argv element, second spelling | mixed | 181 (`S -P -o -I -f <(sed -n 2p re.txt) \| wc -l`) | cli/tests/pr/pr_merge.rs (26); cli/tests/pr/pr.rs (19); cli/tests/store/events.rs (10); cli/tests/pr/pr_jira.rs (10); cli/tests/flow/plan.rs (10) | decision | Why mixed: same verbs as row 1. same as the row above |
| 3 | flag `--pr` | flag | object | 96 (`S -P -o -I -f <(sed -n 3p re.txt) \| wc -l`) | cli/tests/flow/job.rs (26); cli/tests/pr/pr.rs (8); cli/tests/pr/pr_reply.rs (8); cli/tests/flow/policy.rs (8); pi/skills/skill.test.ts (5) | `--workflow` | every `--pr <key>` caller; skill.test.ts:228,542,731; clap definitions (another part) |
| 4 | `until-pr` / `until_pr` / `until-PR` | flag, JSON key, test name | object | 34 (`S -P -o -I -f <(sed -n 4p re.txt) \| wc -l`) | core/src/incident_tests.rs (27); cli/tests/store/events.rs (2) | `--until-workflow` | incident tests (core/src/incident_tests.rs); the stored incident value |
| 5 | JSON key or value `"pr"` (not argv, not gh) | JSON key in details, data, Job rows | object | 136 (`S -P -o -I -f <(sed -n 5p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (31); cli/tests/flow/job.rs (18); cli/tests/pr/pr_crud.rs (13); cli/tests/flow/plan_overlap.rs (8); cli/tests/pr/pr_start.rs (7) | `"workflow"` | contract; skill text `data.pr`; the pi adapter reads `data.pr`; protocol number |
| 6 | JSON key `"prs"` | JSON key (list of Workflows) | object | 83 (`S -P -o -I -f <(sed -n 6p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (16); cli/tests/pr/pr_crud.rs (13); cli/tests/flow/plan.rs (13); cli/tests/flow/plan_show.rs (9); cli/tests/pr/pr_start.rs (6) | `"workflows"` | plan show/list JSON; plan-done event data |
| 7 | JSON keys `base_pr` `from_pr` `to_pr` `needs_other_pr` | JSON key, error detail | object | 30 (`S -P -o -I -f <(sed -n 7p re.txt) \| wc -l`) | cli/tests/flow/job.rs (7); cli/tests/pr/pr.rs (5); cli/tests/pr/pr_crud.rs (5); cli/tests/pr/pr_start.rs (4); cli/tests/store/events.rs (1) | `base_workflow` etc. | contract; event data of `pr-base-changed` |
| 8 | table `prs` (SQL) | table | object | 333 (`S -P -o -I -f <(sed -n 8p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (36); cli/tests/flow/job.rs (20); cli/tests/pr/pr.rs (19); cli/tests/pr/pr_start.rs (15); cli/tests/flow/plan.rs (11) | `workflows` | applied migrations keep the name; triggers; contract §4.4 DDL (store.rs:187 compares it) |
| 9 | table `pr_needs` (SQL) | table | object | 38 (`S -P -o -I -f <(sed -n 9p re.txt) \| wc -l`) | cli/tests/store/store.rs (9); cli/tests/pr/pr_local_needs.rs (6); cli/tests/pr/pr_crud.rs (6); cli/tests/store/store_local_needs.rs (5); cli/tests/pr/pr.rs (2) | `workflow_needs` | migration; triggers |
| 10 | table `pr_posts` (SQL) | table | mixed | 36 (`S -P -o -I -f <(sed -n 10p re.txt) \| wc -l`) | cli/tests/store/store_pr_posts.rs (7); cli/tests/pr/pr_post_auto_merge.rs (5); cli/tests/pr/pr_reaction_queue.rs (4); cli/tests/flow/plan_delete.rs (4); cli/tests/pr/pr_post.rs (3) | decision | Why mixed: keyed by the Workflow, holds the post of a pull request. row is keyed by Plan and Workflow key but holds the Slack post of a pull request |
| 11 | table `pull_requests` and `pull_requests_hist` (SQL) | table | pull request | 231 (`S -P -o -I -f <(sed -n 11p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (38); cli/tests/tui/tui_read.rs (12); cli/tests/pr/pr_crud.rs (10); cli/tests/pr/pr_merge.rs (9); cli/tests/flow/job.rs (9) | unchanged | none; the column `prs.pull_request` keeps its name too |
| 12 | `pull_request` / `PULL_REQUEST` (column, JSON key, fn, const) | column, JSON key, helper name | pull request | 177 (`S -P -o -I -f <(sed -n 12p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (25); cli/tests/tui/inbox_rules.rs (21); cli/tests/flow/job.rs (13); core/src/pr/comments_tests.rs (9); cli/tests/flow/plan_show.rs (9) | unchanged | `prs.pull_request` column (migration 5) |
| 13 | `pull_request_*` and `*_pull_request` names | test helper, test name | pull request | 107 (`S -P -o -I -f <(sed -n 13p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (57); cli/tests/pr/pr_crud.rs (6); cli/tests/pr/pr_adopt_add.rs (5); cli/tests/tui/tui_read.rs (4); cli/tests/tui/inbox_rules.rs (4) | unchanged | none |
| 14 | `pullRequest` / `PullRequest` (GitHub GraphQL, TS field) | GitHub API name, TS field | pull request | 58 (`S -P -o -I -f <(sed -n 14p re.txt) \| wc -l`) | cli/tests/pr/pr_reply.rs (8); cli/tests/engine/watch_auto_merge.rs (7); core/src/pr/comments_tests.rs (6); cli/tests/pr/pr.rs (6); cli/tests/pr/pr_view.rs (5) | unchanged | GitHub API; mutation `enablePullRequestAutoMerge` |
| 15 | columns `pr_number` `pr_url` `pr_opened_at` | column of `prs`, JSON key | pull request | 231 (`S -P -o -I -f <(sed -n 15p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (10); cli/tests/pr/pr_crud.rs (7); cli/tests/flow/job.rs (6); cli/tests/pr/pr_adopt_pub.rs (4); cli/tests/flow/driver_open.rs (4) | unchanged | they hold the pull request's data on a Workflow row; migration if the table is renamed (columns move with it) |
| 16 | `pr_status` (helper and error detail) | test helper fn, error detail key | object | 74 (`S -P -o -I -f <(sed -n 16p re.txt) \| wc -l`) | cli/tests/pr/pr_start.rs (24); cli/tests/pr/pr_crud.rs (24); cli/tests/pr/pr.rs (17); cli/tests/flow/plan_cancel.rs (2); cli/tests/flow/driver.rs (1) | `workflow_status` | error detail in the contract (cli/tests/flow/driver.rs:439) |
| 17 | watch config `pr_poll_*` | config key | pull request | 24 (`S -P -o -I -f <(sed -n 17p re.txt) \| wc -l`) | cli/tests/engine/watch.rs (7); cli/tests/flow/config.rs (5); pi/skills/skill.test.ts (2); cli/tests/common/mod.rs (2); core/src/pr/poll_tests.rs (1) | unchanged | machine config files; skill.test.ts:433 |
| 18 | config `pr_open_escalate_minutes` | config key | pull request | 4 (`S -P -o -I -f <(sed -n 18p re.txt) \| wc -l`) | cli/tests/flow/config.rs (2) | unchanged | machine config; cli/tests/flow/config.rs |
| 19 | error codes `pr_*` | error code | mixed | 94 (`S -P -o -I -f <(sed -n 19p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (36); cli/tests/pr/pr_start.rs (10); cli/tests/flow/job.rs (9); cli/tests/pr/pr.rs (7); cli/tests/pr/pr_post.rs (6) | decision | Why mixed: a code such as `pr_not_ready` names the pull request, `pr_in_use` names the object. each code names the object or the pull request; contract table; skill.test.ts:513 (pr_title_required) and the skills' backticked codes |
| 20 | Event kinds `pr-*` | Event kind, inbox kind, string | mixed | 235 (`S -P -o -I -f <(sed -n 20p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (49); cli/tests/pr/pr_start.rs (28); cli/tests/pr/pr_crud.rs (20); cli/tests/store/events.rs (9); cli/tests/pr/pr_comments.rs (9) | decision | Why mixed: `pr-started` and `pr-added` are the object, `pr-merged` and `pr-feedback` the pull request, `pr-opened` both. contract §5.2 (pi/test/event.test.ts, cli/tests/store/events.rs SPEC); pi adapter KINDS; protocol number (retier.test.ts:183); stored events |
| 21 | type `PrKey` | Rust id newtype | object | 21 (`S -P -o -I -f <(sed -n 21p re.txt) \| wc -l`) | core/src/pr/incident_tests.rs (7); cli/tests/pr/merge.rs (3); cli/tests/pr/git_lead_branch.rs (3); engine/src/watch/incident_tests.rs (2); core/src/loops_ports_tests.rs (2) | `WorkflowKey` | core/src/ids; every use |
| 22 | `Pr(Ready\|NotReady\|NotFound\|Opened\|Failing\|Row\|State)` | Rust type, variant | mixed | 41 (`S -P -o -I -f <(sed -n 22p re.txt) \| wc -l`) | cli/tests/tui/inbox_rules.rs (12); cli/tests/tui/tui_read.rs (8); cli/tests/pr/pr.rs (5); core/src/job/incident_tests.rs (3); cli/tests/tui/tui_widgets_pr_menu.rs (2) | decision | Why mixed: `PrRow` the object, `PrReady` and `PrState` the pull request. `PrRow` is the Workflow row; `PrState`, `PrReady` read the pull request (cpd_tui::read: unverified) |
| 23 | `Pr[A-Z]\w*` all (catch-all) | Rust type, enum variant, `ErrorCode::Pr*` | mixed | 63 (`S -P -o -I -f <(sed -n 23p re.txt) \| wc -l`) | cli/tests/tui/inbox_rules.rs (12); cli/tests/tui/tui_read.rs (8); core/src/pr/incident_tests.rs (7); cli/tests/pr/pr.rs (5); core/src/job/incident_tests.rs (3) | decision | Why mixed: the same split as row 22 over every `Pr*` name. `ErrorCode::PrNotFound` etc. are the Rust side of the `pr_*` error codes |
| 24 | `Pr` (Rust struct named `Pr`) | Rust type | object | 33 (`S -P -o -I -f <(sed -n 24p re.txt) \| wc -l`) | cli/tests/pr/merge.rs (17); cli/tests/flow/attempt.rs (7); core/src/janitor_tests.rs (6) | `Workflow` | test-local `struct Pr` (cli/tests/flow/attempt.rs:32, cli/tests/pr/merge.rs) |
| 25 | bare word `pr` (identifier, SQL column, prose in comments) | variable, parameter, field, SQL column, comment | object | 1062 (`S -P -o -I -f <(sed -n 25p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (75); cli/tests/pr/pr_crud.rs (49); core/src/janitor_tests.rs (33); cli/tests/pr/merge.rs (30); cli/tests/flow/plan_overlap.rs (29) | `workflow` | column `jobs.pr` (stored; migration); helper parameters `pr: &str` |
| 26 | field access `.pr` | field, method (`fx.pr(`, `job.pr`) | object | 291 (`S -P -o -I -f <(sed -n 26p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (127); cli/tests/pr/pr.rs (44); core/src/inbox/pr_tests.rs (43); core/src/janitor_tests.rs (10); cli/tests/tui/tui_read.rs (8) | `.workflow` | the CLI helper `fx.pr(&[...])` of cli/tests/pr/pr_crud.rs:75 and 4 more |
| 27 | `pr_*` named helpers and tests (object) | test fn, helper | mixed | 313 (`S -P -o -I -f <(sed -n 27p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (39); cli/tests/pr/main.rs (25); cli/tests/flow/job.rs (17); cli/tests/pr/pr_crud.rs (15); cli/tests/flow/driver_open.rs (13) | decision | Why mixed: a helper such as `pr_add` is the object, `pr_view` reads both. test names are listed by `cpd test run <name>`; Jobs name tests in their Spec; renaming a test breaks those lists |
| 28 | other `pr_*` names | test fn, identifier | mixed | 741 (`S -P -o -I -f <(sed -n 28p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (74); cli/tests/pr/pr.rs (69); cli/tests/pr/pr_start.rs (46); cli/tests/pr/main.rs (27); cli/tests/flow/job.rs (22) | decision | Why mixed: a test name pr_* names either, and only a read of each test tells. bulk of the test names; same hazard |
| 29 | `*_pr` suffix names | test fn, helper, variable | mixed | 272 (`S -P -o -I -f <(sed -n 29p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (67); cli/tests/flow/job.rs (27); cli/tests/pr/merge.rs (21); core/src/incident_tests.rs (14); cli/tests/flow/plan_cancel.rs (13) | decision | Why mixed: `add_pr` the object, `open_pr` the pull request it opens. `open_pr`, `add_pr`, `base_pr`, `shaped_pr`, `locked_pr` |
| 30 | `pr` word inside a long test name (`_pr_`, `_prs_`) | test name | mixed | 427 (`S -P -o -I -f <(sed -n 30p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (51); cli/tests/pr/pr.rs (26); cli/tests/pr/pr_start.rs (22); cli/tests/pr/merge.rs (15); cli/tests/flow/driver_open.rs (13) | decision | Why mixed: the word sits in a sentence that names either; not told by regex. test names |
| 31 | prose `PR` / `PRs` (comments, assert text, docs) | comment, assert text, doc prose | mixed | 1001 (`S -P -o -I -f <(sed -n 31p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (80); cli/tests/pr/pr_crud.rs (59); pi/skills/skill.test.ts (47); cli/tests/flow/job.rs (35); cli/tests/flow/plan.rs (34) | decision | Why mixed: the prose uses PR for both senses, the very problem of the User's decision. see User-facing strings |
| 32 | prose `pull request` | comment, assert text, fixture | pull request | 247 (`S -P -o -I -f <(sed -n 32p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (58); cli/tests/pr/pr_crud.rs (13); cli/tests/engine/watch.rs (11); cli/tests/flow/driver_open.rs (10); cli/tests/pr/pr_adopt_pub.rs (9) | unchanged | none |
| 33 | consts `PR_*` | const, fixture | mixed | 59 (`S -P -o -I -f <(sed -n 33p re.txt) \| wc -l`) | cli/tests/flow/driver_open.rs (11); engine/src/watch/stacked_ready_tests.rs (8); cli/tests/engine/watch.rs (6); cli/tests/flow/driver.rs (5); cli/tests/common/mod.rs (5) | decision | Why mixed: `PR_URL` the pull request, `PR_KEY` the object. `PR_URL` `PR_TITLE` are the pull request; `PR_KEY` `PR_ID_ANSWER` the object |
| 34 | GitHub node ids `PRRT_*` `PRR_*` | fixture id | other (GitHub review thread and review ids) | 22 (`S -P -o -I -f <(sed -n 34p re.txt) \| wc -l`) | cli/tests/pr/pr_reply.rs (12); cli/tests/pr/pr_view.rs (2); cli/tests/pr/pr_view_feedback.rs (2); cli/tests/flow/feedback_surface.rs (2); cli/tests/pr/pr_post.rs (1) | unchanged | GitHub API |
| 35 | `pullrequestreview-*` ids | fixture id | pull request | 3 (`S -P -o -I -f <(sed -n 35p re.txt) \| wc -l`) | cli/tests/pr/pr_reply.rs (2); cli/tests/pr/pr_review.rs (1) | unchanged | GitHub |
| 36 | GitHub CLI `gh pr <verb>` | GitHub CLI argv | pull request | 45 (`S -P -o -I -f <(sed -n 36p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (9); cli/tests/tui/tui_merge.rs (5); pi/test/noblock.test.ts (3); pi/skills/skill.test.ts (3); core/src/pr/gh_runner_tests.rs (3) | unchanged | GitHub CLI |
| 37 | env var and const `CPD_TEST_PR_POLL`, `PR_POLL_EVERY_TICK` | env var, const | pull request | 12 (`S -P -o -I -f <(sed -n 37p re.txt) \| wc -l`) | cli/tests/engine/watch.rs (5); cli/tests/common/mod.rs (5) | unchanged | env var read by the engine |
| 38 | test file/name `tui_widgets_pr_*` | test file, test name | mixed | 2 (`S -P -o -I -f <(sed -n 38p re.txt) \| wc -l`) | cli/tests/tui/main.rs (2) | decision | Why mixed: the widget shows the pull request row of a Plan (the object) by its number (the pull request). file names |
| 39 | hyphen words `*-pr`, `pr-*` (fixture keys, branches) | fixture key, branch, title | object | 280 (`S -P -o -I -f <(sed -n 39p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (51); cli/tests/pr/pr_start.rs (28); cli/tests/pr/pr_crud.rs (22); core/src/incident_tests.rs (10); pi/skills/skill.test.ts (9) | keep (a fixture key) | none |
| 40 | bare `prs` word and `.prs` field | variable, field, prose plural | object | 402 (`S -P -o -I -f <(sed -n 40p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (37); cli/tests/pr/pr.rs (20); cli/tests/flow/job.rs (20); cli/tests/pr/pr_start.rs (15); core/src/janitor_tests.rs (14) | `workflows` | none |
| 41 | consts `*_PR_*`, `PRS`, `*_PR` | const, fixture | mixed | 17 (`S -P -o -I -f <(sed -n 41p re.txt) \| wc -l`) | core/src/pr/comments_tests.rs (10); cli/tests/pr/pr.rs (2); cli/tests/engine/watch.rs (2); cli/tests/common/mod.rs (2); cli/tests/tui/tui_widgets_inbox.rs (1) | decision | Why mixed: `PRS` is pull request numbers, `ITEM_PR_READY` an inbox kind of the pull request, `*_PR` the object. `PRS` in core/src/pr/comments_tests.rs:8 is a list of pull request numbers; `ITEM_PR_READY` an inbox kind |
| 42 | policy domain `[pr]`, `[pr.post]`, `pr.gate`, `pr.jira_required`, `pr.post.*` | policy TOML table, `cpd policy show` key | mixed | 145 (`S -P -o -I -f <(sed -n 42p re.txt) \| wc -l`) | cli/tests/flow/policy_post.rs (22); core/src/janitor_tests.rs (13); cli/tests/flow/policy_jira.rs (9); cli/tests/flow/policy.rs (8); core/src/policy/exposure_tests.rs (6) | decision | Why mixed: one table holds `pr.gate` (the Gate before the pull request opens), `pr.jira_required` (the Workflow) and `pr.post` (the pull request). the user's policy files (`[pr]` in owner.toml) and contract §8.3; skill.test.ts:587 (`pr.gate`); exposure_tests.rs:25 |
| 43 | `pr_*` after a string newline (`\npr_poll_seconds`) | config key inside a TOML text string | pull request | 16 (`S -P -o -I -f <(sed -n 43p re.txt) \| wc -l`) | cli/tests/flow/config.rs (6); engine/src/watch/ports_tests.rs (2); cli/tests/pr/pr_start.rs (1); cli/tests/engine/watch.rs (1); cli/tests/engine/watch_team.rs (1) | unchanged | machine config key `pr_poll_seconds` |
| 44 | inbox subject `pr:<owner>/<repo>#<n>` | inbox item subject string | pull request | 48 (`S -P -o -I -f <(sed -n 44p re.txt) \| wc -l`) | cli/tests/tui/inbox_rules.rs (8); core/src/store/schema/blue_tests.rs (6); core/src/inbox/claim_tests.rs (5); cli/tests/tui/tui_widgets_inbox_actions.rs (5); core/src/inbox/blue_tests.rs (4) | unchanged | stored inbox subjects (migration 0013-pr-inbox) |
| 45 | `pr <verb>` in prose, logs, fake `gh` scripts | log text, comment, fake `gh` script | mixed | 346 (`S -P -o -I -f <(sed -n 45p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (55); pi/skills/skill.test.ts (20); cli/tests/tui/tui_merge.rs (20); cli/tests/pr/pr_crud.rs (17); cli/tests/pr/pr_start.rs (14) | decision | Why mixed: `pr poll` the pull request, `pr add` the object. log lines the User reads and tests pin; `gh pr <verb>` is GitHub's and stays |
| 46 | fixture value `'pr'`, `'pr{n}'`, `pr1` | fixture key (string value) | object | 16 (`S -P -o -I -f <(sed -n 46p re.txt) \| wc -l`) | engine/src/watch/provider_hold_e3_tests.rs (1); core/src/store/schema/reaction_refusal_tests.rs (1); cli/tests/store/store_pr_posts.rs (1); cli/tests/store/store_local_needs.rs (1); cli/tests/pr/pr_reaction_queue.rs (1) | keep (a fixture key) | none |
| 47 | stored value `pr cancelled` (`jobs.cancel_reason`) | stored column value, asserted as `cancelled/pr cancelled/-` | object | 12 (`S -P -o -I -f <(sed -n 47p re.txt) \| wc -l`) | cli/tests/pr/pr_crud.rs (9) | `workflow cancelled` | stored in existing databases: a migration must rewrite it; where cancel_reason is set is another part |
| 48 | escaped-JSON keys `\"pr\"`, `\"prs\"` in a format string | key inside an escaped JSON string hashed with sha256 | object | 10 (`S -P -o -I -f <(sed -n 48p re.txt) \| wc -l`) | cli/tests/flow/plan.rs (7); cli/tests/flow/plan_outline.rs (3) | `workflow` / `workflows` | the Plan fingerprint is the sha256 of this JSON (plan.rs:819, plan_outline.rs:305): renaming the key changes every stored fingerprint, and an approved Plan review stops matching |
| 49 | paths `core/src/pr/`, `cli/tests/pr_open_job.rs`, `pr_open_job::` | path in a fixture's test_files | mixed | 13 (`S -P -o -I -f <(sed -n 49p re.txt) \| wc -l`) | cli/tests/flow/job.rs (5); core/src/pr/incident_tests.rs (1) | decision | Why mixed: `core/src/pr` holds both the Workflow code and the pull request code. fixture strings; `core/src/pr` holds pull-request code and Workflow code |
| 50 | branch suffix `/pr{n}` | fixture Lead Branch | object | 1 (`S -P -o -I -f <(sed -n 50p re.txt) \| wc -l`) | cli/tests/flow/context_distill.rs (1) | keep | none |
| 51 | catch-all: standalone `pr` / `prs` at a string, SQL or comment edge | SQL column list, `jobs.pr = 'b'` set clause, assert message, comment | object | 2217 (`S -P -o -I -f <(sed -n 51p re.txt) \| wc -l`) | cli/tests/pr/pr.rs (181); cli/tests/pr/pr_crud.rs (155); cli/tests/pr/pr_start.rs (84); cli/tests/flow/job.rs (78); cli/tests/flow/plan.rs (57) | `workflow` / `workflows` | column `jobs.pr`, `pr_posts.pr`, `slack_reactions.pr`: every SQL string that names it |

### Helpers that spell a command (rename once, fix many tests)

Commands: `rg -n 'fn .*pr' cli/tests` for the helper definitions; the caller counts are `S -P -o -I '<call>' | wc -l`.

| Helper | Where | Spells | Calls (occurrences, files) |
|---|---|---|---|
| `fn pr(&self, args)` (calls `pr_in`) | cli/tests/pr/pr_crud.rs:75 | `cpd pr <args> --plan pay-1` (a `pr add` also cancels the stub drafts) | `\.pr\(`: 269 in 16 files (all helpers of this name) |
| `fn pr(&self, args)` (calls `pr_in`) | cli/tests/pr/pr_adopt_add.rs:78 | `cpd pr <args> --plan <plan>` via `pr_in` at line 71 | shared count above |
| `fn pr(&self, command)` | cli/tests/pr/pr.rs:1323 | `cpd pr <command> --plan pay` | shared count above |
| `fn pr(&self, command)` | cli/tests/pr/pr_adopt_pub.rs:163 | `cpd pr <command> --plan pay` | shared count above |
| `fn add_pr` | cli/tests/common/engine.rs:290 | SQL `INSERT INTO prs` of Workflow `key` in `dispatch` (not the CLI) | `\badd_pr\(`: 22 in 5 files (4 other definitions: flow/job.rs:894, flow/attempt.rs:222, pr/merge.rs:298, engine/src/watch/open_tests.rs `add_web_pr`) |
| `fn shaped_pr` | cli/tests/flow/job.rs:1466 | SQL for a Workflow with a `shape` | `\bshaped_pr\(`: 12 in 3 files |
| `fn locked_pr` / `locked_shaped_pr` | cli/tests/pr/pr_crud.rs:1030 | SQL for a locked Workflow | `\blocked_pr\(`: 43 in 1 file; `locked_shaped_pr`: 4 in 1 file |
| `fn open_pr` | cli/tests/pr/pr_crud.rs:886 | opens a Workflow's pull request through `pr open` (mixed: also in flow/job_chain.rs:11, flow/driver.rs:844, engine/watch.rs:1977, tooling/testkit.rs:63) | `\bopen_pr\(`: 22 in 6 files |
| `fn add_job_in(pr, key, needs)` | cli/tests/flow/driver.rs:113 | SQL or `job add --pr`; parameter named `pr` | `\badd_job_in\(`: 17 in 6 files (also common/engine.rs:204, engine/watch.rs:203) |
| `fn pr_status` | cli/tests/pr/pr.rs:194 | reads `prs.status` | `\bpr_status\(`: 73 in 4 files (also pr_crud.rs:1055, pr_start.rs:204, flow/plan_cancel.rs:163) |
| `fn pull_request` / `pull_request_row` | cli/tests/flow/job.rs:34 | SQL for a row of `pull_requests` (pull request: unchanged) | `\bpull_request\(`: 48 in 10 files |
| `fn pr(env, ...)` (SQL seeders) | cli/tests/flow/context_pending.rs:32 | SQL for a Workflow; same name in flow/plan_outline.rs:48, flow/plan_overlap.rs:36, flow/plan_show.rs:40, flow/plan_render_cancelled.rs:41, engine/watch_react.rs:190, engine/watch_auto_merge.rs:201 | `\bpr\(&fx` 54 in 5 files; `\bpr\(env` 2 in 2 files; `\bpr\(fx` 2 in 2 files |

Renaming the helpers fixes the 269 `.pr(` calls, but not the 392 argv literals `["pr", ...]` spread over 60 files, the 96 `--pr` flags over 32 files, or the 113 files with `prs` in SQL: those tests spell the command or table themselves. A shared `fn workflow(...)` helper would not remove the literals; the literals are the larger part.

### Tests that tie docs to code

| Test | Ties | What breaks on the rename |
|---|---|---|
| pi/skills/skill.test.ts | reads `docs/commands.md` (line 6), `docs/contract.md` (line 279), `pi/lib/updates.ts` (280), `core/src/spec/worker.md` (746), `pi/lib/testguard.ts` (748) and every `SKILL.md` and reference of cpd-lead and cpd-manager | Every `cpd` command and `--flag` the skills name must exist in commands.md (lines 94 to 103); the backticked snake_case words must be error codes, causes, reasons, config keys or fields of the contract (395 to 400); the id-format row `PR key (R34)` (413 to 418); the Plan phase chain (425 to 426); `pr_poll_seconds` (433); 85 matching lines (116 occurrences) of `PR`/`pr` in assertions on skill text. Skills, commands.md and contract.md must be renamed in one change. Line 110 forbids the word `workflow` |
| cli/tests/tooling/interface.rs | `docs/interface.md` is generated from the `cpd --help` tree; `CPD_BLESS=1` rewrites it (line 67) | Nothing in the file; after the rename the generated file must be regenerated with `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface`. The file names no command |
| cli/tests/store/store.rs | `contract()` (line 179) and `contract_ddl_blocks()` (line 191): the ```sql blocks of contract §4.4 are compared with the schema | The DDL for `prs`, `pr_needs`, `pr_posts`, `jobs.pr` in contract.md and in the migrations must change together |
| cli/tests/store/events.rs | `SPEC` (line 213 on): §5.2 "written out independently of `Kind::spec`", 18 matches | Every `pr-*` Event kind and every `pr` data field in the list |
| pi/test/event.test.ts | `contractKinds()` reads contract §5.2 and compares to `KINDS` of `pi/lib/event.ts` | the `pr-*` kinds |
| core/src/policy/exposure_tests.rs | line 107 on: the domains table of contract §8.3 (Exposure column) against the code; `SETS` (line 23) lists the domain `pr` | the policy domain `pr` |
| core/src/policy/machine_tests.rs | line 497 on: the `mcp` row of the Machine domains table | none of the `pr` words (0 matches); it is in the part only through the docs path |
| cli/tests/tooling/layout.rs | line 111: names `docs/implementing.md` | none: no match |

No test of the part reads `CONTEXT.md` or any glossary: `S -l 'CONTEXT\.md' | wc -l` prints 0. No test parses the Context records (`pro-d136`): the glossary has no test today.

### Tests that already guard vocabulary

One test guards a word list today: `pi/skills/skill.test.ts:106` to `112`, `nothing of cpd 1, of the removed Reviewer or of the retired grounded Lead is left`. For each skill file it runs `assert.doesNotMatch(text, /re/, "<name>: <reason>")` for `grounded|forked_from`, `--wf|CPD_WF`, `\bworkflows?\b` ("the term Workflow is retired"), and `herdr|\bbd\b|...`. Style to follow: a regex per retired word, the file name and the reason in the message, text only, no process. **Line 110 conflicts with the rename and must be rewritten with it.**

Other scans of source text, whose style a later guard can copy: `cli/tests/tooling/pill_sites.rs:65` (`offenders(forbidden, allowed)` returns `path:line: text`; an allow-list with the reason beside each path), `cli/tests/tooling/guards.rs` (scans `cli/tests/<area>/*.rs`; asserts it found more than 50 files, so a wrong directory cannot pass), `cli/tests/tooling/dead_code.rs`, `pi/test/no-fixed-sleeps.test.ts` (a ratchet with a per-file allow-list `pi/test/sleep-allow.txt`), `pi/test/no-black-text.test.ts`. None of them guards the word PR. The User's rules (pro-d6, pro-d7: test only our code, mock the rest) hold for all of them: they read files and run nothing external.

### The new name is already in use

`rg -P -i '\bworkflows?\b'` over the part (`S -P -o -I '(?i)\bworkflows?\b' | wc -l`) prints 42: 17 are "workflow worktree" (an older name of the Lead worktree: core/src/driver/ports_tests.rs, core/src/merge/linear_tests.rs, engine/src/watch/schedule_tests.rs, core/src/attempt/submit_tests.rs), 23 are GitHub Actions workflows (core/src/pr/deploys_tests.rs, core/src/policy/deploys_tests.rs, core/src/inbox/deploy_tests.rs: `[deploys] workflows`, `workflow_runs`, `actions/workflows`), 1 is the retiring assertion (skill.test.ts:110), 1 is a cpd 1 fixture comment (cli/tests/tui/tui_plan.rs:43). The split is by sample, not by reading all 42 lines: unverified for the rest. `--wf` and `CPD_WF` (cpd 1) have 3 matches.

## User-facing strings

Every row is an asserted string that a person or agent reads. Where is `path:line`; the Text is the quoted words (at most 15); Fix is the proposal for the Sense given.

| Where | Text | Shown to | Sense | Fix |
|---|---|---|---|---|
| engine/src/watch/local_time_tests.rs:51 | `PR polling pauses until 2026-10-20 07:00 EDT.` | User | pull request | unchanged (the poll of GitHub) |
| cli/tests/pr/pr.rs:1952 | `PR repo (#812) of Plan pay needs attention: checks failed.` | Lead | mixed | decision: "PR repo" is the Workflow key, "#812" its pull request |
| cli/tests/pr/pr.rs:1952 | `Read the details with cpd pr view --plan pay --pr repo.` | Lead | object | `cpd workflow view --plan pay --workflow repo` |
| cli/tests/pr/pr.rs:3318 | `cpd pr rebase --pr repo --plan pay --abort` | Lead | mixed | decision: `pr rebase` acts on a Workflow and its pull request branch |
| cli/tests/pr/pr.rs:2805 | `PR shop (#20) of Plan bob` | Lead | mixed | decision: "PR shop" is the Workflow, "#20" the pull request |
| cli/tests/flow/plan_show.rs:184 | ## PR pay\n\nRepo: `/r/pay` · Base: `main` · Needs: none | User | object | `## Workflow pay` |
| cli/tests/flow/plan_show.rs:293 | ## PR web\n\nRepo: `/r/web` · Base: `main` · Needs: `api` | User | object | `## Workflow web` |
| cli/tests/flow/plan_show.rs:123 | `No PRs yet.` | User | object | "No Workflows yet." |
| cli/tests/flow/plan_show.rs:307 | `every other Job of the PR` | User | object | "of the Workflow" |
| cli/tests/flow/plan_show.rs:307 | `Open the pull request: web` | User | pull request | unchanged (title of the open Job) |
| cli/tests/flow/plan.rs:872 | `Added by cpd when the PR starts` | User | object | "when the Workflow starts" |
| cli/tests/flow/plan.rs:869 | `Open the pull request: Change the PR` | Lead | pull request | unchanged title; "Change the PR" is fixture prose |
| cli/tests/flow/plan_delete.rs:227 | `the Plan's Jobs, Attempts, PRs, Events and Sessions are gone` | log | object | "Workflows" |
| cli/tests/flow/plan_render_waiting.rs:43 | `a link to the base PR:` | log | object | "base Workflow" |
| cli/tests/flow/plan_render_waiting.rs:45 | `the base PR waits for nothing:` | log | object | "base Workflow" |
| cli/tests/flow/job.rs:989 | `cpd pr add` | Lead | object | `cpd workflow add` (refusal message of a Job for an unknown PR) |
| cli/tests/flow/job_open_jira.rs:32 | `cpd pr edit repo --jira` | Lead | object | `cpd workflow edit repo --jira` |
| cli/tests/flow/job_open_description.rs:22 | `cpd pr describe --pr repo` | Lead | object | `cpd workflow describe --workflow repo` |
| engine/src/watch/open_description_tests.rs:68 | `cpd pr describe --pr repo` | Lead | object | `cpd workflow describe --workflow repo` |
| cli/tests/engine/watch.rs:2315 | `pr poll: gh search failed transiently (HTTP 502), failing since` | log | pull request | unchanged (`pr poll` polls GitHub) |
| cli/tests/engine/watch.rs:1919 | no open Job for PR repo but the one `pr open` queued | log | mixed | decision (`pr open` acts on a Workflow and opens its pull request) |
| engine/src/watch/open_tests.rs:129 | `Open the pull request: {PR_TITLE}` | User | pull request | unchanged |
| engine/src/watch/open_tests.rs:262 | `only PR web's was added` | log | object | "only Workflow web's" |
| core/src/incident_tests.rs:334 | `a merged until-PR ends the incident` | log | object | "until-Workflow" (follows the flag) |
| core/src/janitor_tests.rs:227 | `an outcome for PR {}: {outcomes:?}` | log | object | "Workflow" |
| core/src/janitor_tests.rs:253 | `PR {key} is untouched` | log | object | "Workflow {key}" |
| cli/tests/flow/gate.rs:325 | `the PR gate failure is the open Job's cause` | log | mixed | decision (the gate runs at the open Job's merge, before the pull request opens) |
| cli/tests/flow/driver_fix.rs:74 | `the Job's merge pushed the Lead Branch to the pull request's head` | log | pull request | unchanged |
| pi/test/testguard.test.ts:91 | `the full suite is the Gate's, run by the PR's open Job` | Worker | object | "the Workflow's open Job" |
| pi/test/review-chat.test.ts:162 | `Split the first PR.` | Lead | object | "Split the first Workflow." (plan-review feedback sentence, fixture text) |
| pi/skills/skill.test.ts:218 | `No local review before the PR opens` | Lead | pull request | unchanged ("the PR opens" is the pull request) |
| pi/skills/skill.test.ts:209 | `### PR` | Lead | step | decision: the Deploy heading; "### Pull request" or "### Open" |
| pi/skills/skill.test.ts:260 | `- **PR shape:**` | Lead | object | `- **Workflow shape:**` |
| pi/skills/skill.test.ts:267 | `docs-only or config-only PR skips the test Job` | Lead | object | "Workflow" |
| pi/skills/skill.test.ts:757 | `A PR is a substantial deliverable` | Lead | object | "A Workflow is a substantial deliverable" |
| pi/skills/skill.test.ts:662 | `cpd opens the PRs, you do not` | Lead | pull request | unchanged ("cpd opens the pull requests"), but the same test says "oversees the PRs whose Jobs are all merged" (Workflows) |
| pi/skills/skill.test.ts:668 | `Oversee all-merged PRs` | Manager | object | "Workflows" |
| pi/skills/skill.test.ts:677 | `cpd pr add <key> --repo <path> --title "<title>" --shape docs` | Lead | object | `cpd workflow add <key> ...` |
| pi/skills/skill.test.ts:542 | `cpd pr push --pr <key>` | Lead | mixed | decision (`pr push` pushes a Workflow's Lead Branch to its pull request) |
| pi/skills/skill.test.ts:469 | `Phases: grounding → plan → dispatch → pr → done.` | log | phase | stale phase `pr`; the test's own wrong-text fixture |
| pi/skills/skill.test.ts:413 | `PR key \\(R34\\)` | Lead | object | "Workflow key (R34)" (pins a contract row name; contract is another part) |
| pi/skills/skill.test.ts:645 | `the PR's open and update Jobs push it` | Lead | object | "the Workflow's open and update Jobs" |
| pi/skills/skill.test.ts:627 | Append one Job per actionable item to the PR named in `data\.pr` | Lead | mixed | decision (the Workflow named in `data.pr`, whose pull request has the item) |

Not listed: the other `Open the pull request:` title asserts (12 occurrences in 5 files: cli/tests/pr/pr_crud.rs, cli/tests/flow/plan.rs, cli/tests/flow/plan_show.rs, engine/src/watch/open_tests.rs, cli/tests/pr/pr.rs; same text, same sense: pull request, unchanged). The help texts of `cpd pr ...` are not asserted in the part: they come from the command tree (docs/interface.md), which `interface.rs` regenerates.

## Wrong or stale

| path:line | what is wrong | why |
|---|---|---|
| pi/skills/skill.test.ts:110 | The test forbids the word `workflow` in both skills; the rename brings it back (quote: `the term Workflow is retired`) | Fails after the rename: decision D3 (retired) is superseded by pro-d136/pro-d138. Rewrite the test, keep the `--wf` and `CPD_WF` (cpd 1) assertions around it |
| pi/skills/skill.test.ts:469 | A fixture of wrong text names the old phase `pr` (quote: `Phases: grounding → plan → dispatch → pr → done.`) | The phase is `deploy`; the fixture still catches a wrong chain, but reads as a stale one |
| pi/skills/skill.test.ts:209 | Test pins the Deploy heading `### PR` (step sense) in a list of required headings (quote: `### PR`) | Stale once PR means only the pull request; the heading text is a decision |
| pi/skills/skill.test.ts:660 | One test name, two senses: PRs opened (pull requests), "the PRs whose Jobs are all merged" (Workflows) (quote: `cpd opens the PRs: cpd-lead never opens one or waits`) | mixed |
| cli/tests/flow/plan.rs:1731 | Name says phase `pr`; the body sets `phase = 'dispatch'` (line 1737) (quote: `fn a_plan_in_phase_pr_with_no_live_job_archives`) | Phase `pr` is retired (`deploy`); name and body disagree |
| cli/tests/pr/pr.rs:3256 | Comment names phase `pr`; the fixture is `rebase_fx("deploy")` (line 3255) (quote: `A Job Merged in phase pr: on the Lead Branch`) | Phase `pr` is retired (`deploy`) |
| cli/tests/tui/tui_read.rs:189 | Comment names phase `pr` twice (also "`shut` (pr, closed unmerged)") (quote: `rel` (phase pr, PR 7 with a failing check)) | Phase `pr` is retired (`deploy`) |
| cli/tests/engine/watch.rs:2203 | Test name: "a Plan in pr" is the old phase (quote: `e5_polls_with_a_plan_in_pr_too_and_does_not_poll_more_often`) | Phase is `deploy` |
| cli/tests/engine/doctor.rs:686 | Test name: "dispatch_or_pr" lists the old phase (quote: `no_engine_is_needed_when_no_plan_is_in_dispatch_or_pr`) | Phase is `deploy` |
| cli/tests/store/events.rs:218 | SPEC lists optional field `pr`; the event data at cli/tests/flow/plan.rs:1201 holds `prs` (quote: `("plan-phase", "cp", "", &["from", "to"], &["jobs", "pr"])`) | Unverified: a mismatch or an old name; needs `Kind::spec` (core, another part) |
| cli/tests/flow/plan_delete.rs:227 | "PRs" listed beside Jobs and Attempts: the object, not the pull request (quote: `the Plan's Jobs, Attempts, PRs, Events and Sessions are gone`) | Becomes "Workflows" |
| cli/tests/pr/pr.rs:3196 | One sentence, two senses: "the PR is open" is the object's status `open`; the push goes to the pull request (quote: `where the PR is open, its Lead Branch is pushed`) | mixed |
| cli/tests/flow/gate.rs:172 | "PR gate": the Gate (pre-merge hook) that runs at the open Job's merge (quote: `fn pr_gate(hook: &str) -> String`) | mixed: it guards the pull request and belongs to the Workflow's open Job; the policy key is `pr.gate` |
| cli/tests/pr/pr_crud.rs:1690 | A stored `jobs.cancel_reason` value `pr cancelled` is asserted 12 times in 9 lines (quote: `cancelled/pr cancelled/-`) | A stored value, not text: after the rename, existing rows keep the old value unless a migration rewrites it |
| cli/tests/flow/plan.rs:819 | The Plan fingerprint is hashed over JSON holding the key `prs` (also plan_outline.rs:305, plan.rs:1931) (quote: `prs\\":{prs}`) | Renaming the key changes every stored fingerprint: an approved Plan review stops matching |
| core/src/policy/exposure_tests.rs:25 | The policy domain `pr` carries the pull request's checks and posting and the Workflow's gate: one table, two senses (quote: `("pr", "[pr]\nfeedback.checks = \"none\"\n")`) | mixed: decision on the user's `[pr]` policy tables |
| engine/src/watch/schedule_tests.rs:149 | "workflow worktree" is an older name of the Lead worktree (also core/src/driver/ports_tests.rs:646, merge/linear_tests.rs:3) (quote: workflow worktree `/repos/r.lead`) | Collides with the new Workflow: 17 occurrences in 4 files; decide whether the old name stays |
| cli/tests/tui/tui_plan.rs:43 | A cpd 1 fixture comment uses Workflow for the old Plan (quote: cpd 1's `workflow show` of cpd-rust's `cpd` Workflow) | History: true of cpd 1, reads as the new term; label it cpd 1 |
| core/src/pr/deploys_tests.rs:37 | GitHub Actions workflow (written in full): correct; `[deploys] workflows` and `workflow_runs` likewise (quote: `unscripted workflow: {path}`) | Not stale: a collision. 15 occurrences must survive a `pr` to `workflow` sweep |
| pi/test/retier.test.ts:187 | "the PR's base": the Workflow's `base` or the pull request's base branch? (quote: `the next free number on the PR's base`) | mixed, unverified; the comment explains a protocol number rule |

## Counts

- Files: 501 in the part; 207 with a match; 0 read in full; 294 searched only.
- Rows: Identifier families 51; User-facing strings 43; Wrong or stale 20; helpers 12; docs-tying tests 8.
- Occurrences by sense: the sum of the family counts is object 5169, mixed 4378, pull request 1203, other 22. The families overlap (6326 occurrences in all), so this is the weight of each sense, not a partition.
- Matching lines by the senses of the families that hit the line (4973 lines): only `mixed` 1499; only `object` 1338; `mixed`+`object` 1212; only `pull request` 562; `mixed`+`pull request` 148; `mixed`+`object`+`pull request` 138; `object`+`pull request` 66; others 10 (`other` senses). Measured by the Python pass of Coverage (sense per family, not per read of the line).
- Classes (files): 1 CLI 97, 2 JSON or table 53, 3 text 18, 4 generated file 0, 5 fixture or comment 39.
- Tests: 4097 Rust `#[test]` functions in the part; 409 have `pr` or `prs` as a word of their name, 53 more name `pull` only; 1183 pi `test(...)`/`it(...)` titles, 19 with PR as a word (a Python one-off over the files; not reproducible with rg alone: unverified by a second method). Test names are what `scripts/tests.sh run <name>` and Job Specs name, so renaming them is a cross-Plan hazard.
- Stale phase `pr`: 6 places (1 fixture text, 3 test names, one of them with a body that sets `dispatch`, 2 comments). Collisions with the new word: 17 "workflow worktree", 23 GitHub Actions workflows, 1 retiring assertion.
