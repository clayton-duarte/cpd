import type { Session } from '../../model/types';

export interface SessionFrameNodeData {
  session?: Session;
  [key: string]: unknown;
}

/**
 * The labelled container around a session's workflow(s). Border and label
 * per the A2-4 spec: border rgba(230,230,230,.10), label at 60% white.
 * Sized by ELK to its children (the lead card + workflow); this component
 * only draws the chrome, it does not position anything.
 */
export function SessionFrameNode({ data }: { data: SessionFrameNodeData }) {
  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        border: '1px solid var(--line)',
        borderRadius: 'var(--mantine-radius-sm)',
        boxSizing: 'border-box',
        position: 'relative',
        pointerEvents: 'none',
      }}
    >
      <span
        style={{
          position: 'absolute',
          top: 8,
          left: 20,
          fontSize: 12,
          color: 'var(--fg-muted)',
        }}
      >
        {data.session?.name}
      </span>
    </div>
  );
}
