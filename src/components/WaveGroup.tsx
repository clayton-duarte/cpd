import type { ReactNode } from 'react';
import { Box, Group, Stack, Text } from '@mantine/core';
export interface WaveGroupProps {
  label: string;
  children: ReactNode;
}

/** Purely presentational box grouping jobs in the same dependency wave. */
export function WaveGroup({ label, children }: WaveGroupProps) {
  return (
    <Box
      style={{
        border: '1px solid var(--line)',
        borderRadius: 'var(--mantine-radius-sm)',
        backgroundColor: 'var(--bg-panel)',
        padding: 'var(--mantine-spacing-md)',
        display: 'inline-block',
      }}
    >
      <Stack gap="xs">
        <Text size="xs" c="var(--fg-faint)">
          {label}
        </Text>
        <Group gap="md" wrap="nowrap" align="flex-start">
          {children}
        </Group>
      </Stack>
    </Box>
  );
}
