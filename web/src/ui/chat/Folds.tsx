import type { JSX } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { CompactionItem, FoldItem } from "../../protocol/domain.ts";

export function FoldRow(props: { item: FoldItem }): JSX.Element {
  const { item } = props;
  return (
    <details data-kind="fold" data-fold={item.foldKind} open={item.defaultOpen}>
      <summary>{item.title}</summary>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{item.body}</ReactMarkdown>
    </details>
  );
}

export function CompactionRow(props: { item: CompactionItem }): JSX.Element {
  const { item } = props;
  return (
    <details data-kind="compaction">
      <summary>{item.tokensBefore === undefined ? "Compaction" : `Compaction (${item.tokensBefore} tokens)`}</summary>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{item.summary}</ReactMarkdown>
    </details>
  );
}
