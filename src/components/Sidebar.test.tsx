import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { Sidebar, MIN_WIDTH, MAX_WIDTH } from './Sidebar';
import { initialNav, selectLead, selectPlan, type NavState } from '../model/navigation';
import { sampleData } from '../fixtures/sample';
import * as client from '../engine/client';
import type { ConversationId, ConversationNode } from '../engine/types';

vi.mock('../engine/client');

let conversationsSignalHandlers: Array<() => void> = [];
vi.mock('../engine/useEngineStream', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../engine/useEngineStream')>();
  return {
    ...actual,
    onConversationsSignal: (listener: () => void) => {
      conversationsSignalHandlers.push(listener);
      return () => {
        conversationsSignalHandlers = conversationsSignalHandlers.filter((l) => l !== listener);
      };
    },
  };
});

function emitConversationsSignal() {
  for (const listener of conversationsSignalHandlers) listener();
}

function mockConversations(conversations: ConversationNode[]) {
  vi.mocked(client.getConversations).mockResolvedValue({ conversations });
}

beforeEach(() => {
  mockConversations([]);
  conversationsSignalHandlers = [];
});

afterEach(() => {
  vi.clearAllMocks();
});

function widthPx(panel: HTMLElement): number {
  const match = panel.style.width.match(/([\d.]+)rem/);
  return match ? parseFloat(match[1]) * 16 : NaN;
}

function renderSidebar(nav = initialNav, onNavigate: (next: NavState) => void = () => {}) {
  return render(
    <MantineProvider>
      <Sidebar data={sampleData} nav={nav} onNavigate={onNavigate} />
    </MantineProvider>,
  );
}

describe('Sidebar', () => {
  it('renders every lead collapsed by default (no plans visible)', () => {
    renderSidebar();
    expect(screen.getByText('Harvest planner')).toBeTruthy();
    expect(screen.getByText('Grove automation')).toBeTruthy();
    expect(screen.getByText('New orchard survey')).toBeTruthy();
    expect(screen.queryByText('Storefront banner refresh')).toBeNull();
  });

  it('expanding a lead row reveals its plans, and a plan reveals its jobs', () => {
    renderSidebar();
    fireEvent.click(screen.getByTestId('expand-s2'));
    expect(screen.getByText('Automated drip-irrigation scheduling')).toBeTruthy();
    expect(screen.queryByText('Model soil moisture thresholds')).toBeNull();

    fireEvent.click(screen.getByTestId('expand-w3'));
    expect(screen.getByText('Model soil moisture thresholds')).toBeTruthy();
  });

  it('clicking a lead row calls onNavigate with selectLead', () => {
    let navigated: unknown = null;
    renderSidebar(initialNav, (next) => {
      navigated = next;
    });
    fireEvent.click(screen.getByText('Grove automation'));
    expect(navigated).toEqual(selectLead('s2'));
  });

  it('clicking a plan row calls onNavigate with selectPlan', () => {
    let navigated: unknown = null;
    const nav = selectLead('s2');
    renderSidebar(nav, (next) => {
      navigated = next;
    });
    fireEvent.click(screen.getByTestId('expand-s2'));
    fireEvent.click(screen.getByText('Automated drip-irrigation scheduling'));
    expect(navigated).toEqual(selectPlan(nav, 'w3'));
  });

  it('clicking a job row calls onNavigate with selectPlan for its parent plan', () => {
    let navigated: unknown = null;
    const nav = selectPlan(selectLead('s2'), 'w3');
    renderSidebar(nav, (next) => {
      navigated = next;
    });
    fireEvent.click(screen.getByTestId('expand-s2'));
    fireEvent.click(screen.getByTestId('expand-w3'));
    fireEvent.click(screen.getByText('Model soil moisture thresholds'));
    expect(navigated).toEqual(selectPlan({ level: 'plans', leadId: 's2' }, 'w3'));
  });

  it('highlights the current lead row when leadId is set', () => {
    renderSidebar(selectLead('s2'));
    expect(screen.getByTestId('row-s2').getAttribute('data-selected')).toBe('true');
    expect(screen.getByTestId('row-s1').getAttribute('data-selected')).toBe('false');
  });

  it('highlights the current plan row only at the jobs level', () => {
    const nav = selectPlan(selectLead('s2'), 'w3');
    renderSidebar(nav);
    fireEvent.click(screen.getByTestId('expand-s2'));
    expect(screen.getByTestId('row-w3').getAttribute('data-selected')).toBe('true');
  });

  it('no job row is ever highlighted', () => {
    const nav = selectPlan(selectLead('s2'), 'w3');
    renderSidebar(nav);
    fireEvent.click(screen.getByTestId('expand-s2'));
    fireEvent.click(screen.getByTestId('expand-w3'));
    expect(screen.getByTestId('row-j17').getAttribute('data-selected')).toBe('false');
  });

  it('expansion state survives navigating away and back', () => {
    let nav = initialNav;
    const { rerender } = render(
      <MantineProvider>
        <Sidebar
          data={sampleData}
          nav={nav}
          onNavigate={(next) => {
            nav = next;
          }}
        />
      </MantineProvider>,
    );
    fireEvent.click(screen.getByTestId('expand-s2'));
    expect(screen.getByText('Automated drip-irrigation scheduling')).toBeTruthy();

    // Simulate navigating to a different lead and back -- re-render with new nav prop.
    rerender(
      <MantineProvider>
        <Sidebar data={sampleData} nav={selectLead('s1')} onNavigate={() => {}} />
      </MantineProvider>,
    );
    rerender(
      <MantineProvider>
        <Sidebar data={sampleData} nav={initialNav} onNavigate={() => {}} />
      </MantineProvider>,
    );
    // s2 is still expanded -- expansion is independent of selection/navigation.
    expect(screen.getByText('Automated drip-irrigation scheduling')).toBeTruthy();
  });

  it('can be collapsed via a toggle', () => {
    renderSidebar();
    expect(screen.getByText('Harvest planner')).toBeTruthy();
    fireEvent.click(screen.getByTestId('sidebar-toggle'));
    expect(screen.queryByText('Harvest planner')).toBeNull();
    fireEvent.click(screen.getByTestId('sidebar-toggle'));
    expect(screen.getByText('Harvest planner')).toBeTruthy();
  });

  // Note: jsdom performs no real layout, so there is no observable "text is no
  // longer truncated" signal here -- we assert the width *style value* instead
  // and validate the visual truncation fix manually in the browser.
  describe('resizable width', () => {
    it('renders a resize handle with a col-resize cursor', () => {
      renderSidebar();
      const handle = screen.getByTestId('sidebar-resize-handle');
      expect(handle).toBeTruthy();
      expect(handle.style.cursor).toBe('col-resize');
    });

    it('dragging the handle changes the rendered width in the expected direction', () => {
      renderSidebar();
      const handle = screen.getByTestId('sidebar-resize-handle');
      const panel = handle.parentElement as HTMLElement;
      const before = widthPx(panel);

      fireEvent.pointerDown(handle, { clientX: 260 });
      fireEvent.pointerMove(handle, { clientX: 340 });
      fireEvent.pointerUp(handle, { clientX: 340 });

      expect(widthPx(panel)).toBeGreaterThan(before);
    });

    it('clamps to MIN_WIDTH and MAX_WIDTH when dragged far past either end', () => {
      renderSidebar();
      const handle = screen.getByTestId('sidebar-resize-handle');
      const panel = handle.parentElement as HTMLElement;

      fireEvent.pointerDown(handle, { clientX: 260 });
      fireEvent.pointerMove(handle, { clientX: -10000 });
      fireEvent.pointerUp(handle, { clientX: -10000 });
      expect(widthPx(panel)).toBe(MIN_WIDTH);

      fireEvent.pointerDown(handle, { clientX: 260 });
      fireEvent.pointerMove(handle, { clientX: 10000 });
      fireEvent.pointerUp(handle, { clientX: 10000 });
      expect(widthPx(panel)).toBe(MAX_WIDTH);
    });

    it('width survives a collapse/expand cycle', () => {
      renderSidebar();
      const handle = screen.getByTestId('sidebar-resize-handle');
      const panel = handle.parentElement as HTMLElement;

      fireEvent.pointerDown(handle, { clientX: 260 });
      fireEvent.pointerMove(handle, { clientX: 360 });
      fireEvent.pointerUp(handle, { clientX: 360 });
      const draggedWidth = widthPx(panel);

      fireEvent.click(screen.getByTestId('sidebar-toggle'));
      expect(screen.queryByText('Harvest planner')).toBeNull();
      fireEvent.click(screen.getByTestId('sidebar-toggle'));

      const restoredPanel = screen.getByTestId('sidebar-resize-handle').parentElement as HTMLElement;
      expect(widthPx(restoredPanel)).toBe(draggedWidth);
    });
  });

  describe('conversation threads (additive, parallel to the fixture tree)', () => {
    it('does not render a Threads section when there are no conversations', () => {
      mockConversations([]);
      renderSidebar();
      expect(screen.queryByText('Threads')).toBeNull();
    });

    it('renders fetched conversations as a nested Threads section, already expanded', async () => {
      mockConversations([
        { id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' },
        { id: 16 as ConversationId, parentId: 1 as ConversationId, at: 7, title: 'Test thread' },
      ]);
      renderSidebar();

      await waitFor(() => expect(screen.getByText('Threads')).toBeTruthy());
      expect(screen.getByText('Lead')).toBeTruthy();
      await waitFor(() => expect(screen.getByText('Test thread')).toBeTruthy());
    });

    it('clicking a thread calls onSelectConversation with the numeric id, not the string value', async () => {
      mockConversations([{ id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' }]);
      let selected: ConversationId | null = null;
      render(
        <MantineProvider>
          <Sidebar
            data={sampleData}
            nav={initialNav}
            onNavigate={() => {}}
            onSelectConversation={(id) => {
              selected = id;
            }}
          />
        </MantineProvider>,
      );

      await waitFor(() => expect(screen.getByText('Lead')).toBeTruthy());
      fireEvent.click(screen.getByTestId('thread-1'));

      expect(selected).toBe(1);
      expect(typeof selected).toBe('number');
    });

    it('highlights the selected thread', async () => {
      mockConversations([{ id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' }]);
      render(
        <MantineProvider>
          <Sidebar
            data={sampleData}
            nav={initialNav}
            onNavigate={() => {}}
            selectedConversationId={1 as ConversationId}
          />
        </MantineProvider>,
      );

      await waitFor(() =>
        expect(screen.getByTestId('thread-1').getAttribute('data-selected')).toBe('true'),
      );
    });

    it('renders a parent and child thread both visible with no clicks', async () => {
      mockConversations([
        { id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' },
        { id: 16 as ConversationId, parentId: 1 as ConversationId, at: 7, title: 'Test thread' },
      ]);
      renderSidebar();

      await waitFor(() => expect(screen.getByText('Lead')).toBeTruthy());
      await waitFor(() => expect(screen.getByText('Test thread')).toBeTruthy());
    });

    it('a conversations update that adds a new child renders it immediately, expanded', async () => {
      mockConversations([{ id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' }]);
      renderSidebar();

      await waitFor(() => expect(screen.getByText('Lead')).toBeTruthy());
      expect(screen.queryByText('New thread')).toBeNull();

      mockConversations([
        { id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' },
        { id: 22 as ConversationId, parentId: 1 as ConversationId, at: 7, title: 'New thread' },
      ]);
      emitConversationsSignal();

      await waitFor(() => expect(screen.getByText('New thread')).toBeTruthy());
    });

    it('selecting a nested thread keeps it visible (ancestor expanded)', async () => {
      mockConversations([
        { id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' },
        { id: 16 as ConversationId, parentId: 1 as ConversationId, at: 7, title: 'Test thread' },
      ]);
      render(
        <MantineProvider>
          <Sidebar
            data={sampleData}
            nav={initialNav}
            onNavigate={() => {}}
            selectedConversationId={16 as ConversationId}
          />
        </MantineProvider>,
      );

      await waitFor(() => expect(screen.getByText('Test thread')).toBeTruthy());
    });

    // Per-element geometry mocking: a helper that gives a specific element its own scrollHeight/
    // clientHeight/getBoundingClientRect, independent of every other element (unlike stubbing
    // HTMLElement.prototype globally, which made every element -- scrollable Paper or not --
    // report identical mocked values and let a wrong-element bug pass; see K4 card).
    function mockElementGeometry(
      el: HTMLElement,
      geo: { scrollHeight: number; clientHeight: number; top: number; bottom: number },
    ) {
      Object.defineProperty(el, 'scrollHeight', { configurable: true, value: geo.scrollHeight });
      Object.defineProperty(el, 'clientHeight', { configurable: true, value: geo.clientHeight });
      el.getBoundingClientRect = () =>
        ({
          top: geo.top,
          bottom: geo.bottom,
          left: 0,
          right: 0,
          width: 0,
          height: geo.bottom - geo.top,
          x: 0,
          y: geo.top,
          toJSON() {},
        }) as DOMRect;
    }

    it('scrolls the new row into view when the real scroller (clipped outer Paper) overflows, even though the inner Stack does not', async () => {
      const manyConversations: ConversationNode[] = Array.from({ length: 30 }, (_, i) => ({
        id: (i + 1) as ConversationId,
        parentId: null,
        at: null,
        title: `Thread ${i + 1}`,
      }));
      mockConversations(manyConversations);
      vi.mocked(client.createConversation).mockResolvedValue({ id: 99 });

      const scrollIntoViewSpy = vi.fn();
      const originalScrollIntoView = Element.prototype.scrollIntoView;
      Element.prototype.scrollIntoView = scrollIntoViewSpy;

      let selectedConversationId: ConversationId | undefined;
      const { container, rerender } = render(
        <MantineProvider>
          {/* Outer clipped scroller mimicking App.tsx's Paper[data-testid=panel-tree]: scrollable,
              rect [56,265]. */}
          <div data-testid="outer-scroller" style={{ overflowY: 'auto' }}>
            <Sidebar
              data={sampleData}
              nav={initialNav}
              onNavigate={() => {}}
              selectedConversationId={selectedConversationId}
              onConversationCreated={(id) => {
                selectedConversationId = id;
              }}
            />
          </div>
        </MantineProvider>,
      );
      await waitFor(() => expect(screen.getByText('Thread 1')).toBeTruthy());

      fireEvent.click(screen.getByTestId('new-session-button'));
      await waitFor(() => expect(selectedConversationId).toBe(99));

      mockConversations([
        ...manyConversations,
        { id: 99 as ConversationId, parentId: null, at: null, title: 'New thread' },
      ]);
      emitConversationsSignal();

      rerender(
        <MantineProvider>
          <div data-testid="outer-scroller" style={{ overflowY: 'auto' }}>
            <Sidebar
              data={sampleData}
              nav={initialNav}
              onNavigate={() => {}}
              selectedConversationId={selectedConversationId}
              onConversationCreated={(id) => {
                selectedConversationId = id;
              }}
            />
          </div>
        </MantineProvider>,
      );

      await waitFor(() => expect(screen.getByText('New thread')).toBeTruthy());

      const outer = container.querySelector('[data-testid="outer-scroller"]') as HTMLElement;
      // Inner Stack (Sidebar's own root): same scrollHeight as clientHeight -- NOT scrollable --
      // and an unclipped rect extending past the visible area, matching the measured repro.
      const inner = outer.firstElementChild as HTMLElement;
      mockElementGeometry(outer, { scrollHeight: 293, clientHeight: 207, top: 0, bottom: 265 });
      mockElementGeometry(inner, { scrollHeight: 293, clientHeight: 293, top: 1, bottom: 406 });
      const row = screen.getByTestId('thread-99');
      mockElementGeometry(row, { scrollHeight: 0, clientHeight: 0, top: 310, bottom: 338 });

      // The scroll effect already ran once (before geometry was mocked, since the real DOM
      // nodes didn't exist yet). Re-trigger it with the mocks now in place by re-emitting the
      // conversations signal with a fresh array reference, which recomputes conversationTreeData
      // (an effect dependency) and re-runs the scroll-into-view effect.
      mockConversations([
        ...manyConversations,
        { id: 99 as ConversationId, parentId: null, at: null, title: 'New thread' },
      ]);
      emitConversationsSignal();

      await waitFor(() => expect(scrollIntoViewSpy).toHaveBeenCalledWith({ block: 'nearest' }));

      Element.prototype.scrollIntoView = originalScrollIntoView;
    });

    it('does not scroll when the newly selected row is already fully visible against the real scroller', async () => {
      mockConversations([{ id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' }]);
      vi.mocked(client.createConversation).mockResolvedValue({ id: 99 });

      const scrollIntoViewSpy = vi.fn();
      const originalScrollIntoView = Element.prototype.scrollIntoView;
      Element.prototype.scrollIntoView = scrollIntoViewSpy;

      let selectedConversationId: ConversationId | undefined;
      const { container, rerender } = render(
        <MantineProvider>
          <div data-testid="outer-scroller" style={{ overflowY: 'auto' }}>
            <Sidebar
              data={sampleData}
              nav={initialNav}
              onNavigate={() => {}}
              selectedConversationId={selectedConversationId}
              onConversationCreated={(id) => {
                selectedConversationId = id;
              }}
            />
          </div>
        </MantineProvider>,
      );
      await waitFor(() => expect(screen.getByText('Lead')).toBeTruthy());

      fireEvent.click(screen.getByTestId('new-session-button'));
      await waitFor(() => expect(selectedConversationId).toBe(99));

      mockConversations([
        { id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' },
        { id: 99 as ConversationId, parentId: null, at: null, title: 'New thread' },
      ]);
      emitConversationsSignal();

      rerender(
        <MantineProvider>
          <div data-testid="outer-scroller" style={{ overflowY: 'auto' }}>
            <Sidebar
              data={sampleData}
              nav={initialNav}
              onNavigate={() => {}}
              selectedConversationId={selectedConversationId}
              onConversationCreated={(id) => {
                selectedConversationId = id;
              }}
            />
          </div>
        </MantineProvider>,
      );

      await waitFor(() => expect(screen.getByText('New thread')).toBeTruthy());

      const outer = container.querySelector('[data-testid="outer-scroller"]') as HTMLElement;
      const inner = outer.firstElementChild as HTMLElement;
      // Not scrollable: scrollHeight <= clientHeight on the real scroller.
      mockElementGeometry(outer, { scrollHeight: 200, clientHeight: 200, top: 0, bottom: 200 });
      mockElementGeometry(inner, { scrollHeight: 200, clientHeight: 200, top: 0, bottom: 200 });
      const row = screen.getByTestId('thread-99');
      mockElementGeometry(row, { scrollHeight: 0, clientHeight: 0, top: 10, bottom: 30 });

      expect(scrollIntoViewSpy).not.toHaveBeenCalled();

      Element.prototype.scrollIntoView = originalScrollIntoView;
    });

    it('clicking a fixture lead/plan/job row never calls onSelectConversation', async () => {
      mockConversations([{ id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' }]);
      let conversationSelected = false;
      renderSidebar(initialNav);
      // Re-render with a tracked onSelectConversation to assert it's untouched by fixture clicks.
      const onSelectConversation = () => {
        conversationSelected = true;
      };
      const { unmount } = render(
        <MantineProvider>
          <Sidebar
            data={sampleData}
            nav={initialNav}
            onNavigate={() => {}}
            onSelectConversation={onSelectConversation}
          />
        </MantineProvider>,
      );
      fireEvent.click(screen.getAllByText('Grove automation')[0]);
      expect(conversationSelected).toBe(false);
      unmount();
    });
  });
});
