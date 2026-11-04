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

function compact(n: number, kDigits = 0): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(kDigits)}k`;
  return String(n);
}
const str = (v: string): string => v;

export function Footer(props: {
  footer: FooterStats | null; cwd?: string; sessionKey?: string; contextWindow?: number;
  tokens?: { input: number; output: number; cacheRead: number; cacheWrite: number }; cacheHitPercent?: number;
}): JSX.Element {
  const f: FooterStats = props.footer ?? {
    model: NOT_ATTACHED,
    tier: NOT_ATTACHED,
    thinking: NOT_ATTACHED,
    contextPercent: NOT_ATTACHED,
    cost: NOT_ATTACHED,
    branch: NOT_ATTACHED,
  };
  const tk = props.tokens;
  const ctxWin = props.contextWindow;
  const pct = f.contextPercent.state === "known" ? f.contextPercent.value : null;
  return (
    <footer aria-label="Session footer" data-part="telemetry" className="flex h-[30px] items-center gap-3 px-3 text-xs font-mono">
      {props.cwd !== undefined && <span data-part="cwd">{props.cwd}</span>}
      {props.sessionKey !== undefined && <span data-part="session-key">{props.sessionKey}</span>}
      {tk !== undefined && <span data-part="tokens">{`↑${compact(tk.input)} ↓${compact(tk.output)} R${compact(tk.cacheRead)} W${compact(tk.cacheWrite)}`}</span>}
      {props.cacheHitPercent !== undefined && <span data-part="cache">{`CH ${props.cacheHitPercent.toFixed(1)}%`}</span>}
      <span data-stat="model">model: <Value stat={f.model} format={str} /></span>
      <span data-stat="tier">tier: <Value stat={f.tier} format={str} /></span>
      <span data-stat="thinking">thinking: <Value stat={f.thinking} format={str} /></span>
      <span data-stat="context">context: <Value stat={f.contextPercent} format={(v) => (ctxWin === undefined ? `${v}%` : `${v}% / ${compact(ctxWin, 1)}`)} /></span>
      {pct !== null && (
        <span role="progressbar" data-part="context-bar" aria-label="Context used" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}
          className="inline-block h-1 w-16 overflow-hidden rounded bg-muted">
          <span className="block h-full bg-primary" style={{ width: `${Math.min(100, Math.max(0, pct))}%` }} />
        </span>
      )}
      <span data-stat="cost">cost: <Value stat={f.cost} format={(v) => `$${v.toFixed(3)}`} /></span>
      <span data-stat="branch">branch: <Value stat={f.branch} format={str} /></span>
    </footer>
  );
}
