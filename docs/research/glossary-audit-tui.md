# Glossary audit: cockpit (tui)

Part: `tui/src/**`, tests included. Audit only: no file but this one changed. Sense codes: `object` (the cpd object that becomes a Workflow), `pull request`, `phase`, `step`, `mixed`, `other`. Counts were taken at base commit `d98c1820`; the `rg` commands run from the repo root. Families overlap on purpose (a test name such as `a_pr_ready_item_…` is counted in its test-name family and nowhere else, but prose rows overlap with the identifier rows), so the family counts do not sum to the token total (see Counts).

**Main findings**
- The Plan view draws **one box per Workflow** and writes no word "PR" in it. Today a box reads ` <key> · <repo-dir> 󱦰 <base> · #<number> <state> 󱆭 ` (`plan/boxes.rs:134-186`): the key is the Workflow, `#<number> <state> 󱆭` is its pull request (a link). Before a pull request exists it reads `<status>`, `waiting on <keys>` or `cancelled · superseded by <key>` (`plan/boxes.rs:158-163`). No label needs a new word; the box title names a Workflow by key only.
- The words "PR"/"PRs" the User reads in the cockpit are few: the legend sections `PRs (Dispatch)` and `PR Checks`, the Pulse legend `a PR opened` and `a Job or a PR merged`, the foot line `no PR yet: cpd pr add`, and the error `No PR of Plan {plan} points at {pull}.`. The rest of the cockpit text says "pull request" and means it.
- The Deploy column draws only the machine and the Inbox (`widgets/draw_tests.rs:206` pins that no `PRs` section and no `acme/repo#` row is drawn): the `PRs` section its docs and `PlanCard.prs` describe is not drawn. `PrRow`/`PlanCard.prs` have no view (`read/widgets.rs:78`).
- Inbox kinds in tui: `pr-ready` (a pull request of yours, ready to merge; keeps its meaning), `pr-failing`/`pr-changes` (no longer opened), `review-requested`, `deploy`. `pr-ready` rows are matched by the subject prefix `pr:<owner>/<repo>#<n>` (pull request).
- Event kinds with `pr-` prefix that tui reads: `pr-added`, `pr-edited`, `pr-cancelled`, `pr-started` (object), `pr-opened`, `pr-merged` (mixed), `pr-feedback` (pull request).

## Coverage

Every path of the part is `tui/src/**` (170 files: 168 `.rs`, `plan/fixtures/cpd-chart.txt`, `history/fixtures/session.jsonl`; `tui/Cargo.toml` is outside `tui/src` and not part of it). 88 files have `tests` in the name (`*_tests.rs`, `tests.rs`; there is no `tui/src/layout_tests/` directory: the layout tests are `layout_tests.rs`, `layout_plan_tests.rs`, `layout_dialog_tests.rs`, `layout_popup_tests.rs`, `layout_section_tests.rs`, `layout_tree_tests.rs`).

| Depth | Paths | How |
|---|---|---|
| Read the whole line of every match | all 63 files with a match (list: `rg -l '<union>' tui/src`) | `rg -n` of the union below, to /tmp, read in full: 586 lines in non-test files, 343 in test files |
| Read in full | `read/pr_look.rs` (41 lines) | `read` |
| Read a range around the match | `plan/boxes.rs:126-190`, `plan/mod.rs:214-225`, `read/flow.rs:93-160`, `layout_plan_tests.rs:1-20`, `layout_section_tests.rs:1-30`, `widgets/column.rs:90-100,150-162`, `widgets/draw_tests.rs:186-215`, `phase_dot_views_tests.rs:1-6` | `read`/`sed -n` |
| Skimmed | the file names and sizes of all 170 files | `find tui/src -type f \| xargs wc -l` |
| Not read | the 107 files with no match, and every line without a match of the 63 others | no `PR`/`pull request` word there; unverified that they say nothing a person reads about a Workflow in other words (for example `Dispatch`) |

Union of patterns (the acceptance set, plus the uppercase and prose forms): `\bprs?\b|Pr[A-Z]|pr_|_pr\b|pull_request|PullRequest|\bPRs?\b|[Pp]ull request|\bPull\b|\bPr\b|pr-[a-z]`.

Commands behind the counts:

- Files: `find tui/src -type f | wc -l` = 170; `find tui/src -type f -name '*tests*' | wc -l` = 88; `rg -l '<union>' tui/src | wc -l` = 63; the same with `--glob '*tests*'` = 30.
- Tokens: `rg -o '\w*(\bprs?\b|Pr[A-Z]|pr_|_pr\b|pull_request|PullRequest)\w*' tui/src | wc -l` = 996; `rg -o '\bPRs?\b|\b[Pp]ull requests?\b|\bPull\b' tui/src | wc -l` = 360.
- Non-test lines: `rg -n '<union>' tui/src --glob '!*tests*'` = 586 lines; test lines: `--glob '*tests*'` = 343 (union without `\bPull\b|\bPr\b`; those add the Rust types `Pull`, `Pr`).
- Per family: the command in the Count column of the next section.
- **Coverage check** (acceptance): a script took every match of `\w*(\bprs?\b|Pr[A-Z]|pr_|_pr\b|pull_request|PullRequest)\w*` in every file of `tui/src` and checked that it overlaps a match of at least one family regex below. Result: **0 uncovered matches**. The script is not committed (a one-off in /tmp); the family regexes are quoted in full so the check can be redone.
- The `plan/fixtures/cpd-chart.txt` hits (`contract-gate-pr`, `rs-pr`, `gate-at-pr`) are Job names in a fixture: `other`.

## Identifier families

Count commands run from the repo root and print exactly the Count (`rg -oP` counts matches); the top-5 files are by matches: the same command with `-H`, then `cut -d: -f1 \| sort \| uniq -c \| sort -rn`. All counts taken at one commit: other Plans may move them by up to 2%.

| Family | Surface | Sense | Count | Files (top 5) | After the rename | Hazard |
|---|---|---|---|---|---|---|
| PrState | type (enum Open/Closed/Merged) | pull request | `rg -oP -- '\bPrState\b' tui/src \| wc -l` = 94 | plan/boxes.rs 23, read/look_tests.rs 20, palette.rs 12, read/pr_look.rs 10, read/shared.rs 8 | unchanged | none outside tui (re-exported by read/mod.rs) |
| PrNode | type (one box of the Plan view) | object | `rg -oP -- '\bPrNode\b' tui/src \| wc -l` = 60 | plan/box_lines_tests.rs 15, plan/boxes.rs 15, plan/mod.rs 5, phase_dot_views_tests.rs 4, read/flow.rs 4 | `WorkflowNode` | read/mod.rs re-export; 60 uses, tests construct it literally |
| PrStatus | type (waiting/dispatch/open/merged/cancelled) | object | `rg -oP -- '\bPrStatus\b' tui/src \| wc -l` = 47 | plan/boxes.rs 15, read/flow.rs 14, plan/mod.rs 3, updating_label.rs 2, failed_pill_tests.rs 2 | `WorkflowStatus` | `as_str` strings are stored `prs.status` values: parsed in read/flow.rs:341 |
| PrKey | type from cpd_core::ids | object | `rg -oP -- '\bPrKey\b' tui/src \| wc -l` = 46 | read/flow.rs 9, plan/boxes.rs 4, plan/mod.rs 4, pace_tests.rs 3, phase_dot_views_tests.rs 3 | `WorkflowKey` | owned by cpd_core: tui follows core; error label "PR key" in read/flow.rs/widgets.rs |
| PrFacts/PrLook | type (inputs and output of the state-word look) | pull request | `rg -oP -- '\bPr(Facts\|Look)\b' tui/src \| wc -l` = 21 | read/look_tests.rs 12, read/pr_look.rs 5, plan/boxes.rs 2, read/mod.rs 2 | unchanged | none |
| pr_look | fn and module (read/pr_look.rs) | pull request | `rg -oP -- '\bpr_look\b' tui/src \| wc -l` = 21 | read/look_tests.rs 13, plan/boxes.rs 4, read/mod.rs 3, read/pr_look.rs 1 | unchanged (file name unchanged) | `mod pr_look;` file name |
| pr_state/pr_open | fn in palette.rs | pull request | `rg -oP -- '\bpr_(state\|open)\b' tui/src \| wc -l` = 21 | palette.rs 7, read/look_tests.rs 5, plan/boxes.rs 4, read/pr_look.rs 3, widgets/draw_tests.rs 1 | unchanged | none |
| PR_* consts | const (PR_LEGEND, PR_CONFLICT, PR_CLOSED_ASKED, PR_UPDATING_*) | pull request | `rg -oP -- '\bPR_[A-Z_]+\b' tui/src \| wc -l` = 28 | palette.rs 12, read/pr_look.rs 5, widgets/legend.rs 5, read/look_tests.rs 5, widgets/draw_tests.rs 1 | unchanged | none |
| ITEM_PR_READY | const (glyph U+F12F4 of a `pr-ready` item) | pull request | `rg -oP -- '\bITEM_PR_READY\b' tui/src \| wc -l` = 3 | palette.rs 2, widgets/legend.rs 1 | unchanged | none |
| Pr (struct) | type read::Pr (a Plan's first Workflow: number, url, opened_at, state, checks) | mixed | `rg -oP -- '\bPr\b' tui/src \| wc -l` = 4 | read/shared.rs 3, read/mod.rs 1 | decision | read/shared.rs:202,318,409; re-exported read/mod.rs:59 |
| PrRow | type (a Workflow's pull request: key, number, state, polled) | mixed | `rg -oP -- '\bPrRow\b' tui/src \| wc -l` = 6 | read/widgets.rs 5, read/mod.rs 1 | decision | read/widgets.rs; `cli/tests/pr.rs` reads it (outside this part) |
| Kind::PrReady | enum variant (Inbox kind `pr-ready`) | pull request | `rg -oP -- '\bPrReady\b' tui/src \| wc -l` = 16 | widgets/inbox.rs 6, widgets/needs_tests.rs 3, read/dot.rs 2, read/dot_pr_tests.rs 2, widgets/deploy_tests.rs 1 | unchanged | Inbox kind string stored in `items.kind`; tests seed it in SQL |
| Kind::PrFailing/PrChanges | enum variants (Inbox kinds, no longer opened) | pull request | `rg -oP -- '\bPr(Failing\|Changes)\b' tui/src \| wc -l` = 12 | widgets/inbox.rs 4, read/dot.rs 4, widgets/inbox_margin_tests.rs 3, widgets/inbox_indent.rs 1 | unchanged | stored `items.kind` of old rows |
| Kind::PrAdded/Edited/Cancelled/Started | enum variants (Event kinds `pr-added` ...) | object | `rg -oP -- '\bPr(Added\|Edited\|Cancelled\|Started)\b' tui/src \| wc -l` = 8 | tree/kinds.rs 8 | `WorkflowAdded` ... (Event kinds renamed) | Event kinds are stored in `events.kind` CHECK (read/pulse.rs:141) and old rows |
| Kind::PrOpened/PrMerged | enum variants (Event kinds `pr-opened`, `pr-merged`) | mixed | `rg -oP -- '\bPr(Opened\|Merged)\b' tui/src \| wc -l` = 16 | plan/pulse.rs 5, read/pulse.rs 4, tree/kinds.rs 4, palette.rs 2, widgets/legend.rs 1 | decision | stored Event kinds; Pulse legend text "a PR opened" |
| pr_merged | field of Bucket (set by Event `pr-merged`) | mixed | `rg -oP -- '\bpr_merged\b' tui/src \| wc -l` = 9 | read/pulse.rs 4, plan/pulse.rs 2, read/pulse_tests.rs 2, plan/mod.rs 1 | decision | read/pulse.rs:134; plan/pulse.rs:208 draws nothing for it |
| pr_nodes/pr_needs/pr_blockers | fn and method (read/flow.rs; `display.pr_needs`) | object | `rg -oP -- '\bpr_(nodes\|needs\|blockers)\b' tui/src \| wc -l` = 5 | read/flow.rs 5 | `workflow_nodes` ... | `display.*` live in cpd_core (outside this part) |
| pr_rows (flow) | local in read/flow.rs | object | `rg -oP -- '(?<=let )pr_rows\b' tui/src \| wc -l` = 1 | read/flow.rs 1 | `workflow_rows` | none |
| pr_rows (legend) | locals in legend.rs (rows of PR states) | pull request | `rg -oP -- '(?<=Vec<Vec<Line<\'static>>> = )palette::PR_LEGEND\|\bmut pr_rows\b\|\bpr_rows\.push\|\bpr_rows\b(?=\s*\})\|(?<=rows: )pr_rows\b' tui/src \| wc -l` = 7 | widgets/legend.rs 7 | unchanged | none |
| sample_pr/to_pr | test helper / test param | object | `rg -oP -- '\b(sample_pr\|to_pr)\b' tui/src \| wc -l` = 12 | plan/box_lines_tests.rs 4, plan/mod.rs 4, plan/box_tests.rs 3, plan/box_link_tests.rs 1 | `sample_workflow`, `to_workflow` | none |
| pr_number/pr_url/pr_opened_at | column of table `prs` (in SQL strings) and tui-side field names | mixed | `rg -oP -- '\bpr_(number\|url\|opened_at)\b' tui/src \| wc -l` = 25 | phase_dot_views_tests.rs 3, tree/prepared_tests.rs 3, updating_label.rs 3, read/shared.rs 3, widgets/draw_tests.rs 3 | decision | columns of `prs` (applied migrations): SQL in read/shared.rs:389, read/flow.rs:323, read/widgets.rs:195 and 6 test fixtures |
| pull_requests | table (SQL, comments) | pull request | `rg -oP -- '\bpull_requests\b' tui/src \| wc -l` = 34 | read/widgets.rs 5, read/flow.rs 5, read/shared.rs 4, phase_dot_views_tests.rs 3, updating_label.rs 2 | unchanged | none |
| pull_request (column) | column `prs.pull_request` (FK to pull_requests.id) in SQL and comments | pull request | `rg -oP -- '\bpull_request\b' tui/src \| wc -l` = 15 | widgets/draw_tests.rs 3, tree/prepared_tests.rs 3, read/flow.rs 2, widgets/mod.rs 1, read/shared.rs 1 | unchanged (but the table that owns it is renamed) | SQL in widgets/mod.rs:248, read/flow.rs, read/shared.rs, updating_label.rs:123 |
| pr_start_failed | error code (engine starts a Workflow; log colour) | object | `rg -oP -- '\bpr_start_failed\b' tui/src \| wc -l` = 2 | logs/view.rs 2 | `workflow_start_failed` | error code stored in the log table; logs/view.rs:144 |
| pr-added..-started | Event kind string | object | `rg -oP -- '(?<![-\w])pr-(added\|edited\|cancelled\|started)\b' tui/src \| wc -l` = 10 | read/pulse.rs 5, read/pulse_tests.rs 4, read/flow.rs 1 | `workflow-*` | stored `events.kind` (CHECK list at read/pulse.rs:141) |
| pr-opened/pr-merged | Event kind string | mixed | `rg -oP -- '(?<![-\w])pr-(opened\|merged)\b' tui/src \| wc -l` = 15 | read/pulse.rs 12, read/pulse_tests.rs 3 | decision | stored Event kinds; Pulse bucket and legend |
| pr-ready | Inbox kind string | pull request | `rg -oP -- '(?<![-\w])pr-ready\b' tui/src \| wc -l` = 25 | widgets/inbox.rs 5, merge_at_once_tests.rs 5, inbox_link_tests.rs 4, widgets/mod.rs 2, read/dot.rs 2 | unchanged | stored `items.kind`; 9 files |
| pr-failing/pr-changes/pr-feedback | Inbox/Event kind string | pull request | `rg -oP -- '(?<![-\w])pr-(failing\|changes\|feedback)\b' tui/src \| wc -l` = 6 | read/dot.rs 3, read/pulse.rs 2, read/pulse_tests.rs 1 | unchanged | stored kinds |
| pr-base/pr-title/hyphenated names | migration name `pr-base` (comment) and test fixture names `long-pr-title-key`, `contract-gate-pr` | other | `rg -oP -- '(?<!-)-pr\b\|\bpr-(?!(added\|edited\|cancelled\|started\|opened\|merged\|ready\|failing\|changes\|feedback)\b)' tui/src \| wc -l` = 8 | plan/box_lines_tests.rs 4, plan/fixtures/cpd-chart.txt 3, read/stacked_ready_tests.rs 1 | unchanged (a fixture/history name) | none; applied migration name stays |
| subject prefix pr: | Inbox item subject prefix `pr:<owner>/<repo>#<n>` (string) | pull request | `rg -oP -- '\bpr:(?=[^ ])' tui/src \| wc -l` = 14 | widgets/inbox.rs 3, widgets/needs_tests.rs 3, inbox_link_tests.rs 2, read/flow.rs 2, merge/mod.rs 2 | unchanged | stored `items.subject`; Pull::from_subject in merge/mod.rs:40 |
| cpd pr merge/poll/post | CLI command words (prose and strings) | pull request | `rg -oP -- '\bpr (merge\|poll\|post\|reply)\b' tui/src \| wc -l` = 22 | merge/mod.rs 7, logs/view.rs 3, merge/marker.rs 3, read/shared.rs 2, read/widgets.rs 2 | unchanged | argv built in merge/mod.rs:116,120 and widgets/mod.rs:259-260 |
| cpd pr open | CLI command word (comment) | mixed | `rg -oP -- '\bpr open\b' tui/src \| wc -l` = 3 | widgets/draw_tests.rs 2, read/shared.rs 1 | decision | acts on a Workflow and opens its pull request |
| cpd pr add/cancel | CLI command words (UI text, comment) | object | `rg -oP -- '\bpr (add\|cancel)\b' tui/src \| wc -l` = 3 | plan/mod.rs 2, read/flow.rs 1 | `workflow add` | plan/mod.rs:220 shows `cpd pr add` to the User |
| "pr" argv word | string element of an argv `["pr", "merge"]` | pull request | `rg -oP -- '"pr"(?=[,\)\]\s]\|\.)' tui/src \| wc -l` = 9 | view_ports_tests.rs 4, merge/mod.rs 2, widgets/mod.rs 1, merge_at_once_tests.rs 1, merge/action_d550.rs 1 | unchanged (subcommand of merge/poll/post) | 4 of the 9 are `merge`/`poll` argv; widgets/mod.rs:260 prefixes `post` |
| --pr flag | CLI flag `--pr <key>` (a string and a comment) | object | `rg -oP -- '--pr\b' tui/src \| wc -l` = 2 | widgets/mod.rs 2 | `--workflow` | widgets/mod.rs:259; flag value is a Workflow key |
| bare pr: field/param | field/param named pr (Job.pr, `pr: &PrNode`, `pr: PrKey`) | object | `rg -oP -- '\bpr: ' tui/src \| wc -l` = 24 | plan/boxes.rs 4, plan/mod.rs 3, worked_on_tests.rs 2, phase_dot_views_tests.rs 2, read/flow.rs 2 | `workflow` | JobNode.pr mirrors column `jobs.pr` |
| pr( test helper | test helper fn `pr("api", &[])` building a PrNode | object | `rg -oP -- '(?<![-\w])(?<!\w\.)pr\(' tui/src \| wc -l` = 73 | plan/boxes.rs 56, plan/box_lines_tests.rs 11, read/dot_pr_tests.rs 4, failed_pill_tests.rs 2 | `workflow(...)` | plan/boxes.rs, box_lines_tests.rs |
| pr.<field> | field access on a PrNode/PrKey (`pr.key`, `pr.status`) | object | `rg -oP -- '\bpr\.' tui/src \| wc -l` = 16 | plan/boxes.rs 9, plan/mod.rs 3, plan/needs_tests.rs 2, read/shared.rs 1, read/widgets.rs 1 | `workflow.` | none |
| SQL jobs.pr/j.pr | column `jobs.pr` | object | `rg -oP -- '\b(j\|jobs)\.pr\b' tui/src \| wc -l` = 4 | read/flow.rs 3, plan/boxes.rs 1 | `jobs.workflow` | column of `jobs` (applied migrations) |
| SQL table prs | table `prs` (SQL and comments) | object | `rg -oP -- '(?:FROM\|JOIN\|INTO\|UPDATE) prs\b\|`prs`\|\bprs r\b' tui/src \| wc -l` = 30 | plan/boxes.rs 5, read/shared.rs 4, widgets/mod.rs 2, read/widgets.rs 2, read/pulse_tests.rs 2 | `workflows` | read/shared.rs:391,392; read/flow.rs:327; read/widgets.rs:196; widgets/mod.rs:248; plan/mod.rs:1220; 6 test fixtures |
| .prs field | field `PlanBoxes.prs` / `PlanCard.prs` access | object | `rg -oP -- '\.prs\b' tui/src \| wc -l` = 21 | plan/boxes.rs 10, plan/mod.rs 4, pace_tests.rs 2, plan/needs_tests.rs 2, worked_on_tests.rs 1 | `.workflows` | read/flow.rs:51, read/widgets.rs:87 |
| prs local/param | local, param, struct field and prose `prs` | object | `rg -oP -- '(?<!FROM )(?<!JOIN )(?<!INTO )(?<!UPDATE )(?<![`.])\bprs\b(?! r\b)\|(?<=`)prs\b(?!`)' tui/src \| wc -l` = 123 | plan/boxes.rs 52, plan/box_lines_tests.rs 43, widgets/legend.rs 8, plan/mod.rs 7, updating_label.rs 3 | `workflows` | none; legend.rs `let prs = Section` and updating_label.rs `prs` hold PR *states* (pull request): see User-facing |
| bare pr other | bare `pr` in prose, local variables and `jobs.pr` mentions | mixed | `rg -oP -- '(?<![-\w])(?<!\w\.)(?:(?<!")\|(?=pr[^"]))pr(?![-\w:(]\| (merge\|poll\|post\|reply\|open\|add\|cancel)\b)(?!\.)(?!: )' tui/src \| wc -l` = 38 | worked_on_tests.rs 6, widgets/draw_tests.rs 5, read/flow.rs 4, plan/mod.rs 4, tree/prepared_tests.rs 4 | decision (read each; most are the object) | none |
| PR / PRs (prose) | prose: comment, doc, log string, label, test string | mixed | `rg -oP -- '\bPRs?\b' tui/src \| wc -l` = 191 | read/flow.rs 30, plan/boxes.rs 26, palette.rs 13, plan/mod.rs 13, section.rs 11 | decision per row (see User-facing strings) | tests pin "PRs", "PR Checks", "a PR opened", "no PR yet" |
| pull request(s) (prose) | prose: comment, string | pull request | `rg -oP -- '\b[Pp]ull requests?\b' tui/src \| wc -l` = 85 | palette.rs 8, plan/boxes.rs 8, widgets/inbox.rs 6, read/widgets.rs 6, merge/marker.rs 6 | unchanged | legend text strings pinned by widgets/deploy_tests.rs:77, widgets/draw_tests.rs:330 |
| pull / Pull | type read::Pull (a pull request: number, url, state, clean, conflicting) | pull request | `rg -oP -- '\bPull\b' tui/src \| wc -l` = 84 | merge/mod.rs 15, plan/boxes.rs 12, merge/marker.rs 10, read/look_tests.rs 9, pace_tests.rs 5 | unchanged | 84 uses |
| test-name words _pr_/pr_ | snake_case test and fn names containing pr (not covered above) | mixed | `rg -oP -- '\b[a-z0-9_]*(?:_pr_\|_pr\b\|\bpr_\|_pull_requests?)[a-z0-9_]*' tui/src \| wc -l` = 175 | read/look_tests.rs 29, plan/boxes.rs 21, plan/box_link_tests.rs 11, phase_dot_views_tests.rs 10, read/flow.rs 10 | decision per test (see Wrong or stale) | renamed with the term; no stored value |

Why each `mixed` family is `mixed`: `PrRow`, `Pr`, `pr_number/pr_url/pr_opened_at` carry a Workflow's key or table plus its pull request's number, url or state in one value; `pr-opened`, `pr-merged`, `pr_merged`, `PrOpened`, `PrMerged` and the Pulse text `a PR opened` / `a Job or a PR merged` are true of both (the Workflow opens its pull request; a merged pull request is a merged Workflow); `pr open` acts on a Workflow and opens its pull request; the `PR / PRs (prose)` and `bare pr other` rows hold each use of the bare word whose sense is decided per row in User-facing strings (and, for comments, per sentence: not listed one by one, 191 + 38 uses; the rows below list those a person reads). Proposal (the User decides): the object-only identifiers become `Workflow*`; the pull-request-only ones stay; for each mixed row the proposal is in its User-facing row.

## User-facing strings

Text is quoted from the cited line (at most 15 words). 'Shown to': User = drawn in the cockpit or a dialog; Lead/Worker/Manager = in a doc comment or a string an agent reads; log = the cockpit's log or a test assertion. Only the rows that carry the word PR/pull request about a Workflow or a pull request are listed; rows that merely say `pull request` and mean it are marked `pull request` with Fix `unchanged`.

| Where (path:line) | Text | Shown to | Sense | Fix |
|---|---|---|---|---|
| tui/src/plan/boxes.rs:145 | `format!(" · {} {TO} {} · ", pr.repo_name, pr.base)` | User | mixed | Box title today: ` <key> · <repo> 󱦰 <base> · #<n> <state> 󱆭`. The key is a Workflow; `#<n> <state>` is the pull request: no word "PR" is drawn |
| tui/src/plan/boxes.rs:159 | `format!("cancelled · superseded by {by}")` | User | object | status of a Workflow in the box title; no change of word needed |
| tui/src/plan/boxes.rs:161 | `format!("waiting on {}", pr.waiting_on.join(", "))` | User | object | lists Workflow keys; unchanged text |
| tui/src/plan/mod.rs:220 | `"no PR yet: cpd pr add"` | User | object | `no Workflow yet: cpd workflow add`; the command word changes too |
| tui/src/plan/mod.rs:221 | `"no Plan yet"` | User | other | a Plan with Jobs unset: unchanged (Plan stays) |
| tui/src/widgets/mod.rs:257 | `No PR of Plan {plan} points at {pull}.` | User | mixed | "No Workflow of Plan {plan} has the pull request {pull}." |
| tui/src/widgets/mod.rs:259 | `["post", "--pr", &key, "--plan", plan.as_str()]` | Lead | object | `--workflow`; `post` stays (it acts on the pull request) |
| tui/src/widgets/legend.rs:198 | `title: "PRs (Dispatch)"` | User | mixed | section lists pull request states (open, merged, closed, conflict, updating): "Pull requests (Dispatch)" or keep "PRs"; column no longer draws this section |
| tui/src/widgets/legend.rs:200 | `title: "PR Checks"` | User | pull request | unchanged: checks are the pull request's |
| tui/src/widgets/legend.rs:173 | `your own broken pull request that nobody is fixing` | User | pull request | unchanged |
| tui/src/widgets/legend.rs:226 | `a pull request of yours, coloured by its state` | User | pull request | unchanged: Inbox `pr-ready` row |
| tui/src/widgets/legend.rs:190 | `a closed pull request is waiting while its Lead handles it` | log | pull request | comment only |
| tui/src/widgets/legend.rs:194 | `its Lead asks you` | User | pull request | unchanged |
| tui/src/widgets/legend.rs:196 | `its update Job is fixing the conflict` | User | pull request | unchanged (the update Job belongs to the Workflow, the conflict to the pull request: "its" is ambiguous: mixed) |
| tui/src/widgets/legend.rs:382 | `"a PR opened"` | User | mixed | Pulse legend for Event `pr-opened`: the Workflow opens its pull request: "a pull request opened" |
| tui/src/widgets/legend.rs:384 | `"a Job or a PR merged"` | User | mixed | Event `job-merged`/`pr-merged`: "a Job or a Workflow merged" |
| tui/src/plan/pulse.rs:109 | `"a PR opened".to_owned()` | User | mixed | same text as legend.rs:382 (two copies; DRY) |
| tui/src/plan/pulse.rs:111 | `"a Job or a PR merged".to_owned()` | User | mixed | same text as legend.rs:384 (two copies) |
| tui/src/read/flow.rs:339 | `format!("PR {plan}/{key}")` | User | object | error text of a bad row: "Workflow {plan}/{key}" |
| tui/src/read/flow.rs:351 | `has pr_number {number} and no pull request row` | User | mixed | error: column `pr_number` (object table) and pull request row |
| tui/src/read/flow.rs:319 | `id("PR key", &need, PrKey::parse)` | User | object | error label "PR key" appears at read/flow.rs:319,377,382,404 and read/widgets.rs:213: "Workflow key" |
| tui/src/read/shared.rs:408 | `PR of Plan {plan} has no pull request row` | User | mixed | error text; says a Workflow has no pull request row |
| tui/src/read/shared.rs:414 | `&format!("PR of Plan {plan}")` | User | object | label of a Checks parse error |
| tui/src/read/widgets.rs:203 | `format!("PR {plan}/{key}")` | User | object | error label |
| tui/src/merge/mod.rs:122 | `pr poll after merge {pull}: {}: {}` | log | pull request | unchanged |
| tui/src/logs/view.rs:144 | `["pr_start_failed", "push_failed"]` | log | object | error code, colours a log line for the Lead; renamed with the Event/code |
| tui/src/logs/view.rs:443 | `the engine's own start of a PR (source `watch`) is for the Lead too` | log | object | comment: start of a Workflow |
| tui/src/read/pulse.rs:141 | `'pr-feedback', 'pr-added', 'pr-edited', 'pr-cancelled',` | Worker | mixed | SQL CHECK-list copy of Event kinds: pr-added/edited/cancelled/started object; pr-feedback pull request; pr-opened/merged mixed |
| tui/src/read/pulse.rs:142 | `'pr-started', 'pr-opened', 'pr-merged', 'escalation'` | Worker | mixed | same list, second line |
| tui/src/widgets/inbox.rs:10 | `(a `pr-ready` row has none: its menu has Merge)` | Lead | pull request | Inbox kind `pr-ready`: the pull request is ready to merge; unchanged |
| tui/src/widgets/inbox.rs:290 | `the stuck Jobs and PR check/review state reach no Inbox` | Lead | pull request | comment |
| tui/src/widgets/mod.rs:8 | `on a `pr-ready` row Merge or Post by its status` | Lead | pull request | module doc |
| tui/src/widgets/mod.rs:240 | ``Post`: `pr post --pr <key> --plan <plan>`` | Lead | mixed | doc: `pr post` acts on the pull request but is addressed by Workflow key |
| tui/src/plan/mod.rs:16 | `clicks open a Lead tab, a Pulse row or a PR link` | Lead | pull request | module doc: "a PR link" is the pull request link in the box title |
| tui/src/plan/mod.rs:4 | `as one box per PR ([`boxes`], R34 A15)` | Lead | object | module doc: one box per Workflow |
| tui/src/plan/boxes.rs:1 | `The plan view's PR boxes (commands.md §13.2` | Lead | object | module doc: "Workflow boxes" |
| tui/src/widgets/draw_tests.rs:206 | `r.contains("PRs") \|\| r.contains("acme/repo#")` | log | mixed | test asserts the column no longer draws a `PRs` section |
| tui/src/tree/prepared_tests.rs:259 | `Some("cpd: PR feedback")` | Lead | pull request | state label of a Lead blocked by pull request feedback (written by core, not tui) |
| tui/src/read/pulse_tests.rs:42 | `then_some("cpd: PR feedback")` | Lead | pull request | same label in a fixture |
| tui/src/widgets/column.rs:96 | `the spinner in the glyph cell of a `pr-ready` item whose pull` | Lead | pull request | doc comment |
| tui/src/palette.rs:31 | `A running Job; phase `dispatch`; PR checks pending.` | Lead | mixed | doc of Meaning: "PR checks" is the pull request; `dispatch` is a current Plan phase |
| tui/src/palette.rs:33 | `A landing Job (checking or merging); an open PR; phase `deploy`.` | Lead | mixed | "an open PR" means pull request: unchanged |
| tui/src/palette.rs:37 | `a closed PR its Lead asked about, a conflict or failed checks on a PR` | Lead | pull request | doc comment |
| tui/src/palette.rs:146 | `activity, PR-opened and` | Lead | mixed | doc of the Pulse colour for Event `pr-opened` |
| tui/src/section.rs:142 | `collapsible_title("PRs", Some(3), 26)` | log | other | test fixture title text only: any word would do |
| tui/src/section_title_tests.rs:10 | `[("PRs", Some(3)), ("Logs", None)]` | log | other | test fixture title text only |
| tui/src/read/flow.rs:1 | `The `plan` view's snapshot (commands.md §17): a Plan's PRs, each with its` | Lead | object | module doc |
| tui/src/read/flow.rs:124 | `The PRs it needs, sorted (none is `cancelled`: `pr cancel` refuses a PR another one needs).` | Lead | object | doc comment |
| tui/src/read/shared.rs:200 | `A Plan's Pull Request: the columns of `prs`, with what `pr poll` last stored` | Lead | mixed | doc of struct `Pr` (type `Pr` in read/shared.rs:318: a Workflow with its pull request); `Pr` alone is the uncovered name: see Wrong or stale |

49 rows.

### How the Plan view box reads today

`plan/boxes.rs:140-186` (`title_of`) builds the top border of each box: ` ` + key (bold) + ` · <repo directory> 󱦰 <base> · ` + then, once a pull request exists, `#<number> ` (clickable) + state word (`draft`, `open`, `closed`, `merged`, `conflict`, `updating`; coloured by `read/pr_look.rs`) + optionally the Lead's dot + ` 󱆭` (the link cue); before a pull request the muted status word (`waiting`, `dispatch`, `open`, `merged`, `cancelled`) or `waiting on <keys>` or `cancelled · superseded by <key>`. So a box is a **Workflow** (key, repo, base, status, needs) that **shows its pull request** (number, state, link) when it has one: the box names both and says neither "PR" nor "Workflow". The foot line is `no PR yet: cpd pr add` when the Plan has no Workflow (`plan/mod.rs:220`).

### The legend (`widgets/legend.rs:sections`)

Sections in order: meanings, jobs, dots, `PRs (Dispatch)` (open, merged, closed + `its Lead handles it`/`its Lead asks you`, conflict, `updating` ×2: all **pull request** states, drawn from `read/pr_look.rs`), Inbox (`deploy`, `pr-ready` = `a pull request of yours, coloured by its state`, `review-requested`), Pulse (`a PR opened`, `a Job or a PR merged`), `PR Checks` (check states: pull request). The Deploy column itself never draws pull-request rows (see main findings).

### Layout tests that would change if a label changed

Layout tests pin geometry, not label text: `layout_plan_tests.rs` (1 test, 4 widths × 2 scrolls × 2 cases = 16 geometry checks; the case string `"pr box, …"` at line 15 and `name: "pr box"` at `plan/mod.rs:796` are labels of the test only), `layout_section_tests.rs` (3 tests, 12 geometry checks, no `PR` word). A new word in the box title would move cells in: `plan/boxes.rs` (8 `snap(` snapshots pinning 56 rows), `plan/box_lines_tests.rs` (title row of every box: 21 asserts, 8 lines naming a state word), `plan/box_link_tests.rs` (23 lines naming a state word, link cells), `plan/margin_tests.rs` (5), `updating_label.rs` (30), `read/look_tests.rs` (41 lines naming a state or conflict word), `phase_dot_views_tests.rs` (15), `worked_on_tests.rs` (10). Labels that would change text (the five `PR` strings) pin: `section.rs` 6 rows (fixture word `PRs`, free to keep), `section_title_tests.rs` 4, `widgets/draw_tests.rs` 2 (`PRs` absent, `PR Checks` present), `widgets/legend.rs` 2 (`a PR opened`, `a Job or a PR merged`), `plan/pulse.rs` 2, `updating_label.rs` 1 (`PRs (Dispatch)`), `read/flow.rs` and `plan/mod.rs` 2 each (`no PR yet`). Rows pinned per file count `rg -c` lines at the cited patterns: `rg -c 'PRs \(Dispatch\)|"PRs"|PR Checks|a PR opened|a Job or a PR merged|no PR yet' tui/src`.

## Wrong or stale

| path:line | what is wrong | why |
|---|---|---|
| tui/src/widgets/legend.rs:198 | section `PRs (Dispatch)` lists pull request states under a column name | the Deploy column draws no PR rows (`widgets/draw_tests.rs:206`); `Dispatch` is a Plan phase and a column nickname, not the column's name (see next row) |
| tui/src/read/widgets.rs:1 | "the Dispatch column"; `draw_tests.rs:1`, `read/widgets.rs:22`, `logs/view.rs:160`, `read/shared.rs:309`, `read/pr_look.rs:1`, `view_ports_tests.rs:955` say Dispatch, while `menu.rs:1`, `widgets/host.rs:1`, `widgets/mod.rs:166`, `section.rs:32`, `merge_at_once_tests.rs:2`, `runner_tests.rs:2` say Deploy for the same column | two names for one column; `deploy` is the phase name now. Not a PR word: flagged for the glossary owner |
| tui/src/phase_dot_views_tests.rs:2 | "the open-PR rows of the `PRs` section" | the column draws no such rows (`widgets/draw_tests.rs:206`); `read/widgets.rs:78` says "No view draws it". Whether the test still checks such rows: unverified |
| tui/src/read/widgets.rs:78 | `PlanCard.prs`/`PrRow` "No view draws it" | dead model: kept only for `cli/tests/pr.rs`; a rename would touch code with no User |
| tui/src/read/shared.rs:200 | `/// A Plan's Pull Request: the columns of `prs`, with what `pr poll` last stored` for struct `Pr` | one name holds a Workflow (`prs` columns) and its pull request (`pull_requests` row): mixed, the clearest case for a Workflow rename |
| tui/src/read/shared.rs:167 | "The state of a PR that is `merged` or not, whose pull request row says `row_state`" | "PR" and "pull request" in one sentence name two things (the Workflow and the pull request); after the rename the first is a Workflow |
| tui/src/read/flow.rs:141 | `A PR's pull request, for its box's title.` | same: a Workflow's pull request |
| tui/src/read/flow.rs:150 | `The PR is ready to merge (pro-d118: …` on struct `Pull` | says PR where the pull request is meant; in `Pull` the subject is the pull request |
| tui/src/widgets/mod.rs:257 | `No PR of Plan {plan} points at {pull}.` | one sentence with PR = Workflow and `pull` = pull request |
| tui/src/read/shared.rs:408 | `PR of Plan {plan} has no pull request row` (and the comment at :407) | PR = Workflow; the pull request row is the other thing |
| tui/src/read/flow.rs:339 | `format!("PR {plan}/{key}")` error label; `id("PR key", …)` at :319,:377,:382,:404 and `read/widgets.rs:213` | names a Workflow key with the word PR |
| tui/src/plan/mod.rs:220 | `no PR yet: cpd pr add` | the User reads a Workflow command under the name PR |
| tui/src/widgets/legend.rs:382 and :384; plan/pulse.rs:109 and :111 | the same two strings, `a PR opened` and `a Job or a PR merged`, appear twice (legend and Pulse sample) | duplicated text (DRY) and both are `mixed` (Event `pr-opened`/`pr-merged`) |
| tui/src/read/pulse.rs:139-142 | SQL kind list `'pr-added', …, 'pr-merged'` is a copy of the engine's Event kind list | copy of a list core owns: must change in lockstep with core (unverified whether core has the same list: not read, outside the part) |
| tui/src/widgets/legend.rs:467 and widgets/draw_tests.rs:331 | `!shown.contains("or pr")` | a guard against the old phase word `pr` in the legend line (sense `phase`, always stale): correct today, would be a dead guard once the word is gone from the codebase |
| tui/src/plan/mod.rs:2 | `cpd/cpd-ui` (`tui/src/flow.rs`, `pr.rs`, `workers.rs`) | history reference to the cpd-ui files; `pr.rs` there means pull request (unverified) |
| tui/src/read/pulse.rs:12 | table cell `[Kind::PrOpened] yellow pr-opened` | Event kind stays mixed until the User decides |

No use of `PR` as the old phase name (`phase`) other than the two guards above, and none as the `step` of the Deploy stage, was found in the part (`rg -n 'PR phase|phase.*\bpr\b' tui/src` prints five lines: `pr` there is a boolean/tuple parameter or the word `plan-phase`, not the old phase).

## Counts

- Files: 170 in `tui/src` (168 `.rs`); 63 with a match (30 of them test files); 107 with no match.
- Lines with a match of the union: 586 in non-test files, 343 in test files (929).
- Identifier tokens: 996 (`\w*(\bprs?\b|Pr[A-Z]|pr_|_pr\b|pull_request|PullRequest)\w*`) plus 360 prose forms (`PR`/`PRs`/`pull request(s)`/`Pull`); uppercase prose: 153 `PR`, 38 `PRs`; 76 `pull request`, 9 `pull requests`; `pr-` kebab kinds: 61 (`pr-ready` 25, `pr-merged` 9, `pr-opened` 6, `pr-feedback` 4, `pr-title` 4, others 13).
- Rows: Identifier families 47; User-facing strings 49; Wrong or stale 16.

- Family counts by sense (families overlap, so the total 1494 exceeds the 1356 distinct tokens): mixed 482, object 487, other 8, pull request 517.

- Uncovered matches of the acceptance patterns: 0.
- Layout tests: 6 `layout_*_tests.rs` files, none pins label text; see the layout paragraph for the files whose rows move with a title.
