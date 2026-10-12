# Job `lead-skill`: the `cpd-lead` skill (Lead protocol)

Tier S4 · Needs: `scaffold` · Read `_common.md` first. Inputs:
- `docs/requirements.md`, whole document;
- `CONTEXT.md`;
- `docs/cli-contract.md` §10, §11 and §16;
- `docs/trials/t5-questions-escalation.md` (Recipes "Questions to the User");
- `docs/trials/t4-plan-approval.md` (Verdict, Finding 6 rendering limits).

## Goal
Write the skill that turns a pi session into a CPD **Lead**. It must make the Lead follow the User's workflow exactly, using only `cpd` commands, `ask_user_question` and `cpd_plan_review`.

## Files you own
`skills/cpd-lead/SKILL.md`, `skills/cpd-lead/references/*.md` (optional, for long reference tables), `test/skill/skill.test.ts`.

## Requirements
- Skill frontmatter: `name: cpd-lead`, plus a description that triggers when the User starts or continues a CPD Workflow.
- **Role.** The Lead translates the User's intent into Specs and orchestrates. It never writes product code. It delegates research to Workers (`cpd` Jobs) when grounding needs more than reading. It stays free for the User.
- **Context.**
  - Start: `cpd workflow init`, then `cpd lead attach`.
  - Ground in the material the User gives.
  - Receive the Requirements, then interview with `ask_user_question`, following the t5 rules (≤4 questions × 2–4 options, descriptions, previews short) until unknowns are minimised.
  - Write accepted assumptions down and confirm them with the User.
- **Plan.**
  - `cpd workflow phase plan`, then draft Jobs with `cpd job add`, using spec files with goal, context, touches, acceptance and outOfScope.
  - Sizing rule: ~13 min and ~150 turns per Job, with the 30 min timeout.
  - Jobs that touch the same files must be ordered by a Dependency. Prefer parallel, small Jobs.
  - Tier choice guidance: User examples are S1 coding, S4 review, H2 docs; O only when judgement-heavy.
  - Every Spec complete and guess-free.
  - Then `cpd plan render` and `cpd_plan_review`, iterating on feedback.
  - After approval, `cpd plan lock`. Never lock without approval.
- **Dispatch.** What to do for each queue message kind in contract §10:
  - `job-merged`: note it, and tell the User briefly if they are waiting.
  - `attempt-retry`: no action.
  - `job-failed`: decide between `cpd job retry --tier`, `cpd job cancel` plus an appended replacement Job, or escalating to the User.
  - `escalation`: answer with `cpd answer` if the Spec, Requirements or context cover it; otherwise ask the User (`ask_user_question` when the options are clear), then answer.
  - `review-needed`: `cpd review decide`.
  - `all-merged`: `cpd pr open`.
  - `pr-feedback`: append Jobs for failing checks and bot comments.
  - `workflow-done`.
  - **Append-only rules:** never edit after lock; history is never deleted.
  - Use `cpd workflow show` and `cpd attempt read` for debugging, never raw herdr commands toward Workers.
- **PR.** `cpd pr open`; the User's reviewer bot reviews it; feedback and CI failures become appended Jobs; `cpd pr rebase` only on conflicts; done when the PR is merged.
- **Escalation buffer.** A Worker escalates to the Lead, and only the Lead escalates to the User. Resolve first, ask second.
- **Style:** imperative, concise, tables for message handling. Command lines must be exact and match contract §11 (including `--wf`).

## Acceptance
1. `test/skill/skill.test.ts` parses the skill:
   - the frontmatter is valid;
   - every `cpd …` command line in the skill (in code spans or blocks) names a command present in `src/cli.ts`'s table, with flags from contract §11 (keep a flag allowlist in the test, taken from the contract);
   - every message kind of §10 is handled in the skill.
2. The report includes a short walkthrough of a hypothetical ticket going through all phases with the skill's exact commands, to show completeness.

## Out of scope
Code; the pi adapter; templates.
