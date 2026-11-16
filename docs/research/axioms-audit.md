# Axioms audit 1: summary for the User

Audit 1 judged docs, agent-facing text, code comments, dotfiles and records against the 29 axioms. This page groups the result: what was covered, the counts, every contradiction and borderline case for the User to decide, the restatements by the fix that removes them, the records to retire, and what the audit could not check. It does not decide anything and does not restate the findings tables (A5).

## Method

Fourteen Jobs each audited one part, using a candidate-line pass on rule words followed by a read of every hit in place, judged against the 29 axiom titles and texts in the Spec.

- `rec-cockpit` — all 237 in-force records of topic `cockpit`, in full.
- `rec-plan` — all 170 records of topics `plan` and `planning` plus all 147 of `lifecycle`, `engine`, `schema` and `migration`, in full.
- `rec-process` — all 212 records of `process`, `testing`, `hygiene`, `context`, plus all 83 of `architecture`, `config`, `dotfiles`, `machines` and 4 uncurated records, in full.
- `contract-1` — `docs/contract.md` lines 1–1027 (§1 Principles, §2 Model and identifiers, §3 State machines).
- `contract-2` — `docs/contract.md` lines 1028–2090 (§4 Storage through §4.6).
- `contract-3` — `docs/contract.md` lines 2091–3116 (§5 Events, §6 The tmux host, Dango palette, input widget, chat balloons, opening of §7).
- `contract-4` — `docs/contract.md` lines 3117–3791 (§7 git and the Gate through §10 Traceability).
- `commands-1` — `docs/commands.md` lines 1–2111 (§1 Conventions through §9 `pr`).
- `commands-2` — `docs/commands.md` lines 2112–3189 (§10 `session` through §13.3).
- `commands-3` — `docs/commands.md` lines 3190–4613 (§13.3 end through §24 `secret`).
- `docs` — `CONTEXT.md`, `README.md`, `BACKLOG.md`, `docs/implementing.md`, `docs/interface.md`, `docs/cockpit-parity.md`, `docs/versioning.md`, `docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`, `ideas/dango-spinner.sh`, all in full.
- `agents` — the text agents are told: `pi/skills/cpd-lead` and `cpd-manager`, `core/src/spec/worker.md`, `core/src/context/distill_prompt.md`, `core/src/policy/default.toml`, the owner policy on hs, and the agent-facing strings of `pi/lib/*.ts` and `core/src/**/*.rs`.
- `code` — comment lines of `core/`, `cli/`, `engine/`, `tui/`, `pi/`, `scripts/`, `package.json`, `Cargo.toml` (1721 rule-word comments, 943 `hs-` comments), plus the named scripts and test-tooling files.
- `sibling` — dotfiles `AGENTS.md`, `README.md`, `SETUP.md`, `TMUX.md`, `AGENT_TOOLS.md`, `IDEAS.md`, `BACKLOG.md`, `docs/external-config-repos.md`, `.agents/AGENTS.md`, the `.agents/skills/*/SKILL.md` files, the pi-config `README.md` and `AGENTS.md`, and the pi-config `extensions/*.ts`, all in full.

Parts whose Coverage says something in their own part was not read:

- `contract-2`: lines 2091–2095 were read only to confirm the part boundary, not audited.
- `commands-2`: lines 2100–2111 were read only to confirm the part boundary, not audited.
- `agents`: only the strings cpd sends to an agent were read; the full bodies of `pi/lib/*.ts` and `core/src/**/*.rs` were not read, and test code was left to audit 2.
- `code`: of about 767 source files, only files with a rule-word or `hs-` comment line were opened; files with no candidate were not read in full.
- `sibling`: not read were `dotfiles/docs/plans/**`, `dotfiles/docs/research/**`, skill reference files other than `SKILL.md`, `dotfiles/home/`, `scripts/`, `tests/`, `litellm/`, `.github/`, and most of the pi-config installation, settings, skills, prompts, themes, sessions and models.

Every part also says that lines or files belonging to another part were not read; that is scope, not a gap. No record file is a source of a "not read" gap: all three record parts read their files in full.

## Counts

| Part | Restate | Contradict | Borderline |
|---|---:|---:|---:|
| rec-cockpit | 2 | 2 | 1 |
| rec-plan | 7 | 1 | 1 |
| rec-process | 8 | 1 | 2 |
| contract-1 | 3 | 1 | 2 |
| contract-2 | 1 | 1 | 1 |
| contract-3 | 0 | 2 | 1 |
| contract-4 | 2 | 0 | 6 |
| commands-1 | 4 | 0 | 2 |
| commands-2 | 1 | 0 | 1 |
| commands-3 | 3 | 0 | 4 |
| docs | 2 | 4 | 1 |
| agents | 21 | 0 | 0 |
| code | 12 | 0 | 3 |
| sibling | 3 | 1 | 5 |
| **Total** | **69** | **13** | **30** |

The totals are the sum of the 14 `## Counts` lines; the check command is in the commit message.

## Contradictions and borderline cases

Every `contradict` and `borderline` row, grouped by axiom. "Conflict" is the findings file's own reason, not a decision. Borderline rows need the User to say whether they are the axiom restated or its concrete cpd application.

### Convention over configuration (7 contradict, 6 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| rec-cockpit: [hs-d498] | contradict | "pi-compact-tools' fold-toggle code moves inside the cpd repo as an extension cpd owns" | Copies a package's code into cpd instead of taking the dependency. |
| rec-cockpit: [hs-d593] | contradict | "It replaces the rule that @juicesharp/rpiv-ask-user-question is a third-party package, never vendored or forked" | Forks a third-party package instead of taking the dependency. |
| rec-process: [hs-n396] | contradict | "The pi-compact-tools fork (clayton-duarte/pi-compact-tools, hs-d488) keeps upstream's .github/workflows (ci.yml, release.yml) untouched" | Keeps a fork of an upstream package instead of taking the dependency. |
| contract-3: docs/contract.md:2970 | contradict | "an extension cpd owns: a copy of `pi-compact-tools` (nedleeds, MIT) 0.16.6 with the fold language added, shipped inside the adapter" | cpd copies and ships upstream pi-compact-tools instead of taking the npm dependency. |
| contract-3: docs/contract.md:3105 | contradict | "an extension cpd owns: a copy of `@juicesharp/rpiv-ask-user-question` (MIT) 2.13.8, shipped inside the adapter in `pi/ask-user-question/`" | cpd copies and ships a third-party pi package instead of taking the dependency. |
| docs: docs/implementing.md:368 | contradict | "**1. The copy.** `pi/compact-tools/extensions/` ← the fork's `extensions/*.ts` but `compact-tools-release.ts`" | cpd copies and ships upstream pi-compact-tools instead of taking the npm dependency. |
| docs: docs/implementing.md:390 | contradict | "**1. The copy.** `pi/ask-user-question/` ← the 43 `.ts` files of the package under the same relative names" | cpd copies and ships a third-party pi package instead of taking the dependency. |
| contract-1: docs/contract.md:536 | borderline | "The rule is a port of hermes-config `plugins/cockpit/` (`slack_fetch.py`, `refresh.py`, `classify.py`), read-only; each part cites its function" | Cannot tell whether porting hermes-config's classify logic is a fork of what upstream owns, or a cross-language rewrite. |
| contract-4: docs/contract.md:3176 | borderline | "The forms, the order and the templates are worktrunk 0.80.0's, so that a hook written for wt behaves the same" | cpd reimplements worktrunk's hook forms, order and templates itself instead of taking the dependency. |
| contract-4: docs/contract.md:3463 | borderline | "- Every other key has the default below, built into the binary. The file holds only `version`, `machine` and real deviations." | "real deviations" repeats the axiom, but the sentence also states the concrete required keys, so it may be the application. |
| commands-3: docs/commands.md:3573 | borderline | "**The look is pi-compact-tools 0.16.6's tool row, copied.** The adapter does not import or vendor that extension" | Claims it does not vendor pi-compact-tools while copying its row look; cannot tell if this is the forbidden copy. |

### Prose is not enforcement (2 contradict)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| rec-plan: [hs-n232] | contradict | "Freeze enforcement is prose only: the User declined a CI check (per dev-speed's Lead)." | A freeze rule enforced only by prose, which the axiom forbids. |
| contract-2: docs/contract.md:1791 | contradict | "No test can enforce that; the author declares it, and a review checks it." | A migration rule that a build test can check is enforced only by declaration and review. |

### Validate at the boundary, fail loud (6 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| rec-plan: [hs-d196] | borderline | "A store write whose cwd cannot be read records cwd '(unreadable)' instead of failing." | Cannot tell whether recording '(unreadable)' is the silent fallback the axiom forbids. |
| contract-1: docs/contract.md:95 | borderline | "is parsed into types once, at the edge. Unknown flags and unknown config keys are errors. Nothing falls back silently." | States the axiom almost verbatim but also names the cpd inputs, so it may be the concrete application. |
| contract-2: docs/contract.md:1055 | borderline | "when it cannot be read (the directory was deleted under the process) it is the text `(unreadable)` and the write goes on (hs-d196)" | Cannot tell whether recording (unreadable) is the silent fallback the axiom forbids. |
| contract-4: docs/contract.md:3653 | borderline | "Every input is parsed once, at the boundary: a scope file at the first resolve of a command (and by `cpd policy show`), a login at `context new`" | States the axiom almost verbatim but also names the cpd inputs, so it may be the concrete application. |
| contract-4: docs/contract.md:3671 | borderline | "the Session gets no policy text: loud, not defaulted." | "loud, not defaulted" restates the axiom's phrase while describing a concrete mechanic. |
| commands-2: docs/commands.md:2892 | borderline | "is not an error: the view starts with an empty map, logs `scroll positions reset: <reason>`" | A corrupt view-state file resets to defaults with a log line, maybe the silent fallback the axiom forbids. |

### Only authored code is tested (2 contradict, 3 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| contract-1: docs/contract.md:914 | contradict | "the install, run under `LANG=C`, `LC_ALL=C` and an empty `LANG` against a real tmux, passes `watch --check` and restarts the engine" | The contract states an install test runs against a real tmux, against the axiom that every external tool is mocked. |
| docs: docs/implementing.md:240 | contradict | "with the real tmux, the install against a real cockpit passes and every tmux line the fake logs begins `tmux -u`." | An install test runs against the real tmux, against the axiom. |
| commands-3: docs/commands.md:3384 | borderline | "the Job that implements this checks it with a real pi." | Cannot tell whether checking pi's dialog with a real pi is a test of external pi or verification per "Inference is not evidence". |
| code: pi/test/compact-tools/compact-tools.test.ts:67 | borderline | "Tests must never read the developer's real ~/.pi/agent" | Cannot tell if that is "Only authored code is tested" or a cpd isolation detail. |
| sibling: /home/clayton/dotfiles/AGENTS.md:166 | borderline | "it asserts the rendered output matches the live file in `$HOME` for every managed target." | The parity suite reads the live $HOME instead of a fixture, which may be a real external dependency. |

### No flakes, no leaks, no orphans, no sleep, no errors (2 contradict, 2 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| docs: docs/implementing.md:352 | contradict | "failed one test at load 34-50 (the test's name was not kept); two reruns at load 40-42 passed (unverified: a load flake)." | A flaky test is rerun and left as an unverified load flake, against the axiom's zero tolerance. |
| sibling: /home/clayton/dotfiles/AGENTS.md:170 | contradict | "On Linux with chezmoi installed, the 2 tests in `tests/test_services_reentrancy.py` fail: they render a darwin template on Linux." | Documents 2 pre-existing failing tests as acceptable, contrary to the zero-tolerance axiom. |
| rec-process: [hs-d267] | borderline | "When the User wants to ship anyway, a Lead pushes and opens the pull request by hand at the User's request" | Permits opening a PR past a failing test by hand; cannot tell if the User's order overrides the no-bypass rule. |
| docs: docs/implementing.md:138 | borderline | "never a fixed sleep to get a result; waits poll a positive signal." | Restates the no-sleep axiom inside a speed rule; decide if it should point to the axiom instead. |

### Guards only for proven bad behaviour (4 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| contract-3: docs/contract.md:2881 | borderline | "`git stash` is refused in cpd Sessions (hs-d310): refs/stash is shared by every worktree of a repo; commit on your branch instead." | A guard on git, a raw tool, cites shared-refs risk, not evidence of misbehaviour seen in practice. |
| contract-4: docs/contract.md:3678 | borderline | "Agents never call `op` directly (the pi adapter refuses it)." | The adapter blocks a raw CLI tool with no evidence of misbehaviour cited, though the secrets-approval design may justify it. |
| commands-3: docs/commands.md:3626 | borderline | "Decision (User, 2026-10-19). A Manager never blocks: no sleep, polling loop or long wait in its own turn." | A shell guard refuses sleep and watch for Managers with no incident or practice evidence cited. |
| commands-3: docs/commands.md:3774 | borderline | "Decision (User, 2026-10-22, accepted assumptions A1–A5). Agents never call 1Password's `op` directly; the pi adapter refuses it" | A shell guard refuses op with no incident or practice evidence cited, only a User decision. |

### DRY (3 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| rec-process: [hs-d607] | borderline | "The set, as the User confirmed: the 19 of AGENTS.md plus make it work/fast/good in that order (hs-d153)" | Lists all 29 axioms; cannot tell if the concept decision may enumerate them or must point to AGENTS.md. |
| commands-1: docs/commands.md:867 | borderline | "It is derived and holds no information the Plan lacks: it carries no Spec, no Mermaid and no row for a Locked Job." | May be the Plan summary's design constraint, not a restatement of one source of truth. |
| sibling: /home/clayton/dotfiles/.agents/skills/wayfinder/SKILL.md:23 | borderline | "a decision lives in exactly one place, its ticket, so the map never restates it, only gists it and links." | Is this the DRY axiom restated, or the map's own index rule? |

### A primary checkout is a mirror of origin/main (2 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| rec-cockpit: [hs-d618] | borderline | "Repair is a guarded cpd command that acts on the cpd checkout only" | Cannot tell whether "the cpd checkout" is the primary checkout the axiom keeps read-only. |
| sibling: /home/clayton/dotfiles/AGENTS.md:145 | borderline | "### Work in a worktree" | The heading restates the worktree rule, but the section also carries a dotfiles-specific reason; keep or point to the axiom. |

### Make illegal states unrepresentable (1 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| contract-4: docs/contract.md:3730 | borderline | "the `jobs.tier` CHECK of §4.4 stays `[OSH][0-5]`: the rule is the command's and the engine's, not the schema's" | Keeps the allowed-Tier invariant out of the schema and lets stored Jobs sit outside the range, against pushing invariants into schemas. |

### Blocked means it needs the human (1 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| commands-1: docs/commands.md:1614 | borderline | "it waits for its Lead, not for the User" | The waiting/blocked rule is stated as a parenthetical reason, tied to a cpd Session-state mechanic and citing hs-d303. |

### Recency wins (1 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| code: core/src/context/record.rs:316 | borderline | "Recency wins (§2.2): a decision may supersede a decision or a note, a note only a note (a fact never overrules a" | Comment opens with the axiom title "Recency wins"; cannot tell if the cpd supersede-rule detail may restate it or must point to it. |

### Delete first, but check the fence (1 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| code: cli/tests/tooling/dead_code.rs:1 | borderline | "Dead code is deleted, not silenced (finding commands-G1)" | Cannot tell if this restates the axiom or is a cpd `allow(dead_code)` policy. |

### Inference is not evidence (1 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| sibling: /home/clayton/dotfiles/.agents/skills/research/SKILL.md:10 | borderline | "Follow every claim back to the source that owns it." | Is this the source-reading axiom restated, or research method detail? |

### Ask, don't assume (1 borderline)

| Where | Kind | Quote | Conflict |
|---|---|---|---|
| sibling: /home/clayton/dotfiles/.agents/skills/prototype/SKILL.md:17 | borderline | "and state the assumption at the top of the prototype." | Is this the decide-and-state-the-assumption axiom restated, or prototype method detail? |

## Restatements by fix

The 69 `restate` rows, grouped by what fixes them. Each group gives the files, the row count and the part files that hold the rows. No row is repeated here; the findings files hold the quotes and the exact proposed replacements.

### cpd docs and CONTEXT.md — 16 rows

- `docs/contract.md`: 6 rows (lines 229, 308, 826, 2013, 3147, 3455).
- `docs/commands.md`: 8 rows (lines 320, 1263, 1270, 1843, 2243, 3369, 3542, 3746).
- `CONTEXT.md`: 1 row (line 158).
- `docs/implementing.md`: 1 row (line 132; line 217 states the same rule).
- Parts: contract-1, contract-2, contract-4, commands-1, commands-2, commands-3, docs.

Risky exact edits: `contract.md:3147` must keep its worktree-layout sentence while replacing only "The repo's main checkout is never touched"; `contract.md:2013` replaces a whole clause; `commands.md:320`, `:1263`, `:1270`, `:1843`, `:2243`, `:3542` each have an exact replacement string in the findings file and must not be paraphrased; `implementing.md:132` only removes one of two copies (line 217 remains), so the pair should be fixed together.

### cpd skills and Spec templates — 16 rows

- `pi/skills/cpd-lead/SKILL.md`: 8 rows (lines 37, 39, 41, 42, 144, 158, 200, 225).
- `pi/skills/cpd-manager/SKILL.md`: 4 rows (lines 27, 40, 105, 129).
- `core/src/context/distill_prompt.md`: 1 row (line 17).
- `core/src/spec/worker.md`: 3 rows (lines 33, 54, 186).
- Part: agents.

Risky exact edits: `cpd-lead/SKILL.md:39` and `:144` are "delete the sentence/clause"; deleting them can leave a dangling lead-in, so read the paragraph before cutting. `distill_prompt.md:17` deletes a sentence from a prompt that is sent verbatim to a model, so the surrounding instruction must still parse.

### owner policy text — 2 rows

- `/home/clayton/.cpd/policy/owners/clayton-duarte.toml`: 2 rows (lines 14, 15).
- Part: agents.

Risky exact edits: this file is outside the worktree and is owner policy on hs; a Worker cannot edit it, so the fix belongs to the owner-policy batch, not this PR.

### code comments — 15 rows

- `cli/tests/tooling/fake_tmux.rs`: 1 (line 3); `cli/tests/tooling/tripwire_script.rs`: 1 (line 3); `cli/tests/common/mod.rs`: 2 (lines 14, 306); `cli/tests/tui/tui_pulse.rs`: 1 (line 2); `tui/src/tree/another_owner_tests.rs`: 1 (line 2); `tui/src/read/watch_tests.rs`: 1 (line 1); `pi/test/pi-check.test.ts`: 1 (line 11); `pi/test/no-fixed-sleeps.test.ts`: 1 (line 6); `pi/lib/state.ts`: 1 (line 6); `tui/src/background_tests.rs`: 1 (line 1); `tui/src/pump_tests.rs`: 1 (line 1) — 12 rows from part `code`.
- `core/src/driver/worker.rs`: 1 (line 230); `core/src/pr/start.rs`: 1 (line 409); `pi/lib/reminder.ts`: 1 (line 8) — 3 rows from part `agents`.
- Parts: code, agents.

Risky exact edits: 12 of the 15 are comments inside test files. Replacing the comment must not touch the test body, because the real-tmux and real-git test sites are audit 2's, not audit 1's. `pi/lib/state.ts:6` needs its replacement to keep the "kind of a reason" meaning.

### dotfiles — 3 rows

- `/home/clayton/dotfiles/.agents/AGENTS.md`: 1 row (line 1).
- `/home/clayton/dotfiles/BACKLOG.md`: 2 rows (lines 29, 216).
- Part: sibling.

Risky exact edits: deleting `.agents/AGENTS.md` is the riskiest edit in the audit. The findings file says no reader was found by `rg`, but whether an external agent framework auto-loads `.agents/AGENTS.md` is unverified; confirm before deleting. The `BACKLOG.md` lines have exact "see the axiom" replacements.

### pi-config AGENTS.md — 0 rows

No restatement was found in the pi-config `AGENTS.md` itself. At audit time it held exactly the 19 existing axioms and none of the 10 new ones, so there was nothing there to report.

### records to retire — 17 rows

Listed one per line in the next section, from parts rec-cockpit, rec-plan and rec-process. Risky exact edits: per A5 records are retired by superseding decisions, never edited, so none of these is a text edit in this PR; and some findings propose deleting one sentence or clause rather than retiring the whole record.

## Records to retire

Seventeen in-force records restate an axiom. This is the list from the three record parts; retiring is by a superseding decision, never by editing the record.

- `hs-d303` — Blocked means it needs the human (`rec-cockpit`)
- `hs-d335` — No flakes, no leaks, no orphans, no sleep, no errors (`rec-cockpit`)
- `hs-d239` — No planned work only in chat (`rec-plan`)
- `hs-d301` — Decided work skips review (`rec-plan`)
- `hs-d400` — Decided work skips review (`rec-plan`)
- `hs-d491` — No flakes, no leaks, no orphans, no sleep, no errors (`rec-plan`)
- `hs-d573` — Fix the cause, not the symptom (`rec-plan`)
- `hs-d299` — Convention over configuration (`rec-plan`)
- `hs-d250` — YAGNI (`rec-plan`)
- `hs-d27` — Only authored code is tested (`rec-process`)
- `hs-d34` — No flakes, no leaks, no orphans, no sleep, no errors (`rec-process`)
- `hs-d153` — Make it work, make it fast, make it good (`rec-process`)
- `hs-d238` — No flakes, no leaks, no orphans, no sleep, no errors (`rec-process`)
- `hs-d248` — Guards only for proven bad behaviour (`rec-process`)
- `hs-d358` — Recency wins (`rec-process`)
- `hs-d606` — A primary checkout is a mirror of origin/main (`rec-process`)
- `hs-n242` — Convention over configuration (`rec-process`)

## What the audit could not check

- The real-tmux and real-git test sites themselves. Accepted assumption A6 keeps them out of audit 1; audit 2 sizes that migration. Audit 1 reports only prose and comments that state the rule, not the test code that breaks it.
- The parts with partial reads, named in Method: `contract-2` and `commands-2` boundary lines; the full source bodies behind `agents`' agent-facing strings; every source file of `code` without a rule-word or `hs-` comment; and the many dotfiles and pi-config paths `sibling` lists as not read.
- Records only referenced by id were not read as blocks (for example hs-d301, hs-d358, hs-d488, hs-d498, hs-d238, hs-d267, hs-d606); only in-force record blocks in the three record files were judged. Briefs and Plan records were out of scope.
- The pi-config `AGENTS.md` held only axioms 1–19 during the audit; axioms 20–29 were judged from the Spec's titles and texts, not from the live file. Ungoverned by this audit: whether the merged file's wording matches the Spec.
- Unverified items the findings themselves flag: whether an external framework auto-loads `dotfiles/.agents/AGENTS.md`; whether the load flake at `docs/implementing.md:352` is real; whether the real-pi check at `docs/commands.md:3384` is a test of external pi or the required verification.
- No sibling repository other than `dotfiles` and the pi-config files named in Method was read, so a restatement there would be missed.
- This audit does not decide any contradiction and does not verify that the proposed fixes are the right ones; the User owns those decisions (A5).
