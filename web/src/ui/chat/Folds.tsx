import type { JSX } from "react";
import { Markdown } from "./Markdown.tsx";
import { memo } from "react";
import type { CompactionItem, FoldItem } from "../../protocol/domain.ts";

export const FoldRow = memo(function FoldRow(props: { item: FoldItem }): JSX.Element {
  const { item } = props;
  return (
    <details data-kind="fold" data-fold={item.foldKind} open={item.defaultOpen}>
      <summary>{item.title}</summary>
      <Markdown text={item.body} />
    </details>
  );
});

export const CompactionRow = memo(function CompactionRow(props: { item: CompactionItem }): JSX.Element {
  const { item } = props;
  return (
    <details data-kind="compaction">
      <summary>{item.tokensBefore === undefined ? "Compaction" : `Compaction (${item.tokensBefore} tokens)`}</summary>
      <Markdown text={item.summary} />
    </details>
  );
});
