# Harness² (`arXiv:2610.14224v1`): Video, Paper, Case Studies & Appendix H Analysis

- **Date:** 2026-11-02 EST
- **Context:** `research` (records `pro-n1`, `pro-n2`)
- **Video:** *Google's Harness²: Is This the End of Harness Engineering?* (`https://www.youtube.com/watch?v=yCgfyQmqhPM`, `@kaiexplainsYT`, published 2026-11-01, 13m23s)
- **Paper:** *Harness²: Recursive Agent Harnessing for an Open World* (Jiefeng Chen, Ozan Sener, Sercan Ö. Arık, Jinsung Yoon, et al., Google Cloud AI Research, Oct 2026, `arXiv:2610.14224v1`)
- **Repository:** `https://github.com/google-research/harness2`

---

## 1. Video Thesis & Critique (`@kaiexplainsYT`)

### 1.1 Core Premise
Google Cloud AI Research proposes **Harness²**, a meta-harness (`H²`) that treats an agent's execution harness (`H`) — its system prompts, guardrails, memory files, tool guidance, subagents, skills, verification scripts, and tool-loop hooks — as an editable directory tree. Instead of humans hand-tuning prompts and middleware, an outer-loop coding agent iteratively edits the inner agent's harness across rounds using a three-stage loop:
1. **Propose:** Run $K=4$ parallel editor sessions (`--Editor-1`..`4`) that inspect prior task execution trajectories (scores, failure modes, tool-call traces) and propose targeted edits to the harness tree.
2. **Probe:** Deterministically validate the edited harness tree (`tree.validate()`), then evaluate each candidate harness on the task batch.
3. **Compose (`--Composer`):** If no single candidate dominates on every task, a Composer agent reads the per-task trajectory contrasts across all $K$ candidates and synthesizes a unified harness combining the complementary mechanisms.

### 1.2 Headline Benchmark Results & Methodological Caveats
- **Headline claim:** `+11.5` average point improvement across 7 open-world benchmarks (WorkBuddy, JobBench, Terminal-Bench 2.0, BrowseComp, PaperBench, ScienceAgentBench, DailyBench).
- **Caveat 1 — Intentionally blank base harness (`H⁰`):** The starting harness `H⁰` has **empty** system prompts, **0** skills, **0** scripts, **0** subagents, and **0** plugins (`opencode.json` only sets `"*": "allow"` and `bash` timeout `120000` ms). Much of the `+11.5` jump comes from going from *no harness at all* to *having basic verification rules*.
- **Caveat 2 — Cross-model coaching vs. self-improvement:** When `GLM-5` was improved by `+13.7` pts on WorkBuddy (`68.8 → 82.5`), `Claude Opus 4.8` was the outer-loop editor. When `GLM-5` edited its own harness (Table 7), it reached `80.8` vs `82.5` with `Opus 4.8` (`86.0` vs `89.3` on Research; `51.6` vs `55.6` on LAB).
- **Caveat 3 — Judge bias on open-ended rubrics:** On `WB-Web` (auto-generated rubrics graded by Gemini 3.1 Pro), `Opus 4.8` jumped `64.0 → 100.0`, while on human-written rubrics (`DailyBench` / `JobBench`), gains were smaller (`+5.8` to `+10.9`).
- **Caveat 4 — Cost & failure modes:**
  - Discovering a harness for a 30-task split takes ~150 agent rollouts ($4 \text{ rounds} \times 4 \text{ candidates}$), costing `$22` (GLM-5 on WorkBuddy) to `$261` (Opus 4.8 on Terminal-Bench).
  - **Self-confirming checker bug:** On `WB-Code` (`validator cache`), the editor wrote a verification script that shared the editor's own flawed assumption (keying a validator cache by Python `id()` instead of schema content); the checker printed `PASS` 3 times while the true score dropped **11 points (`75 → 64`)**.
  - **Blind selector regressions:** On single-task zero-rubric runs where a blind Selector agent picked among 4 trajectories without validation scores, the Selector picked trajectories that "sounded cleaner" while breaking edge cases, tanking `Code` (`96 → 55`) and `LAB` (`54 → 26`).

---

## 2. Editable Harness Surface (`C1`–`C8`) & Ablations

### 2.1 The 8 Editable Component Slots (`harness2/core/spec.py`, `opencode.py`, `codex.py`)

| Slot | Tier | OpenCode Path | Codex Path | Purpose & Constraints |
|---|---|---|---|---|
| **`C1`** | Always-on | `prompts/system.md` | `AGENTS.md` (`<!-- SECTION: system -->`) | Role, loop shape, communication style |
| **`C2`** | Always-on | `prompts/guardrails.md` | `AGENTS.md` (`<!-- SECTION: guardrails -->`) | Pre-exit completion checklist, stopping rules, safety invariants |
| **`C3`** | Always-on | `prompts/memory.md` | `AGENTS.md` (`<!-- SECTION: memory -->`) | Cross-task standing workspace facts |
| **`C4`** | Always-on | `prompts/tools.md` | `AGENTS.md` (`<!-- SECTION: tools -->`) | Tool selection and sequencing rules |
| **`C5`** | On-demand | `agents/<name>.md` | `.codex/agents/<name>.toml` | Isolated-context subagents (e.g. `research-verifier`, `visual-qa`) |
| **`C6`** | On-demand | `skills/<name>/SKILL.md` | `.agents/skills/<name>/SKILL.md` | Domain playbooks; **must** have a `When <situation>, load skill <name>` trigger line in `C1`–`C4` |
| **`C7`** | On-demand | `scripts/*` | `scripts/*` | Reusable executable CLI scripts (`verify_outputs.py`, etc.); **must** be referenced in `C1`–`C6` |
| **`C8`** | Middleware | `plugin/*.ts` | `.codex/hooks.json` + `scripts/*` | Tool-loop hooks (`tool.execute.before`/`after` or `PreToolUse`/`PostToolUse`/`Stop`) |

**Hard Always-On Caps (`spec.py`):**
- OpenCode: `always_on_max = 24,000` characters across `prompts/*.md` (`C1`–`C4`).
- Codex: `always_on_max = 8,000` characters across `AGENTS.md` (`C1`–`C4`).

### 2.2 Component Ablation Impact (Table 6 — WorkBuddy, GLM-5 + Opus 4.8 Editor)
Starting from the full discovered harness (`82.5` overall vs `68.8` base `H⁰`):
1. **Remove Skills (`C6`):** `82.5 → 77.2` (**`-5.3` pts**; Research drops `89.3 → 79.0`, `-10.3` pts)
2. **Remove Plugins/Hooks (`C8`):** `82.5 → 77.8` (**`-4.7` pts**; LAB drops `55.6 → 45.7`, `-9.9` pts)
3. **Remove Subagents (`C5`):** `82.5 → 80.1` (**`-2.4` pts**)
4. **Remove Scripts (`C7`):** `82.5 → 80.8` (**`-1.7` pts**)
5. **Remove Always-On Prompts (`C1`–`C4`):** `82.5 → 81.0` (**`-1.5` pts** — smallest drop; skills + hooks carry 3.5× more weight than always-on prose)

---

## 3. Concrete Plumbing Fixes Across the Six Case Studies (Figures 9, 15–19)

### 3.1 Case Study 1 — `WB-Research` (`51.9 → 88.2`, `+36.3` pts; Figure 15)
- **Failure modes in `H⁰`:** Agents answered in chat instead of writing the deliverable file to the workspace root, or misquoted numbers from source tables.
- **Discovered harness fixes:**
  - `C2 prompts/guardrails.md`:
    1. *"Every numerical claim, date, percentage, and entity name in the final deliverable must be cross-checked against the raw tool output before finishing."*
    2. *"Always write the final report to the exact file path requested in the workspace root; a chat response alone is never the deliverable."*
  - `C5 agents/fact-checker.md`: Isolated subagent that receives `(draft_path, source_paths)` and checks every number/citation line-by-line.

### 3.2 Case Study 2 — `JobBench Producers` (`43.1 → 77.8`, `+34.7` pts; Figure 9)
- **Failure modes in `H⁰`:** Document generation libraries (`python-docx`, `openpyxl`, `reportlab`, `matplotlib`) exit `0` even when charts have overlapping labels, tables overflow page margins, or formulas evaluate to `#REF!`.
- **Discovered harness fixes:**
  - `C2 prompts/guardrails.md`:
    > *"A successful write call does not establish usability. Before you finish, reopen every produced file with `scripts/verify_outputs.py` and render visual artefacts (PDF/DOCX/PPTX/charts) to PNG via LibreOffice + `pdftoppm` to inspect every page visually."*
  - `C7 scripts/verify_outputs.py`: Deterministic verifier that checks file non-emptiness, parses XML/ZIP structure for `.docx`/`.xlsx`/`.pptx`, checks zero `NaN`/`#REF!` cells, and converts pages to PNGs for visual inspection.

### 3.3 Case Study 3 — `WB-Code` (`72.4 → 84.5`, `+12.1` pts; Figure 16)
- **Failure modes in `H⁰`:** Fixing one bug broke adjacent callers or missed edge cases (empty inputs, Unicode, timezone boundaries).
- **Discovered harness fixes:**
  - `C6 skills/code-verification/SKILL.md`: Triggered whenever editing source code; mandates reproducing the bug with a minimal failing test first, running the existing test suite, and adding boundary tests before exiting.
  - **Documented failure case (`validator cache`, `-11` pts):** Editor wrote a custom `check_cache.py` script that tested cache hits by passing the *same* dict object twice (`id(schema)`), confirming the agent's buggy `id()` cache key 3 times while distinct equal schemas missed or collided in evaluation.

### 3.4 Case Study 4 — `WB-Web` (`64.0 → 100.0` on Opus 4.8; Figure 17)
- **Failure modes in `H⁰`:** Agents wrote HTML/CSS/JS files and exited without ever loading them in a browser; single uncaught `ReferenceError`s or broken flex layouts tanked scores.
- **Discovered harness fixes:**
  - `C2 prompts/guardrails.md`:
    > *"Never finish a web task without serving the app, opening it in headless Playwright, asserting zero `console.error` and `pageerror` events, exercising every interactive control, and taking screenshots at desktop (1280×800) and mobile (375×812) viewports."*
  - `C7 scripts/web_smoke_check.mjs`: Playwright script that boots a local server, attaches `page.on('console')` and `page.on('pageerror')`, clicks interactive elements, and dumps computed styles + screenshots.

### 3.5 Case Study 5 — `Terminal-Bench 2.0` (`60.2 → 77.7`, `+17.5` pts; Figure 18)
- **Failure modes in `H⁰`:** Agents edited files in `/tmp` or a build subdirectory instead of the canonical path expected by the test harness, left background daemons dead or port-bound, or left uncommitted changes in git repos.
- **Discovered harness fixes:**
  - `C2 prompts/guardrails.md`:
    1. *"Verify the exact path the task specification asks for (`ls -la <target>`, `git status`); never leave the fix only in a scratch directory."*
    2. *"Run verification in a fresh subshell (`env -i bash -lc ...`) so success does not depend on exported shell variables that vanish when the session ends."*
  - `C8 .codex/hooks.json` (`Stop` hook): Runs a pre-exit check script that warns if `git status --porcelain` has untracked/uncommitted edits or if expected target files are 0 bytes.

### 3.6 Case Study 6 — `JobBench Social-Science RAs` (`72.0 → 100.0`, `+28.0` pts; Figure 19)
- **Failure modes in `H⁰`:** Stronger reasoning models (`Opus 5`) spent their entire turn budget running exploratory Python scripts and reading CSV columns, hitting the turn cap with **zero bytes written** to the final report. Prose instructions in `prompts/system.md` ("start writing early") were ignored once the model got absorbed in analysis.
- **Discovered harness fix (`C8 plugin/deliverable_watchdog.ts`):**
  ```ts
  // OpenCode plugin hook: tool.execute.after
  let calls = 0;
  let wroteDeliverable = false;
  export default {
    "tool.execute.after": async (input, output) => {
      calls += 1;
      if (["write", "edit"].includes(input.tool) && !String(input.args?.filePath).startsWith("/tmp")) {
        wroteDeliverable = true;
      }
      if (!wroteDeliverable && calls >= 25 && calls % 5 === 0) {
        output.output += `\n\n[WATCHDOG WARNING] You have made ${calls} tool calls without writing any deliverable file. Stop exploring and write a complete skeleton draft of all required output files NOW, then refine.`;
      }
    }
  };
  ```
  - **Why it worked:** It never blocked the tool call; it injected an unskippable reminder directly into the tool result stream at the exact moment the agent was drifting.

---

## 4. Appendix H — The Hand-Written Meta-Harness (`harness2/core/prompts.py` & `tree.py`)

### 4.1 The Meta-Prompts (`harness2/core/prompts.py`, 1,297 LOC)

1. **`_editor_rules(spec)` (`prompts.py:84-175`) — The Constitutional Rules for Editing a Harness:**
   - **Rule 1 (`ALWAYS-ON PROSE IS THE EXPENSIVE OPTION`):**
     > *"Every word of `prompts/*.md` sits in context on every run, competing with the agent's own output budget. Keep always-on files under `always_on_max` chars (`24,000` OpenCode / `8,000` Codex). Prefer a SKILL (`C6`) for anything with a recognisable trigger. If you add always-on text, cut something else to pay for it."*
   - **Rule 2 (`PROSE CAN BE STOPPED READING; A HOOK CANNOT`):**
     > *"When a prior trajectory already had a prose rule and the agent ignored it under load, do NOT make the prose louder (ALL CAPS, 'CRITICAL'). Move the enforcement into a `C8` plugin/hook or a `C7` verification script."*
   - **Rule 3 (`ANTI-OVERFITTING / GENERALITY`):**
     > *"Never hardcode task IDs, file names, benchmark names, or specific answers from the training trajectories. Every rule, skill, script, and hook must be a general operational mechanism that transfers to unseen tasks of the same family."*
   - **Rule 4 (`NO SILENT CATCHES IN HOOKS`):**
     > *"A hook that throws an unhandled exception bricks every tool call in the rollout; wrap hook bodies in `try/catch` and log errors cleanly."*

2. **`propose()` (`prompts.py:420-665`):**
   - Feeds the editor: (a) the editable surface table (`C1`–`C8`), (b) the current best harness tree file listing, (c) the `experience.json` library (`confirmed` lessons + `confounded` anti-patterns), (d) a per-task score table across prior rounds, and (e) anAssigned Focus Lens across the $K=4$ parallel editors (e.g., Editor 1: verification & guardrails; Editor 2: skills & scripts; Editor 3: tool-loop hooks & budget; Editor 4: subagent decomposition).

3. **`compose()` (`prompts.py:667-840`):**
   - Triggered when Candidate A wins on Task $i$ and Candidate B wins on Task $j$.
   - Builds a **Behavioral Contrast Table** (`_behavioral_contrast()`), showing side-by-side where Candidate A's trajectory diverged from Candidate B's, and instructs the Composer to merge the non-conflicting mechanisms into a single harness tree inside `<reasoning>...</reasoning>`.

4. **`experience()` (`prompts.py:1098-1230`):**
   - After each round, compares each candidate's diff against its score delta and extracts structured entries into `experience.json`:
     - `<experience>`: `{ "cue": "<when this failure shape appears>", "lesson": "<checkable mechanism>", "evidence": "<task trajectory delta>", "confidence": "high|medium" }`
     - `<confounded>`: `{ "change": "<what was tried>", "why_confounded": "<why it failed or regressed>", "evidence": "<task score drop>" }`

5. **`select()` (`prompts.py:842-960`):**
   - Used in zero-rubric (test-time self-evolution) mode to pick the best trajectory among $K$ rollouts by checking artifact existence, completeness, and internal consistency.

---

### 4.2 Deterministic Pre-Rollout Validation (`harness2/core/tree.py`, 432 LOC)

Before **any** proposed harness tree is allowed to run a single benchmark task, `tree.validate(root, spec)` runs 7 deterministic static checks. If any check fails, the editor agent is immediately sent back to fix the harness before rollout tokens are spent:

1. **Always-On Character Budget (`_check_always_on_budget`):**
   - Sums character length of `C1`–`C4` (`prompts/*.md` or `AGENTS.md`). Refuses if total $> 24,000$ (OpenCode) or $> 8,000$ (Codex).
2. **No Unescaped Template Braces (`_check_no_jinja`):**
   - Rejects `{{` or `}}` in always-on prompt files so prompt-template engines never crash at runtime.
3. **Mandatory Skill Frontmatter & Situational Trigger Line (`_check_skills`):**
   - Every `skills/<name>/SKILL.md` must have valid YAML frontmatter (`name`, `description`) **and** `<name>` must appear verbatim in an explicit trigger line (`When <situation>, load skill <name>`) inside `C1`–`C4`. An untriggered skill is dead weight and is refused.
4. **Subagent Schema Check (`_check_agents`):**
   - Every `agents/<name>.md` (or `.codex/agents/<name>.toml`) must parse cleanly and declare valid tool permissions.
5. **Referenced Script Existence & Syntax Compilation (`_check_script_refs`):**
   - Scans all markdown/config files for `scripts/<name>` references. Every referenced script must exist on disk, and every `.py` (via `py_compile`), `.sh` (via `bash -n`), and `.js`/`.mjs`/`.ts` file under `scripts/` must syntax-check cleanly.
6. **Plugin / Hook Syntax & Event Name Validation (`_check_plugins` / `_check_codex_hooks`):**
   - Verifies that `plugin/*.ts` or `.codex/hooks.json` only registers valid hook events (`tool.execute.before`, `tool.execute.after`, `PreToolUse`, `PostToolUse`, `Stop`, `UserPromptSubmit`) and that every hook script exists and compiles.
7. **Anti-Leakage / Benchmark Hygiene Check (`_check_leakage`):**
   - Rejects any harness file containing forbidden evaluation terms (`rubric`, `grader`, `benchmark_score`, `ground_truth`) or hardcoded training task IDs.
