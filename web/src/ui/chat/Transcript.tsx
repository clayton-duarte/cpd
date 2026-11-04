import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { JSX, ReactNode } from "react";
import type { QueuedSubmission, ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import type { PendingSubmission } from "../../state/store.ts";
import { groupRows } from "./rows.ts";
import type { Row } from "./rows.ts";
import { DEFAULT_VIEWPORT_PX, browserSizes, findAnchor, isNearBottom, layoutRows, scrollTopForAnchor, visibleRange } from "./virtual.ts";
import type { Layout, SizeObserver } from "./virtual.ts";
import { Markdown } from "./Markdown.tsx";
import { CompactionRow, FoldRow } from "./Folds.tsx";
import { OrphanRow, ToolRow } from "./ToolRow.tsx";

const Item = memo(function Item(props: { item: Exclude<TranscriptItem, ToolItem> }): JSX.Element {
  const { item } = props;
  switch (item.kind) {
    case "user":
      return (
        <article data-kind="user">
          <div style={{ whiteSpace: "pre-wrap" }}>{item.text}</div>
          {item.attachments.map((a, i) => (
            <span key={i} data-attachment={a.kind}>
              {a.kind === "image" ? a.mimeType : a.name}
            </span>
          ))}
        </article>
      );
    case "assistant":
      return (
        <article data-kind="assistant">
          {item.blocks.map((b, i) =>
            b.type === "text" ? (
              <Markdown key={i} text={b.text} />
            ) : (
              <details key={i} data-kind="thinking">
                <summary>Thinking…</summary>
                {b.text}
              </details>
            ),
          )}
          {item.streaming ? (
            <span role="status" data-part="working">
              Working…
            </span>
          ) : null}
        </article>
      );
    case "orphanResult":
      return <OrphanRow item={item} />;
    case "fold":
      return <FoldRow item={item} />;
    case "compaction":
      return <CompactionRow item={item} />;
  }
});

const sameTools = (a: { tools: readonly ToolItem[] }, b: { tools: readonly ToolItem[] }): boolean =>
  a.tools.length === b.tools.length && a.tools.every((t, i) => t === b.tools[i]);

const ToolsRow = memo(function ToolsRow(props: { tools: readonly ToolItem[] }): JSX.Element {
  return (
    <details open data-kind="tool-group">
      <summary>{props.tools.length} tool calls</summary>
      {props.tools.map((t) => (
        <ToolRow key={t.id} tool={t} />
      ))}
    </details>
  );
}, sameTools);

function RowBody(props: { row: Row }): JSX.Element {
  const { row } = props;
  if (row.kind === "item") return row.item.kind === "tool" ? <ToolRow tool={row.item} /> : <Item item={row.item} />;
  return <ToolsRow tools={row.tools} />;
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
  const prev = useRef({ layout, tailEpoch: props.tailEpoch, pending: props.pending.length });

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
    prev.current = { layout, tailEpoch: props.tailEpoch, pending: props.pending.length };
    if (repin && !pinned) setPinned(true);
    if (pinned && log !== null) log.scrollTop = log.scrollHeight;
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
          setPinned(isNearBottom({ scrollTop: log.scrollTop, clientHeight: log.clientHeight, scrollHeight: log.scrollHeight }));
        }}
      >
        {props.hasEarlier ? (
          <button type="button" data-action="load-earlier" onClick={props.onLoadEarlier}>
            Load earlier
          </button>
        ) : null}
        <div ref={spaceRef} data-part="virtual-space" style={{ height: `${layout.total}px`, position: "relative" }}>
          <div data-part="virtual-window" style={{ transform: `translateY(${layout.offsets[start] ?? 0}px)` }}>
            {rows.slice(start, end).map((row) => (
              <MeasuredRow key={row.key} rowKey={row.key} sizes={sizes} onHeight={onHeight}>
                <RowBody row={row} />
              </MeasuredRow>
            ))}
          </div>
        </div>
        {props.queued.length === 0 && props.pending.length === 0 ? null : (
          <ul aria-label="Queued messages">
            {props.queued.map((q) => (
              <li key={q.clientSubmissionId} data-queued={q.clientSubmissionId}>
                {q.text}
              </li>
            ))}
            {props.pending.map((p) => (
              <li key={p.clientSubmissionId} data-pending={p.state}>
                {p.text} {p.state === "sending" ? "sending…" : `failed: ${p.error?.message ?? "unknown error"}`}
              </li>
            ))}
          </ul>
        )}
      </div>
      {pinned ? null : (
        <button type="button" data-action="jump-to-bottom" onClick={jump}>
          Jump to bottom
        </button>
      )}
    </div>
  );
}
