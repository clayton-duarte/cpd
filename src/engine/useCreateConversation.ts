import { useState } from 'react';
import { notifications } from '@mantine/notifications';
import { createConversation } from './client';
import type { ConversationId } from './types';

/**
 * L3: the ONE "create a Context" code path. A Context is the root pi conversation the user
 * creates manually (see DECISIONS.md L3). Both Sidebar's own "New context" button and App.tsx's
 * top-right FAB call this same hook -- there must be exactly one implementation of "create and
 * select a new conversation", not two independent ones that could drift.
 */
export function useCreateConversation(onCreated?: (id: ConversationId) => void) {
  const [creating, setCreating] = useState(false);

  const create = async () => {
    setCreating(true);
    try {
      const { id } = await createConversation();
      onCreated?.(id as ConversationId);
    } catch {
      notifications.show({
        color: 'red',
        title: 'Could not create context',
        message: 'Failed to create a new context. Please try again.',
      });
    } finally {
      setCreating(false);
    }
  };

  return { creating, create };
}
