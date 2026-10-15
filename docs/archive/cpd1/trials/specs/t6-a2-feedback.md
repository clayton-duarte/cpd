# t6 Attempt 2: Reviewer feedback on Attempt 1

Attempt 1 report: `~/projects/cpd/docs/trials/t6-tier-catalog.a1.md` (read it; reuse what holds). Verdict: **rejected**, retry at Tier H3. Execute the original Spec `t6-tier-catalog.md` again, addressing every point below. Write the new report to the Spec's path (`t6-tier-catalog.md`).

1. **Candidate 1 was evaluated too narrowly.** Attempt 1 only tried `createModels()` from `@earendil-works/pi-ai` without providers. pi itself resolves models through the coding-agent layer:
   - `ModelRuntime` / model registry in `@earendil-works/pi-coding-agent`, which extensions see as `ctx.modelRegistry`;
   - `docs/sdk.md` says `createAgentSession()` creates a `ModelRuntime`;
   - `docs/models.md` and `docs/custom-provider.md` may describe how the registry is built.

   Determine whether a documented SDK path (e.g. constructing the same `ModelRuntime` pi uses, or another exported registry API) yields the catalog **with the user's real providers and `thinkingLevelMap`** without starting a session or making network calls. Show code and output. This is the "supported interface" the Spec asks for; reading `models-store.json` is only the fallback.
2. **Stability claims need evidence.** "Format stable across 4 releases" is not shown: the store is one shared file. Either show evidence (e.g. compare how 0.99.1 and 1.1.0 read it, or what writes/refreshes it, such as `pi update` and model catalog refresh, with source references) or label it unverified.
3. **Locating pi must honour pi's own overrides.** pi supports `PI_CODING_AGENT_DIR` (and possibly other env vars; check `docs/environment-variables.md`). The generator must resolve the agent dir the same way pi does, not only `~/.pi/agent`.
4. **Facts in the report must be accurate.** Attempt 1's header says "Executed: 2025-01-13"; the real date is 2026-10-12. Use `date` output.
5. Keep the generator constraints: Node 22, no dependencies outside the pi install, loud failure for unknown models, `minimal` excluded, JSON output shape unchanged.
