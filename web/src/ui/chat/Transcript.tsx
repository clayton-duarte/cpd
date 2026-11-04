import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { JSX, ReactNode } from "react";
import type { QueuedSubmission, ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import type { PendingSubmission } from "../../state/store.ts";
import { groupRows } from "./rows.ts";
import type { Row } from "./rows.ts";
import { DEFAULT_VIEWPORT_PX, browserSizes, findAnchor, isNearBottom, layoutRows, scrollTopForAnchor, visibleRange } from "./virtual.ts";
import type { Layout, SizeObserver } from "./virtual.ts";
import { Markdown } from "./Markdown.tsx";
import { CompactionRow, FoldRow, formatClock } from "./Folds.tsx";
import { FileTextIcon, ImageIcon } from "lucide-react";
import { Button } from "../components/button.tsx";
import { glyphClass } from "../dango-status.ts";
import { ToolGroup } from "./ToolGroup.tsx";
import { OrphanRow, ToolRow } from "./ToolRow.tsx";

const BTN_PILL = "[&::-webkit-details-marker]:hidden";

function ThinkingPill(props: { text: string; time: string | null; defaultOpen: boolean }): JSX.Element {
  const [open, setOpen] = useState(props.defaultOpen);
  return (
    <details data-kind="thinking" open={open} onToggle={(e) => setOpen(e.currentTarget.open)} className="group self-start">
      <summary
        className={`flex h-6 cursor-pointer list-none items-center gap-2 rounded-xl border border-border px-2.5 text-[12px] text-muted-foreground hover:bg-row-hover ${BTN_PILL}`}
      >
        <span data-part="caret" aria-hidden="true" className="font-mono text-[10px] group-open:rotate-90">▸</span>
        <span data-part="label">Thinking</span>
        {props.time === null ? null : <span data-part="meta" className="font-mono text-fg-dim">{props.time}</span>}
      </summary>
      <div data-part="thinking-body" className="mt-1 ml-1 whitespace-pre-wrap border-l-2 border-border pl-3 text-body text-muted-foreground">
        {props.text}
      </div>
    </details>
  );
}

const Item = memo(function Item(props: {
  item: Exclude<TranscriptItem, ToolItem>;
  sessionKey: string | undefined;
  thinking: "folded" | "expanded";
}): JSX.Element {
  const { item, sessionKey, thinking } = props;
  switch (item.kind) {
    case "user": {
      const clock = formatClock(item.at);
      return (
        <article data-kind="user" className="flex flex-col items-end gap-1">
          <div data-part="bubble" className="flex max-w-[78%] flex-col gap-2 rounded-[10px] border border-input bg-surface-raised px-3 py-[9px]">
            <div data-part="text" style={{ whiteSpace: "pre-wrap" }} className="m-0 text-body text-fg-strong">{item.text}</div>
            {item.attachments.length === 0 ? null : (
              <div data-part="attachments" className="flex flex-wrap gap-2">
                {item.attachments.map((a, i) => (
                  <span key={i} data-attachment={a.kind} className="flex items-center gap-2 rounded-md border border-input py-1 pr-2 pl-1">
                    <span data-part="thumb" aria-hidden="true" className="grid h-6 w-[34px] place-items-center rounded-[3px] bg-edge text-muted-foreground">
                      {a.kind === "image" && a.dataBase64 !== undefined ? (
                        <img alt="" src={`data:${a.mimeType};base64,${a.dataBase64}`} className="h-6 w-[34px] rounded-[3px] object-cover" />
                      ) : a.kind === "image" ? (
                        <ImageIcon size={14} />
                      ) : (
                        <FileTextIcon size={14} />
                      )}
                    </span>
                    <span data-part="name" className="font-mono text-[11.5px] text-fg-strong">
                      {a.kind === "image" ? a.mimeType : a.name}
                    </span>
                  </span>
                ))}
              </div>
            )}
          </div>
          <span data-part="meta" className="font-mono text-meta text-muted-foreground">
            {clock === null ? "you" : `you · ${clock}`}
          </span>
        </article>
      );
    }
    case "assistant": {
      const clock = formatClock(item.at);
      let lastText = -1;
      item.blocks.forEach((b, i) => {
        if (b.type === "text") lastText = i;
      });
      return (
        <article data-kind="assistant" className="flex flex-col gap-1.5">
          {item.blocks.map((b, i) =>
            b.type === "text" ? (
              <div key={i} data-part="card" className="flex flex-col gap-1.5 rounded-[10px] border border-border bg-card px-3.5 py-2.5">
                <Markdown text={b.text} />
                {i === lastText && sessionKey !== undefined ? (
                  <span data-part="stamp" className="self-end font-mono text-meta text-muted-foreground">
                    {clock === null ? sessionKey : `${sessionKey} · ${clock}`}
                  </span>
                ) : null}
              </div>
            ) : (
              <ThinkingPill key={`${i}:${thinking}`} text={b.text} time={clock} defaultOpen={thinking === "expanded"} />
            ),
          )}
          {item.streaming ? (
            <span role="status" data-part="working" className="flex h-[26px] items-center gap-2.5 px-2 text-[12px] text-mizu-bright">
              <span aria-hidden="true" className={glyphClass.active} />
              Working…
            </span>
          ) : null}
        </article>
      );
    }
    case "orphanResult":
      return <OrphanRow item={item} />;
    case "fold":
      return <FoldRow item={item} />;
    case "compaction":
      return <CompactionRow item={item} />;
  }
});

function RowBody(props: { row: Row; sessionKey: string | undefined; thinking: "folded" | "expanded"; tools: "grouped" | "expanded" }): JSX.Element {
  const { row } = props;
  const body =
    row.kind === "item" ? (
      row.item.kind === "tool" ? (
        <div data-part="tool-rail" className="pl-3 shadow-[inset_2px_0_0_var(--input)]">
          <ToolRow tool={row.item} />
        </div>
      ) : (
        <Item item={row.item} sessionKey={props.sessionKey} thinking={props.thinking} />
      )
    ) : (
      <ToolGroup key={props.tools} tools={row.tools} mode={props.tools} />
    );
  return (
    <div data-part="row-body" className="py-[3px]">
      {body}
    </div>
  );
}

function MeasuredRow(props: { rowKey: string; sizes: SizeObserver; onHeight(key: string, height: number): void; children: ReactNode }): JSX.Element {
  const { rowKey, sizes } = props;
  const ref = useRef<HTMLDivElement>(null);
  const cb = useRef(props.onHeight);
  cb.current = props.onHeight;
  useEffect(() => {
    const el = ref.current;
    if (el === null) return undefined;
    const d = sizes.observe(el, (h) => cb.current(rowKey, h));
    return () => d.dispose();
  }, [rowKey, sizes]);
  return (
    <div ref={ref} data-row-key={rowKey}>
      {props.children}
    </div>
  );
}

export function Transcript(props: {
  items: readonly TranscriptItem[];
  hasEarlier: boolean;
  queued: readonly QueuedSubmission[];
  pending: readonly PendingSubmission[];
  onLoadEarlier(): void;
  tailEpoch: number;
  sizes?: SizeObserver;
  sessionKey?: string;
  thinking?: "folded" | "expanded";
  tools?: "grouped" | "expanded";
  busy?: boolean;
}): JSX.Element {
  const defaultSizes = useMemo(() => browserSizes(), []);
  const sizes = props.sizes ?? defaultSizes;
  const rows = useMemo(() => groupRows(props.items), [props.items]);
  const keys = useMemo(() => rows.map((r) => r.key), [rows]);
  const heights = useRef(new Map<string, number>());
  const [, setVersion] = useState(0);
  const [viewport, setViewport] = useState(DEFAULT_VIEWPORT_PX);
  const [scrollTop, setScrollTop] = useState(0);
  const [pinned, setPinned] = useState(true);
  const logRef = useRef<HTMLDivElement>(null);
  const spaceRef = useRef<HTMLDivElement>(null);

  const layout = layoutRows(keys, heights.current);
  const layoutRef = useRef<Layout>(layout);
  layoutRef.current = layout;
  const pinnedRef = useRef(pinned);
  pinnedRef.current = pinned;
  const topRef = useRef(0); // scrollTop relative to virtual-space
  const lastScrollTop = useRef(0);
  const busy = props.busy === true;
  const last = props.items[props.items.length - 1];
  const showCue = busy && !(last?.kind === "assistant" && last.streaming === true);
  const cueText = last?.kind === "tool" && last.status === "running" ? `Using ${last.name}…` : "Thinking…";
  const prev = useRef({ layout, tailEpoch: props.tailEpoch, pending: props.pending.length, pinned: false, busy: props.busy === true, cue: showCue });

  // the log is position: relative (app.css), so offsetTop is relative to its scrolling content
  const spaceTop = (): number => spaceRef.current?.offsetTop ?? 0;

  const effectiveTop = pinned ? Math.max(0, layout.total - viewport) : scrollTop;
  const { start, end } = visibleRange(layout, effectiveTop, viewport);

  const onHeight = useRef((key: string, h: number): void => {
    const lay = layoutRef.current;
    const i = lay.keys.indexOf(key);
    if (i < 0) return;
    const old = lay.offsets[i + 1]! - lay.offsets[i]!;
    if (Math.abs(h - old) < 1) return;
    const log = logRef.current;
    if (!pinnedRef.current && log !== null && lay.offsets[i + 1]! <= topRef.current) {
      const top = topRef.current + (h - old);
      topRef.current = top;
      log.scrollTop += h - old;
      setScrollTop(top);
    }
    heights.current.set(key, h);
    setVersion((v) => v + 1);
  }).current;

  useEffect(() => {
    const log = logRef.current;
    if (log === null) return undefined;
    const d = sizes.observe(log, (h) => setViewport(h > 0 ? h : DEFAULT_VIEWPORT_PX));
    return () => d.dispose();
  }, [sizes]);

  useLayoutEffect(() => {
    const log = logRef.current;
    const p = prev.current;
    const epochSame = p.tailEpoch === props.tailEpoch;
    const firstChanged = p.layout.keys[0] !== keys[0];
    let repin = !epochSame || props.pending.length > p.pending;
    if (!repin && !pinned && log !== null && firstChanged && epochSame && p.layout.keys.length > 0 && keys.includes(p.layout.keys[0]!)) {
      const anchor = findAnchor(p.layout, topRef.current);
      const top = anchor === null ? null : scrollTopForAnchor(layout, anchor);
      if (top !== null) {
        log.scrollTop = spaceTop() + top;
        topRef.current = top;
        setScrollTop(top);
      }
    }
    const grew = layout.total !== p.layout.total || !epochSame || props.pending.length > p.pending || (busy && !p.busy) || showCue !== p.cue;
    const justPinned = pinned && !p.pinned;
    prev.current = { layout, tailEpoch: props.tailEpoch, pending: props.pending.length, pinned, busy, cue: showCue };
    if (repin && !pinned) setPinned(true);
    if (pinned && log !== null && (grew || justPinned)) {
      log.scrollTop = log.scrollHeight;
      lastScrollTop.current = log.scrollTop;
    }
  });

  const jump = (): void => {
    setPinned(true);
  };

  return (
    <div data-part="transcript">
      <div
        ref={logRef}
        role="log"
        aria-label="Transcript"
        data-at-bottom={pinned ? "true" : "false"}
        onScroll={(e) => {
          const log = e.currentTarget;
          topRef.current = log.scrollTop - spaceTop();
          setScrollTop(topRef.current);
          if (log.scrollTop < lastScrollTop.current - 1) setPinned(false);
          else setPinned(isNearBottom({ scrollTop: log.scrollTop, clientHeight: log.clientHeight, scrollHeight: log.scrollHeight }));
          lastScrollTop.current = log.scrollTop;
        }}
      >
        {props.hasEarlier ? (
          <button
            type="button"
            data-action="load-earlier"
            className="mx-auto my-2 block rounded-[10px] border border-border px-2.5 py-0.5 font-mono text-meta text-muted-foreground hover:bg-row-hover"
            onClick={props.onLoadEarlier}
          >
            Load earlier
          </button>
        ) : null}
        <div ref={spaceRef} data-part="virtual-space" style={{ height: `${layout.total}px`, position: "relative" }}>
          <div data-part="virtual-window" className="px-4" style={{ transform: `translateY(${layout.offsets[start] ?? 0}px)` }}>
            {rows.slice(start, end).map((row) => (
              <MeasuredRow key={row.key} rowKey={row.key} sizes={sizes} onHeight={onHeight}>
                <RowBody row={row} sessionKey={props.sessionKey} thinking={props.thinking ?? "folded"} tools={props.tools ?? "grouped"} />
              </MeasuredRow>
            ))}
          </div>
        </div>
        {showCue ? (
          <div role="status" data-part="working-cue" className="flex h-[26px] items-center gap-2.5 px-6 text-[12px] text-mizu-bright">
            <span aria-hidden="true" className={glyphClass.active} />
            {cueText}
          </div>
        ) : null}
        {props.queued.length === 0 && props.pending.length === 0 ? null : (
          <ul aria-label="Queued messages" className="m-0 flex list-none flex-col gap-0 p-0 px-4 pb-2.5">
            {props.queued.map((q) => (
              <li key={q.clientSubmissionId} data-queued={q.clientSubmissionId} className="flex flex-col items-end gap-1 py-[3px]">
                <div data-part="bubble" className="max-w-[78%] rounded-[10px] border border-dashed border-line bg-surface-raised px-3 py-[9px]">
                  <div data-part="text" className="whitespace-pre-wrap text-body text-fg-strong">{q.text}</div>
                </div>
                <span data-part="queued-status" className="font-mono text-meta text-muted-foreground">queued</span>
              </li>
            ))}
            {props.pending.map((p) => (
              <li key={p.clientSubmissionId} data-pending={p.state} className="flex flex-col items-end gap-1 py-[3px]">
                <div
                  data-part="bubble"
                  className={
                    p.state === "failed"
                      ? "max-w-[78%] rounded-[10px] border border-status-blocked bg-status-blocked/10 px-3 py-[9px]"
                      : "max-w-[78%] rounded-[10px] border border-input bg-surface-raised px-3 py-[9px] opacity-70"
                  }
                >
                  <div data-part="text" className="whitespace-pre-wrap text-body text-fg-strong">{p.text}</div>
                </div>
                {p.state === "failed" ? (
                  <span data-part="pending-status" data-state="failed" role="alert" className="flex items-center gap-1.5 font-mono text-meta text-status-blocked">
                    <span aria-hidden="true" className={glyphClass.blocked} />
                    {`failed: ${p.error?.message ?? "unknown error"}`}
                  </span>
                ) : (
                  <span data-part="pending-status" data-state={p.state} className="font-mono text-meta text-muted-foreground">
                    {p.state === "sending" ? "sending…" : "sent"}
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
      {pinned ? null : (
        <Button
          variant="outline"
          size="sm"
          type="button"
          data-action="jump-to-bottom"
          className="rounded-full border-input bg-popover px-3 text-[12px] text-fg-strong"
          onClick={jump}
        >
          Jump to bottom
        </Button>
      )}
    </div>
  );
}
