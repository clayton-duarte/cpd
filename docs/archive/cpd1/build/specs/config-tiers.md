# Job `config-tiers`: config, paths, locks, slots, Tiers, `doctor`, `tiers`

Tier S3 · Needs: `scaffold` · Read `_common.md` first. Contract sections: §4, §5, §7, §9, §11 (`doctor`, `tiers`). Trial evidence: `docs/trials/t6-tier-catalog.md`.

## Goal
The foundation every module uses: validated config, state paths, file locks, machine-wide concurrency slots, Tier parsing, validation and ladder, and the two diagnostic commands.

## Files you own
`src/lib/config.ts`, `src/lib/paths.ts`, `src/lib/lock.ts`, `src/lib/slots.ts`, `src/lib/tiers.ts`, `src/lib/pi-install.ts`, `src/commands/doctor.ts`, `src/commands/tiers.ts`, `test/lib/{config,paths,lock,slots,tiers}.test.ts`, `test/commands/{doctor,tiers}.test.ts`.

## Requirements
- `config.ts`: zod schema exactly as §5 (strict, unknown keys rejected); `loadConfig()` reads `$CPD_HOME/config.json` (`CPD_HOME` default `~/.cpd`). A missing file gives `config_missing`, invalid content gives `config_invalid` with the zod issues in details.
- `paths.ts`: pure functions for every path in §4 (`cpdHome()`, `workflowDir(wf)`, `ledgerDir(wf)`, `leadQueueDir(sessionId)`, `specPath(wf, attemptId)`, …) and the id validators of §3 (`assertWorkflowId`, `assertJobKey`, `attemptId(job, n)`, `attemptBranch(wf, attemptId)`, `leadBranch(wf)`, `agentName(wf, attemptId, role)`).
- `lock.ts`: `withLock(path, purpose, fn, {timeoutMs=60000})` per §4: exclusive create, `{pid, at, purpose}`, stale (dead pid) takeover logged to stderr, exit 5 `lock_timeout`, always released (also on throw).
- `slots.ts`: `acquireSlot(concurrency, {wf, attempt})` returns the slot index or null, and `releaseSlot(index, attempt)`, per §9, stale slots reclaimed. The holder must be checked on release (never release a slot held by another attempt).
- `pi-install.ts`: locate the installed pi release exactly as §7 (realpath of `pi` on `PATH` → agent dir → `install/current-version` → release dir). Fail loud if any step fails (`pi_not_found`).
- `tiers.ts`:
  - `parseTier("S1")`, `formatTier`, `levelForEffort`.
  - `loadCatalog(config)`: dynamic `import()` of the release's `@earendil-works/pi-coding-agent` `ModelRuntime.create({allowModelNetwork: false})`, and `getSupportedThinkingLevels` from the same release's `@earendil-works/pi-ai`, resolving both through `import.meta.resolve`, or file paths under the release's `node_modules`. Do not copy the function.
  - `validTier(catalog, tier)`, `piModelArg(catalog, tier)` returning `"<provider/id>:<level>"`.
  - `nextTier(catalog, tier)` per the §7 ladder.
  - `attemptTier(catalog, jobTier, n)`.
  - Unit tests use an injected fake catalog; one integration test uses the real installed pi (it is installed).
- `doctor`: all checks in §11, each with found and expected versions, parsed from each tool's `--version` output. Exit 1 if any fails.
- `tiers`: prints the catalog as in §11.

## Acceptance
1. Ladder table test, exact: `H1→H2`, `H2→H3`, `H3→S3`, `S0→S1`, `S4→S5`, `S5→O5`, `O1→O2`, `O4→O5`, `O5→none`. `attemptTier(S1, n=1..4)` = `S1, S2, S3, S4`. `H3` attempt 2 = `S3`.
2. Validation with the real catalog: `O0` invalid, `H4` invalid, `S0` valid; `piModelArg("S3")` = `github-copilot/claude-sonnet-5.5:high`.
3. Locks:
   - two concurrent `withLock` holders serialise (test with two child processes);
   - a lock file with a dead pid is taken over;
   - the timeout gives exit 5 `lock_timeout`.
4. Slots: with concurrency 2, a third acquire returns null; releasing frees it; a stale slot (dead pid) is reclaimed.
5. Config: missing gives `config_missing`, an unknown key gives `config_invalid`, and a valid file loads.
6. `cpd doctor` on this machine passes all checks (record the transcript in the report); `cpd tiers` prints the §7 table values.

## Out of scope
Ledger, git, herdr. Do not create `~/.cpd`; tests use `sandboxEnv()`.
