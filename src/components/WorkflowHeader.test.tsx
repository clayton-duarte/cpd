import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { WorkflowHeader } from './WorkflowHeader';
import { sampleData } from '../fixtures/sample';
import { FLOW_ORDER } from '../model/derive';

function renderHeader() {
  const workflow = sampleData.workflows[0];
  render(
    <MantineProvider>
      <WorkflowHeader workflow={workflow} />
    </MantineProvider>,
  );
  return workflow;
}

describe('WorkflowHeader', () => {
  it('renders all five flow labels', () => {
    renderHeader();
    for (const label of FLOW_ORDER) {
      expect(screen.getAllByText(label).length).toBeGreaterThan(0);
    }
  });

  it('marks exactly one label as current', () => {
    renderHeader();
    const current = screen.getAllByTestId('flow-label-current');
    expect(current).toHaveLength(1);
    expect(current[0].textContent).toContain('Dispatch');
  });

  it('contains Dispatch for phase combat and no engine phase string', () => {
    const { container } = render(
      <MantineProvider>
        <WorkflowHeader workflow={sampleData.workflows[0]} />
      </MantineProvider>,
    );
    expect(container.textContent).toContain('Dispatch');
    expect(container.textContent).not.toMatch(/combat|main1|main2|upkeep|draw(?!er)|\bend\b/);
  });

  it('shows #128 for pr and #2 for attempt', () => {
    renderHeader();
    expect(
      screen.getByText((_, el) => el?.tagName === 'P' && el.textContent === '#128'),
    ).toBeTruthy();
    expect(
      screen.getByText((_, el) => el?.textContent === '#2' && el.children.length === 0),
    ).toBeTruthy();
  });
});
