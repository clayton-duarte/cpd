import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { WorkflowHeader } from './WorkflowHeader';
import { sampleData } from '../fixtures/sample';

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
  it('shows #128 for pr and #2 for attempt', () => {
    renderHeader();
    expect(
      screen.getByText((_, el) => el?.tagName === 'P' && el.textContent === '#128'),
    ).toBeTruthy();
    expect(
      screen.getByText((_, el) => el?.textContent === '#2' && el.children.length === 0),
    ).toBeTruthy();
  });

  it('renders the workflow title', () => {
    const workflow = renderHeader();
    expect(screen.getByText(workflow.title)).toBeTruthy();
  });
});
