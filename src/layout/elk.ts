import ELK from 'elkjs/lib/elk-api.js';
import ELKSync from 'elkjs/lib/main.js';
import type { LayoutFn, LayoutGraph, LayoutResult } from './types';

/**
 * ELK layout options. 'layered' + 'RIGHT' matches the GitHub-Actions
 * left-to-right wave drawing. INCLUDE_CHILDREN makes nested compound nodes
 * (session frame > workflow > wave group > job) lay out together.
 */
const ELK_OPTIONS: Record<string, string> = {
  'elk.algorithm': 'layered',
  'elk.direction': 'RIGHT',
  'elk.layered.spacing.nodeNodeBetweenLayers': '80',
  'elk.spacing.nodeNode': '24',
  'elk.hierarchyHandling': 'INCLUDE_CHILDREN',
  'elk.padding': '[top=40,left=20,bottom=20,right=20]',
  // Interactive placement: keeps existing nodes near their current spot
  // instead of a full repack when the graph grows. This is required for
  // the append-stability property D26 demands; see elk.test.ts.
  'elk.interactive': 'true',
  'elk.layered.layering.strategy': 'INTERACTIVE',
  'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
  'elk.layered.crossingMinimization.forceNodeModelOrder': 'true',
  'elk.layered.nodePlacement.strategy': 'SIMPLE',
};

function toElkNode(node: LayoutGraph['nodes'][number]) {
  return {
    id: node.id,
    width: node.width,
    height: node.height,
  };
}

/**
 * Build the nested ELK input tree from a flat LayoutGraph, using each
 * node's parentId to nest it under its parent's `children` array. Edges are
 * attached at the root so ELK can route across hierarchy levels.
 */
function toElkGraph(graph: LayoutGraph) {
  const elkNodeById = new Map(graph.nodes.map((n) => [n.id, toElkNode(n)] as const));
  const childrenByParent = new Map<string | undefined, ReturnType<typeof toElkNode>[]>();

  // ELK places a compound node's children in the REVERSE of the supplied
  // order. Reverse here so an appended node (last in our array) ends up
  // last in the visual stack too, leaving earlier nodes' relative order
  // (and therefore position) unchanged — required for append stability
  // (D26).
  for (const node of [...graph.nodes].reverse()) {
    const elkNode = elkNodeById.get(node.id)!;
    const list = childrenByParent.get(node.parentId) ?? [];
    list.push(elkNode);
    childrenByParent.set(node.parentId, list);
  }

  function attachChildren(elkNode: ReturnType<typeof toElkNode>, id: string) {
    const children = childrenByParent.get(id);
    if (children && children.length > 0) {
      (elkNode as { children?: unknown[] }).children = children;
      for (const child of children) {
        attachChildren(child, child.id);
      }
    }
  }

  const roots = childrenByParent.get(undefined) ?? [];
  for (const root of roots) {
    attachChildren(root, root.id);
  }

  return {
    id: 'root',
    layoutOptions: ELK_OPTIONS,
    children: roots,
    edges: graph.edges.map((e) => ({ id: e.id, sources: [e.source], targets: [e.target] })),
  };
}

/**
 * Flattens the laid-out ELK tree into a LayoutResult keyed by node id.
 *
 * COORDINATE CONVENTION: positions returned here are RELATIVE to each node's
 * parent (ELK's native output), NOT absolute canvas coordinates. This matches
 * React Flow's `parentId` + relative-position contract, which A2-2 will use
 * directly without re-deriving offsets.
 */
function flatten(elkNode: { id: string; x?: number; y?: number; width?: number; height?: number; children?: unknown[] }, out: LayoutResult): void {
  if (elkNode.id !== 'root') {
    out[elkNode.id] = {
      id: elkNode.id,
      x: elkNode.x ?? 0,
      y: elkNode.y ?? 0,
      width: elkNode.width ?? 0,
      height: elkNode.height ?? 0,
    };
  }
  for (const child of (elkNode.children as typeof elkNode[] | undefined) ?? []) {
    flatten(child, out);
  }
}

function createElk(): ELK {
  // Run ELK in a real Web Worker so layout never blocks the UI thread. In
  // environments with no Worker global (e.g. the Vitest/Node test runner),
  // fall back to elkjs's built-in bundled worker shim — same algorithm and
  // output, just not off-thread.
  if (typeof Worker !== 'undefined') {
    return new ELK({
      workerUrl: new URL('elkjs/lib/elk-worker.js', import.meta.url),
    });
  }
  return new ELKSync() as unknown as ELK;
}

export const elkLayout: LayoutFn = async (graph: LayoutGraph): Promise<LayoutResult> => {
  const elk = createElk();
  const elkGraph = toElkGraph(graph);
  const laidOut = await elk.layout(elkGraph as Parameters<ELK['layout']>[0]);
  const result: LayoutResult = {};
  flatten(laidOut as Parameters<typeof flatten>[0], result);
  return result;
};
