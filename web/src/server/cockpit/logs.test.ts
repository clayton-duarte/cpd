import { describe, expect, it } from "vitest";
import { readLogs } from "./logs.ts";

const line = (o: Record<string, unknown>): string => JSON.stringify({ at: "2026-01-01T00:00:00.000Z", text: "t", plan: null, context: null, session: null, job: null, attempt: null, ...o });
const deps = (lines: string[]) => ({
  calls: [] as (readonly string[])[],
  async cpd(argv: readonly string[]) { this.calls.push(argv); return lines.join("\n") + "\n"; },
});

describe("logs", () => {
  it("logs keeps the latest 500 events", async () => {
    const d = deps(Array.from({ length: 620 }, (_, i) => line({ id: i + 1, kind: "plan-phase", text: `e${i + 1}` })));
    const out = await readLogs(d);
    expect(d.calls[0]).toEqual(["events", "follow", "--since", "0", "--until-idle"]);
    expect(out).toHaveLength(500);
    expect(out[0]).toMatchObject({ id: 121, kind: "plan-phase", text: "e121", tone: "normal", at: "2026-01-01T00:00:00.000Z" });
    expect(out[499]?.id).toBe(620);
  });
  it("logs marks error and lead error tones", async () => {
    const out = await readLogs(deps([
      line({ id: 1, kind: "plan-phase" }),
      line({ id: 2, kind: "error", plan: "p", data: { source: "watch", code: "step_failed" } }),
      line({ id: 3, kind: "error", plan: "p", data: { source: "pr", code: "cleanup_failed" } }),
      line({ id: 4, kind: "error", plan: "p", data: { source: "watch", code: "push_failed" } }),
      line({ id: 5, kind: "error", plan: "p", data: { source: "watch", code: "workflow_start_failed" } }),
      line({ id: 6, kind: "error", plan: null, data: { source: "pr" } }),
    ]));
    expect(out.map((e) => e.tone)).toEqual(["normal", "error", "lead-error", "lead-error", "lead-error", "error"]);
  });
  it("logs returns gh_rate_limited and gh_failed error events with the error tone", async () => {
    const out = await readLogs(deps([
      line({ id: 1, kind: "error", data: { source: "watch", code: "gh_rate_limited" } }),
      line({ id: 2, kind: "error", data: { source: "watch", code: "gh_failed" } }),
    ]));
    expect(out.map((e) => [e.id, e.kind, e.tone])).toEqual([[1, "error", "error"], [2, "error", "error"]]);
  });
});
