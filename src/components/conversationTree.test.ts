import { describe, expect, it } from 'vitest';
import { buildConversationTreeData } from './conversationTree';
import type { ConversationId, ConversationNode } from '../engine/types';

function node(id: number, parentId: number | null, title: string): ConversationNode {
  return { id: id as ConversationId, parentId: parentId as ConversationId | null, at: null, title };
}

describe('buildConversationTreeData', () => {
  it('nests a child under its parent', () => {
    const nodes = [node(1, null, 'Lead'), node(16, 1, 'Test thread')];

    const data = buildConversationTreeData(nodes);

    expect(data).toEqual([
      {
        value: '1',
        label: 'Lead',
        children: [{ value: '16', label: 'Test thread', children: [] }],
      },
    ]);
  });

  it('handles a root-only list with no children', () => {
    const nodes = [node(1, null, 'Lead')];

    const data = buildConversationTreeData(nodes);

    expect(data).toEqual([{ value: '1', label: 'Lead', children: [] }]);
  });

  it('nests a fork of a fork at the correct depth', () => {
    const nodes = [node(1, null, 'Lead'), node(16, 1, 'Thread A'), node(17, 16, 'Thread A sub')];

    const data = buildConversationTreeData(nodes);

    expect(data).toEqual([
      {
        value: '1',
        label: 'Lead',
        children: [
          {
            value: '16',
            label: 'Thread A',
            children: [{ value: '17', label: 'Thread A sub', children: [] }],
          },
        ],
      },
    ]);
  });
});
