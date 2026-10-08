import { useState } from 'react';
import { Button, Group, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import { JobCard } from './components/JobCard';
import { WorkflowHeader } from './components/WorkflowHeader';
import { sampleData } from './fixtures/sample';

const NEUTRAL_SWATCHES: Array<{ label: string; value: string; contrast?: string }> = [
  { label: '--bg-deep', value: 'var(--bg-deep)' },
  { label: '--bg', value: 'var(--bg)' },
  { label: '--bg-panel', value: 'var(--bg-panel)' },
  { label: '--bg-raise', value: 'var(--bg-raise)' },
  { label: '--line', value: 'var(--line)' },
  { label: '--line-strong', value: 'var(--line-strong)' },
  { label: '--fg-faint', value: 'var(--fg-faint)', contrast: '4.95:1' },
  { label: '--fg-muted', value: 'var(--fg-muted)', contrast: '7.24:1' },
  { label: '--fg', value: 'var(--fg)', contrast: '10.47:1' },
  { label: '--fg-bright', value: 'var(--fg-bright)', contrast: '14.23:1' },
  { label: '--on-solid', value: 'var(--on-solid)', contrast: '16.75:1' },
];

const ACCENT_ROWS: Array<{ hue: string; parts: Array<{ label: string; value: string }> }> = [
  {
    hue: 'Red',
    parts: [
      { label: '--red', value: 'var(--red)' },
      { label: '--red-solid', value: 'var(--red-solid)' },
      { label: '--red-tint', value: 'var(--red-tint)' },
      { label: '--red-edge', value: 'var(--red-edge)' },
    ],
  },
  {
    hue: 'Green',
    parts: [
      { label: '--green', value: 'var(--green)' },
      { label: '--green-solid', value: 'var(--green-solid)' },
      { label: '--green-tint', value: 'var(--green-tint)' },
      { label: '--green-edge', value: 'var(--green-edge)' },
    ],
  },
  {
    hue: 'Blue',
    parts: [
      { label: '--blue', value: 'var(--blue)' },
      { label: '--blue-solid', value: 'var(--blue-solid)' },
      { label: '--blue-tint', value: 'var(--blue-tint)' },
      { label: '--blue-edge', value: 'var(--blue-edge)' },
    ],
  },
];

/**
 * The state gallery, moved here from App.tsx per A2-4: how the user checks
 * state legibility (including "Disable color") and how later cards verify
 * they broke nothing. Reachable at the #/gallery hash route.
 */
export function Gallery() {
  const [colorDisabled, setColorDisabled] = useState(false);

  return (
    <Stack p="xl" gap="xl">
      <Group justify="space-between">
        <Title order={1}>CPD state gallery</Title>
        <Group gap="sm">
          <Button
            variant={colorDisabled ? 'filled' : 'outline'}
            onClick={() => setColorDisabled((v) => !v)}
          >
            {colorDisabled ? 'Enable color' : 'Disable color'}
          </Button>
          <Text size="sm" c="var(--fg-faint)">
            <a href="#/" style={{ color: 'inherit' }}>
              ← back to canvas
            </a>
          </Text>
        </Group>
      </Group>

      <Stack gap="xs">
        <Title order={3}>Neutrals</Title>
        <Group gap="md">
          {NEUTRAL_SWATCHES.map((s) => (
            <Stack key={s.label} gap="var(--space-1)" align="center">
              <div
                style={{
                  width: 48,
                  height: 48,
                  borderRadius: 'var(--space-1)',
                  backgroundColor: s.value,
                  border: '1px solid var(--line-strong)',
                }}
              />
              <Text size="xs" c="var(--fg-muted)">
                {s.label}
              </Text>
              {s.contrast && (
                <Text size="xs" c="var(--fg-faint)">
                  {s.contrast}
                </Text>
              )}
            </Stack>
          ))}
        </Group>
      </Stack>

      <Stack gap="xs">
        <Title order={3}>Accents</Title>
        <Stack gap="sm">
          {ACCENT_ROWS.map((row) => (
            <Group key={row.hue} gap="md">
              <Text size="sm" w={60}>
                {row.hue}
              </Text>
              {row.parts.map((p) => (
                <Stack key={p.label} gap="var(--space-1)" align="center">
                  <div
                    style={{
                      width: 48,
                      height: 48,
                      borderRadius: 'var(--space-1)',
                      backgroundColor: p.value,
                      border: '1px solid var(--line-strong)',
                    }}
                  />
                  <Text size="xs" c="var(--fg-muted)">
                    {p.label}
                  </Text>
                </Stack>
              ))}
            </Group>
          ))}
        </Stack>
      </Stack>

      <Stack gap="xs">
        <Title order={3}>Jobs</Title>
        <SimpleGrid cols={{ base: 1, sm: 2, md: 3, lg: 4 }} spacing="md">
          {sampleData.jobs.map((job) => (
            <Stack key={job.id} gap="var(--space-1)" align="center">
              <JobCard job={job} colorDisabled={colorDisabled} />
              <Text size="xs" c="var(--fg-faint)">
                {job.status}
              </Text>
            </Stack>
          ))}
        </SimpleGrid>
      </Stack>

      <Stack gap="xs">
        <Title order={3}>Workflow header</Title>
        <WorkflowHeader workflow={sampleData.workflows[0]} />
      </Stack>
    </Stack>
  );
}
