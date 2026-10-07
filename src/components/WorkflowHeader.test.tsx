import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { WorkflowHeader } from './WorkflowHeader';
import { sampleData } from '../fixtures/sample';
import type { Workflow } from '../model/types';

function renderWorkflow(workflow: Workflow) {
  render(
    <MantineProvider>
      <WorkflowHeader workflow={workflow} />
    </MantineProvider>,
  );
  return workflow;
}

describe('WorkflowHeader', () => {
  it('renders ticket key, title, branch and PR when all are present', () => {
    const workflow = sampleData.workflows[0];
    renderWorkflow(workflow);

    expect(screen.getByText(workflow.ticket!.key)).toBeTruthy();
    expect(screen.getByText(workflow.ticket!.title)).toBeTruthy();
    expect(screen.getByText(workflow.branch!.name)).toBeTruthy();
    expect(screen.getByText(`#${workflow.pr!.number}`)).toBeTruthy();
  });

  it('uses the ticket title over the lead title when a ticket exists', () => {
    const workflow = sampleData.workflows[0];
    renderWorkflow(workflow);

    expect(screen.queryByText(workflow.title)).toBeNull();
    expect(screen.getByText(workflow.ticket!.title)).toBeTruthy();
  });

  it('renders attempt badge when attempt >= 2', () => {
    const workflow = sampleData.workflows[0];
    renderWorkflow(workflow);
    expect(
      screen.getByText((_, el) => el?.textContent === `#${workflow.attempt}` && el.children.length === 0),
    ).toBeTruthy();
  });

  it('links carry correct href and rel', () => {
    const workflow = sampleData.workflows[0];
    renderWorkflow(workflow);

    const ticketLink = screen.getByText(workflow.ticket!.key).closest('a');
    expect(ticketLink?.getAttribute('href')).toBe(workflow.ticket!.url);
    expect(ticketLink?.getAttribute('rel')).toBe('noopener noreferrer');
    expect(ticketLink?.getAttribute('target')).toBe('_blank');

    const branchLink = screen.getByText(workflow.branch!.name).closest('a');
    expect(branchLink?.getAttribute('href')).toBe(workflow.branch!.url);
    expect(branchLink?.getAttribute('rel')).toBe('noopener noreferrer');

    const prLink = screen.getByText(`#${workflow.pr!.number}`).closest('a');
    expect(prLink?.getAttribute('href')).toBe(workflow.pr!.url);
    expect(prLink?.getAttribute('rel')).toBe('noopener noreferrer');
  });

  it('renders only the title, with no ticket/branch/PR slots, when all are absent', () => {
    const workflow = sampleData.workflows[1];
    expect(workflow.ticket).toBeUndefined();
    expect(workflow.branch).toBeUndefined();
    expect(workflow.pr).toBeUndefined();

    renderWorkflow(workflow);

    expect(screen.getByText(workflow.title)).toBeTruthy();
    expect(screen.queryByRole('link')).toBeNull();
  });
});
