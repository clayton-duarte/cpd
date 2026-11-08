import { describe, expect, it } from "vitest";
import { allCpdCommands, toArgv } from "../cpd-argv.ts";
import type { SettingsChanges } from "../../protocol/cockpit.ts";
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

const config = { ok: true, config: { decide_tier: "JX", context_distill_tier: "GX", worker_tier: "H3", lead_tier: "S3", manager_tier: "O3" } };
// `cpd config` also carries the machine-wide Worker count (docs/commands.md §14.2, `concurrency`, contract §8.2).
const configWithConcurrency = { ok: true, config: { ...config.config, concurrency: 3 } };
const experimental = { ok: true, experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true } };

describe("settings", () => {
  it("settings.read reads the Tiers from cpd config and the switches from cpd config experimental", async () => {
    const { cpd } = fake({ config, "config experimental": experimental });
    expect(await readSettings(cpd)).toEqual({
      tiers: { decide: "JX", auxiliar: "GX", worker: "H3", lead: "S3", manager: "O3" },
      experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true },
    });
  });

  it("a decide tier of JX round-trips", async () => {
    // The decide role's default is JX (contract §9, air-d113, air-d115, air-d120): the settings read
    // carries it like every other role's Tier, and a J Tier is not mistaken for an unknown letter.
    const config = { ok: true, config: { decide_tier: "JX", context_distill_tier: "GX", worker_tier: "H3", lead_tier: "S3", manager_tier: "O3" } };
    const { cpd } = fake({ config, "config experimental": experimental });
    const read = await readSettings(cpd);
    expect((read.tiers as unknown as Record<string, string>).decide).toBe("JX");
  });

  it("settings.read fails loud on a malformed Tier", async () => {
    const bad = { ok: true, config: { ...config.config, lead_tier: "Z9" } };
    const { cpd } = fake({ config: bad, "config experimental": experimental });
    await expect(readSettings(cpd)).rejects.toMatchObject({ code: "cpd_malformed_json" });
  });

  it("settings.save sends all five Tiers, the changed ones over the current ones, then each switch, then retier all, in order", async () => {
    const { cpd, calls } = fake({ config, "config experimental": experimental });
    await saveSettings(cpd, { tiers: { worker: "H2", lead: "S2" }, experimental: { write_curation: false, durable_workers: true } });
    expect(calls).toEqual([
      "config",
      "config experimental",
      "config tiers set --decide JX --auxiliar GX --worker H2 --lead S2 --manager O3",
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

  it("settings.read exposes the concurrency from cpd config", async () => {
    const { cpd } = fake({ config: configWithConcurrency, "config experimental": experimental });
    expect(await readSettings(cpd)).toEqual({
      tiers: { decide: "JX", auxiliar: "GX", worker: "H3", lead: "S3", manager: "O3" },
      experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true },
      concurrency: 3,
    });
  });

  it("settings.save writes all five Tiers and the concurrency, through toArgv", async () => {
    const { cpd, calls } = fake({ config: configWithConcurrency, "config experimental": experimental });
    await saveSettings(cpd, { tiers: { worker: "H2", lead: "S2" }, concurrency: 4 } as SettingsChanges);
    // `cpd config concurrency set <n>` (commands.md §14.2c), the builder's `config.concurrency.set` variant (web/CONTRACT.md §19.3).
    const concurrency = ["config", "concurrency", "set", "4"].join(" ");
    expect(calls).toEqual([
      toArgv({ kind: "config.read" }).join(" "),
      toArgv({ kind: "config.experimental.read" }).join(" "),
      toArgv({ kind: "config.tiers.set", tiers: { decide: "JX", auxiliar: "GX", worker: "H2", lead: "S2", manager: "O3" } }).join(" "),
      concurrency,
      toArgv({ kind: "session.retierAll" }).join(" "),
    ]);
    // Every argv the save spawns is the one the shared builder produces, never a hand-written array (web/CONTRACT.md §18, §19.3).
    const built = new Set(allCpdCommands().map((cmd) => toArgv(cmd).join(" ")));
    expect(calls.every((line) => built.has(line))).toBe(true);
  });

  it("settings.save stops at the first refusal", async () => {
    const { cpd, calls } = fake({ config, "config experimental": experimental }, "config experimental write_curation off");
    await expect(saveSettings(cpd, { tiers: { lead: "S2" }, experimental: { write_curation: false, failure_triage: false } })).rejects.toThrow("refused");
    expect(calls).toEqual(["config", "config experimental", "config tiers set --decide JX --auxiliar GX --worker H3 --lead S2 --manager O3", "config experimental write_curation off"]);
  });
});
