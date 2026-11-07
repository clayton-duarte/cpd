import { describe, expect, it } from "vitest";
import type { Cpd } from "./cli.ts";
import { readSettings, saveSettings } from "./settings.ts";

function fake(replies: Record<string, Record<string, unknown>>, failOn?: string): { cpd: Cpd; calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    cpd: {
      async json(args) {
        const line = args.join(" ");
        calls.push(line);
        if (line === failOn) throw new Error(`refused: ${line}`);
        return replies[line] ?? { ok: true };
      },
    },
  };
}

const config = { ok: true, config: { context_distill_tier: "GX", worker_tier: "H3", lead_tier: "S3", manager_tier: "O3" } };
const experimental = { ok: true, experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true } };

describe("settings", () => {
  it("settings.read reads the Tiers from cpd config and the switches from cpd config experimental", async () => {
    const { cpd } = fake({ config, "config experimental": experimental });
    expect(await readSettings(cpd)).toEqual({
      tiers: { auxiliar: "GX", worker: "H3", lead: "S3", manager: "O3" },
      experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true },
    });
  });

  it("settings.read fails loud on a malformed Tier", async () => {
    const bad = { ok: true, config: { ...config.config, lead_tier: "Z9" } };
    const { cpd } = fake({ config: bad, "config experimental": experimental });
    await expect(readSettings(cpd)).rejects.toMatchObject({ code: "cpd_malformed_json" });
  });

  it("settings.save runs tiers, then each switch, then retier all, in order", async () => {
    const { cpd, calls } = fake({ config, "config experimental": experimental });
    await saveSettings(cpd, { tiers: { worker: "H2", lead: "S2" }, experimental: { write_curation: false, durable_workers: true } });
    expect(calls).toEqual([
      "config",
      "config experimental",
      "config tiers set --auxiliar GX --worker H2 --lead S2 --manager O3",
      "config experimental durable_workers on",
      "config experimental write_curation off",
      "session retier --all",
    ]);
  });

  it("settings.save of a switch alone does not retier", async () => {
    const { cpd, calls } = fake({});
    await saveSettings(cpd, { experimental: { failure_triage: false } });
    expect(calls).toEqual(["config experimental failure_triage off"]);
  });

  it("settings.save stops at the first refusal", async () => {
    const { cpd, calls } = fake({ config, "config experimental": experimental }, "config experimental write_curation off");
    await expect(saveSettings(cpd, { tiers: { lead: "S2" }, experimental: { write_curation: false, failure_triage: false } })).rejects.toThrow("refused");
    expect(calls).toEqual(["config", "config experimental", "config tiers set --auxiliar GX --worker H3 --lead S2 --manager O3", "config experimental write_curation off"]);
  });
});
