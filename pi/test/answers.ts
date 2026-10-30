import { PROTOCOL } from "../lib/event.ts";

/**
 * What the adapter parses at activation, one answer per call, keyed as `Fixture.respond` (`pi/test/fake.ts`) keys it: `<a>-<b>` of `cpd <a> <b>`, or `--version`.
 * The one source of the tests' fake `cpd` and of pi-check's fake (docs/implementing.md "Checking a pi version"): a field, a command or a protocol number the adapter
 * starts requiring is added here once. `node pi/test/answers.ts <cpd argv…>` prints the answer of an argv as JSON, `{"ok":true}` when there is none.
 */

export const ID = "596d007f-6775-4134-b77a-b356cc523842";

export const sessionShow = (over: Record<string, unknown> = {}) => ({
  ok: true,
  session: { id: ID, role: "lead", context: "ctx", plan: "p1", attempt: null, name: "the lead", state: "stopped", plan_lead: true, grounded_lead: false, registered_event: 100, ...over },
  open_escalations: [],
});

/** `sessionShow` with the default `loop_guard_turns` (commands.md §15.15): what a `session show` answers; `sessionShow` alone is the answer without it (a `bad_output`, loop-guard.test.ts). */
export const shown = (over: Record<string, unknown> = {}) => sessionShow({ loop_guard_turns: 20, ...over });

/** A `cpd policy show` output (commands.md §22.1): the domains of contract §8.3, each with its audience; `texts` sets a domain's text and `source`. */
export const policyShow = (role = "lead", texts: Record<string, string> = {}, source = "owner:/p/o.toml") => {
  const audience: Record<string, string[]> = {
    pr: ["lead", "worker"], deploy: ["lead"], code: ["lead", "worker"], tests: ["lead", "worker"], monitor: ["lead", "worker"], instructions: ["manager", "lead", "worker"],
  };
  const policy = Object.fromEntries(Object.entries(audience).map(([d, a]) => [d, { source: d in texts ? source : "default", audience: a, keys: {}, text: texts[d] ?? "" }]));
  return { ok: true, key: { owner: "o", repo: null }, role, files: [], policy };
};

export const answers = (): Record<string, unknown> => ({
  "session-show": shown(),
  "context-pending": { ok: true, context: "ctx", plans: [] }, // a Manager reads it at every turn start (§15.7)
  "policy-show": policyShow(), // no text anywhere: no section
  "--version": { ok: true, version: "0.0.0", schema: 6, store_schema: 6, protocol: PROTOCOL },
});

if (import.meta.main) {
  const argv = process.argv.slice(2);
  const key = argv[0] === "--version" ? "--version" : argv.slice(0, 2).join("-");
  console.log(JSON.stringify(answers()[key] ?? { ok: true }));
}
