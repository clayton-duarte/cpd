import { useMemo } from 'react';
import { Background, Controls, ReactFlow, type Node } from '@xyflow/react';
import type { LeadCardData } from '../model/levels';
import { SPACE_CARDS } from '../layout/spacing';
import { LeadNode } from './nodes/LeadNode';

export interface LeadsLevelProps {
  leads: LeadCardData[];
  onSelect: (leadId: string) => void;
}

const CARD_WIDTH = 220;
const CARD_HEIGHT = 80;
const COLUMNS = 4;

const nodeTypes = {
  lead: LeadNode,
};

/**
 * Leads level: one node per lead session. Leads have no dependency edges
 * between them -- they are a set, not a graph -- so positions are a simple
 * row/grid computed directly (no elkjs), matching the previous flex-wrap
 * rhythm (SPACE_CARDS gap, same card width).
 */
export function LeadsLevel({ leads, onSelect }: LeadsLevelProps) {
  const nodes: Node[] = useMemo(
    () =>
      leads.map((lead, i) => ({
        id: lead.id,
        type: 'lead',
        position: {
          x: (i % COLUMNS) * (CARD_WIDTH + SPACE_CARDS) + SPACE_CARDS,
          y: Math.floor(i / COLUMNS) * (CARD_HEIGHT + SPACE_CARDS) + SPACE_CARDS,
        },
        data: { lead },
        draggable: false,
        connectable: false,
        selectable: false,
      })),
    [leads],
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
