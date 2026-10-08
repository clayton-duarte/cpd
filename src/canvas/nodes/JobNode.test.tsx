import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { ReactFlowProvider } from '@xyflow/react';
import { MantineProvider } from '@mantine/core';
import { JobNode } from './JobNode';
import { sampleData } from '../../fixtures/sample';

function renderJobNode(selected: boolean) {
  const job = sampleData.jobs[0];
  return render(
    <MantineProvider>
      <ReactFlowProvider>
        <JobNode data={{ job }} selected={selected} />
      </ReactFlowProvider>
    </MantineProvider>,
  );
}

describe('JobNode selection', () => {
  it('a selected job node carries data-selected="true"', () => {
    const { container } = renderJobNode(true);
    const root = container.querySelector('[data-selected]');
    expect(root?.getAttribute('data-selected')).toBe('true');
    expect(root?.getAttribute('aria-selected')).toBe('true');
  });

  it('an unselected job node carries data-selected="false"', () => {
    const { container } = renderJobNode(false);
    const root = container.querySelector('[data-selected]');
    expect(root?.getAttribute('data-selected')).toBe('false');
    expect(root?.getAttribute('aria-selected')).toBe('false');
  });
});
