import { Fragment, memo, useState } from "react";
import type { JSX } from "react";
import { TOOL_PREVIEW_LINES } from "../../protocol/domain.ts";
import { glyphClass } from "../dango-status.ts";
import type { Glyph, Tone } from "../dango-status.ts";
import { Trace, TraceCollapsible, TraceCollapsibleContent, TraceCollapsibleTrigger } from "../components/trace.tsx";
import type { OrphanResultItem, ToolItem, ToolResult } from "../../protocol/domain.ts";

const DETAIL_KEYS = ["command", "query", "queries", "q", "url", "urls", "path", "file_path", "paths", "pattern", "prompt", "question", "claim", "name", "id", "tool"];

const tidy = (s: string): string => s.replace(/[\u0000-\u001f\u007f]/g, " ").replace(/ +/g, " ").trim();
const asText = (v: unknown): string => {
  if (Array.isArray(v)) return v.map(asText).join(", ");
  if (typeof v === "string") return v;
  return JSON.stringify(v) ?? "";
};
const str = (v: unknown): string => (typeof v === "string" ? v : "");

function detailOf(name: string, args: Readonly<Record<string, unknown>>): string {
  switch (name) {
    case "read":
    case "write":
    case "edit":
    case "ls":
      return str(args["path"]);
    case "bash":
    case "powershell":
      return str(args["command"]).split("\n")[0] ?? "";
    case "grep":
      return `/${str(args["pattern"])}/ in ${str(args["path"])}`;
    case "find":
      return `${str(args["pattern"])} in ${str(args["path"])}`;
  }
  const key = DETAIL_KEYS.find((k) => k in args);
  const first = Object.values(args)[0];
  const v = key !== undefined ? args[key] : first;
  if (v === undefined) return "";
  return tidy(asText(v));
}

const splitLines = (text: string): string[] => (text === "" ? [] : text.replace(/\n$/, "").split("\n"));

export function duration(ms: number): string {
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

const diffKind = (l: string): "add" | "remove" | "context" =>
  l.startsWith("+") && !l.startsWith("+++") ? "add" : l.startsWith("-") && !l.startsWith("---") ? "remove" : "context";

export const toolName = (tool: ToolItem): string => (tool.name.startsWith("mcp__") ? tool.name.slice(5) : tool.name);
export const toolDetail = (tool: ToolItem): string => detailOf(tool.name, tool.args);

export const statusTone = (s: ToolItem["status"] | "failed" | "running" | "done"): Tone & Glyph => (s === "running" ? "active" : s === "failed" ? "blocked" : "done");

function diffCounts(diff: string): { added: number; removed: number } {
  const kinds = diff.split("\n").map(diffKind);
  return { added: kinds.filter((k) => k === "add").length, removed: kinds.filter((k) => k === "remove").length };
}

function statusOf(tool: ToolItem): string {
  if (tool.status === "running") return "Running";
  if (tool.status === "failed") {
    const reason = splitLines(tool.result?.text ?? "").map((l) => l.trim()).find((l) => l !== "");
    return reason === undefined ? "Failed" : `Failed: ${reason}`;
  }
  let s = "Done";
  const ms = tool.result?.durationMs;
  if (typeof ms === "number") s += ` in ${duration(ms)}`;
  if (tool.name === "edit") {
    const kinds = (tool.diff === undefined ? [] : tool.diff.split("\n")).map(diffKind);
    s += ` (+${kinds.filter((k) => k === "add").length} -${kinds.filter((k) => k === "remove").length})`;
  } else {
    const n = splitLines(tool.result?.text ?? "").length;
    s += ` (${n} ${n === 1 ? "line" : "lines"})`;
  }
  return s;
}

/** Keep every changed line, one context line either side, and hunk headers. */
function trimDiff(lines: readonly string[]): string[] {
  const keep = lines.map((l, i) => {
    if (diffKind(l) !== "context" || l.startsWith("@@")) return true;
    return [lines[i - 1], lines[i + 1]].some((n) => n !== undefined && diffKind(n) !== "context");
  });
  return lines.filter((_, i) => keep[i]);
}

function Body(props: { lines: readonly string[]; render: (line: string, i: number) => JSX.Element; wrap: (children: JSX.Element[]) => JSX.Element }): JSX.Element {
  const [all, setAll] = useState(false);
  const shown = all ? props.lines : props.lines.slice(0, TOOL_PREVIEW_LINES);
  const rest = props.lines.length - shown.length;
  return (
    <>
      {props.wrap(shown.map(props.render))}
      {rest > 0 ? (
        <button type="button" data-action="expand-output" className="py-0.5 font-mono text-meta text-fg-strong" onClick={() => setAll(true)}>
          Show {rest} more lines
        </button>
      ) : null}
    </>
  );
}

function Output(props: { result: ToolResult | undefined; diff?: string }): JSX.Element | null {
  if (props.diff !== undefined) {
    return (
      <Body
        lines={trimDiff(props.diff.split("\n"))}
        render={(l, i) => {
          const kind = diffKind(l);
          const cls = kind === "add" ? "bg-status-done/[0.11]" : kind === "remove" ? "bg-status-blocked/10" : "text-muted-foreground";
          return (
            <div key={i} data-diff={kind} className={`block px-2.5 break-words whitespace-pre-wrap text-foreground ${cls}`}>
              {kind === "context" ? null : (
                <span data-part="sign" className={kind === "add" ? "text-status-done" : "text-status-blocked"}>
                  {l.slice(0, 1)}
                </span>
              )}
              {kind === "context" ? l : l.slice(1)}
            </div>
          );
        }}
        wrap={(c) => (
          <div data-part="diff" className="rounded-md bg-card font-mono text-[11.5px] leading-[1.55]">
            {c}
          </div>
        )}
      />
    );
  }
  if (props.result === undefined) return null;
  return <PlainOutput text={props.result.text} />;
}

function PlainOutput(props: { text: string }): JSX.Element {
  const lines = splitLines(props.text);
  return <Body lines={lines} render={(l) => <span data-part="output-row">{l}</span>} wrap={(c) => <pre data-part="output" className="rounded-md bg-card px-2.5 py-1.5 font-mono text-[11.5px] leading-[1.5] break-words whitespace-pre-wrap text-muted-foreground">{c.map((e, i) => <Fragment key={i}>{i > 0 ? "\n" : ""}{e}</Fragment>)}</pre>} />;
}

export const ToolRow = memo(function ToolRow(props: { tool: ToolItem; defaultOpen?: boolean }): JSX.Element {
  const { tool } = props;
  const [open, setOpen] = useState(props.defaultOpen === true);
  const tone = statusTone(tool.status);
  const isDiff = tool.name === "edit" && tool.diff !== undefined;
  const delta = isDiff ? diffCounts(tool.diff ?? "") : null;
  return (
    <Trace data-kind="tool" data-tool={tool.name} data-status={tool.status} data-state={open ? "open" : "closed"}>
      <TraceCollapsible open={open} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger className="w-full">
          <span data-part="summary" className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2.5 text-[12px]">
            <span data-part="glyph" aria-hidden="true" data-tone={tone} data-glyph={tone} className={glyphClass[tone]} />
            <span data-part="name" className="font-mono text-muted-foreground">{toolName(tool)}</span>
            <span data-part="detail" className="min-w-0 font-mono break-words whitespace-normal text-foreground">{toolDetail(tool)}</span>
            {delta === null ? null : (
              <span data-part="delta" className="font-mono">
                <span data-part="added" className="text-status-done">{`+${delta.added}`}</span>{" "}
                <span data-part="removed" className="text-status-blocked">{`\u2212${delta.removed}`}</span>
              </span>
            )}
            <span data-part="status" className="font-mono text-meta text-fg-dim">{statusOf(tool)}</span>
          </span>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent forceMount className="mt-1 mb-0 ml-1 border-l-0 pl-0">
          <div data-part="rail" className="flex flex-col gap-1 border-l pl-4">
            {isDiff ? (open ? <Output result={tool.result} diff={tool.diff ?? ""} /> : null) : <Output result={tool.result} />}
          </div>
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
});

export const OrphanRow = memo(function OrphanRow(props: { item: OrphanResultItem }): JSX.Element {
  return (
    <article data-kind="orphan-result">
      <p>Result of {props.item.name} (call not loaded)</p>
      <PlainOutput text={props.item.result.text} />
    </article>
  );
});
