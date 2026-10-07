import type { ReactNode } from 'react';
import { Box, Group, Stack, Text } from '@mantine/core';
import { w } from '../theme';

export interface WaveGroupProps {
  label: string;
  children: ReactNode;
}

/** Purely presentational box grouping jobs in the same dependency wave. */
export function WaveGroup({ label, children }: WaveGroupProps) {
  return (
    <Box
      style={{
        border: `1px solid rgba(230,230,230,.10)`,
        borderRadius: 'var(--mantine-radius-sm)',
        backgroundColor: 'rgba(230,230,230,.02)',
        padding: 'var(--mantine-spacing-md)',
        display: 'inline-block',
      }}
    >
      <Stack gap="xs">
        <Text size="xs" c={w(0.45)}>
          {label}
        </Text>
        <Group gap="md" wrap="nowrap" align="flex-start">
          {children}
        </Group>
      </Stack>
    </Box>
  );
}
