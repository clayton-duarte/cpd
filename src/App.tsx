import { useEffect, useState } from 'react';
import { ReactFlowProvider } from '@xyflow/react';
import { Group, Stack, Text } from '@mantine/core';
import { Canvas } from './canvas/Canvas';
import { Gallery } from './Gallery';
import { sampleData } from './fixtures/sample';
import './canvas/xyflow-theme.css';

/** Trivial hash-based routing: no router dependency, per the A2-4 spec. */
function useHashRoute(): string {
  const [hash, setHash] = useState(() => window.location.hash);
  useEffect(() => {
    const onHashChange = () => setHash(window.location.hash);
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, []);
  return hash;
}

function TopBar() {
  const project = sampleData.projects[0];
  const session = sampleData.sessions[0];
  return (
    <Group
      px="md"
      h="var(--space-6)"
      justify="space-between"
      style={{ borderBottom: '1px solid var(--line)', flexShrink: 0 }}
    >
      <Stack gap={0} justify="center">
        <Text size="sm" fw={600} c="var(--fg-bright)">
          {session.name}
        </Text>
        <Text size="xs" c="var(--fg-faint)">
          {project.name} · {project.repos.join(' · ')}
        </Text>
      </Stack>
      <Text size="xs" c="var(--fg-faint)">
        <a href="#/gallery" style={{ color: 'inherit' }}>
          gallery
        </a>
      </Text>
    </Group>
  );
}

function App() {
  const hash = useHashRoute();

  if (hash === '#/gallery') {
    return <Gallery />;
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh' }}>
      <TopBar />
      <div style={{ flex: 1, minHeight: 0 }}>
        <ReactFlowProvider>
          <Canvas data={sampleData} />
        </ReactFlowProvider>
      </div>
    </div>
  );
}

export default App;
