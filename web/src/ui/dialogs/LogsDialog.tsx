import { useEffect, useRef } from "react";
import type { ReactElement } from "react";
import { cn } from "../../lib/utils.ts";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "../components/dialog.tsx";

export interface LogEvent {
  readonly id: number;
  /** RFC 3339 timestamp. */
  readonly at: string;
  readonly kind: string;
  readonly text: string;
  /** From the server; without it an `error` kind is red. */
  readonly tone?: "error" | "lead-error" | "normal";
}

const two = (n: number): string => String(n).padStart(2, "0");
/** `HH:MM:SS` in the local zone; the raw text when it does not parse. */
function clock(at: string): string {
  const d = new Date(at);
  return Number.isNaN(d.getTime()) ? at : `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}
function toneClass(e: LogEvent): string | undefined {
  const tone = e.tone ?? (e.kind === "error" ? "error" : "normal");
  return tone === "error" ? "text-status-blocked" : tone === "lead-error" ? "text-status-active" : undefined;
}

function Rows({ events }: { events: readonly LogEvent[] }): ReactElement {
  const end = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  useEffect(() => {
    if (follow.current) end.current?.scrollIntoView({ block: "end" });
  }, [events]);
  if (events.length === 0) return <p className="font-mono text-meta text-fg-dim">none yet</p>;
  return (
    <div
      data-part="log-rows"
      className="max-h-[60vh] overflow-y-auto font-mono text-meta"
      onScroll={(e) => {
        const el = e.currentTarget;
        follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 2;
      }}
    >
      {events.map((e) => (
        <div key={e.id} data-part="log-row" className={cn("whitespace-pre-wrap", toneClass(e))}>
          <span data-part="log-time" className="text-fg-dim">{clock(e.at)}</span>
          {` ${e.text === "" ? e.kind : e.text}`}
        </div>
      ))}
      <div ref={end} />
    </div>
  );
}

export function LogsDialog(props: { open: boolean; onClose(): void; events: readonly LogEvent[] }): ReactElement {
  return (
    <Dialog open={props.open} onOpenChange={(o) => { if (!o) props.onClose(); }}>
      <DialogContent data-part="logs-dialog" aria-describedby={undefined} className="sm:max-w-3xl">
        <DialogHeader><DialogTitle>Logs</DialogTitle></DialogHeader>
        <Rows events={props.events} />
      </DialogContent>
    </Dialog>
  );
}
