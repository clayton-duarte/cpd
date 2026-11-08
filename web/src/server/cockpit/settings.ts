import { WebError } from "../../protocol/domain.ts";
import { SETTINGS_ROLES, SETTINGS_SWITCHES, type SettingsChanges, type SettingsModel, type SettingsRole } from "../../protocol/cockpit.ts";
import { toArgv, type TiersArgv } from "../cpd-argv.ts";
import type { Cpd } from "./cli.ts";

/** `cpd config` key of each role Tier (scout-settings). */
const TIER_KEY: Readonly<Record<SettingsRole, string>> = {
  auxiliar: "context_distill_tier", worker: "worker_tier", lead: "lead_tier", manager: "manager_tier",
};
// The Tier letters in ladder order, J first (contract §9, air-d113, air-d115, air-d120): J's model
// offers efforts 0-3, so `J4` (like `J5`) is refused while the other letters still take 0-4 or X.
const TIER = /^(?:[GHSO](?:[0-4]|X)|J(?:[0-3]|X))$/;
/** The decide role's config key (contract §9, air-d113, air-d115, air-d120). */
const DECIDE_KEY = "decide_tier";
/** The decide role's default, JX (contract §9): sent back when the read predates `decide_tier`. */
const DECIDE_DEFAULT = "JX";
/** `config tiers set`'s five roles, decide first (contract §9, air-d113, air-d115, air-d120). */
const TIERS_ROLES = ["decide", ...SETTINGS_ROLES] as const;

const bad = (field: string): never => { throw new WebError("cpd_malformed_json", `settings: missing or mistyped field ${field}`, { field }); };
const obj = (v: unknown, field: string): Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : bad(field);

export async function readSettings(cpd: Cpd): Promise<SettingsModel> {
  const config = obj((await cpd.json(toArgv({ kind: "config.read" })))["config"], "config");
  const experimental = obj((await cpd.json(toArgv({ kind: "config.experimental.read" })))["experimental"], "experimental");
  const tier = (key: string): string => {
    const v = config[key];
    return typeof v === "string" && TIER.test(v) ? v : bad(key);
  };
  // The decide Tier rides along when `cpd config` carries its key, like every other role's Tier.
  const decide = config[DECIDE_KEY] === undefined ? undefined : tier(DECIDE_KEY);
  const flag = (name: string): boolean => {
    const v = experimental[name];
    return typeof v === "boolean" ? v : bad(name);
  };
  const raw = config["concurrency"];
  const concurrency = raw === undefined
    ? {}
    : { concurrency: typeof raw === "number" && Number.isInteger(raw) && raw >= 1 && raw <= 16 ? raw : bad("concurrency") };
  return {
    tiers: {
      auxiliar: tier(TIER_KEY.auxiliar), worker: tier(TIER_KEY.worker),
      lead: tier(TIER_KEY.lead), manager: tier(TIER_KEY.manager),
      ...(decide === undefined ? {} : { decide }),
    } as SettingsModel["tiers"],
    experimental: {
      durable_workers: flag("durable_workers"), write_curation: flag("write_curation"),
      failure_triage: flag("failure_triage"), turn_question_guard: flag("turn_question_guard"),
    },
    ...concurrency,
  };
}

/** Saves in the TUI's order: Tiers, then the Worker count, then each switch, then retier all when a Tier changed. Stops at the first refusal.
 * `config tiers set` requires all five roles (air-n37): the changed ones come from the dialog, the rest
 * from the settings read, so no role is ever dropped. */
export async function saveSettings(cpd: Cpd, changes: SettingsChanges): Promise<void> {
  // The decide role is not a `SETTINGS_ROLES` member, but the write path carries it like the other four.
  const changed = changes.tiers as Readonly<Record<string, string | undefined>> | undefined;
  const roles = TIERS_ROLES.filter((r) => changed?.[r] !== undefined);
  if (roles.length > 0) {
    // `cpd config tiers set` requires all five role flags (air-n37): send the changed Tiers over the current ones.
    const current = await readSettings(cpd);
    const read = current.tiers as Readonly<Record<string, string | undefined>>;
    const tiers: TiersArgv = {
      decide: changed?.["decide"] ?? read["decide"] ?? DECIDE_DEFAULT,
      auxiliar: changed?.["auxiliar"] ?? current.tiers.auxiliar,
      worker: changed?.["worker"] ?? current.tiers.worker,
      lead: changed?.["lead"] ?? current.tiers.lead,
      manager: changed?.["manager"] ?? current.tiers.manager,
    };
    await cpd.json(toArgv({ kind: "config.tiers.set", tiers }));
  }
  if (changes.concurrency !== undefined) await cpd.json(toArgv({ kind: "config.concurrency.set", n: changes.concurrency }));
  for (const key of SETTINGS_SWITCHES) {
    const on = changes.experimental?.[key];
    if (on !== undefined) await cpd.json(toArgv({ kind: "config.experimental.set", key, on }));
  }
  if (roles.length > 0) await cpd.json(toArgv({ kind: "session.retierAll" }));
}
