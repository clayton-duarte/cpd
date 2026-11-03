import type { JSX } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { QueuedSubmission, ToolItem, TranscriptItem, WhenBusy, WireError } from "../../protocol/domain.ts";
import { CompactionRow, FoldRow } from "./Folds.tsx";
import { OrphanRow, ToolRow } from "./ToolRow.tsx";

// Structural copy of PendingSubmission (web/src/state/store.ts, contract 6.2), which does not exist yet.
export interface PendingSubmission {
  readonly clientSubmissionId: string;
  readonly text: string;
  readonly imageCount: number;
  readonly whenBusy: WhenBusy;
  readonly state: "sending" | "failed";
  readonly error?: WireError;
}

function Item(props: { item: Exclude<TranscriptItem, ToolItem> }): JSX.Element {
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
              <ReactMarkdown key={i} remarkPlugins={[remarkGfm]}>
                {b.text}
              </ReactMarkdown>
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
}

export function Transcript(props: {
  items: readonly TranscriptItem[];
  hasEarlier: boolean;
  queued: readonly QueuedSubmission[];
  pending: readonly PendingSubmission[];
  onLoadEarlier(): void;
}): JSX.Element {
  const nodes: JSX.Element[] = [];
  let run: ToolItem[] = [];
  const flush = (): void => {
    if (run.length === 1) {
      const t = run[0] as ToolItem;
      nodes.push(<ToolRow key={t.id} tool={t} />);
    } else if (run.length > 1) {
      nodes.push(
        <details key={`group:${(run[0] as ToolItem).id}`} open data-kind="tool-group">
          <summary>{run.length} tool calls</summary>
          {run.map((t) => (
            <ToolRow key={t.id} tool={t} />
          ))}
        </details>,
      );
    }
    run = [];
  };
  for (const item of props.items) {
    if (item.kind === "tool") {
      run.push(item);
      continue;
    }
    flush();
    nodes.push(<Item key={item.id} item={item} />);
  }
  flush();
  return (
    <>
      <div role="log" aria-label="Transcript">
        {props.hasEarlier ? (
          <button type="button" data-action="load-earlier" onClick={props.onLoadEarlier}>
            Load earlier
          </button>
        ) : null}
        {nodes}
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
    </>
  );
}
