import { Card } from '@mantine/core';
import type { ReactNode } from 'react';

export interface CardShellProps {
  children: ReactNode;
  /** CSS color value for the border (already resolved, e.g. 'var(--line-strong)'). */
  borderColor: string;
  dashed?: boolean;
  opacity?: number;
  width?: number;
}

/**
 * Shared outer card chrome (border, padding, background, width) used by
 * JobCard and the upper-level Leads/Plans cards, so the three near-identical
 * card components share one place for the anatomy instead of copy-pasting it.
 */
export function CardShell({ children, borderColor, dashed = false, opacity = 1, width = 220 }: CardShellProps) {
  return (
    <Card
      withBorder
      padding="var(--pad)"
      w={width}
      style={{
        backgroundColor: 'var(--bg-panel)',
        borderColor,
        borderStyle: dashed ? 'dashed' : 'solid',
        borderWidth: 1,
        opacity,
      }}
    >
      {children}
    </Card>
  );
}
