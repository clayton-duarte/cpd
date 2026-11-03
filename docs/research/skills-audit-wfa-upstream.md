# Skills audit: bundled writing-for-agents (upstream copy)

## Coverage

- Baseline: `a954761dfe6e7b6316f955ada638e363c170af55` (`git merge-base HEAD origin/main`).
- Read in full: `pi/skills/writing-for-agents/SKILL.md` (1-81), `SKILL-MECHANICS.md` (1-22), `NOTICE` (1-8).
- Also listed: `ls -la pi/skills/writing-for-agents` shows LICENSE, NOTICE, SKILL-MECHANICS.md, SKILL.md. LICENSE not read (not named for reading).
- pi's `node_modules/@earendil-works/pi-coding-agent/docs/skills.md` was readable (grep of it and lines on invocation, frontmatter and discovery); no linked files were opened, and `settings.md` was not read.
- Not verified: byte-for-byte equality with upstream (no network, no upstream copy in the worktree).
- Findings here are advice for the User (local note or upstream issue); the directory is not edited.

## Findings

| ID | Lens | Where | Kind | Quote | Evidence | Proposed fix |
|---|---|---|---|---|---|---|
| WU-1 | 4 | pi/skills/writing-for-agents/SKILL.md:80 | conflict | Without a pruning discipline the default fate is **sediment**: stale layers that settle because adding feels safe and removing feels risky | Skill SKILL.md:81 "delete the whole sentence" and "Go find them" (:72) push deletion; axiom "Delete first, but check the fence": "never remove what you don't understand". The skill has no fence check; its no-op test is "model-relative" run by running the document. | Local note: apply pruning advice only after finding why a line exists; or upstream issue asking for a fence-check step. |
| WU-2 | 4 | pi/skills/writing-for-agents/SKILL.md:74 | conflict | A prohibition earns its place only as a hard guardrail you cannot phrase positively | Axioms are written with prohibitions, e.g. "Never vendor, copy, or fork what upstream owns" (Convention over configuration) and "Never burn tokens where a deterministic script". Skill advises positive phrasing and pairing a prohibition with a target. | Local note: for cpd skills and AGENTS.md, the axioms' "Never" lines stand; treat this advice as a default, not a rule. |
| WU-3 | 2 | pi/skills/writing-for-agents/SKILL-MECHANICS.md:9 | wrong | You can still type its name | pi docs skills.md:45,53: forced loading is `/skill:name`; `enableSkillCommands` controls whether skill commands appear in discovery (manual `/skill:name` still works). Typing the bare name does nothing. | Local note: in pi the human invokes `/skill:<name>`; upstream issue to say so. |
| WU-4 | 2 | pi/skills/writing-for-agents/SKILL-MECHANICS.md:10 | style | the `description` becomes human-facing: a one-line summary, trigger lists stripped | pi docs skills.md:85: "declared skills without descriptions are not loaded"; :74 `description` is "Routing description shown to the model". The description stays required in pi, so it cannot be dropped, only shortened. Whether pi still shows it to the model when `disable-model-invocation: true` is unverified (skills.md:79 says only "Hide the skill from automatic model selection"). | Local note: keep a valid description on every pi skill; check pi source before relying on "zero context load". |
| WU-5 | 2 | pi/skills/writing-for-agents/SKILL-MECHANICS.md:22 | style | user-invoked skills have no description, so nothing but the human can reach them | Contradicts pi docs skills.md:85 (a description is required; a skill without one is not loaded). Reach by the model by reading the file path is not shown to be blocked: unverified. | Local note: read as "no model-facing pointer"; do not rely on it as enforcement. |
| WU-6 | 4 | pi/skills/writing-for-agents/NOTICE:6 | style | What came in: SKILL.md and SKILL-MECHANICS.md, byte for byte. | `ls` shows LICENSE also present (NOTICE:1 names it, but the "What came in" list omits it). `agents/openai.yaml` absent: confirmed (`ls: .../agents: No such file or directory`). Axiom Explicit over implicit. | Local note: add LICENSE to the "What came in" list. |
| WU-7 | 4 | pi/skills/writing-for-agents/NOTICE:1 | style | is a copy of the skill of the same name of mattpocock/skills | Axiom "Convention over configuration": "Never vendor, copy, or fork what upstream owns — take the dependency". NOTICE gives a written reason only as "no use in pi" for the dropped file, none for copying vs depending. Whether a pi-package of this skill exists is unverified (no web). | Local note: record why the skill is copied rather than taken as a pinned `pi-package` dependency. |

## Considered, not reported

- Check (a), other claims in SKILL-MECHANICS.md (frontmatter `disable-model-invocation` name, model-invoked skills fire from description): match skills.md:53,79; nothing found.
- Check (b), NOTICE vs directory: four files present, `agents/openai.yaml` absent as stated; only WU-6 found. "Changes: none" is unverified without upstream.
- Check (c), "Prose is not enforcement": the skill is advice, not a rule set; no quotable conflict.
- Check (c), "Brevity is not vagueness": skill is long (81 lines, dense) but advice itself says shorter is better; style opinion, not reported.
- Lens 2: SKILL.md mentions no cpd command, flag, code, Event, phase or Tier; nothing to drift. Nothing found.
- SKILL.md:3 `description` is under 1024 characters and `name` matches the directory (skills.md:81): not individually measured; unverified.
