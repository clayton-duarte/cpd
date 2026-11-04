import { ChevronDown } from "lucide-react";
import { useRef, useState } from "react";
import type { Capabilities, ConfigureChange, FooterStats, ModelChoice, Stat } from "../../protocol/domain.ts";

export type ConfigureHandler = (change: ConfigureChange) => void;

const BUSY_REASON = "A run is pending: the model changes when the session is idle";
const known = <T,>(s: Stat<T> | undefined): T | null => (s !== undefined && s.state === "known" ? s.value : null);
const withCurrent = (list: readonly string[], current: string | null): string[] => (current !== null && !list.includes(current) ? [...list, current] : [...list]);

export function TierSelector(props: { capabilities: Capabilities | null; footer: FooterStats | null; models: readonly ModelChoice[] | null; busy: boolean; onConfigure: ConfigureHandler; onRequestModels(): void }): React.JSX.Element {
  const { capabilities: caps, footer, models, busy } = props;
  const requested = useRef(false);
  const [open, setOpen] = useState(false);
  const tier = known(footer?.tier);
  const model = known(footer?.model);
  const thinking = known(footer?.thinking);

  const reasonOf = (s: { supported: boolean; reason?: string } | undefined, busyBlocks: boolean): string | null => {
    if (caps === null || s === undefined) return "Not attached";
    if (!s.supported) return s.reason ?? "Unsupported";
    return busyBlocks && busy ? BUSY_REASON : null;
  };
  const tierReason = reasonOf(caps?.configure.tier, false);
  const modelReason = reasonOf(caps?.configure.model, true);
  const thinkingReason = reasonOf(caps?.configure.thinking, true);

  const tiers = withCurrent(caps?.configure.tier.options ?? [], tier);
  const modelOptions = models !== null && models.length > 0
    ? models.map((m) => ({ value: m.model, text: m.name }))
    : [{ value: model ?? "", text: model ?? "unknown" }];
  const levels = withCurrent(models?.find((m) => m.model === model)?.thinkingLevels ?? [], thinking);

  const note = (reason: string | null) => (reason === null ? null : <small data-reason>{reason}</small>);
  const modelShort = model === null ? "unknown" : model.slice(model.lastIndexOf("/") + 1);
  return (
    <div data-part="tier-selector" className="relative">
      <button type="button" data-part="tier-pill" aria-haspopup="true" aria-expanded={open} onClick={() => setOpen(!open)}
        className="inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs font-mono">
        <span data-part="pill-tier">{tier ?? "unknown"}</span>
        <span data-part="pill-model">{modelShort}</span>
        <span data-part="pill-thinking">{thinking ?? "unknown"}</span>
        <ChevronDown aria-hidden="true" size={12} />
      </button>
      {open && (
    <fieldset aria-label="Session settings" data-part="tier-menu" className="absolute bottom-full z-10 mb-1 rounded-md border bg-popover p-2">
      <select aria-label="Tier" value={tier ?? ""} disabled={tierReason !== null} title={tierReason ?? undefined}
        onChange={(e) => props.onConfigure({ kind: "tier", tier: e.currentTarget.value })}>
        {tier === null && <option value="" disabled>unknown</option>}
        {tiers.map((t) => <option key={t} value={t}>{t}</option>)}
      </select>
      {note(tierReason)}
      <select aria-label="Model" value={model ?? ""} disabled={modelReason !== null} title={modelReason ?? undefined}
        onFocus={() => { if (!requested.current) { requested.current = true; props.onRequestModels(); } }}
        onChange={(e) => props.onConfigure({ kind: "model", model: e.currentTarget.value })}>
        {modelOptions.map((o) => <option key={o.value} value={o.value}>{o.text}</option>)}
      </select>
      {note(modelReason)}
      <select aria-label="Thinking level" value={thinking ?? ""} disabled={thinkingReason !== null} title={thinkingReason ?? undefined}
        onChange={(e) => props.onConfigure({ kind: "thinking", level: e.currentTarget.value })}>
        {thinking === null && <option value="" disabled>unknown</option>}
        {levels.map((l) => <option key={l} value={l}>{l}</option>)}
      </select>
      {note(thinkingReason)}
    </fieldset>
      )}
    </div>
  );
}
