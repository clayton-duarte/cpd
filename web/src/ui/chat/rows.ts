import type { AssistantBlock, ToolItem, TranscriptItem } from "../../protocol/domain.ts";

export type ChainStep =
  | {
      readonly kind: "thinking";
      readonly key: string;
      readonly text: string;
      readonly at?: string;
      readonly running: boolean;
    }
  | {
      readonly kind: "tool";
      readonly key: string;
      readonly tool: ToolItem;
    }
  | {
      readonly kind: "tools";
      readonly key: string;
      readonly tools: readonly ToolItem[];
    };

export type Row =
  | { readonly kind: "item"; readonly key: string; readonly item: TranscriptItem }
  | { readonly kind: "tools"; readonly key: string; readonly tools: readonly ToolItem[] }
  | {
      readonly kind: "chain";
      readonly key: string;
      readonly steps: readonly ChainStep[];
      readonly stepCount: number;
      readonly toolCount: number;
      readonly thoughtCount: number;
      readonly failedCount: number;
      readonly running: boolean;
      readonly at?: string;
    };

type RawChainEntry =
  | { kind: "thinking"; key: string; text: string; at?: string; running: boolean }
  | { kind: "tool"; item: ToolItem };

function flushChain(entries: RawChainEntry[]): Row | null {
  if (entries.length === 0) return null;
  const steps: ChainStep[] = [];
  let thoughtCount = 0;
  let toolCount = 0;
  let failedCount = 0;
  let running = false;
  let lastAt: string | undefined;

  let i = 0;
  while (i < entries.length) {
    const e = entries[i]!;
    if (e.kind === "thinking") {
      thoughtCount++;
      if (e.running) running = true;
      if (e.at !== undefined) lastAt = e.at;
      steps.push({
        kind: "thinking",
        key: e.key,
        text: e.text,
        ...(e.at === undefined ? {} : { at: e.at }),
        running: e.running,
      });
      i++;
      continue;
    }
    const tools: ToolItem[] = [e.item];
    while (i + tools.length < entries.length && entries[i + tools.length]!.kind === "tool") {
      tools.push((entries[i + tools.length] as { kind: "tool"; item: ToolItem }).item);
    }
    for (const t of tools) {
      toolCount++;
      if (t.status === "failed") failedCount++;
      if (t.status === "running") running = true;
    }
    if (tools.length > 1) {
      steps.push({ kind: "tools", key: `group:${e.item.id}`, tools });
    } else {
      steps.push({ kind: "tool", key: e.item.id, tool: e.item });
    }
    i += tools.length;
  }

  const firstKey = steps[0]!.key;
  return {
    kind: "chain",
    key: `chain:${firstKey}`,
    steps,
    stepCount: thoughtCount + toolCount,
    toolCount,
    thoughtCount,
    failedCount,
    running,
    ...(lastAt === undefined ? {} : { at: lastAt }),
  };
}

export function groupRows(items: readonly TranscriptItem[]): Row[] {
  const rows: Row[] = [];
  let chainBuffer: RawChainEntry[] = [];

  const commitChain = (): void => {
    const chain = flushChain(chainBuffer);
    if (chain !== null) rows.push(chain);
    chainBuffer = [];
  };

  for (const item of items) {
    if (item.kind === "tool") {
      chainBuffer.push({ kind: "tool", item });
      continue;
    }
    if (item.kind === "assistant") {
      let textBlocks: Extract<AssistantBlock, { type: "text" }>[] = [];
      let textStart = 0;
      let thoughtIndex = 0;
      let emittedText = false;
      let hasThinking = false;
      let hasText = false;
      const commitText = (last: boolean): void => {
        if (textBlocks.length === 0) return;
        commitChain();
        rows.push({
          kind: "item",
          key: emittedText ? `${item.id}:text:${textStart}` : item.id,
          item: { ...item, streaming: item.streaming && last, blocks: textBlocks },
        });
        emittedText = true;
        textBlocks = [];
      };
      item.blocks.forEach((block, idx) => {
        if (block.type === "thinking") {
          commitText(false);
          hasThinking = true;
          chainBuffer.push({
            kind: "thinking",
            key: `${item.id}:thinking:${thoughtIndex++}`,
            text: block.text,
            ...(item.at === undefined ? {} : { at: item.at }),
            running: item.streaming === true && idx === item.blocks.length - 1,
          });
        } else if (block.text.trim().length > 0) {
          if (textBlocks.length === 0) textStart = idx;
          hasText = true;
          textBlocks.push(block);
        }
      });
      commitText(true);
      if (item.streaming && !hasThinking && !hasText) {
        commitChain();
        rows.push({ kind: "item", key: item.id, item: { ...item, blocks: [] } });
      }
      continue;
    }
    commitChain();
    rows.push({ kind: "item", key: item.id, item });
  }
  commitChain();
  return rows;
}
