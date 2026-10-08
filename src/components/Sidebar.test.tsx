import { describe, expect, it } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { Sidebar } from './Sidebar';
import { initialNav, selectLead, selectPlan, type NavState } from '../model/navigation';
import { sampleData } from '../fixtures/sample';

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
});
