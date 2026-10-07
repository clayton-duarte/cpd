import { useState } from 'react';
import { Button, Group, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import { ReactFlowProvider } from '@xyflow/react';
import { JobCard } from './components/JobCard';
import { LeadCard } from './components/LeadCard';
import { sampleData } from './fixtures/sample';
import { TOKENS, w } from './theme';
import { Canvas } from './canvas/Canvas';
import './canvas/xyflow-theme.css';

const PALETTE_SWATCHES: Array<{ label: string; value: string }> = [
  { label: 'black', value: TOKENS.black },
  { label: 'white', value: TOKENS.white },
  { label: 'red', value: TOKENS.red },
  { label: 'green', value: TOKENS.green },
  { label: 'blue', value: TOKENS.blue },
];

const WHITE_OPACITY_LEVELS: Array<{ label: string; opacity: number }> = [
  { label: 'Primary text', opacity: 1 },
  { label: 'Secondary text', opacity: 0.6 },
  { label: 'Muted text / placeholders', opacity: 0.45 },
  { label: 'Card borders', opacity: 0.25 },
  { label: 'Dividers', opacity: 0.15 },
  { label: 'Panel / card surfaces', opacity: 0.05 },
];

function App() {
  const [colorDisabled, setColorDisabled] = useState(false);
  const session = sampleData.sessions[0];

  return (
    <Stack p="xl" gap="xl">
      <Group justify="space-between">
        <Title order={1}>CPD state gallery</Title>
        <Button
          variant={colorDisabled ? 'filled' : 'outline'}
          onClick={() => setColorDisabled((v) => !v)}
        >
          {colorDisabled ? 'Enable color' : 'Disable color'}
        </Button>
      </Group>

      <Stack gap="xs">
        <Title order={3}>Palette</Title>
        <Group gap="md">
          {PALETTE_SWATCHES.map((s) => (
            <Stack key={s.label} gap={4} align="center">
              <div
                style={{
                  width: 48,
                  height: 48,
                  borderRadius: 4,
                  backgroundColor: s.value,
                  border: `1px solid ${w(0.25)}`,
                }}
              />
              <Text size="xs" c={w(0.6)}>
                {s.label}
              </Text>
            </Stack>
          ))}
        </Group>
        <Stack gap={4} mt="sm">
          {WHITE_OPACITY_LEVELS.map((lvl) => (
            <Group key={lvl.label} gap="sm">
              <div
                style={{
                  width: 120,
                  height: 16,
                  backgroundColor: w(lvl.opacity),
                }}
              />
              <Text size="xs" c={w(0.6)}>
                {lvl.label} ({Math.round(lvl.opacity * 100)}%)
              </Text>
            </Group>
          ))}
        </Stack>
      </Stack>

      <Stack gap="xs">
        <Title order={3}>Lead</Title>
        <LeadCard session={session} />
      </Stack>

      <Stack gap="xs">
        <Title order={3}>Jobs</Title>
        <SimpleGrid cols={{ base: 1, sm: 2, md: 3, lg: 4 }} spacing="md">
          {sampleData.jobs.map((job) => (
            <Stack key={job.id} gap={4} align="center">
              <JobCard job={job} colorDisabled={colorDisabled} />
              <Text size="xs" c={w(0.45)}>
                {job.status}
              </Text>
            </Stack>
          ))}
        </SimpleGrid>
      </Stack>

      <Stack gap="xs">
        <Title order={3}>Canvas</Title>
        <div style={{ height: 600, border: `1px solid ${w(0.15)}` }}>
          <ReactFlowProvider>
            <Canvas data={sampleData} />
          </ReactFlowProvider>
        </div>
      </Stack>
    </Stack>
  );
}

export default App;
