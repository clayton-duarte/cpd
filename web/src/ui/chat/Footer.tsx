import type { JSX } from "react";
import type { FooterStats, Stat } from "../../protocol/domain.ts";
import { Button } from "../components/button.tsx";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "../components/hover-card.tsx";
import { Progress } from "../components/progress.tsx";

const NOT_ATTACHED = { state: "unknown", reason: "not attached" } as const;

function Value<T>(props: {
  stat: Stat<T>;
  format: (v: T) => string;
}): JSX.Element {
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
  footer: FooterStats | null;
  cwd?: string;
  sessionKey?: string;
  contextWindow?: number;
  tokens?: {
    input: number;
    output: number;
    cacheRead: number;
    cacheWrite: number;
  };
  cacheHitPercent?: number;
  attachError?: { code: string; message: string } | null;
  onRetry?: () => void;
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
  const pct =
    f.contextPercent.state === "known" ? f.contextPercent.value : null;
  const clampedPct = pct === null ? 0 : Math.min(100, Math.max(0, pct));

  return (
    <footer
      aria-label="Session footer"
      data-part="telemetry"
      className="flex items-center justify-end gap-3 px-2.5 pb-1.5 text-xs text-muted-foreground"
    >
      {props.attachError ? (
        <span data-attach-status role="status" className="mr-auto flex items-center gap-2 text-destructive">
          <span>
            {props.attachError.code}: {props.attachError.message}
          </span>
          <Button type="button" variant="outline" size="sm" data-action="retry" onClick={props.onRetry}>
            Retry
          </Button>
        </span>
      ) : null}
      <HoverCard openDelay={10} closeDelay={100}>
        <HoverCardTrigger asChild>
          <Button
            type="button"
            variant="outline"
            size="sm"
            data-stat="context"
            className="shrink-0"
          >
            {pct === null ? (
              <Value stat={f.contextPercent} format={(v) => `${v}% context`} />
            ) : (
              `${pct}% context`
            )}
          </Button>
        </HoverCardTrigger>
        <HoverCardContent align="end" className="flex w-64 flex-col gap-3">
          <div className="flex items-center justify-end gap-3 text-sm">
            <span className="font-medium">Context</span>
            <span className="text-muted-foreground">
              {pct === null
                ? "unknown"
                : ctxWin === undefined
                  ? `${pct}%`
                  : `${pct}% / ${compact(ctxWin, 1)}`}
            </span>
          </div>
          <Progress
            data-part="context-bar"
            value={clampedPct}
            aria-label="Context used"
          />
        </HoverCardContent>
      </HoverCard>
    </footer>
  );
}
