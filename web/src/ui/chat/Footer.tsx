import type { JSX } from "react";
import type { FooterStats, Stat } from "../../protocol/domain.ts";

const NOT_ATTACHED = { state: "unknown", reason: "not attached" } as const;

function Value<T>(props: { stat: Stat<T>; format: (v: T) => string }): JSX.Element {
  const { stat, format } = props;
  return stat.state === "known" ? (
    <>{format(stat.value)}</>
  ) : (
    <span data-state="unknown" title={stat.reason}>
      unknown
    </span>
  );
}

const str = (v: string): string => v;

export function Footer(props: { footer: FooterStats | null }): JSX.Element {
  const f: FooterStats = props.footer ?? {
    model: NOT_ATTACHED,
    tier: NOT_ATTACHED,
    thinking: NOT_ATTACHED,
    contextPercent: NOT_ATTACHED,
    cost: NOT_ATTACHED,
    branch: NOT_ATTACHED,
  };
  return (
    <footer aria-label="Session footer">
      <span data-stat="model">model: <Value stat={f.model} format={str} /></span>{" "}
      <span data-stat="tier">tier: <Value stat={f.tier} format={str} /></span>{" "}
      <span data-stat="thinking">thinking: <Value stat={f.thinking} format={str} /></span>{" "}
      <span data-stat="context">context: <Value stat={f.contextPercent} format={(v) => `${v}%`} /></span>{" "}
      <span data-stat="cost">cost: <Value stat={f.cost} format={(v) => `$${v.toFixed(3)}`} /></span>{" "}
      <span data-stat="branch">branch: <Value stat={f.branch} format={str} /></span>
    </footer>
  );
}
