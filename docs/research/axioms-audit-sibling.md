# Axioms audit 1: dotfiles and pi-config prose

## Coverage

- `/home/clayton/dotfiles/AGENTS.md` lines 1-233, full read.
- `/home/clayton/dotfiles/README.md` lines 1-495, full read.
- `/home/clayton/dotfiles/SETUP.md` lines 1-407, full read.
- `/home/clayton/dotfiles/TMUX.md` lines 1-107, full read.
- `/home/clayton/dotfiles/AGENT_TOOLS.md` lines 1-52, full read.
- `/home/clayton/dotfiles/IDEAS.md` lines 1-494, full read.
- `/home/clayton/dotfiles/BACKLOG.md` lines 1-295, full read.
- `/home/clayton/dotfiles/docs/external-config-repos.md` lines 1-218, full read.
- `/home/clayton/dotfiles/.agents/AGENTS.md` lines 1-45, full read; `git -C /home/clayton/dotfiles log --oneline -- .agents/AGENTS.md` (3 auto-sync commits, last 2026-10-04); `rg -n 'agents/AGENTS' /home/clayton/dotfiles --glob '!.git'` and the same over `/home/clayton/.pi/agent` and this worktree: no reader found.
- `/home/clayton/dotfiles/.agents/skills/*/SKILL.md`, full reads: `prototype` 1-26, `grilling` 1-28, `setup-matt-pocock-skills` 1-116, `wayfinder` 1-128, `domain-modeling` 1-74, `research` 1-12.
- `/home/clayton/.pi/agent/README.md` lines 1-54, full read.
- `/home/clayton/.pi/agent/AGENTS.md` lines 1-77, full read. It holds exactly the 19 axioms 1-19, one `##` section each, with no other content and no duplicate: the 10 axioms 20-29 are not yet present, so there is nothing in it to report.
- `/home/clayton/.pi/agent/extensions/cpd-aux-agents.ts` 1-106, `herdr-agent-state.ts` 1-261, `notify.ts` 1-57, full reads (comments and strings). No axiom restated or contradicted.
- How: full reads of every named file, plus `rg -n -i` over the named files and skills on rule words (never, always, must, no, rule, policy, prose, enforce, idempotent, flake, leak, orphan, sleep, mock, worktree, checkout, upstream, vendor, supersede, recency, source of truth, assume, fail loud, silently, reversible, YAGNI, DRY, KISS) and reads around every hit.
- NOT read, and why: `dotfiles/docs/plans/**` and `dotfiles/docs/research/**` (excluded by the Spec); `dotfiles/.agents/skills/*/agents/*.md` and each skill's other reference files, e.g. `LOGIC.md`, `UI.md`, `ADR-FORMAT.md`, `CONTEXT-FORMAT.md`, `issue-tracker-*.md` (the Spec named only `SKILL.md`); `dotfiles/home/`, `scripts/`, `tests/`, `litellm/`, `.github/` and every other file not named; `pi/agent/settings.json`, `keybindings.json`, `skills/`, `prompts/`, `themes/`, `sessions/`, `install/`, `bin/`, `models-store.json` and pi's installed docs (not named).

## Findings

| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| /home/clayton/dotfiles/.agents/AGENTS.md:1 | DRY | restate | "# Conventions" | delete the file (45 lines; an ALL-CAPS copy of axioms 1-19 plus a 20th entry "CONFIRMATION BY RESEARCH OR EMPIRICAL CHECKING" that duplicates the axiom "Inference is not evidence"); no reader found: rg 'agents/AGENTS' over dotfiles, pi-config and this worktree has no hit, only `.agents/skills/` is referenced (BACKLOG.md:224); unverified whether an external agent framework auto-loads `.agents/AGENTS.md`, so confirm before deleting |
| /home/clayton/dotfiles/BACKLOG.md:29 | Prose is not enforcement | restate | "Prose is not enforcement." | replace line 29 with: "see the axiom \"Prose is not enforcement\"" |
| /home/clayton/dotfiles/BACKLOG.md:216 | Prose is not enforcement | restate | "Rules that must stick need mandatory routing or deterministic enforcement." | replace line 216 with: "see the axiom \"Prose is not enforcement\"" |
| /home/clayton/dotfiles/AGENTS.md:170 | No flakes, no leaks, no orphans, no sleep, no errors | contradict | "On Linux with chezmoi installed, the 2 tests in `tests/test_services_reentrancy.py` fail: they render a darwin template on Linux." | raise to the User: documents 2 pre-existing failing tests as acceptable, contrary to the zero-tolerance axiom |
| /home/clayton/dotfiles/AGENTS.md:166 | Only authored code is tested | borderline | "it asserts the rendered output matches the live file in `$HOME` for every managed target." | raise to the User: the parity suite reads the live $HOME instead of a fixture, which may be a real external dependency |
| /home/clayton/dotfiles/AGENTS.md:145 | A primary checkout is a mirror of origin/main | borderline | "### Work in a worktree" | raise to the User: the heading restates the worktree rule, but the section also carries a dotfiles-specific reason; keep or point to the axiom |
| /home/clayton/dotfiles/.agents/skills/wayfinder/SKILL.md:23 | DRY | borderline | "a decision lives in exactly one place, its ticket, so the map never restates it, only gists it and links." | raise to the User: is this the DRY axiom restated, or the map's own index rule |
| /home/clayton/dotfiles/.agents/skills/research/SKILL.md:10 | Inference is not evidence | borderline | "Follow every claim back to the source that owns it." | raise to the User: is this the source-reading axiom restated, or research method detail |
| /home/clayton/dotfiles/.agents/skills/prototype/SKILL.md:17 | Ask, don't assume | borderline | "and state the assumption at the top of the prototype." | raise to the User: is this the decide-and-state-the-assumption axiom restated, or prototype method detail |

## Counts
restate 3, contradict 1, borderline 5
