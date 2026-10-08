import { createContext, useContext, useEffect, useState } from 'react';
import { ReactFlowProvider } from '@xyflow/react';
import { Group, Stack, Text } from '@mantine/core';
import { Canvas } from './canvas/Canvas';
import { LeadsLevel } from './canvas/LeadsLevel';
import { PlansLevel } from './canvas/PlansLevel';
import { Gallery } from './Gallery';
import { sampleData } from './fixtures/sample';
import { ascend, initialNav, selectLead, selectPlan, type NavState } from './model/navigation';
import { leadsLevel, plansLevel, dataForPlan } from './model/levels';
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

interface BreadcrumbSegment {
  label: string;
  nav: NavState;
}

function breadcrumbSegments(nav: NavState): BreadcrumbSegment[] {
  const project = sampleData.projects[0];
  const segments: BreadcrumbSegment[] = [{ label: project.name, nav: initialNav }];

  if (nav.leadId) {
    const session = sampleData.sessions.find((s) => s.id === nav.leadId);
    if (session) segments.push({ label: session.name, nav: selectLead(session.id) });
  }

  if (nav.planId) {
    const workflow = sampleData.workflows.find((w) => w.id === nav.planId);
    if (workflow && nav.leadId) {
      segments.push({ label: workflow.title, nav: selectPlan({ level: 'plans', leadId: nav.leadId }, workflow.id) });
    }
  }

  return segments;
}

interface NavContextValue {
  nav: NavState;
  onNavigate: (next: NavState) => void;
}

const NavContext = createContext<NavContextValue>({ nav: initialNav, onNavigate: () => {} });

function TopBar() {
  const { nav, onNavigate } = useContext(NavContext);
  const project = sampleData.projects[0];
  const segments = breadcrumbSegments(nav);

  return (
    <Group
      px="md"
      py="var(--space-2)"
      justify="space-between"
      style={{ borderBottom: '1px solid var(--line)', flexShrink: 0 }}
    >
      <Stack gap={0} justify="center">
        <Group gap="var(--space-1)" wrap="nowrap">
          {segments.map((segment, i) => (
            <Group key={i} gap="var(--space-1)" wrap="nowrap">
              {i > 0 && (
                <Text size="sm" c="var(--fg-faint)">
                  ›
                </Text>
              )}
              <Text
                component="a"
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  onNavigate(segment.nav);
                }}
                size="sm"
                fw={i === segments.length - 1 ? 600 : 400}
                c={i === segments.length - 1 ? 'var(--fg-bright)' : 'var(--fg-faint)'}
                style={{ textDecoration: 'none' }}
              >
                {segment.label}
              </Text>
            </Group>
          ))}
        </Group>
        <Text size="xs" c="var(--fg-faint)">
          {project.repos.join(' · ')}
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

function CanvasLevel({ nav, onNavigate }: { nav: NavState; onNavigate: (next: NavState) => void }) {
  if (nav.level === 'leads') {
    return <LeadsLevel leads={leadsLevel(sampleData)} onSelect={(leadId) => onNavigate(selectLead(leadId))} />;
  }

  if (nav.level === 'plans' && nav.leadId) {
    return (
      <PlansLevel
        plans={plansLevel(sampleData, nav.leadId)}
        onSelect={(planId) => onNavigate(selectPlan(nav, planId))}
      />
    );
  }

  if (nav.level === 'jobs' && nav.planId) {
    return (
      <ReactFlowProvider>
        <Canvas data={dataForPlan(sampleData, nav.planId)} />
      </ReactFlowProvider>
    );
  }

  return null;
}

function App() {
  const hash = useHashRoute();
  const [nav, setNav] = useState<NavState>(initialNav);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setNav((current) => ascend(current));
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  if (hash === '#/gallery') {
    return <Gallery />;
  }

  return (
    <NavContext.Provider value={{ nav, onNavigate: setNav }}>
      <div style={{ display: 'flex', flexDirection: 'column', height: '100vh' }}>
        <TopBar />
        <div style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
          <CanvasLevel nav={nav} onNavigate={setNav} />
        </div>
      </div>
    </NavContext.Provider>
  );
}

export default App;
