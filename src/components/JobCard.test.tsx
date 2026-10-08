import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { JobCard } from './JobCard';
import { sampleData } from '../fixtures/sample';

describe('JobCard', () => {
  it('renders no run button when onRun is absent', () => {
    const job = sampleData.jobs[0];
    render(
      <MantineProvider>
        <JobCard job={job} />
      </MantineProvider>,
    );

    expect(screen.queryByRole('button', { name: /run/i })).toBeNull();
  });

  it('renders a run button and calls onRun with the job id when clicked', async () => {
    const job = sampleData.jobs[0];
    const onRun = vi.fn();
    render(
      <MantineProvider>
        <JobCard job={job} onRun={onRun} />
      </MantineProvider>,
    );

    const button = screen.getByRole('button', { name: /run/i });
    expect(button).toBeTruthy();
    fireEvent.click(button);

    expect(onRun).toHaveBeenCalledWith(job.id);
  });
});
