# Skills audit: cpd-lead, cpd-manager, writing-for-agents

## Summary

- Audited `pi/skills/cpd-lead`, `pi/skills/cpd-manager` and `pi/skills/writing-for-agents` (the User's scope) under four lenses; findings only, no skill is edited.
- Baseline: `a954761dfe6e7b6316f955ada638e363c170af55` (`git merge-base HEAD origin/main`).
- Totals: 109 findings in seven part files: 14 wrong, 16 conflict, 60 bloat, 19 style (see [Counts](#counts)).
- 1. M7 is refuted at the baseline: the resolver for `<context>:<plan>` landed in `85337fc6f` (#790), an ancestor of the baseline (DM-1, DL considered list).
- 2. `cpd-lead` cites six wrong Orchestrating step numbers, left by an editing note that is still in the text (LW-1 to LW-6, LW-13).
- 3. `cpd-lead` says a PR body "opens only when you do", against its own Rule that cpd opens pull requests (LW-7, LW-8).
- 4. `cpd-lead` attaches `agent_forbidden` to commands the docs do not refuse that way (DL-1); the two skills disagree on the draft-parking list and the sequencing role (XS-2, XS-3).
- 5. Both skills hold large duplicated blocks and branch-only reference inline: four blocks shared word for word (XS-6 to XS-9), 2403-character Rule (LW-31), Manager steps buried (MG-2).
- Eight prior open axiom findings on `cpd-lead` are still open ([LA part](skills-audit-lead-axioms.md), Prior audit status).

## Method

- Lens 1 internal contradictions and duplication; lens 2 drift from the real `cpd` CLI and docs; lens 3 fit to `writing-for-agents`; lens 4 the axioms. Findings only.
- Parts and what each read:
  - LW: `cpd-lead` SKILL.md and its three references, lenses 1 and 3.
  - LA: the same four files against the axioms, lens 4, plus the prior audit rows about `cpd-lead`.
  - MG: `cpd-manager` SKILL.md against `writing-for-agents`, and the Manager's findings M1 to M6.
  - DL: `cpd-lead` against live `--help` of 68 subcommands, `docs/commands.md` and the adapter source (lens 2).
  - DM: `cpd-manager` against the CLI and docs (lens 2), and M7.
  - XS: `cpd-lead` against `cpd-manager` and `docs/commands.md` section 15.24.
  - WU: the bundled `writing-for-agents` (SKILL.md, SKILL-MECHANICS.md, NOTICE) against pi docs and the axioms.
- Rubric: `pi/skills/writing-for-agents/SKILL.md` and `SKILL-MECHANICS.md`. Axiom source: the 29 axioms in the user's `AGENTS.md`, and the prior `axioms-audit-agents.md`.
- Kinds: `wrong` (stated falsehood), `conflict` (two statements disagree), `bloat` (repeats or does not bear on the task), `style`.
- Not reported by design: what `pi/skills/skill.test.ts` already proves (that a named command, flag, code or number exists in the docs).

## Coverage

Union of the seven Coverage sections. Baseline in every part: `a954761df...`.

- Read in full: `cpd-lead/SKILL.md` lines 1-451 (`wc -l` is 451, the Spec said 452; LW, LA, DL, XS), `references/spec-files.md` 1-161, `questions.md` 1-58, `pr-description.md` 1-104 (LW, LA, DL).
- Read in full: `cpd-manager/SKILL.md` 1-173 (MG, DM, XS); `references/` holds only `.gitkeep`.
- Read in full: `writing-for-agents` SKILL.md 1-81, SKILL-MECHANICS.md 1-22 (LW, MG, WU), NOTICE 1-8 (WU); `docs/commands.md` section 15.24 lines 5158-5175 (XS).
- Gaps:
  - DL: two long lines of `spec-files.md` were shown cut (line 30 after column 1200, line 60 after column 2200); the rest of both is unverified.
  - DL: the loop guard and `cpd reminder` text carry no number in the four files, so nothing was compared. The live `cpd` binary equals the baseline: unverified (not built).
  - DM: `session send` was never run, so M7's send behaviour is unverified; `done` in the phase list and the 30-second bash limit were not checked.
  - MG: M7 not covered by MG (DM covers it). `contract.md:1194` vs `:1205` (E6 removed or not): unverified which is current.
  - WU: no byte comparison with upstream (no network); LICENSE not read; pi `settings.md` not read; whether pi shows a description under `disable-model-invocation` is unverified; the 1024-character description bound is not measured; a pinned `pi-package` of the skill: unverified.
  - LA: no file outside the named four and the prior audit rows; no `cpd` command beyond read-only ones.
  - LW: `docs/commands.md` grepped only for `draft_parked` and `gh pr edit`.
  - XS: also checked, not audited: several `--help` outputs, `docs/commands.md` lines 180 and 205.
- This report was assembled by reading the seven part files and re-running the checks quoted under M7. No skill, reference or doc was read beyond that; any claim here is as verified as its part file says.

## Counts

Every total is computed over the part files with this command (`rows` is `grep -hE '^\| (LW|LA|MG|DL|DM|XS|WU)-[0-9]+ ' docs/research/skills-audit-*.md`; kind is column 5, lens column 3; per part, the same grep on that part file). Output of the script, run on the part files at the baseline:

| Part | wrong | conflict | bloat | style | Total |
|---|---:|---:|---:|---:|---:|
| LW | 6 | 6 | 23 | 6 | 41 |
| LA | 1 | 4 | 19 | 2 | 26 |
| MG | 0 | 2 | 11 | 3 | 16 |
| DL | 3 | 0 | 2 | 2 | 7 |
| DM | 1 | 0 | 0 | 0 | 1 |
| XS | 2 | 2 | 5 | 2 | 11 |
| WU | 1 | 2 | 0 | 4 | 7 |
| **All** |       14 |       16 |       60 |       19 |      109 |

| Lens | wrong | conflict | bloat | style | Total |
|---|---:|---:|---:|---:|---:|
| 1 |        6 |        9 |       27 |        2 |       44 |
| 2 |        7 |        0 |        2 |        4 |       13 |
| 3 |        0 |        0 |       12 |        9 |       21 |
| 4 |        1 |        7 |       19 |        4 |       31 |
| **All** |       14 |       16 |       60 |       19 |      109 |

By skill (the `Where` column), computed the same way: `cpd-lead` 81 (11 wrong, 12 conflict, 48 bloat, 10 style), `cpd-manager` 21 (2 wrong, 2 conflict, 12 bloat, 5 style), `writing-for-agents` 7 (1 wrong, 2 conflict, 0 bloat, 4 style). Total 109.

## Manager findings M1 to M7

The Manager's claims; the verified numbers are those of the part files, and the Manager's differed where stated. The M3 and M5 rows hold the part's verdict.

| M-id | Manager's claim | Verdict | Verified numbers | Part file and Finding ID |
|---|---|---|---|---|
| M1 | `description` is 48 words, repeats the body's opening and states the role trigger twice | partly | 67 words, not 48 (line 3); repeats line 8; trigger twice in line 3 | [MG](skills-audit-manager.md): MG-1 |
| M2 | Steps buried; host, MCP, secrets and bug protocol inlined instead of disclosed | partly | `## 2. Plans` at line 99 (not 97); 17 Rules bullets (confirmed); host `## 5` lines 169-173 (not 172-176); MCP line 69 (not 68-70); secrets line 44 (not 39-41); bug protocol 54-58 (not 51-55); the other roles' workflows are one paragraph (line 23) | [MG](skills-audit-manager.md): MG-2, MG-3, MG-4 |
| M3 | Messaging, records, `plan new`, turn-start and "never answer for the User" are scattered | partly | `session send <context>:<plan>` at lines 50, 62, 122, 134, 136, 145, 154, 159, 160, 163 (the Manager's lines for `context ask` 144/163 hold none; real 45, 60, 85, 93); `plan new` lines 49, 61, 95, 109, 110; "never answer for the User" is lines 40 and 167 (step 7 forbids relaying, not a third copy) | [MG](skills-audit-manager.md): MG-5, MG-6, MG-7 |
| M4 | "never" 36 times in 176 lines; 15 forbidden subcommands listed | partly | 37 "never" in 173 lines (I re-ran `grep -oi never` and `wc -l`: 37, 173); lines 47-48 name 21 forbidden commands, not 15 | [MG](skills-audit-manager.md): MG-8 |
| M5 | Raw decision citations, section numbers with no path, Lead-only commands in a Manager file | confirmed | Citations at lines 53, 54, 69, 152, 153, 171, 173; `commands.md §24` and `§15.17` do carry the file name, `contract §3.7 E6` has none; Lead-only commands at lines 126-127 (and 122) | [MG](skills-audit-manager.md): MG-9, MG-10 |
| M6 | Step 4 "Check pending work first" comes after Intake, Interview and Split | confirmed | Steps: 1 Intake 101, 2 Interview 102, 3 Split 103, 4 Check pending 104 | [MG](skills-audit-manager.md): MG-11 |
| M7 | `cpd session send <context>:<plan>` fails with `invalid_session_id` at 6 places (48, 59, 132, 142, 152, 157) | refuted at the baseline | The resolver is on main: `core/src/session/resolve.rs` exists in the worktree (`SessionRef::Key` at line 47); `git merge-base --is-ancestor 85337fc6f HEAD` exits 0 (`85337fc6f Resolve Sessions by agent key ... (#790)`); 7 lines, not 6: 50, 62, 134, 136, 145, 159, 160 (I re-ran `grep -n`); `cpd session show skills-audit:skills-audit-lead` returned `"ok":true` in DM; `session send` was never run: unverified. The Manager's run likely used an older installed binary: unverified | [DM](skills-audit-drift-manager.md): DM-1 |

## Findings that are wrong or conflict

30 rows (14 wrong, 16 conflict), every row of those kinds from the seven part files. Wrong first.

### Wrong (14)

| ID | Part | Summary |
|---|---|---|
| LW-1 | [LW](skills-audit-lead-wfa.md) | `cpd-lead` SKILL.md:231 says step 6; draft forms are step 7. |
| LW-2 | [LW](skills-audit-lead-wfa.md) | SKILL.md:231 says `(step 5)` for queued decided work; it is step 7. |
| LW-3 | [LW](skills-audit-lead-wfa.md) | SKILL.md:319 says step 6; it is step 7. |
| LW-4 | [LW](skills-audit-lead-wfa.md) | SKILL.md:441 says step 6; it is step 7. |
| LW-5 | [LW](skills-audit-lead-wfa.md) | SKILL.md:290 says step 4; edit and cancel are step 5. |
| LW-6 | [LW](skills-audit-lead-wfa.md) | SKILL.md:321 says step 4; dropping a draft is step 5. |
| LA-1 | [LA](skills-audit-lead-axioms.md) | Same wrong step 6 and `(step 5)` references at :231, :319, :441. |
| DL-1 | [DL](skills-audit-drift-lead.md) | SKILL.md:44 claims `agent_forbidden` for commands that the docs refuse that way only for the six archive commands. |
| DL-2 | [DL](skills-audit-drift-lead.md) | Stale steps 6 and 5 at :231 and :441. |
| DL-3 | [DL](skills-audit-drift-lead.md) | Stale step 4 at :290 and :321. |
| DM-1 | [DM](skills-audit-drift-manager.md) | M7 row: the claim is refuted at the baseline, not a defect (see M7). |
| XS-1 | [XS](skills-audit-cross-skill.md) | `cpd-lead` SKILL.md:47 names Plan id, `<plan>.<prefix>` and UUID as addresses; commands.md:5164 says no skill does. |
| XS-2 | [XS](skills-audit-cross-skill.md) | Manager line 134 omits `failed` from the `draft_parked` needs list (docs/commands.md:205). |
| WU-3 | [WU](skills-audit-wfa-upstream.md) | SKILL-MECHANICS.md:9 says typing the name invokes a skill; pi uses `/skill:name`. |

### Conflict (16)

| ID | Part | Summary |
|---|---|---|
| LW-7 | [LW](skills-audit-lead-wfa.md) | SKILL.md:435 "opens only when you do" contradicts Rule :45 (cpd opens PRs). |
| LW-8 | [LW](skills-audit-lead-wfa.md) | spec-files.md:70 repeats that clause. |
| LW-9 | [LW](skills-audit-lead-wfa.md) | `sleep 10; tail` at :115 against `Never poll, sleep` at :57. |
| LW-10 | [LW](skills-audit-lead-wfa.md) | `gh pr edit --base` at :409 against the `gh` read-only rules at :42, :204. |
| LW-11 | [LW](skills-audit-lead-wfa.md) | :281 lists a draft that needs a queued Job as legal; :58 and :290 call it `draft_parked`. |
| LW-12 | [LW](skills-audit-lead-wfa.md) | spec-files.md:99 sends research to a Worker as an ordinary Job; SKILL.md:142 uses explore Jobs. |
| LA-2 | [LA](skills-audit-lead-axioms.md) | :208 says commit the policy file in the Lead worktree; :40 says it lives outside the repo. |
| LA-3 | [LA](skills-audit-lead-axioms.md) | Same draft-form conflict as LW-11. |
| LA-4 | [LA](skills-audit-lead-axioms.md) | Same research-Job conflict as LW-12. |
| LA-5 | [LA](skills-audit-lead-axioms.md) | pr-description.md:97 Good example names no registered follow-up key, against :26 and :41. |
| MG-12 | [MG](skills-audit-manager.md) | Manager line 45 bans `cpd context ask`; lines 85 and 93 present it as a record to write. |
| MG-14 | [MG](skills-audit-manager.md) | Bug protocol stated as prose (lines 53-58) against "Prose is not enforcement". |
| XS-3 | [XS](skills-audit-cross-skill.md) | Lead :241 says the Manager handles sequencing; Manager :122 says it never sequences across Leads. |
| XS-4 | [XS](skills-audit-cross-skill.md) | Bug protocol step 3 differs between the skills (Workflow / Job vs Plan / Workflow / Job). |
| WU-1 | [WU](skills-audit-wfa-upstream.md) | Pruning advice (delete) against axiom "check the fence". |
| WU-2 | [WU](skills-audit-wfa-upstream.md) | Positive-phrasing advice against the axioms' own "Never" lines. |

## Bloat and style

Counts (computed above): `cpd-lead` 48 bloat and 10 style; `cpd-manager` 12 bloat and 5 style; `writing-for-agents` 0 bloat and 4 style. Total 60 bloat, 19 style over all rows.

- `cpd-lead` (the three most useful, by size of text removed):
  - LW-31 ([LW](skills-audit-lead-wfa.md)): a 2403-character Rules bullet at SKILL.md:58; move behind a reference.
  - LW-32 ([LW](skills-audit-lead-wfa.md)): a 1659-character shape paragraph at :206; disclose.
  - LW-17 ([LW](skills-audit-lead-wfa.md)): "only carries out a decision" is stated at seven places; keep one.
- `cpd-manager`:
  - MG-2 ([MG](skills-audit-manager.md)): the Plans steps sit under about 98 lines of preamble.
  - MG-3 ([MG](skills-audit-manager.md)): the `host` Manager section loaded on every run.
  - MG-5 ([MG](skills-audit-manager.md)): the `session send` syntax repeated at ten lines.
- `writing-for-agents` (no bloat rows; the three most useful style rows):
  - WU-4 ([WU](skills-audit-wfa-upstream.md)): a description is required by pi, so it cannot be dropped.
  - WU-5 ([WU](skills-audit-wfa-upstream.md)): "no description" does not stop the model; unverified.
  - WU-7 ([WU](skills-audit-wfa-upstream.md)): no written reason for copying rather than depending.

## Proposed fixes

Proposed only: nothing is made or registered. The User decides what becomes work.

- `cpd-lead`:
  - Fix the step references and delete the editing note: LW-1 to LW-6, LW-13, LA-1, DL-2, DL-3, DL-4.
  - Delete the "opens only when you do" clause in both files: LW-7, LW-8.
  - Reconcile `sleep`, `gh pr edit`, the draft forms, the policy-file commit and the research Job text with the Rules: LW-9 to LW-12, LA-2 to LA-4.
  - Correct the `agent_forbidden` attachment and the address list: DL-1, XS-1.
  - Cut duplicates and move branch-only text into `references/`: LW-14 to LW-35, LA-6 to LA-14, LW-36, LW-37.
  - Replace axiom restatements with a pointer: LA-17 to LA-24.
  - Small style fixes: LW-38 to LW-41, DL-6, DL-7, LA-25, LA-26.
- `cpd-manager`:
  - Move the Plans steps first and the host, MCP, secrets and bug-protocol text into `references/`: MG-2 to MG-4.
  - Keep one `session send` syntax and one turn-start message: MG-5 to MG-7, MG-13, MG-15.
  - Drop decision citations, Lead-only commands and the forbidden-command list: MG-8 to MG-10.
  - Reorder step 4: MG-11. Resolve the `context ask` ban: MG-12. Description: MG-1. MG-14, MG-16 as listed.
  - Add `failed` to the needs list and drop `instead of cpd job dispatch`: XS-2, XS-10.
  - M7: no fix needed; the claim is refuted (DM-1).
- Both skills (shared text):
  - One source for the shared blocks, and one bug-protocol text: XS-4 to XS-9.
  - Reconcile the sequencing role: XS-3. Clean up the duplicated blank line: XS-11. Draft-parking and `register planned work` wording: XS-5.
- `writing-for-agents` (local notes or upstream issues; the directory is not edited): WU-1 to WU-7.

## Part files

- [LW: cpd-lead, contradictions and writing-for-agents fit](skills-audit-lead-wfa.md)
- [LA: cpd-lead against the axioms](skills-audit-lead-axioms.md)
- [MG: cpd-manager](skills-audit-manager.md)
- [DL: cpd-lead drift](skills-audit-drift-lead.md)
- [DM: cpd-manager drift and M7](skills-audit-drift-manager.md)
- [XS: cpd-lead against cpd-manager](skills-audit-cross-skill.md)
- [WU: bundled writing-for-agents](skills-audit-wfa-upstream.md)
