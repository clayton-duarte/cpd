import { WebError } from "../../protocol/domain.ts";
import { SETTINGS_ROLES, SETTINGS_SWITCHES, type SettingsChanges, type SettingsModel, type SettingsRole } from "../../protocol/cockpit.ts";
import { toArgv } from "../cpd-argv.ts";
import type { Cpd } from "./cli.ts";

/** `cpd config` key of each role Tier (scout-settings). */
const TIER_KEY: Readonly<Record<SettingsRole, string>> = {
  auxiliar: "context_distill_tier", worker: "worker_tier", lead: "lead_tier", manager: "manager_tier",
};
const TIER = /^[GHSO]([0-4]|X)$/;

const bad = (field: string): never => { throw new WebError("cpd_malformed_json", `settings: missing or mistyped field ${field}`, { field }); };
const obj = (v: unknown, field: string): Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : bad(field);

export async function readSettings(cpd: Cpd): Promise<SettingsModel> {
  const config = obj((await cpd.json(toArgv({ kind: "config.read" })))["config"], "config");
  const experimental = obj((await cpd.json(toArgv({ kind: "config.experimental.read" })))["experimental"], "experimental");
  const tier = (role: SettingsRole): string => {
    const v = config[TIER_KEY[role]];
    return typeof v === "string" && TIER.test(v) ? v : bad(TIER_KEY[role]);
  };
  const flag = (name: string): boolean => {
    const v = experimental[name];
    return typeof v === "boolean" ? v : bad(name);
  };
  return {
    tiers: { auxiliar: tier("auxiliar"), worker: tier("worker"), lead: tier("lead"), manager: tier("manager") },
    experimental: {
      durable_workers: flag("durable_workers"), write_curation: flag("write_curation"),
      failure_triage: flag("failure_triage"), turn_question_guard: flag("turn_question_guard"),
    },
  };
}

/** Saves in the TUI's order: Tiers, then each switch, then retier all when a Tier changed. Stops at the first refusal.
 * `config tiers set` requires all four roles (air-n37): the changed ones come from the dialog, the rest
 * from the settings read, so no role is ever dropped. */
export async function saveSettings(cpd: Cpd, changes: SettingsChanges): Promise<void> {
  const roles = SETTINGS_ROLES.filter((r) => changes.tiers?.[r] !== undefined);
  if (roles.length > 0) {
    // `cpd config tiers set` requires all four role flags (air-n37): send the changed Tiers over the current ones.
    const current = await readSettings(cpd);
    const tiers = Object.fromEntries(SETTINGS_ROLES.map((r) => [r, changes.tiers?.[r] ?? current.tiers[r]])) as Record<SettingsRole, string>;
    await cpd.json(toArgv({ kind: "config.tiers.set", tiers }));
  }
  for (const key of SETTINGS_SWITCHES) {
    const on = changes.experimental?.[key];
    if (on !== undefined) await cpd.json(toArgv({ kind: "config.experimental.set", key, on }));
  }
  if (roles.length > 0) await cpd.json(toArgv({ kind: "session.retierAll" }));
}
