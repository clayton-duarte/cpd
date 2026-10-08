import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { NewJobModal } from './NewJobModal';
import * as client from '../engine/client';
import type { ConversationId } from '../engine/types';

function renderModal(onCreated: () => void = vi.fn()) {
  return render(
    <MantineProvider>
      <NewJobModal conversation={16 as ConversationId} opened onClose={() => {}} onCreated={onCreated} />
    </MantineProvider>,
  );
}

describe('NewJobModal', () => {
  it('disables submit when the title is blank', () => {
    renderModal();
    const submit = screen.getByRole('button', { name: /create/i });
    expect(submit.hasAttribute('disabled')).toBe(true);
  });

  it('calls createJob with the typed title and command, then notifies the caller', async () => {
    const createJobSpy = vi.spyOn(client, 'createJob').mockResolvedValue({
      job: { id: 'j1', title: 'Build', status: 'draft', needs: [] },
    });
    const onCreated = vi.fn();
    renderModal(onCreated);

    fireEvent.change(screen.getByLabelText(/title/i), { target: { value: 'Build' } });
    fireEvent.change(screen.getByLabelText(/command/i), { target: { value: 'echo hi' } });

    const submit = screen.getByRole('button', { name: /create/i });
    expect(submit.hasAttribute('disabled')).toBe(false);
    fireEvent.click(submit);

    await waitFor(() => expect(createJobSpy).toHaveBeenCalledWith(16, { title: 'Build', command: 'echo hi' }));
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
  });

  it('shows a visible error message when creation fails with a 400', async () => {
    vi.spyOn(client, 'createJob').mockRejectedValue(new Error('Request failed: 400 Bad Request'));
    renderModal();

    fireEvent.change(screen.getByLabelText(/title/i), { target: { value: 'Build' } });
    fireEvent.click(screen.getByRole('button', { name: /create/i }));

    await waitFor(() => expect(screen.getByText(/400/)).toBeTruthy());
  });
});
