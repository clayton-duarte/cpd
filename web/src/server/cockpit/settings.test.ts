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

const config = { ok: true, config: { decide_tier: "JX", context_distill_tier: "GX", worker_tier: "H1", lead_tier: "S1", manager_tier: "O2" } };
// `cpd config` also carries the machine-wide Worker count (docs/commands.md §14.2, `concurrency`, contract §8.2).
const configWithConcurrency = { ok: true, config: { ...config.config, concurrency: 3 } };
const experimental = { ok: true, experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true } };

describe("settings", () => {
  it("settings.read reads the Tiers from cpd config and the switches from cpd config experimental", async () => {
    const { cpd } = fake({ config, "config experimental": experimental });
    expect(await readSettings(cpd)).toEqual({
      tiers: { decide: "JX", auxiliar: "GX", worker: "H1", lead: "S1", manager: "O2" },
      experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true },
    });
  });

  it("a decide tier of JX round-trips", async () => {
    // The decide role's default is JX (contract §9, air-d113, air-d115, air-d120): the settings read
    // carries it like every other role's Tier, and a J Tier is not mistaken for an unknown letter.
    const config = { ok: true, config: { decide_tier: "JX", context_distill_tier: "GX", worker_tier: "H1", lead_tier: "S1", manager_tier: "O2" } };
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
    await saveSettings(cpd, { tiers: { worker: { model: "H", effort: "2" }, lead: { model: "S", effort: "2" } }, experimental: { write_curation: false, durable_workers: true } } as unknown as SettingsChanges);
    expect(calls).toEqual([
      "config",
      "config experimental",
      "config tiers set --decide JX --auxiliar GX --worker H2 --lead S2 --manager O2",
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
      tiers: { decide: "JX", auxiliar: "GX", worker: "H1", lead: "S1", manager: "O2" },
      experimental: { durable_workers: false, write_curation: true, failure_triage: true, turn_question_guard: true },
      concurrency: 3,
    });
  });

  it("settings.save writes all five Tiers and the concurrency, through toArgv", async () => {
    const { cpd, calls } = fake({ config: configWithConcurrency, "config experimental": experimental });
    await saveSettings(cpd, { tiers: { worker: { model: "H", effort: "2" }, lead: { model: "S", effort: "2" } }, concurrency: 4 } as unknown as SettingsChanges);
    // `cpd config concurrency set <n>` (commands.md §14.2c), the builder's `config.concurrency.set` variant (web/CONTRACT.md §19.3).
    const concurrency = ["config", "concurrency", "set", "4"].join(" ");
    expect(calls).toEqual([
      toArgv({ kind: "config.read" }).join(" "),
      toArgv({ kind: "config.experimental.read" }).join(" "),
      toArgv({ kind: "config.tiers.set", tiers: { decide: "JX", auxiliar: "GX", worker: "H2", lead: "S2", manager: "O2" } }).join(" "),
      concurrency,
      toArgv({ kind: "session.retierAll" }).join(" "),
    ]);
    // Every argv the save spawns is the one the shared builder produces, never a hand-written array (web/CONTRACT.md §18, §19.3).
    const built = new Set(allCpdCommands().map((cmd) => toArgv(cmd).join(" ")));
    expect(calls.every((line) => built.has(line))).toBe(true);
  });

  it("settings.save stops at the first refusal", async () => {
    const { cpd, calls } = fake({ config, "config experimental": experimental }, "config experimental write_curation off");
    await expect(saveSettings(cpd, { tiers: { lead: { model: "S", effort: "2" } }, experimental: { write_curation: false, failure_triage: false } } as unknown as SettingsChanges)).rejects.toThrow("refused");
    expect(calls).toEqual(["config", "config experimental", "config tiers set --decide JX --auxiliar GX --worker H1 --lead S2 --manager O2", "config experimental write_curation off"]);
  });

  it("settings.save carries per-role model and effort as the role's Tier flag", async () => {
    const { cpd, calls } = fake({ config, "config experimental": experimental });
    await saveSettings(cpd, {
      tiers: { decide: { model: "J", effort: "0" }, worker: { model: "H", effort: "2" }, lead: { model: "S", effort: "X" } },
    } as unknown as SettingsChanges);
    expect(calls).toEqual([
      "config",
      "config experimental",
      "config tiers set --decide J0 --auxiliar GX --worker H2 --lead SX --manager O2",
      "session retier --all",
    ]);
  });

  it("settings.read accepts catalog tiers from core and rejects unknown", async () => {
    // Regression test and catalog validation:
    // readSettings accepts worker_tier H3, manager_tier O3, and S4 when present in the catalog.
    // readSettings rejects tiers not present in the catalog.
    const catalog = {
      ok: true,
      tiers: [
        { tier: "JX", letter: "J", effort: "X", model: "decide-model" },
        { tier: "GX", letter: "G", effort: "X", model: "gemini" },
        { tier: "H1", letter: "H", effort: "1", model: "haiku" },
        { tier: "H3", letter: "H", effort: "3", model: "haiku" },
        { tier: "S1", letter: "S", effort: "1", model: "sonnet" },
        { tier: "S4", letter: "S", effort: "4", model: "sonnet" },
        { tier: "O2", letter: "O", effort: "2", model: "opus" },
        { tier: "O3", letter: "O", effort: "3", model: "opus" },
      ],
      ranges: {
        decide: { floor: "JX", ceiling: "JX", allowed: ["JX"] },
        auxiliar: { floor: "GX", ceiling: "GX", allowed: ["GX"] },
        worker: { floor: "H1", ceiling: "S4", allowed: ["H1", "H3", "S1", "S4"] },
        lead: { floor: "S1", ceiling: "O3", allowed: ["S1", "S4", "O2", "O3"] },
        manager: { floor: "O2", ceiling: "O3", allowed: ["O2", "O3"] },
      },
    };
    const withTier = (key: string, value: string) => fake({
      config: { ok: true, config: { ...config.config, [key]: value } },
      "config experimental": experimental,
      "config tiers list": catalog,
    }).cpd;

    // Accepted because they are in the catalog (including H3, S4, O3)
    for (const ok of ["H1", "H3", "S4"]) {
      expect((await readSettings(withTier("worker_tier", ok))).tiers.worker).toBe(ok);
    }
    expect((await readSettings(withTier("manager_tier", "O3"))).tiers.manager).toBe("O3");

    // Rejected because they are not in the catalog (e.g. unknown letters M2, K1 or invalid efforts H5, Z1)
    for (const bad of ["M2", "K1", "L0", "P2", "A1", "H5", "Z1", "H9", "foo"]) {
      await expect(readSettings(withTier("worker_tier", bad))).rejects.toMatchObject({ code: "cpd_malformed_json" });
    }
  });
});
