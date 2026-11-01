import { CpdError, isObject } from "./cpd.ts";

/** The key of the section in pi's `systemPromptOptions.sections` (contract §8.3 "Delivery to agents"). */
export const POLICY_SECTION = "cpd-policy";

/**
 * The text of a `cpd policy show` output (commands.md §22.1, §15.10): every domain whose `audience` holds the output's `role`
 * and whose `text` is not empty, in the output's order, each as `## <domain> (<source>)` (the domain's own name: `pr`, `workflow`, ...) and its text; blocks are separated by a blank line.
 * `undefined` when no domain has text. The adapter trusts the output's `role` and `audience`; it never picks a key or reads a file.
 * An output of another shape is `bad_output`.
 */
export function policySection(show: Record<string, unknown>): string | undefined {
  const bad = (why: string) => new CpdError("bad_output", `cpd policy show: ${why}`);
  const role = show.role;
  if (typeof role !== "string") throw bad("`role` is not a string");
  if (!isObject(show.policy)) throw bad("`policy` is not an object");
  const blocks: string[] = [];
  for (const [domain, d] of Object.entries(show.policy)) {
    if (!isObject(d) || typeof d.source !== "string" || typeof d.text !== "string" || !Array.isArray(d.audience)) {
      throw bad(`domain ${domain} lacks source, audience or text`);
    }
    if (d.audience.includes(role) && d.text.trim() !== "") blocks.push(`## ${domain} (${d.source})\n${d.text.trim()}`);
  }
  return blocks.length === 0 ? undefined : blocks.join("\n\n");
}
