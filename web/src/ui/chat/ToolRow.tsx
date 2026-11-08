import {
  CircleXIcon,
  FilePenIcon,
  FileTextIcon,
  SearchIcon,
  TerminalIcon,
  WrenchIcon,
} from "lucide-react";
import { memo, useState } from "react";
import type { JSX } from "react";
import { TOOL_PREVIEW_LINES } from "../../protocol/domain.ts";
import type {
  OrphanResultItem,
  ToolItem,
  ToolResult,
} from "../../protocol/domain.ts";
import { Button } from "../components/button.tsx";
import {
  CodeBlock,
  CodeBlockActions,
  CodeBlockContent,
  CodeBlockCopyButton,
  CodeBlockFooter,
  CodeBlockLine,
} from "../components/code-block.tsx";
import { Spinner } from "../components/spinner.tsx";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "../components/trace.tsx";
import type { Glyph, Tone } from "../dango-status.ts";

const DETAIL_KEYS = [
  "command",
  "query",
  "queries",
  "q",
  "url",
  "urls",
  "path",
  "file_path",
  "paths",
  "pattern",
  "prompt",
  "question",
  "claim",
  "name",
  "id",
  "tool",
];

const cut = (s: string, n: number): string =>
  s.length > n ? `${s.slice(0, n)}…` : s;
const tidy = (s: string): string =>
  s
    .replace(/[\u0000-\u001f\u007f]/g, " ")
    .replace(/ +/g, " ")
    .trim();
const asText = (v: unknown): string => {
  if (Array.isArray(v)) return v.map(asText).join(", ");
  if (typeof v === "string") return v;
  return JSON.stringify(v) ?? "";
};
const str = (v: unknown): string => (typeof v === "string" ? v : "");

function detailOf(
  name: string,
  args: Readonly<Record<string, unknown>>,
): string {
  switch (name) {
    case "read":
    case "write":
    case "edit":
    case "ls":
      return cut(str(args["path"]), 100);
    case "bash":
    case "powershell":
      return cut(str(args["command"]).split("\n")[0] ?? "", 100);
    case "grep":
      return cut(`/${str(args["pattern"])}/ in ${str(args["path"])}`, 100);
    case "find":
      return cut(`${str(args["pattern"])} in ${str(args["path"])}`, 100);
  }
  const key = DETAIL_KEYS.find((k) => k in args);
  const first = Object.values(args)[0];
  const v = key !== undefined ? args[key] : first;
  if (v === undefined) return "";
  return cut(tidy(asText(v)), 160);
}

const splitLines = (text: string): string[] =>
  text === "" ? [] : text.replace(/\n$/, "").split("\n");

export function duration(ms: number): string {
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

const diffKind = (l: string): "add" | "remove" | "context" =>
  l.startsWith("+") && !l.startsWith("+++")
    ? "add"
    : l.startsWith("-") && !l.startsWith("---")
      ? "remove"
      : "context";

export const toolName = (tool: ToolItem): string =>
  tool.name.startsWith("mcp__") ? tool.name.slice(5) : tool.name;
export const toolDetail = (tool: ToolItem): string =>
  detailOf(tool.name, tool.args);

export const statusTone = (
  s: ToolItem["status"] | "failed" | "running" | "done",
): Tone & Glyph =>
  s === "running" ? "active" : s === "failed" ? "blocked" : "done";

function diffCounts(diff: string): { added: number; removed: number } {
  const kinds = diff.split("\n").map(diffKind);
  return {
    added: kinds.filter((k) => k === "add").length,
    removed: kinds.filter((k) => k === "remove").length,
  };
}

function statusOf(tool: ToolItem): string {
  if (tool.status === "running") return "Running";
  if (tool.status === "failed") return "Failed";
  let s = "Done";
  const ms = tool.result?.durationMs;
  if (typeof ms === "number") s += ` in ${duration(ms)}`;
  if (tool.name === "edit") {
    const kinds = (tool.diff === undefined ? [] : tool.diff.split("\n")).map(
      diffKind,
    );
    s += ` (+${kinds.filter((k) => k === "add").length} -${kinds.filter((k) => k === "remove").length})`;
  } else {
    const n = splitLines(tool.result?.text ?? "").length;
    s += ` (${n} ${n === 1 ? "line" : "lines"})`;
  }
  return s;
}

export function ToolIcon(props: {
  name: string;
  status: ToolItem["status"] | "done";
}): JSX.Element {
  if (props.status === "running") {
    return <Spinner />;
  }
  if (props.status === "failed") {
    return <CircleXIcon className="text-destructive" />;
  }
  switch (props.name) {
    case "bash":
    case "powershell":
      return <TerminalIcon />;
    case "read":
    case "ls":
      return <FileTextIcon />;
    case "edit":
    case "write":
      return <FilePenIcon />;
    case "grep":
    case "find":
      return <SearchIcon />;
    default:
      return <WrenchIcon />;
  }
}

function Output(props: {
  result: ToolResult | undefined;
  diff?: string;
  failed?: boolean;
}): JSX.Element | null {
  const [all, setAll] = useState(false);

  if (props.diff !== undefined) {
    const lines = props.diff.split("\n");
    const shown = all ? lines : lines.slice(0, TOOL_PREVIEW_LINES);
    const rest = lines.length - shown.length;
    return (
      <CodeBlock size="sm" data-part="diff">
        <CodeBlockActions>
          <CodeBlockCopyButton value={props.diff} />
        </CodeBlockActions>
        <CodeBlockContent className="max-h-64">
          {shown.map((l, i) => {
            const kind = diffKind(l);
            const variant =
              kind === "add"
                ? "added"
                : kind === "remove"
                  ? "removed"
                  : "default";
            return (
              <CodeBlockLine key={i} data-diff={kind} variant={variant}>
                {kind === "context" ? l : l.slice(1)}
              </CodeBlockLine>
            );
          })}
        </CodeBlockContent>
        {rest > 0 ? (
          <CodeBlockFooter>
            <Button
              type="button"
              variant="ghost"
              size="xs"
              data-action="expand-output"
              className="font-mono"
              onClick={() => setAll(true)}
            >
              Show {rest} more lines
            </Button>
          </CodeBlockFooter>
        ) : null}
      </CodeBlock>
    );
  }

  if (props.result === undefined) return null;
  return <PlainOutput text={props.result.text} failed={props.failed} />;
}

function PlainOutput(props: { text: string; failed?: boolean }): JSX.Element {
  const [all, setAll] = useState(false);
  const lines = splitLines(props.text);
  const shown = all ? lines : lines.slice(0, TOOL_PREVIEW_LINES);
  const rest = lines.length - shown.length;
  const shownText = shown.join("\n");
  return (
    <CodeBlock size="sm" variant={props.failed ? "destructive" : "default"}>
      <CodeBlockActions>
        <CodeBlockCopyButton value={props.text} />
      </CodeBlockActions>
      <CodeBlockContent wrap className="max-h-64" data-part="output">
        {shownText}
      </CodeBlockContent>
      {rest > 0 ? (
        <CodeBlockFooter>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            data-action="expand-output"
            className="font-mono"
            onClick={() => setAll(true)}
          >
            Show {rest} more lines
          </Button>
        </CodeBlockFooter>
      ) : null}
    </CodeBlock>
  );
}

export const ToolRow = memo(function ToolRow(props: {
  tool: ToolItem;
  nested?: boolean;
}): JSX.Element {
  const { tool } = props;
  const [open, setOpen] = useState(false);
  const tone = statusTone(tool.status);
  const delta =
    tool.name === "edit" && tool.diff !== undefined
      ? diffCounts(tool.diff)
      : null;
  const traceStatus =
    tool.status === "running"
      ? "running"
      : tool.status === "failed"
        ? "error"
        : undefined;
  const hasOutput =
    (tool.name === "edit" &&
      tool.diff !== undefined &&
      tool.diff.trim() !== "") ||
    (tool.result !== undefined && tool.result.text.trim() !== "");

  const line = (
    <TraceLine status={traceStatus}>
      <TraceIcon data-part="glyph" data-tone={tone} data-glyph={tone}>
        <ToolIcon name={tool.name} status={tool.status} />
      </TraceIcon>
      <span data-part="name" className="shrink-0 text-sm font-sans text-muted-foreground">
        {toolName(tool)}
      </span>
      <TraceTitle data-part="detail" className="font-mono text-xs text-muted-foreground/70">
        {toolDetail(tool)}
      </TraceTitle>
      {delta === null ? null : (
        <TraceDescription data-part="delta" className="font-mono text-xs text-muted-foreground/70">
          <span data-part="added" className="text-status-done">
            {`+${delta.added}`}
          </span>{" "}
          <span data-part="removed" className="text-status-blocked">
            {`\u2212${delta.removed}`}
          </span>
        </TraceDescription>
      )}
      <TraceDescription data-part="status" className="font-mono text-xs text-muted-foreground/70">
        {statusOf(tool)}
      </TraceDescription>
    </TraceLine>
  );

  if (!hasOutput) {
    return (
      <div
        data-kind="tool"
        data-tool={tool.name}
        data-status={tool.status}
      >
        {line}
      </div>
    );
  }

  return (
    <TraceCollapsible
      data-kind="tool"
      data-tool={tool.name}
      data-status={tool.status}
      open={open}
      onOpenChange={setOpen}
    >
      <TraceCollapsibleTrigger>{line}</TraceCollapsibleTrigger>
      <TraceCollapsibleContent>
        <Output
          result={tool.result}
          failed={tool.status === "failed"}
          {...(tool.name === "edit" && tool.diff !== undefined
            ? { diff: tool.diff }
            : {})}
        />
      </TraceCollapsibleContent>
    </TraceCollapsible>
  );
});

export const OrphanRow = memo(function OrphanRow(props: {
  item: OrphanResultItem;
}): JSX.Element {
  const [open, setOpen] = useState(true);
  return (
    <Trace data-kind="orphan-result">
      <TraceCollapsible open={open} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger>
          <TraceLine className="font-mono text-xs">
            <TraceIcon>
              <WrenchIcon />
            </TraceIcon>
            <TraceTitle>
              Result of {props.item.name} (call not loaded)
            </TraceTitle>
          </TraceLine>
        </TraceCollapsibleTrigger>
        <TraceCollapsibleContent>
          <PlainOutput text={props.item.result.text} />
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
});
