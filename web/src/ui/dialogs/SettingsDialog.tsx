import { useRef, useState } from "react";
import type { ReactElement } from "react";
import { SETTINGS_ROLES, SETTINGS_SWITCHES } from "../../protocol/cockpit.ts";
import type { SettingsChanges, SettingsModel, SettingsRole, SettingsSwitch } from "../../protocol/cockpit.ts";
import { EFFORTS, MODEL_LETTERS, TIER_COUNT, parseTier, tierAt, tierIndex } from "../../protocol/tier.ts";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "../components/dialog.tsx";

export type { SettingsChanges, SettingsRole, SettingsSwitch };
export const SWITCHES = SETTINGS_SWITCHES;
export type SwitchName = SettingsSwitch;
/** The read of `cpd config` / `cpd config experimental` the dialog shows. */
export type SettingsRead = SettingsModel;

/** The dialog's Tier rows: the decide role (contract §9, air-d113) above the four role Tiers. */
const TIER_ROLES = ["decide", ...SETTINGS_ROLES] as const;
type TierRole = (typeof TIER_ROLES)[number];
const TIER_ROLE_LABEL: Readonly<Record<TierRole, string>> = {
  decide: "decisioning", auxiliar: "auxiliar", worker: "worker", lead: "lead", manager: "manager",
};
/** The decide role's default (contract §9): a read that predates `decide_tier` still shows it. */
const DECIDE_DEFAULT = "JX";
/** The dialog's Tier values, the decide role defaulted when the read omits its key. */
const readTiers = (read: SettingsRead): Record<TierRole, string> =>
  Object.fromEntries(TIER_ROLES.map((r) => {
    const t = (read.tiers as Readonly<Record<string, string | undefined>>)[r];
    return [r, t ?? (r === "decide" ? DECIDE_DEFAULT : "")];
  })) as Record<TierRole, string>;

/** Applies one role's new Tier and moves the linked boundaries so worker < lead < manager still holds; null when no coherent set exists. */
function link(tiers: Record<TierRole, string>, role: TierRole, value: string): Record<TierRole, string> | null {
  const next = { ...tiers, [role]: value };
  const idx = (r: TierRole): number | null => tierIndex(next[r]);
  const set = (r: TierRole, i: number): boolean => {
    if (i < 0 || i >= TIER_COUNT) return false;
    next[r] = tierAt(i);
    return true;
  };
  if (role !== "worker" && role !== "lead" && role !== "manager") return next;
  if (idx("worker") === null || idx("lead") === null || idx("manager") === null) return next;
  const at = (r: TierRole): number => idx(r) as number;
  if (role === "worker") {
    if (at("lead") <= at("worker") && !set("lead", at("worker") + 1)) return null;
    if (at("manager") <= at("lead") && !set("manager", at("lead") + 1)) return null;
  } else if (role === "lead") {
    if (at("worker") >= at("lead") && !set("worker", at("lead") - 1)) return null;
    if (at("manager") <= at("lead") && !set("manager", at("lead") + 1)) return null;
  } else {
    if (at("lead") >= at("manager") && !set("lead", at("manager") - 1)) return null;
    if (at("worker") >= at("lead") && !set("worker", at("lead") - 1)) return null;
  }
  return next;
}
const coherent = (t: Record<TierRole, string>): boolean => {
  const i = (["worker", "lead", "manager"] as const).map((r) => tierIndex(t[r]));
  return TIER_ROLES.every((r) => parseTier(t[r]) !== null) && i[0]! < i[1]! && i[1]! < i[2]!;
};

function Form(props: { read: SettingsRead; onSave(changes: SettingsChanges): void; onCancel(): void }): ReactElement {
  const { read } = props;
  const [tiers, setTiers] = useState<Record<TierRole, string>>(() => readTiers(read));
  const [flags, setFlags] = useState<Record<SettingsSwitch, boolean>>({ ...read.experimental });
  const [concurrency, setConcurrency] = useState<string>(read.concurrency === undefined ? "" : String(read.concurrency));
  const concurrencyRef = useRef<HTMLInputElement>(null);
  const valid = coherent(tiers);
  const save = (): void => {
    const changedTiers = TIER_ROLES.filter((r) => tiers[r] !== readTiers(read)[r]);
    const changedFlags = SETTINGS_SWITCHES.filter((k) => flags[k] !== read.experimental[k]);
    // The count is read from the field itself: a bad value is refused by the CLI, never clamped or dropped here (air-d70).
    const raw = concurrencyRef.current?.value ?? concurrency;
    const count = Number(raw);
    props.onSave({
      ...(changedTiers.length === 0 ? {} : { tiers: Object.fromEntries(changedTiers.map((r) => [r, parseTier(tiers[r])])) }),
      ...(changedFlags.length === 0 ? {} : { experimental: Object.fromEntries(changedFlags.map((k) => [k, flags[k]])) }),
      ...(raw === "" || !Number.isInteger(count) || count === read.concurrency ? {} : { concurrency: count }),
    } as unknown as SettingsChanges);
  };
  return (
    <>
      <section className="grid gap-2">
        <h3 className="font-semibold text-fg-strong">Models + Tiers</h3>
        <div className="flex items-center gap-2">
          <span className="w-24 font-mono text-meta">Preset</span>
          <span className="font-mono text-meta text-fg-dim">(none)</span>
          <Button type="button" variant="outline" size="xs" disabled>Load</Button>
        </div>
        <p className="text-meta text-fg-dim">the default of each role; its range follows</p>
        <p className="text-meta text-fg-dim">decisioning runs the cheap decisions: routing, classification and the logic gates</p>
        {TIER_ROLES.map((r) => (
          <label key={r} className="flex items-center gap-2">
            <span className="w-24 font-mono text-meta">{TIER_ROLE_LABEL[r]}</span>
            {(["model", "effort"] as const).map((part) => (
              <select
                key={part}
                aria-label={`${r} ${part}`}
                value={parseTier(tiers[r])?.[part] ?? ""}
                onChange={(e) => {
                  const cur = parseTier(tiers[r]) ?? { model: "J", effort: "X" };
                  const next = link(tiers, r, `${part === "model" ? e.currentTarget.value : cur.model}${part === "effort" ? e.currentTarget.value : cur.effort}`);
                  if (next !== null) setTiers(next);
                }}
                aria-invalid={parseTier(tiers[r]) === null}
                className="h-7 rounded-md border border-border bg-background px-2 font-mono text-meta"
              >
                {parseTier(tiers[r]) === null ? <option value="" disabled /> : null}
                {(part === "model" ? MODEL_LETTERS : EFFORTS).map((o) => <option key={o} value={o}>{o}</option>)}
              </select>
            ))}
          </label>
        ))}
        <label className="flex items-center gap-2">
          <span className="w-24 font-mono text-meta">concurrency</span>
          <input
            ref={concurrencyRef}
            type="number"
            min={1}
            max={16}
            aria-label="concurrency"
            value={concurrency}
            onChange={(e) => setConcurrency(e.target.value)}
            aria-invalid={concurrency !== "" && !(Number.isInteger(Number(concurrency)) && Number(concurrency) >= 1 && Number(concurrency) <= 16)}
            className="h-7 w-16 rounded-md border border-border bg-background px-2 font-mono text-meta"
          />
        </label>
      </section>
      <section className="grid gap-2">
        <h3 className="font-semibold text-fg-strong">Experimental</h3>
        {SETTINGS_SWITCHES.map((k) => (
          <div key={k} className="flex items-center gap-2">
            <span className="w-48 font-mono text-meta">{k}</span>
            <button
              type="button"
              role="switch"
              aria-label={k}
              aria-checked={flags[k]}
              onClick={() => setFlags({ ...flags, [k]: !flags[k] })}
              className={cn("h-6 w-10 rounded-full border border-border px-0.5 text-left", flags[k] ? "bg-status-done" : "bg-surface-pressed")}
            >
              <span className={cn("block size-4 rounded-full bg-fg-strong transition-transform", flags[k] ? "translate-x-4" : "")} />
            </button>
            <span className="font-mono text-meta text-fg-dim">{flags[k] ? "on" : "off"}</span>
          </div>
        ))}
      </section>
      <DialogFooter>
        <Button type="button" disabled={!valid} onClick={save}>Save</Button>
        <Button type="button" variant="outline" onClick={props.onCancel}>Cancel</Button>
      </DialogFooter>
    </>
  );
}

export function SettingsDialog(props: {
  open: boolean;
  /** Null while the read is on its way. */
  read: SettingsRead | null;
  onSave(changes: SettingsChanges): void;
  onCancel(): void;
}): ReactElement {
  return (
    <Dialog open={props.open} onOpenChange={(o) => { if (!o) props.onCancel(); }}>
      <DialogContent data-part="agent-tiers-dialog" aria-describedby={undefined}>
        <DialogHeader><DialogTitle>Agent Tiers</DialogTitle></DialogHeader>
        {props.read === null ? (
          <p className="text-fg-dim">Loading…</p>
        ) : (
          <Form read={props.read} onSave={props.onSave} onCancel={props.onCancel} />
        )}
      </DialogContent>
    </Dialog>
  );
}
