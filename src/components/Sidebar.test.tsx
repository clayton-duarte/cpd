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

    it('scrolls the new row into view when the panel is full and scrollable', async () => {
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
      const originalScrollHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'scrollHeight');
      const originalClientHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight');
      const originalGetBoundingClientRect = HTMLElement.prototype.getBoundingClientRect;
      Object.defineProperty(HTMLElement.prototype, 'scrollHeight', { configurable: true, value: 1000 });
      Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, value: 200 });
      // Simulate the panel's viewport (0-200) and a row below the fold (281-310), matching the
      // measured repro in the card.
      HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
        const isRow = this.getAttribute('data-testid') === 'thread-99';
        const top = isRow ? 281 : 0;
        const bottom = isRow ? 310 : 200;
        return { top, bottom, left: 0, right: 0, width: 0, height: bottom - top, x: 0, y: top, toJSON() {} };
      };

      let selectedConversationId: ConversationId | undefined;
      const { rerender } = render(
        <MantineProvider>
          <Sidebar
            data={sampleData}
            nav={initialNav}
            onNavigate={() => {}}
            selectedConversationId={selectedConversationId}
            onConversationCreated={(id) => {
              selectedConversationId = id;
            }}
          />
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
          <Sidebar
            data={sampleData}
            nav={initialNav}
            onNavigate={() => {}}
            selectedConversationId={selectedConversationId}
            onConversationCreated={(id) => {
              selectedConversationId = id;
            }}
          />
        </MantineProvider>,
      );

      await waitFor(() => expect(screen.getByText('New thread')).toBeTruthy());
      await waitFor(() => expect(scrollIntoViewSpy).toHaveBeenCalledWith({ block: 'nearest' }));

      Element.prototype.scrollIntoView = originalScrollIntoView;
      HTMLElement.prototype.getBoundingClientRect = originalGetBoundingClientRect;
      if (originalScrollHeight) Object.defineProperty(HTMLElement.prototype, 'scrollHeight', originalScrollHeight);
      if (originalClientHeight) Object.defineProperty(HTMLElement.prototype, 'clientHeight', originalClientHeight);
    });

    it('does not scroll when the newly selected row is already fully visible / not scrollable', async () => {
      mockConversations([{ id: 1 as ConversationId, parentId: null, at: null, title: 'Lead' }]);
      vi.mocked(client.createConversation).mockResolvedValue({ id: 99 });

      const scrollIntoViewSpy = vi.fn();
      const originalScrollIntoView = Element.prototype.scrollIntoView;
      Element.prototype.scrollIntoView = scrollIntoViewSpy;
      const originalScrollHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'scrollHeight');
      const originalClientHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight');
      // Not scrollable: scrollHeight <= clientHeight.
      Object.defineProperty(HTMLElement.prototype, 'scrollHeight', { configurable: true, value: 200 });
      Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, value: 200 });

      let selectedConversationId: ConversationId | undefined;
      const { rerender } = render(
        <MantineProvider>
          <Sidebar
            data={sampleData}
            nav={initialNav}
            onNavigate={() => {}}
            selectedConversationId={selectedConversationId}
            onConversationCreated={(id) => {
              selectedConversationId = id;
            }}
          />
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
          <Sidebar
            data={sampleData}
            nav={initialNav}
            onNavigate={() => {}}
            selectedConversationId={selectedConversationId}
            onConversationCreated={(id) => {
              selectedConversationId = id;
            }}
          />
        </MantineProvider>,
      );

      await waitFor(() => expect(screen.getByText('New thread')).toBeTruthy());
      expect(scrollIntoViewSpy).not.toHaveBeenCalled();

      Element.prototype.scrollIntoView = originalScrollIntoView;
      if (originalScrollHeight) Object.defineProperty(HTMLElement.prototype, 'scrollHeight', originalScrollHeight);
      if (originalClientHeight) Object.defineProperty(HTMLElement.prototype, 'clientHeight', originalClientHeight);
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
