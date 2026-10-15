# Job `lead-everywhere`: every session the User starts is a Lead

Tier S2 · Needs: none (appended 2026-10-13 at the User's request: "starting the lead should be simpler, all sessions I start are leads") · Contract sections: §11 (`lead start`), §12 steps 5 and 9 (Worker and Reviewer argv), §15 (pi adapter), §16 (skill).

## Goal
Make the cpd Lead set (the cpd pi adapter, the `cpd-lead` skill, plannotator, rpiv-ask-user-question) installable once as global pi packages, so any pi session the User starts inside herdr is a Lead with no special command. Sessions that cpd starts itself (Workers and Reviewers) must not get Lead powers.

## Files you own
`pi/package.json`, `pi/extension.ts`, `pi/lib/*.ts`, `pi/skills/**` (the skill moves here), `skills/**` (removed after the move), `src/herdr/agents.ts`, `src/dispatch/attempt-run.ts` (only the Worker and Reviewer argv), `src/commands/lead-start.ts`, `test/skill/skill.test.ts`, `test/pi/**`, `test/herdr/agents.test.ts`, `test/dispatch/attempt-run.test.ts` (only argv assertions), `README.md`, `docs/cli-contract.md`.

## Requirements
- **Skill in the package.** Move `skills/cpd-lead/` to `pi/skills/cpd-lead/` (`git mv`, references included). Make the package expose it: a conventional `skills/` directory or `"skills"` under the `pi` key of `pi/package.json`; check pi 1.2 `docs/packages.md`. Then `-e <repo>/pi`, or the package in settings, loads extension and skill together. Update `test/skill/skill.test.ts` and every path that names the old location.
- **No env required.** The adapter must work in a plain `pi` session: `CPD_HOME` defaults to `~/.cpd` as in the CLI. Verify, and add a test if missing.
- **Workers and Reviewers are not Leads.** cpd starts them with `--no-skills` and `--exclude-tools` for every Lead-only tool:
  - `ask_user_question` (rpiv);
  - `cpd_plan_review` (cpd);
  - any tool `npm:@plannotator/pi-extension@0.28.17` registers (inspect the installed package and list the names in your report).

  Reviewers keep `--tools read,grep,find,ls` as today. Put the flags in the argv builders; the argv tests pin them.
- **`cpd lead start`** keeps working and needs nothing extra for the skill.
- **`README.md`** (repo root; create it if missing) gets a short "Use cpd" section:
  - prerequisites: dotfiles (`~/projects/cpd`, `cpd` on PATH, `~/.cpd/config.json`);
  - the one-time global install: add these entries to `packages` in `~/.pi/agent/settings.json`:
    - `"../../projects/cpd/pi"` (a path relative to the settings file, so it works for any home directory; the two Macs have different `$HOME`s);
    - `"npm:@plannotator/pi-extension@0.28.17"`;
    - `"npm:@juicesharp/rpiv-ask-user-question@2.13.8"`;
  - "start any pi session in a herdr pane; it is a Lead".

## Acceptance
1. Argv tests: the Worker argv contains `--no-skills` and the exact `--exclude-tools` list, and the Reviewer argv contains them too, next to its `--tools` allowlist.
2. A test proves the pi package exposes the skill: it lists `pi/skills/cpd-lead/SKILL.md` through the manifest or the conventional directory. Record a `pi -e ./pi --list-skills`-style transcript if pi offers one, otherwise the closest proof.
3. Adapter test: no `CPD_HOME` set means the queue dir is under `~/.cpd`.
4. `./verify.sh` or `npm run verify` passes (the Gate is now `npm run verify`).

## Out of scope
Editing the User's pi settings (the Lead does it after merge); `--fork` for `cpd lead start` (not needed: with the global install a fork is just `pi --fork <session>`).
