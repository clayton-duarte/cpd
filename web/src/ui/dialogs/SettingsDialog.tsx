import { useRef, useState } from "react";
import type { ReactElement } from "react";
import { SETTINGS_ROLES, SETTINGS_SWITCHES } from "../../protocol/cockpit.ts";
import type { SettingsChanges, SettingsModel, SettingsRole, SettingsSwitch } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "../components/dialog.tsx";

export type { SettingsChanges, SettingsRole, SettingsSwitch };
export const SWITCHES = SETTINGS_SWITCHES;
export type SwitchName = SettingsSwitch;
/** The read of `cpd config` / `cpd config experimental` the dialog shows. */
export type SettingsRead = SettingsModel;

const TIER = /^[GHSO]([0-4]|X)$/;

function Form(props: { read: SettingsRead; onSave(changes: SettingsChanges): void; onCancel(): void }): ReactElement {
  const { read } = props;
  const [tiers, setTiers] = useState<Record<SettingsRole, string>>({ ...read.tiers });
  const [flags, setFlags] = useState<Record<SettingsSwitch, boolean>>({ ...read.experimental });
  const [concurrency, setConcurrency] = useState<string>(read.concurrency === undefined ? "" : String(read.concurrency));
  const concurrencyRef = useRef<HTMLInputElement>(null);
  const valid = SETTINGS_ROLES.every((r) => TIER.test(tiers[r]));
  const save = (): void => {
    const changedTiers = SETTINGS_ROLES.filter((r) => tiers[r] !== read.tiers[r]);
    const changedFlags = SETTINGS_SWITCHES.filter((k) => flags[k] !== read.experimental[k]);
    // The count is read from the field itself: a bad value is refused by the CLI, never clamped or dropped here (air-d70).
    const raw = concurrencyRef.current?.value ?? concurrency;
    const count = Number(raw);
    props.onSave({
      ...(changedTiers.length === 0 ? {} : { tiers: Object.fromEntries(changedTiers.map((r) => [r, tiers[r]])) }),
      ...(changedFlags.length === 0 ? {} : { experimental: Object.fromEntries(changedFlags.map((k) => [k, flags[k]])) }),
      ...(raw === "" || !Number.isInteger(count) || count === read.concurrency ? {} : { concurrency: count }),
    });
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
        {SETTINGS_ROLES.map((r) => (
          <label key={r} className="flex items-center gap-2">
            <span className="w-24 font-mono text-meta">{r}</span>
            <input
              aria-label={`${r} tier`}
              value={tiers[r]}
              onChange={(e) => setTiers({ ...tiers, [r]: e.target.value.toUpperCase() })}
              aria-invalid={!TIER.test(tiers[r])}
              className="h-7 w-16 rounded-md border border-border bg-background px-2 font-mono text-meta"
            />
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
