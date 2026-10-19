# Research: scoping a policy to a repo or GitHub owner

Job `research` of Plan `repo-policy` (attempt research-a1), run on hs. The question: how cpd can make sure a policy (instructions an agent receives, or cpd's own behaviour) applies only to its repo or GitHub owner (user `clayton-duarte`, orgs `missionlane` and `missionlane-scratch`), so that a Manager, Lead or Worker working on a repo gets that repo's policies and never another's, by construction (hs-d261, hs-d262, hs-d263).

Scope rules kept by this research:

- No missionlane or missionlane-scratch repo was read, cloned or queried (no `git`, `gh` or `gh api` call named one). Every example is the cpd repo or a `~/projects` personal repo.
- `~/.pi/agent/AGENTS.md` was read, not changed. No option below edits it, moves content out of it or depends on editing it (A6).
- Nothing here is a decision. Section 5 gives options and section 6 a recommendation (A5).

Conventions in this file:

- `$PI` is `/home/clayton/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent`, the pi 1.3.0 install on hs. Install paths, the pi version and the installed skills are **unverified on pro** (A3).
- `path:line` for cpd is this worktree, based on commit `14cc223`, unless marked "installed". The installed adapter `~/.local/share/cpd/pi` is **not** identical to this worktree's `pi/`: it equals `origin/main` (`bba1765`, 7 commits later; checked with `git archive origin/main pi | diff -r`), which adds the hs-d239 reminder (`lib/reminder.ts`) and hs-d239 lines in both skills. Both are listed where they differ.
- Record ids (`hs-d220` and so on) exist only in hs's database (hs-d262).

## 1. Inventory

Every place a policy reaches an agent today, or where cpd's own code applies one.

### 1.1 pi's user-level resources (every pi on the machine)

| # | Item | Where | What it carries |
|---|---|---|---|
| I1 | Global context file | `~/.pi/agent/AGENTS.md:1-77` (`ls -la ~/.pi/agent`) | The User's axioms. Loaded for every pi run that does not pass `--no-context-files` (section 3). |
| I2 | Global skills | `ls ~/.pi/agent/skills` → only `.gitkeep`; `ls ~/.agents` → absent | Nothing. |
| I3 | Global prompts | `ls ~/.pi/agent/prompts` → only `.gitkeep` | Nothing. |
| I4 | Global extensions | `ls ~/.pi/agent/extensions` → `cpd-aux-agents.ts`, `herdr-agent-state.ts`, `notify.ts` | Compaction model and trigger, herdr state, notifications. `grep -n "before_agent_start\|systemPrompt"` finds no hit: none injects instructions. |
| I5 | `SYSTEM.md` / `APPEND_SYSTEM.md` in the agent dir | `ls ~/.pi/agent` → neither exists | Nothing. |
| I6 | Global pi packages | `~/.pi/agent/settings.json` `packages` (plannotator, rpiv-ask-user-question, ghostty theme) | plannotator's `before_agent_start` (`~/.pi/agent/npm/node_modules/@plannotator/pi-extension/index.ts:2124`) adds plan-mode framing only in its planning/executing phase; rpiv's (`reconcile.ts:46`) fixes the tool list. Neither carries repo policy. |
| I7 | Ancestor context files | `ls ~/AGENTS.md ~/CLAUDE.md ~/projects/AGENTS.md ~/projects/CLAUDE.md /home/AGENTS.md /AGENTS.md` → none exists | Nothing today; but pi would load any of them into every repo below (section 3.2). |

### 1.2 The repos' own files

| # | Item | Where | What it carries |
|---|---|---|---|
| I8 | Repo context file and repo skills | `ls -a` of the cpd repo and `~/projects/julias-portfolio-*`: no `AGENTS.md`, `CLAUDE.md`, `.pi/` or `.agents/`. `ls -a ~/dotfiles`: `AGENTS.md` (195 lines, "Instructions for AI agents working in this repo") and `.agents/skills/` (six skills) | cpd and the portfolio repos: nothing. dotfiles: its own instructions and skills, which reach its Workers and any Lead started inside it (section 3.2). |
| I9 | Repo hooks: setup, quick check, Gate | `.config/wt.toml:1-10` (contract §7.3) | `pre-start` (`npm ci`), `pre-commit` (typecheck, cargo build), `pre-merge` (`npm run verify`). Repo-owned. |
| I10 | Repo protected list | `.config/cpd.toml` (absent in cpd; contract §7.5) | Read from the Lead Branch's tree, `core/src/git/tamper.rs:16-26`. |
| I11 | Repo CI | `.github/workflows/ci.yml` | What `pr-feedback` reacts to (I27). Repo-owned. |

### 1.3 The cpd adapter package (shared: cpd itself)

| # | Item | Where | What it carries |
|---|---|---|---|
| I12 | `cpd-lead` skill | `pi/skills/cpd-lead/SKILL.md` (267 lines; installed 276) | The Lead's whole flow. Reaches every Lead, and every Worker as a skill description (I15). |
| I13 | `cpd-lead` references | `pi/skills/cpd-lead/references/spec-files.md`, `references/questions.md` | Spec format, sizing, Tiers, bounded commands; dialog style. |
| I14 | `cpd-manager` skill | `pi/skills/cpd-manager/SKILL.md` (88 lines) | The Manager's flow. |
| I15 | How the skills reach pi | `pi/package.json` `"pi": {"extensions": ["./extension.ts"], "skills": ["./skills"]}`; passed as `-e <adapter>` (`core/src/host/pi.rs:263-271`) | Both skills load for Manager, Lead **and Worker**: a `-e` package's skills load even under the Worker's `--no-skills` (section 3.3). This Worker's own system prompt lists `cpd-lead` and `cpd-manager` at `~/.local/share/cpd/pi/skills/…`. |
| I16 | Adapter entry | `pi/extension.ts:1-7` | Calls `adapter(pi)`; no text. |
| I17 | Adapter Event delivery | `pi/lib/adapter.ts:377-416`, `pi/lib/delivery.ts:21-29` | Events as `cpd` custom messages; text is the Event's own text (I27 for `pr-feedback`). |
| I18 | Manager turn-start Context update | `pi/lib/adapter.ts:421-456` (`before_agent_start`, Manager only, hs-d209 in the comment at :422), `pi/lib/updates.ts:5` | New records, Plan Events and pending work of the Manager's Context. |
| I19 | Lead turn-end reminder (installed only) | installed `lib/adapter.ts:490-499`, `lib/reminder.ts:8` | Hidden message `cpd reminder (hs-d239): …` added to the Lead's next run. |
| I20 | Adapter comments citing hs-d209 | `pi/lib/adapter.ts:402`, `:422` | JSDoc comments; never sent to a model. |

### 1.4 What cpd writes into a Session's first message, prompt and env

| # | Item | Where | What it carries |
|---|---|---|---|
| I21 | Manager's first message | `core/src/context/manager.rs:144-148` → `run/<ctx>/manager-message.md` | `You are the Manager of Context … Follow the cpd-manager skill.` + the Context (I24). |
| I22 | Lead's first message and brief | `core/src/plan/new.rs:241-258` → `run/<plan>/lead-message.md` | Header `You are the Lead of Plan … follow the cpd-lead skill.`, the brief (free text written by the Manager or User, `--brief-file`), then the Context (I24). |
| I23 | Worker's prompt and Spec | `core/src/host/pi.rs:225-245` (`-- "Read <spec> and follow it exactly…"`); Spec template `core/src/spec/worker.md:1-129`, rendered by `core/src/spec/mod.rs:174-237` | Fixed rules (lines 9-36), the quick-check commands from `.config/wt.toml` (13-22), protected paths (23-26, `spec/mod.rs:26`), then the Lead's free-text `goal`, `context`, `touches`, `acceptance`, `out_of_scope` (38-74). |
| I24 | Context records and digest | contract §2.2 "Context record" (`docs/contract.md:161-176`), §4.6; `core/src/context/digest.rs:175-210`; distiller prompt `core/src/context/distill_prompt.md:1-12` | Free-text records of the whole Context (all its Plans, all their repos), or the digest plus newer records. A Context spans repos: `plan_repos` lists each Plan's PR repos (`core/src/context/mod.rs:407-431`). |
| I25 | pi command line and env | `core/src/host/pi.rs:169-176` (`CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `PI_SKIP_VERSION_CHECK`), `:202-222` (Lead/Manager plannotator vars), `:244` (`CPD_ATTEMPT`), `:227` (Worker `--approve --no-skills --exclude-tools …`), `:254` (distiller `--no-context-files …`) | No repo, owner or policy is passed today. |

### 1.5 cpd's own behaviour that encodes a policy

| # | Item | Where | What it does |
|---|---|---|---|
| B1 | Branch names | `core/src/pr/add.rs:239-240` (`cpd/<plan>/<key>`), `core/src/git/worktree.rs:16-23` (`cpd/<plan>`, `cpd/<plan>--<attempt>`); contract §7.1 (hs-d43) | Fixed names; a repo with a branch-name convention cannot change them. |
| B2 | PR title | `core/src/pr/open.rs:189-192`; contract §7.6 `pr open` step 3 | Plan title, or `<title> (<key>)`; the Plan's Jira key is not used. |
| B3 | PR body | `core/src/pr/open.rs:199-210`, `core/src/pr/gh.rs:205` | `# <title>`, a Jobs table, `Generated by cpd`; `gh pr create --body` replaces any repo PR template. |
| B4 | `pr open` | `core/src/pr/open.rs:44-51`; contract §7.6 | Gate, `git push -u`, `gh pr create` (ready, not draft). |
| B5 | `pr rebase` | `core/src/pr/rebase.rs:60-61`; contract §7.6 | Rebases the Lead Branch and pushes `--force-with-lease`: the one push that is **not** plain. |
| B6 | `pr push` | `core/src/pr/push.rs:31`; contract §7.6 | Gate, then a plain push. |
| B7 | Plain pushes elsewhere | `core/src/git/worktree.rs:175-186` (Lead Branch at start and merge, contract §7.2, §7.4 step 8) | `git push [-u] origin <branch>`, never forced. |
| B8 | `pr-feedback` rules | `core/src/pr/pulls.rs:312-331` (`attention`), `core/src/pr/poll.rs:181-190`, text `core/src/pr/feedback.rs:33-39`; contract §7.6 `pr poll` steps 2 and 6 | Feedback when the rollup of **all** checks becomes `failed`, the review decision `changes_requested`, the PR `closed`, or `conflicting`. Comments without a review decision, approvals and pending checks raise nothing. |
| B9 | The poller | `core/src/pr/gh.rs:273-276` (`author:@me`, `review-requested:@me` searches); contract §7.6 `pr poll` step 1 | One search over every repo of the active `gh` account; no owner filter. |
| B10 | Gate and quick check | contract §7.3, `.config/wt.toml` | Run only from the repo's own `.config/wt.toml`; a missing file or table means **no** check and no setup (contract §7.3, "A missing file or a missing table means no commands"). |
| B11 | Tamper check | `core/src/git/tamper.rs:13-26`, `core/src/spec/mod.rs:26`; contract §7.5 | Always protects `.config/wt.toml`, `.config/cpd.toml`, `.cpd/`, plus the repo's list. |
| B12 | Machine config | `~/.cpd/config.toml` (contract §8; `core/src/config/file.rs:5` `deny_unknown_fields`) | Machine-wide only: Tiers, timeouts, `lead_pi_packages`, `pi_adapter`. No per-repo or per-owner key. |
| B13 | Lead conflict handling (in skill text, not code) | `pi/skills/cpd-lead/SKILL.md:170`, `:265` (hs-d220) | The Lead merges `origin/<base>` in the Lead worktree, runs no local tests, pushes plainly. |
| B14 | Who reviews (in skill text) | `pi/skills/cpd-lead/SKILL.md:242`, `:260` | "The User's reviewer bot or colleagues review each PR on GitHub while CI runs." |

## 2. Classification

Classes: **M** = cpd mechanics, neutral, keep. **P** = a personal-repo or building-cpd choice presented as general. **O** = owner-dependent behaviour cpd will need to branch on.

### 2.1 Inventory items

| Item | Class | Why |
|---|---|---|
| I1 global `AGENTS.md` | M (the User's, read only, A6) | Axioms, no repo-specific rule (read in full, `~/.pi/agent/AGENTS.md:1-77`). Stays for every role. |
| I2-I6 global skills, prompts, extensions, packages | M | No instruction content (commands in 1.1). |
| I7 ancestor context files | M today, risk | None exists; any `AGENTS.md` placed in `~/projects` or `~` would reach every repo below it (section 3.2), so it can never hold one owner's policy. |
| I8 repo `AGENTS.md` | M (convention) | The repo's own instructions; the right home when the repo can hold them. Today reaches Workers only (section 3.6). |
| I9-I11 repo hooks, protected list, CI | M (convention, repo-owned) | Already per repo by construction: read from the PR's own tree (contract §7.3, §7.5). Gap: a repo that cannot hold `.config/wt.toml` gets no setup, no quick check and no Gate (B10) — **O**. |
| I12 `cpd-lead` skill, in general | M with P lines | Mostly cpd's flow; the P/O lines are in 2.2. |
| I13 `spec-files.md:37` "90 on the User's machines, hs-d11" | P | A machine-config value cited in a shared skill; wrong on a machine with another `attempt_minutes` (pro: unverified). |
| I13 `spec-files.md` Tier table "The User's default" `S1` | P (User preference, not repo) | Personal, but not per repo or owner; out of this research's key. |
| I13 `spec-files.md` research path `docs/research/<topic>.md` | P (mild) | The cpd repo's convention (hs-d4) shown as the example path. |
| I13 `questions.md:5` (hs-n48) | M | How to talk to the User; not repo-specific. Cites an hs id. |
| I14 `cpd-manager` skill | M | Context and Plan mechanics; installed `:84` cites hs-d239 (2.2). |
| I15 skills reach Workers | M, risk | Any owner text left in the shared skills reaches Workers too. |
| I16-I18, I20 adapter | M | Delivery mechanics; I20 is a code comment. |
| I19 reminder (installed) | M | Registering planned work is cpd mechanics; its text names hs-d239. |
| I21-I22 first messages | M | Fixed headers. The brief is free text: a channel, not a policy. |
| I23 Worker Spec template | M | Rules 9-36 are cpd mechanics; quick check and protected paths come from the repo. The `context` field is free text the Lead fills: whatever the Lead holds can be copied into a Worker. |
| I24 Context records, digest | M as mechanism, **channel for mixing** | A Context spans repos (`docs/contract.md:149`: "The knowledge several Plans share"); its records reach the Manager and every Lead of the Context. A work decision recorded in a Context that also holds a personal Plan reaches that Plan's Lead. |
| I25 command line and env | M | No policy today; the natural place to carry the key. |
| B1 branch names | O | Fixed `cpd/…` names; a repo may require another pattern (guess: common in org repos; no work repo was read). |
| B2 PR title | O | A repo or org may require a title format (for example a ticket key; the Plan already stores `jira`, `docs/contract.md:178-192`). |
| B3 PR body | O | `--body` bypasses a repo's PR template. |
| B4 `pr open` ready, not draft | O | A repo whose CI deploys or pings reviewers on open may want draft first (guess). |
| B5 `pr rebase` force-with-lease | O, and contradicts the skill | The skill says "plain, never forced" (`SKILL.md:170`, `:265`), but `pr rebase` force-pushes with a lease. On a PR with human review threads a force push rewrites what reviewers saw (guess). |
| B6, B7 plain pushes | M | Safe on any repo. |
| B8 `pr-feedback` rules | O | "Any check failed" counts every check in the rollup, required or not, deploys included; a review bot that comments without a review decision raises nothing. A repo with many checks, peer reviews, an external agent review and deploys will likely want other triggers (hs-d263 names this case). |
| B9 the poller | O | One `gh` account's `author:@me`; if work and personal PRs use different `gh` accounts, only the active one is seen (contract §7.6 step 3 already mentions "the User switched `gh` accounts"). Unverified on pro. |
| B10 Gate and quick check | M; O when the repo has no `.config/wt.toml` | See I9-I11. |
| B11 tamper check | M | Repo-owned list. |
| B12 machine config | M | Machine-wide by design; adding owner keys is a schema change (`deny_unknown_fields`). |
| B13 hs-d220 conflict handling | P and O | See 2.2. |
| B14 who reviews | P and O | Describes the personal repos' review (the User's bot); a repo with peer reviewers and an external agent review differs. |

### 2.2 The known lines

| Line | Text (short) | Class | Reasoning |
|---|---|---|---|
| `pi/skills/cpd-lead/SKILL.md:170` (installed `:179`) | `pr-feedback` `conflict`: "resolve it yourself per hs-d220 … merge origin/<base> … git push (plain, never forced); run no local tests: the PR's CI is the check" | **P** and **O** | hs-d220 is a building-cpd decision written as a rule for every repo. "The PR's CI is the check" assumes the repo has CI worth trusting; "merge, not rebase" and "push plainly" are a repo's history policy. On pro the id `hs-d220` resolves to nothing (hs-d262). |
| `pi/skills/cpd-lead/SKILL.md:265` (installed `:274`) | `rebase_conflict`: the same hs-d220 procedure | **P** and **O** | Same as above. |
| `pi/skills/cpd-lead/SKILL.md:58` (installed `:67`) | "a derived summary that cites records by id, such as `[hs-d43]`" | **M** (format example), P in form | Explains the digest's citation format; harmless, but the example id is hs's and means nothing on pro. A machine-neutral example (`[<machine>-d43]`) would carry the same meaning. |
| `pi/lib/adapter.ts:402` (installed `:409`), `:422` (`:429`) | JSDoc "(hs-d209, D3)" / "(hs-d209, D1 …)" | **M** | Code comments; never sent to a model. Keep. |

### 2.3 Full grep of `hs-*` ids, `clayton-duarte` and `missionlane`

Command: `git grep -nE "hs-[dnqa][0-9]"` (and `-cE` for counts), `git grep -nE "clayton-duarte|missionlane"`, both outside `target/` and `node_modules/`.

**Hits that reach an agent** (skills, adapter text, prompts, cpd output):

| Hit | Reaches | Class |
|---|---|---|
| `pi/skills/cpd-lead/SKILL.md:58` `[hs-d43]` | Lead, Worker (skill) | M, example id |
| `pi/skills/cpd-lead/SKILL.md:170` hs-d220 | Lead, Worker (skill) | P and O |
| `pi/skills/cpd-lead/SKILL.md:265` hs-d220 | Lead, Worker (skill) | P and O |
| `pi/skills/cpd-lead/references/questions.md:5` hs-n48 | Lead | M (User dialog style) |
| `pi/skills/cpd-lead/references/spec-files.md:37` hs-d11 | Lead | P (hs config value) |
| `pi/skills/cpd-lead/references/spec-files.md:44` hs-n97, hs-n107 | Lead, then copied into Specs | M (bounded commands) |
| `core/src/context/distill_prompt.md:10` `[hs-d43]`, `[hs-d43, hs-n7]` | the distiller | M, example ids |
| installed only: `skills/cpd-lead/SKILL.md:38`, `:277`; `skills/cpd-manager/SKILL.md:84` hs-d239 | Lead, Manager, Worker (skill) | M (register planned work), hs id |
| installed only: `lib/reminder.ts:8` `REMINDER_TEXT` "cpd reminder (hs-d239)" | Lead | M, hs id |

**Hits that do not reach an agent** (counts only, as asked):

| Where | Lines | Files | Note |
|---|---|---|---|
| `pi/lib` | 2 | 1 | `adapter.ts:402`, `:422`, comments (installed adds `adapter.ts:498` `details: {rule: "hs-d239"}`, metadata not shown to the model, and `reminder.ts:1` comment) |
| `core/src` | 147 | 36 | 90 in `#[cfg(test)]` modules; of the 67 before them, all are comments or SQL comments except `distill_prompt.md:10-11` (above) and `store/migrations/0006.sql:68` (an SQL column comment) |
| `cli/tests` | 90 | 20 | tests |
| `pi/test` | 28 | 5 | tests |
| `pi/skills/skill.test.ts` | 2 | 1 | tests |
| `tui/src` | 10 | 8 | comments or tests |
| `scripts` | 2 | 2 | shell comments |
| `docs/research` | 592 | 17 | research |
| `docs/contract.md`, `docs/commands.md`, `docs/implementing.md`, `docs/adr` | 116, 81, 4, 1 | 4 | docs |
| `CONTEXT.md`, `BACKLOG.md` | 7, 2 | 2 | glossary and backlog; not auto-loaded (not context files) |

`clayton-duarte`: 8 hits, all in `docs/archive/cpd1/` (3) and `docs/research/` (5); none reaches an agent. `missionlane`: 0 hits.

## 3. How pi loads instructions per role

Every fact below cites pi 1.3.0's docs or dist source under `$PI` (unverified on pro).

### 3.1 Each role's cwd

| Role | cwd | Set by | Observed on hs (`sqlite3 -readonly ~/.cpd/cpd.db "select role, cwd, count(*) from sessions group by role, cwd"`) |
|---|---|---|---|
| Manager | the directory `cpd context manager` ran from | `core/src/context/manager.rs:139-143` (`std::env::current_dir()…canonicalize()`), stored in `sessions.cwd`; pi started with `.current_dir(&row.cwd)` at `core/src/session/run.rs:197`; contract `docs/contract.md:501`, `:1629` ("usually `~`") | both Managers: `/home/clayton/.cpd` (guess: the cockpit that ran `context manager` runs in `$CPD_HOME`) |
| Lead | the directory `cpd plan new` ran from, never a repo by design | `core/src/plan/new.rs:57-61` (`caller_cwd`), `:87`, `:263`; pi started at `core/src/session/run.rs:197`; tmux pane `core/src/plan/lead.rs:59`; contract `docs/contract.md:188`, `:1628` ("a Plan may span several repos, so its Lead starts in none of them") | `/home/clayton` ×14, `/home/clayton/projects/cpd` ×13, `/home/clayton/dotfiles` ×4. This Plan's Lead (`repo-policy`): `/home/clayton` |
| Worker | its Attempt worktree, `<repo-parent>/<repo-name>.<plan>--<attempt>` | `core/src/git/worktree.rs:56-58`; `core/src/driver/worker.rs:285` (`launch.spawn(&target.worktree)`); contract `docs/contract.md:1630`, §7.1 | e.g. `/home/clayton/projects/cpd.repo-policy--research-a1` (this Worker) |

So the Manager and Lead cwd is whatever the caller's was, and can be a repo by accident (13 Leads ran in `~/projects/cpd`). A Worker's cwd is always inside its PR's repo checkout.

### 3.2 Which context files pi loads

- Global: `<agent-dir>/AGENTS.override.md`, `AGENTS.md`, `AGENTS.MD`, `CLAUDE.md` or `CLAUDE.MD`, "User instructions applied across working directories" (`$PI/docs/configuration.md:18`); code: `loadProjectContextFiles` loads `loadContextFileFromDir(agentDir)` first (`$PI/dist/core/resource-loader.js:165-172`).
- cwd and **every** parent up to the filesystem root, not stopping at the git root: "Pi loads them from the agent directory, the working directory, and its parent directories" (`$PI/docs/configuration.md:43`); code: the `while (true)` loop that walks `dirname(currentDir)` until `parentDir === currentDir` (`$PI/dist/core/resource-loader.js:177-193`).
- One file per directory, first of `AGENTS.override.md`, `AGENTS.md`, `AGENTS.MD`, `CLAUDE.md`, `CLAUDE.MD` (`$PI/dist/core/resource-loader.js:115-116`; `$PI/docs/configuration.md:45`).
- A nested linked worktree's copy shadows the main repo's (`$PI/dist/core/resource-loader.js:135-164`); cpd's worktrees are siblings, so this does not apply (contract §7.1).
- Context files load regardless of project trust (`$PI/docs/configuration.md:47`, `$PI/docs/security.md:57`); `-nc`/`--no-context-files` disables them all, the global one included (`$PI/docs/cli.md:214-215`; `$PI/dist/core/resource-loader.js:465-470`).

Whether a repo's `AGENTS.md` reaches each role today:

| Role | Repo `AGENTS.md` | Why |
|---|---|---|
| Manager | **No** | cwd `~/.cpd` or `~` is outside every repo. |
| Lead | **No**, by design; **yes by accident** when `plan new` ran inside a repo (then that one repo's file, whichever PRs the Plan has) | cwd is the caller's (3.1). Evidence: 4 Leads on hs ran in `/home/clayton/dotfiles`, which has an `AGENTS.md` and `.agents/skills` (I8); pi loaded both for them (Leads have no `--no-skills`), whatever repos their Plans touched. |
| Worker | **Yes**: its PR's repo's committed `AGENTS.md` (the worktree is a checkout), plus any in `~/projects`, `~`, `/home`, `/` | cwd is the Attempt worktree; ancestors are walked to `/`. |

Of the personal repos checked, only `~/dotfiles` has an `AGENTS.md` (I8); cpd has none, so for cpd Plans this is the mechanism, not current content.

### 3.3 Skills

- At startup pi adds each skill's name, description and path to the system prompt; the full text is read on demand (`$PI/docs/skills.md:43-45`).
- Discovered from the user dir `~/.pi/agent/skills`, project `.pi/skills` (both `$PI/dist/core/package-manager.js:2013-2021`), `~/.agents/skills` (`:2023`, `:2057-2064`) and project `.agents/skills` from cwd up to the git root (`$PI/docs/skills.md:61`; `$PI/dist/core/package-manager.js:291-305`, stops at `gitRepoRoot`). Project `.pi/skills` need project trust (`$PI/dist/core/package-manager.js:2035-2040`).
- Packages declare skills in `package.json` `pi.skills`; the cpd adapter declares `./skills` (`pi/package.json`).
- `-ns`/`--no-skills` "Disables discovered and configured skills. Explicit `--skill` paths still load" (`$PI/docs/cli.md:202-203`). In code, a `-e` package's skills are "cli" skills and survive it: `skillPaths = this.noSkills ? this.mergePaths(cliEnabledSkills, this.additionalSkillPaths) : …` (`$PI/dist/core/resource-loader.js:406`, `:424-426`). So Workers (`--no-skills`, `core/src/host/pi.rs:227`) still get `cpd-lead` and `cpd-manager`; they lose only user and project skills.
- Scoping a skill to a cwd or repo: only by placing it in the repo (`.pi/skills`, `.agents/skills`), which follows cwd. There is no per-skill path filter in the docs read. An extension can add skill paths per cwd through `resources_discover` (`$PI/dist/core/extensions/types.d.ts:551-561`: event has `cwd`, result has `skillPaths`), and `before_agent_start` receives mutable `systemPromptOptions.skills` (`$PI/dist/core/extensions/types.d.ts:720-726`, `$PI/dist/core/system-prompt.d.ts:34-35`, `:49`), so it can drop skills from a run (unverified at runtime).

### 3.4 Extension hooks that inject or replace instructions

- `before_agent_start` gets `systemPrompt` and mutable `systemPromptOptions`; it may return a `message` added to the run, or a whole `systemPrompt` replacement (`$PI/docs/extensions.md:104`; `$PI/dist/core/extensions/types.d.ts:720-726`, `:1125-1129`).
- `systemPromptOptions` holds `contextFiles: {path, content}[]`, `skills`, `appendSystemPrompt`, `promptGuidelines` and `sections` ("Additional XML-wrapped prompt sections keyed by tag name") (`$PI/dist/core/system-prompt.d.ts:20-35`, `:39-50`). So an extension can add a context file or a `<repo-policy>` section per run, or remove one, without replacing the prompt.
- `resources_discover` (after `session_start`, again on reload) adds skill, prompt and theme paths (`$PI/dist/core/extensions/types.d.ts:551-561`).
- `project_trust`: "User-level and command-line extensions can handle the `project_trust` event. The first extension that returns yes or no owns the decision" (`$PI/docs/security.md:63`).
- Command line: `--append-system-prompt <text|path>`, repeatable (`$PI/docs/cli.md:231-232`); `--system-prompt` replaces (`:229-230`). **Passing `--append-system-prompt` suppresses the discovered `APPEND_SYSTEM.md`** (project or agent dir): `let appendSources = this.appendSystemPromptSource; if (!appendSources) { …discoverAppendSystemPromptFile()… }` (`$PI/dist/core/resource-loader.js:480-484`; flag parsed at `$PI/dist/cli/args.js:76-77`).
- Project `.pi/APPEND_SYSTEM.md` and `.pi/SYSTEM.md` load only with project trust (`$PI/docs/configuration.md:32-39`; `$PI/dist/core/resource-loader.js:955-958`).
- The cpd adapter already uses `before_agent_start` for the Manager's Context update (`pi/lib/adapter.ts:452`) and, installed, the Lead's reminder (installed `lib/adapter.ts:494`).

### 3.5 Project trust per role

- Worker: `--approve` (`core/src/host/pi.rs:227`), which "Trusts project-local configuration and resources for this process" (`$PI/docs/cli.md:237-238`). A repo's `.pi/extensions`, `.pi/APPEND_SYSTEM.md` and `.pi/settings.json` packages therefore load into its Workers (skills excepted, `--no-skills`).
- Lead and Manager: no flag; pi falls back to `defaultProjectTrust` (`$PI/docs/security.md:65`), which hs sets to `"always"` (`~/.pi/agent/settings.json`; unverified on pro). Their cwd is normally no repo, so nothing project-level loads.

### 3.6 What cpd passes

| Role | argv (from `core/src/host/pi.rs`) | env |
|---|---|---|
| Manager | `pi --model … [-e <adapter>] [-e <lead_pi_packages>…] --session-id … -n <ctx>-manager -- @<manager-message.md>` (`:179-199`) | `CPD_HOME`, `CPD_SESSION`, `PLANNOTATOR_UPDATE_CHECK=0`, `PI_SKIP_VERSION_CHECK=1`; no `CPD_PLAN` (`:202-210`) |
| Lead | same shape, `-- @<lead-message.md>`; resume: `--session <file>` (`:179-199`) | `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `PLANNOTATOR_DATA_DIR`, `PLANNOTATOR_UPDATE_CHECK`, `PLANNOTATOR_PORT`/`REMOTE` if set, `PI_SKIP_VERSION_CHECK` (`:169-176`, `:211-222`) |
| Worker | `pi --approve --no-skills --exclude-tools <lead tools> [-e <adapter>] --model … [--fork …] --session-id … -n … -- "Read <spec> and follow it exactly. Your attempt id is <a>."` (`:225-242`) | base env + `CPD_ATTEMPT` (`:243-244`) |
| Distiller | `pi --print --no-session --no-extensions --no-skills --no-context-files --no-tools …` (`:252-259`) | `PI_SKIP_VERSION_CHECK` only |

`-e` is skipped for a package the global or project settings already declare (contract `docs/contract.md:1658`; `extensions_to_pass` reads `<cwd>/.pi/settings.json`, `core/src/session/run.rs:177`). Nothing names the repo or owner.

## 4. Keying

### 4.1 How cpd can know a repo's owner

| Source | What it gives | Reliability |
|---|---|---|
| `prs.repo` | The realpath of the repo's main worktree (`core/src/pr/add.rs:57`, `:264-271`); `CHECK (length(repo) >= 1)` only (`.schema prs`) | A path, not an owner. Values on hs: `/home/clayton/projects/cpd`, `/home/clayton/dotfiles` (`select distinct repo from prs`). |
| `pull_requests.repo` | GitHub's `owner/name`, `CHECK (repo LIKE '_%/_%')` (`.schema pull_requests`); written at `pr open` from `gh pr create`'s URL (`core/src/pr/pulls.rs:333-340`) or by the poll | Exact, but exists only **after** `pr open`: too late to choose the Lead's or a Worker's instructions. |
| `git -C <repo> remote get-url origin` | e.g. `git@github.com:clayton-duarte/cpd.git` (cpd, `~/dotfiles`), `https://github.com/clayton-duarte/julias-portfolio-vite.git` | Local, no network, already the remote cpd pushes to (`core/src/git/run.rs:87` `has_origin`). Needs parsing of both URL forms. An SSH host alias (`git@github-work:org/repo.git`) or `url.insteadOf` would hide the host (guess: common when one machine holds two GitHub identities). hs has neither: the `Host` entries of the dotfiles SSH config (`~/dotfiles/home/private_dot_ssh/private_config:7`, `:27`, `:31`, `:35`) are `homeserver`, `mac-pro`, `mac-air`, `*`, and `git config --global --get-regexp '^url\.'` prints nothing. Unverified on pro. No `origin` (allowed, contract §7.2 "A repo with no origin remote") means no owner. |
| `gh repo view --json owner,isInOrganization` (cwd = repo) | `{"owner":{"login":"clayton-duarte"},"isInOrganization":false}` for cpd | Canonical (resolves aliases through `gh`'s remote resolution), but needs network and the right `gh` account; a renamed or transferred repo returns the new owner. |

The key belongs on the PR at `pr add` (the boundary where `--repo` is parsed), not on the machine (A3), and not deferred to `pr open`.

### 4.2 A Plan touching several repos

R34 lets one Plan hold PRs in several repos (contract §2.2 PR, §7 intro). Its one Lead plans and dispatches every PR, so a Plan whose PRs span owners gives one agent both owners' work: mixing cannot be prevented inside that Lead, only by not allowing the state. Two shapes:

- **Cross-repo, same owner** (cpd + dotfiles, both `clayton-duarte`): no conflict; the owner's policy plus each repo's own files.
- **Cross-owner** (a personal repo and a work repo in one Plan): either refused (one owner per Plan, an invariant) or the Lead gets both (mixing). hs-d261 rules out the second.

Today the Plan has no repo at creation (`docs/contract.md:1454`: "no `repo` or `base`"), so its owner is unknown when its Lead starts. Only a key set **before** the Lead starts (on the Context, or on `plan new`) lets cpd choose the Lead's instructions at launch.

### 4.3 What a Manager should receive

A Context spans Plans and so repos (I24), and its records reach the Manager and every Lead. If a Context may hold both owners, its records and digest carry both, and no delivery rule can fix that afterwards. So the Manager should receive exactly the policy of its Context's one owner, which requires a Context to have one owner; or, if a Context may span owners, only neutral cpd mechanics and no owner policy at all (and then its records still mix).

## 5. Options

Each option covers instructions and behaviour. All three share two steps that any mechanism needs:

- **S1, key at the boundary:** parse the owner once at `pr add` from `origin` (both URL forms, `github.com` only), store it on the PR, and refuse what cannot be keyed (no `origin`, unparseable URL) with a typed error rather than a default.
- **S2, neutral shared skills:** remove owner choices from `cpd-lead` (the hs-d220 lines 170 and 265, "the User's reviewer bot" 242 and 260, "90 on the User's machines" in `spec-files.md:37`) and replace hs ids with neutral examples; add a `skill.test.ts` guard that fails on `hs-[dnqa][0-9]`, `clayton-duarte` or `missionlane` in `pi/skills/**` (prose is not enforcement). hs-d220 then moves into a policy of the owner (or repo) it was decided for.

### Option A: repo-only, by cwd

**Where policy lives:** only in each repo: `AGENTS.md` (instructions) and new typed keys in `.config/cpd.toml` (behaviour: feedback triggers, PR body, branch prefix, conflict procedure).

**Delivery:** pi's own context-file loading. Workers already get it (3.2). Leads: cpd starts a single-repo Plan's Lead with cwd = its Lead worktree (or adds that repo's `AGENTS.md` through the adapter's `before_agent_start` `contextFiles`). Manager: nothing.

**Branching:** cpd reads the PR's own `.config/cpd.toml` from the Lead Branch, as the tamper check does.

| | |
|---|---|
| Mixing prevented by construction | Partly. A Worker sees only its worktree's repo. A Lead of a cross-owner Plan has no single repo; refusing cross-repo Plans undoes R34. Context records still mix (4.3). Ancestor `AGENTS.md` in `~/projects` would reach every repo. |
| Work repo that cannot hold a cpd file | Nothing works: no `AGENTS.md` of the User's, no `.config/cpd.toml`, and no `.config/wt.toml` means no setup, quick check or Gate (B10). Fails the brief's case. |
| Smallest build | S2; new `.config/cpd.toml` keys; Lead cwd or adapter injection of the repo's `AGENTS.md`. |
| Risks | A team repo's `AGENTS.md` is the team's, not the User's; the User's per-owner rules (how the Lead treats peer reviews) have no home. The Manager gets no policy. |

### Option B: a per-owner profile in `$CPD_HOME`, keyed and enforced in the database (typed)

**Where policy lives:** `$CPD_HOME/owners/<owner>/` (per instance, like `config.toml`, which "is managed by the User's dotfiles", `docs/contract.md:1884`):

- `AGENTS.md`: the owner's instructions for every role (for `clayton-duarte`: hs-d220's conflict procedure, "the User's review bot reviews");
- `profile.toml`: typed behaviour, `deny_unknown_fields` like `config.toml`, holding only keys there is evidence for (hs-d248), for example `pr_feedback.checks = "any" | "required"`, `pr.body = "cpd" | "repo-template"`, `pr.title = "plan" | "jira-prefixed"`, `conflict = "merge-base" | "rebase"`; plus a per-repo fallback for repos that cannot hold `.config/wt.toml` (`[repos."<owner>/<name>"] pre-start/pre-commit/pre-merge`).

A repo's own files win: `.config/wt.toml`, `.config/cpd.toml` and the repo's `AGENTS.md` (Workers, via cwd) are used when present; the profile supplies only what the repo cannot hold.

**Key and invariants (the "by construction" part):**

- `contexts.owner` set at `context new --owner <login>` (required for new Contexts; existing ones migrated from their PRs' `origin`, all `clayton-duarte` on hs).
- S1 stores `prs.owner`; a trigger refuses a PR whose owner differs from its Context's (`owner_mismatch`). So a Context, its Manager, its Plans' Leads and their Workers have exactly one owner, and Context records cannot mix either.
- A missing `owners/<owner>/` directory is a typed error at `context new` and at launch (`owner_unknown`), never an empty fallback.

**Delivery:** cpd resolves the owner from the Session's Context and passes the file at launch, for Manager, Lead and Worker alike. Two ways, same key:

- the adapter adds `owners/<owner>/AGENTS.md` as a context file in `before_agent_start` (`systemPromptOptions.contextFiles`, 3.4), the owner and path read once from `cpd session show` at `session_start`; keeps a repo's `.pi/APPEND_SYSTEM.md` working; or
- `--append-system-prompt <file>` on the pi command line (3.4): simplest, but suppresses any `APPEND_SYSTEM.md` (`resource-loader.js:480-484`).

**Branching:** a typed `OwnerProfile` loaded with the PR's owner where the code branches today: `pulls::attention` (B8), `pr open` title and body (B2, B3), and the skill's conflict procedure moves from skill text into the owner's `AGENTS.md` (or, later, into a command if evidence shows it should be enforced).

| | |
|---|---|
| Mixing prevented by construction | Yes for every role: one owner per Context is a database invariant; launch reads the file from that owner's directory only; skills are neutral and guarded by a test (S2). Remaining channel: free text the User or Manager writes about another owner, which no mechanism can stop. |
| Work repo that cannot hold a cpd file | Its owner's `AGENTS.md` for instructions, its `profile.toml` for behaviour, and the per-repo hook fallback for setup, quick check and Gate. Nothing in the repo. |
| Smallest build | S1 + S2; migration: `contexts.owner`, `prs.owner`, trigger; `context new --owner`; `owners/<owner>/AGENTS.md` delivered to the three roles (adapter `contextFiles`, or `--append-system-prompt`); `owners/clayton-duarte/AGENTS.md` takes hs-d220's lines. `profile.toml` starts **empty** and gains one key per observed divergence (the first work Plan's `pr-feedback` is the likely first). |
| Risks | The profile must exist on each machine: hs-d262 says instances share nothing but cpd, so the files travel with the User's dotfiles or by hand (where the work profile may live is open question Q2). A Plan that truly needs a personal and a work repo must be split across Contexts. Owner from `origin` can be hidden by SSH aliases; S1 must handle or refuse them. |

### Option C: per-owner policy delivered by the adapter, behaviour through owner hooks

**Where policy lives:** `$CPD_HOME/owners/<owner>/` as in B, but behaviour is executable hooks rather than a typed profile: `hooks/pr-body`, `hooks/pr-title`, `hooks/feedback` (stdin: the PR row, stdout: JSON), and optional owner skills in `skills/`.

**Delivery:** entirely in the adapter, per run: `resources_discover` adds `owners/<owner>/skills` (3.3), `before_agent_start` adds the `AGENTS.md` as a `<repo-policy>` section or context file, keyed by the owner of the repo the Session works on. For a Lead whose Plan spans owners, the adapter injects per PR key (`<repo-policy pr="…">`), so cross-owner Plans stay allowed.

| | |
|---|---|
| Mixing prevented by construction | No for Leads and Managers of cross-owner Plans and Contexts: one agent holds both owners' text, separated only by labels (prose). Yes for Workers. |
| Work repo that cannot hold a cpd file | Owner hooks and files as in B. |
| Smallest build | S1 + S2; the adapter's two handlers; a hook runner in `pr open` and `pr poll`; a `cpd policy show --session` command for the adapter. |
| Risks | Hooks are arbitrary scripts with no types (illegal states representable); per-run injection depends on pi's extension API (pi-version churn, adapter only, unverified on pro); allowing cross-owner Plans contradicts hs-d261. |

### Comparison

| | A repo-only | B owner profile, DB-enforced | C adapter + hooks |
|---|---|---|---|
| Worker gets only its repo's/owner's policy | yes | yes | yes |
| Lead gets only one owner's | only single-repo Plans | yes (invariant) | no for cross-owner Plans |
| Manager gets only one owner's | gets none | yes (invariant) | no for cross-owner Contexts |
| Context records cannot mix | no | yes | no |
| Repo that cannot hold cpd files | fails | profile + per-repo hook fallback | hooks |
| Behaviour typed | yes (repo keys) | yes (`OwnerProfile`) | no (scripts) |
| Global `AGENTS.md` untouched | yes | yes | yes |
| Repo's own files win | yes | yes | yes |

## 6. Recommendation and open questions

**Recommendation: Option B**, built in this order, each step on evidence:

1. S2: neutral skills plus the grep guard in `pi/skills/skill.test.ts`; hs-d220's procedure and "the User's review bot" move to `owners/clayton-duarte/AGENTS.md`.
2. S1 and the one-owner-per-Context invariant (`contexts.owner`, `prs.owner`, trigger, `context new --owner`, `owner_unknown` / `owner_mismatch`).
3. Delivery of `owners/<owner>/AGENTS.md` to Manager, Lead and Worker through the adapter's `before_agent_start` `contextFiles` (keeps a repo's `.pi/APPEND_SYSTEM.md`; `--append-system-prompt` is the smaller fallback if that trade-off is accepted).
4. `profile.toml` with no keys; the first key lands when a work PR shows a divergence (likely `pr-feedback` triggers, B8), each with its reason (convention over configuration).

Why: it is the only option where mixing is impossible for all three roles and for Context records, it keeps the repo's own files first, it needs nothing in a work repo, and it leaves the global `AGENTS.md` alone. Its cost is one invariant (no cross-owner Plan or Context) that hs-d261 already implies.

Open questions for the User:

- **Q1.** Is "one owner per Context (so per Plan)" acceptable? A change that needs a personal and a work repo would be two Plans in two Contexts.
- **Q2.** Where does the work owners' policy file live on pro? hs-d262 says instances share only cpd; `config.toml` is managed by the personal dotfiles (`docs/contract.md:1884`; `~/dotfiles/home/dot_cpd/create_config.toml.tmpl` creates it on every machine, and `~/dotfiles/home/.chezmoidata/pi.yaml` pins pi `1.3.0` for all three). May work policy text sit in the personal dotfiles repo (a `clayton-duarte` repo), or must it be written on pro only?
- **Q3.** Are `missionlane` and `missionlane-scratch` one policy (one profile, two keys) or two?
- **Q4.** hs-d220 (merge the base, no local tests, CI is the check): is it a `clayton-duarte` owner policy, or only the cpd repo's? If the cpd repo's, it belongs in the cpd repo's own `AGENTS.md` and must also reach the Lead (whose cwd is not the repo).
- **Q5.** Should `pr rebase`'s `--force-with-lease` (B5) stay for every owner, given that the skill tells Leads to push "plain, never forced"?
- **Q6.** Do work and personal PRs use the same `gh` account on pro? If not, the poller (B9) sees only one, and the owner key also selects the `gh` account (unverified on pro).
- **Q7.** How should a repo with no `origin`, or with an SSH host alias, be keyed: refused, or keyed by an explicit `--owner` at `pr add`?
