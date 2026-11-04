import { memo, useState } from "react";
import type { JSX } from "react";
import { TOOL_PREVIEW_LINES } from "../../protocol/domain.ts";
import { glyphClass } from "../dango-status.ts";
import type { Glyph, Tone } from "../dango-status.ts";
import type { OrphanResultItem, ToolItem, ToolResult } from "../../protocol/domain.ts";

const DETAIL_KEYS = ["command", "query", "queries", "q", "url", "urls", "path", "file_path", "paths", "pattern", "prompt", "question", "claim", "name", "id", "tool"];

const cut = (s: string, n: number): string => (s.length > n ? `${s.slice(0, n)}…` : s);
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
      return cut(str(args["command"]).split("\n")[0] ?? "", 160);
    case "grep":
      return `/${str(args["pattern"])}/ in ${str(args["path"])}`;
    case "find":
      return `${str(args["pattern"])} in ${str(args["path"])}`;
  }
  const key = DETAIL_KEYS.find((k) => k in args);
  const first = Object.values(args)[0];
  const v = key !== undefined ? args[key] : first;
  if (v === undefined) return "";
  return cut(tidy(asText(v)), 160);
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
    return reason === undefined ? "Failed" : `Failed: ${cut(reason, 140)}`;
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

function Body(props: { lines: readonly string[]; render: (line: string, i: number) => JSX.Element; wrap: (children: JSX.Element[]) => JSX.Element }): JSX.Element {
  const [all, setAll] = useState(false);
  const shown = all ? props.lines : props.lines.slice(0, TOOL_PREVIEW_LINES);
  const rest = props.lines.length - shown.length;
  return (
    <>
      {props.wrap(shown.map(props.render))}
      {rest > 0 ? (
        <button type="button" data-action="expand-output" className="mr-2 ml-11 py-0.5 font-mono text-meta text-fg-strong" onClick={() => setAll(true)}>
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
        lines={props.diff.split("\n")}
        render={(l, i) => {
          const kind = diffKind(l);
          const cls = kind === "add" ? "bg-status-done/[0.11]" : kind === "remove" ? "bg-status-blocked/10" : "text-muted-foreground";
          return (
            <div key={i} data-diff={kind} className={`block px-2.5 whitespace-pre text-foreground ${cls}`}>
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
          <div data-part="diff" className="mr-2 mb-0.5 ml-11 overflow-hidden rounded-md bg-card font-mono text-[11.5px] leading-[1.55]">
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
  return <Body lines={lines} render={(l) => <>{l}</>} wrap={(c) => <pre data-part="output" className="mr-2 mb-0.5 ml-11 rounded-md bg-card px-2.5 py-1.5 font-mono text-[11.5px] leading-[1.5] whitespace-pre-wrap text-muted-foreground">{c.map((e, i) => <span key={i}>{i > 0 ? "\n" : ""}{e}</span>)}</pre>} />;
}

export const ToolRow = memo(function ToolRow(props: { tool: ToolItem; nested?: boolean }): JSX.Element {
  const { tool } = props;
  const tone = statusTone(tool.status);
  const delta = tool.name === "edit" && tool.diff !== undefined ? diffCounts(tool.diff) : null;
  return (
    <details data-kind="tool" data-tool={tool.name} data-status={tool.status} open={tool.name === "edit"} className="group">
      <summary
        className={`flex h-[22px] cursor-pointer list-none items-center gap-2.5 rounded-[5px] pr-2 text-[12px] hover:bg-row-hover [&::-webkit-details-marker]:hidden ${props.nested === true ? "pl-[26px]" : "pl-2"}`}
      >
        <span data-part="glyph" aria-hidden="true" data-tone={tone} data-glyph={tone} className={glyphClass[tone]} />
        <span data-part="name" className="min-w-10 font-mono text-muted-foreground">{toolName(tool)}</span>
        <span data-part="detail" className="min-w-0 flex-1 truncate font-mono text-foreground">{toolDetail(tool)}</span>
        {delta === null ? null : (
          <span data-part="delta" className="font-mono">
            <span data-part="added" className="text-status-done">{`+${delta.added}`}</span>{" "}
            <span data-part="removed" className="text-status-blocked">{`\u2212${delta.removed}`}</span>
          </span>
        )}
        <span data-part="status" className="font-mono text-meta text-fg-dim">{statusOf(tool)}</span>
      </summary>
      <Output result={tool.result} {...(tool.name === "edit" && tool.diff !== undefined ? { diff: tool.diff } : {})} />
    </details>
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
