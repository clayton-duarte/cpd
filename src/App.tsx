import { Badge, Button, Card, Stack, Text, Title, Tooltip } from '@mantine/core';
import { IconAlertTriangle, IconCircleCheck, IconLoader2 } from '@tabler/icons-react';
import { w } from './theme';

/**
 * Throwaway sanity strip. Replaced by card A1-3.
 * Proves the palette renders: text opacities, a card, a badge, a button,
 * a tooltip, and one icon in each accent color.
 */
function App() {
  return (
    <Stack p="xl" gap="md">
      <Title order={1}>CPD sanity strip</Title>
      <Text>Primary text — white at 100%</Text>
      <Text c={w(0.6)}>Secondary text — white at 60%</Text>
      <Text c={w(0.45)}>Muted text — white at 45%</Text>

      <Card withBorder padding="lg">
        <Text>A bordered card surface.</Text>
      </Card>

      <Badge color="blue">Agents working</Badge>

      <Tooltip label="This button does nothing yet">
        <Button>Hover me</Button>
      </Tooltip>

      <Stack gap="xs">
        <IconAlertTriangle color="var(--mantine-color-red-5)" />
        <IconCircleCheck color="var(--mantine-color-green-5)" />
        <IconLoader2 color="var(--mantine-color-blue-5)" />
      </Stack>
    </Stack>
  );
}

export default App;
