import { useMemo } from 'react';
import { Background, Controls, ReactFlow, type Node } from '@xyflow/react';
import type { PlanCardData } from '../model/levels';
import { SPACE_CARDS } from '../layout/spacing';
import { PlanNode } from './nodes/PlanNode';

export interface PlansLevelProps {
  plans: PlanCardData[];
  onSelect: (planId: string) => void;
}

const CARD_WIDTH = 220;
const CARD_HEIGHT = 100;
const COLUMNS = 4;

const nodeTypes = {
  plan: PlanNode,
};

/**
 * Plans level: one node per workflow of the selected lead. Plans have no
 * dependency edges between them -- positions are a simple row/grid computed
 * directly (no elkjs), matching the previous flex-wrap rhythm (SPACE_CARDS
 * gap, same card width).
 */
export function PlansLevel({ plans, onSelect }: PlansLevelProps) {
  const nodes: Node[] = useMemo(
    () =>
      plans.map((plan, i) => ({
        id: plan.id,
        type: 'plan',
        position: {
          x: (i % COLUMNS) * (CARD_WIDTH + SPACE_CARDS) + SPACE_CARDS,
          y: Math.floor(i / COLUMNS) * (CARD_HEIGHT + SPACE_CARDS) + SPACE_CARDS,
        },
        data: { plan },
        draggable: false,
        connectable: false,
        selectable: false,
      })),
    [plans],
  );

  return (
    <ReactFlow
      nodes={nodes}
      edges={[]}
      nodeTypes={nodeTypes}
      nodesDraggable={false}
      nodesConnectable={false}
      elementsSelectable={false}
      onNodeClick={(_, node) => onSelect(node.id)}
      panOnDrag
      zoomOnScroll
      fitView
      onlyRenderVisibleElements
    >
      <Background color="var(--line)" />
      <Controls />
    </ReactFlow>
  );
}
