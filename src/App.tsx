import { createContext, useContext, useEffect, useRef, useState } from 'react';
import { ReactFlowProvider } from '@xyflow/react';
import { Group, Stack, Text } from '@mantine/core';
import { Canvas } from './canvas/Canvas';
import { LeadsLevel } from './canvas/LeadsLevel';
import { PlansLevel } from './canvas/PlansLevel';
import { nextJob } from './model/selection';
import { Sidebar } from './components/Sidebar';
import { ChatPanel } from './components/ChatPanel';
import { Gallery } from './Gallery';
import { sampleData } from './fixtures/sample';
import { ascend, initialNav, selectLead, selectPlan, type Level, type NavState } from './model/navigation';
import { leadsLevel, plansLevel, dataForPlan, jobsLevel } from './model/levels';
import { useLevelTransition } from './canvas/useLevelTransition';
import type { ConversationId } from './engine/types';
import './canvas/xyflow-theme.css';

const LEVEL_DEPTH: Record<Level, number> = { leads: 0, plans: 1, jobs: 2 };

/**
 * Drives the camera transition whenever `nav.level` changes. Must render
 * inside <ReactFlowProvider> -- useReactFlow throws outside it (C5 wiring
 * requirement). Direction is derived by comparing depth against the
 * previously seen level, tracked in a ref (not state -- a state write here
 * would re-render on every navigation for no reason beyond bookkeeping).
 */
function LevelTransitionEffect({ level }: { level: Level }) {
  const transition = useLevelTransition();
  const previousLevel = useRef(level);

  useEffect(() => {
    if (previousLevel.current !== level) {
      const direction = LEVEL_DEPTH[level] > LEVEL_DEPTH[previousLevel.current] ? 'descend' : 'ascend';
      transition(direction);
      previousLevel.current = level;
    }
  }, [level, transition]);

  return null;
}

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

function CanvasLevelContent({
  nav,
  onNavigate,
  selectedJobId,
  onSelectJob,
}: {
  nav: NavState;
  onNavigate: (next: NavState) => void;
  selectedJobId: string | null;
  onSelectJob: (jobId: string | null) => void;
}) {
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
    return <Canvas data={dataForPlan(sampleData, nav.planId)} selectedJobId={selectedJobId} onSelectJob={onSelectJob} />;
  }

  return null;
}

/**
 * The provider is kept mounted across all three levels (not just Jobs) so
 * `useLevelTransition`'s `useReactFlow()` never throws regardless of which
 * level is current -- it must be called unconditionally on every nav change.
 */
function CanvasLevel({
  nav,
  onNavigate,
  selectedJobId,
  onSelectJob,
}: {
  nav: NavState;
  onNavigate: (next: NavState) => void;
  selectedJobId: string | null;
  onSelectJob: (jobId: string | null) => void;
}) {
  return (
    <ReactFlowProvider>
      <LevelTransitionEffect level={nav.level} />
      <CanvasLevelContent
        nav={nav}
        onNavigate={onNavigate}
        selectedJobId={selectedJobId}
        onSelectJob={onSelectJob}
      />
    </ReactFlowProvider>
  );
}

function App() {
  const hash = useHashRoute();
  const [nav, setNav] = useState<NavState>(initialNav);
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [selectedConversationId, setSelectedConversationId] = useState<ConversationId | undefined>(
    undefined,
  );

  // Selection is deliberately NOT part of NavState (see D3 card): NavState
  // answers "which level", selection answers "which job within this level".
  // Clear it whenever the plan changes so it never points at a stale job
  // from a different plan.
  useEffect(() => {
    setSelectedJobId(null);
  }, [nav.planId]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (selectedJobId !== null) {
          setSelectedJobId(null);
          return;
        }
        setNav((current) => ascend(current));
        return;
      }

      if (nav.level !== 'jobs' || !nav.planId) return;

      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
        e.preventDefault();
        const ids = jobsLevel(sampleData, nav.planId).map((j) => j.id);
        setSelectedJobId((current) => nextJob(ids, current, 1));
        return;
      }

      if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
        e.preventDefault();
        const ids = jobsLevel(sampleData, nav.planId).map((j) => j.id);
        setSelectedJobId((current) => nextJob(ids, current, -1));
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [nav, selectedJobId]);

  if (hash === '#/gallery') {
    return <Gallery />;
  }

  return (
    <NavContext.Provider value={{ nav, onNavigate: setNav }}>
      <div style={{ display: 'flex', flexDirection: 'column', height: '100vh' }}>
        <TopBar />
        <div style={{ flex: 1, minHeight: 0, display: 'flex' }}>
          <Sidebar
            data={sampleData}
            nav={nav}
            onNavigate={setNav}
            selectedConversationId={selectedConversationId}
            onSelectConversation={setSelectedConversationId}
          />
          <div style={{ flex: 1, minHeight: 0, display: 'flex' }}>
            <div data-testid="canvas-area" style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
              <CanvasLevel
                nav={nav}
                onNavigate={setNav}
                selectedJobId={selectedJobId}
                onSelectJob={setSelectedJobId}
              />
            </div>
            {nav.level === 'leads' && (
              <div style={{ width: '24rem', flexShrink: 0, borderLeft: '1px solid var(--line)' }}>
                <ChatPanel
                  conversationId={selectedConversationId}
                  onForked={setSelectedConversationId}
                />
              </div>
            )}
          </div>
        </div>
      </div>
    </NavContext.Provider>
  );
}

export default App;
