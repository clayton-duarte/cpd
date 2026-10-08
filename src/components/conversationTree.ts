import type { TreeNodeData } from '@mantine/core';
import type { ConversationNode } from '../engine/types';

/** Builds a Mantine `TreeExpandedState` with every node in `data` expanded -- used so the
 * Threads tree is never rendered collapsed (its sole purpose is to show the user their threads;
 * hiding them by default would defeat it). Pass to `useTree({ initialExpandedState })` or
 * `tree.setExpandedState(...)` after a refetch. */
export function allExpandedState(data: readonly TreeNodeData[]): Record<string, boolean> {
  const state: Record<string, boolean> = {};
  function walk(nodes: readonly TreeNodeData[]) {
    for (const node of nodes) {
      state[node.value] = true;
      if (node.children) walk(node.children);
    }
  }
  walk(data);
  return state;
}

/**
 * Pure mapping of the real conversation tree (ConversationNode[], numeric ids, parentId-linked)
 * into Mantine Tree's `data` shape. Root nodes have `parentId: null`. `value` is the string form
 * of the numeric id -- the single boundary where the branded ConversationId becomes a string, to
 * satisfy Mantine's API; callers convert back to numeric on selection.
 *
 * This is entirely additive/parallel to the fixture-driven Lead/Plan/Job tree in Sidebar.tsx --
 * conversations are NOT mapped onto that model (per lead decision: the two hierarchies are not
 * reconciled in this card).
 */
export function buildConversationTreeData(nodes: readonly ConversationNode[]): TreeNodeData[] {
  const byParent = new Map<number | null, ConversationNode[]>();
  for (const node of nodes) {
    const key = node.parentId === null ? null : Number(node.parentId);
    const siblings = byParent.get(key) ?? [];
    siblings.push(node);
    byParent.set(key, siblings);
  }

  function buildChildren(parentId: number | null): TreeNodeData[] {
    const children = byParent.get(parentId) ?? [];
    return children.map((node) => ({
      value: String(node.id),
      label: node.title,
      children: buildChildren(Number(node.id)),
    }));
  }

  return buildChildren(null);
}
