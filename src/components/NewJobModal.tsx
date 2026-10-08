import { useState } from 'react';
import { Button, Group, Modal, Stack, Text, TextInput } from '@mantine/core';
import { createJob } from '../engine/client';
import type { ConversationId } from '../engine/types';

export interface NewJobModalProps {
  conversation: ConversationId;
  opened: boolean;
  onClose: () => void;
  /** Called once the job is created so the caller can refresh / close. */
  onCreated: () => void;
}

/**
 * H13: the "New job" affordance. Plain Mantine components, zero customisation
 * (standing project rule) -- a Modal with a title TextInput and an optional
 * command TextInput. Submit is disabled while the title is blank, and a 400
 * from the daemon (bad command) surfaces inline instead of vanishing into the
 * console (D115/D122: command must be a non-empty string when present).
 */
export function NewJobModal({ conversation, opened, onClose, onCreated }: NewJobModalProps) {
  const [title, setTitle] = useState('');
  const [command, setCommand] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const reset = () => {
    setTitle('');
    setCommand('');
    setError(null);
    setSubmitting(false);
  };

  const handleClose = () => {
    reset();
    onClose();
  };

  const handleSubmit = async () => {
    setError(null);
    setSubmitting(true);
    try {
      await createJob(conversation, {
        title,
        ...(command.trim() === '' ? {} : { command }),
      });
      reset();
      onCreated();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setSubmitting(false);
    }
  };

  return (
    <Modal opened={opened} onClose={handleClose} title="New job">
      <Stack gap="md">
        <TextInput
          label="Title"
          value={title}
          onChange={(e) => setTitle(e.currentTarget.value)}
          data-autofocus
        />
        <TextInput
          label="Command (optional)"
          value={command}
          onChange={(e) => setCommand(e.currentTarget.value)}
        />
        {error && (
          <Text c="red" size="sm">
            {error}
          </Text>
        )}
        <Group justify="flex-end">
          <Button variant="default" onClick={handleClose}>
            Cancel
          </Button>
          <Button onClick={handleSubmit} disabled={title.trim() === '' || submitting} loading={submitting}>
            Create
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
