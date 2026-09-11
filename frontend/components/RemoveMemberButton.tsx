'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** `admin`/`owner`-only "Remove" control for one row in the members list,
 * via the same-origin `/api/*` proxy. Mirrors `DeleteTrainButton.tsx`'s
 * confirm-modal shape exactly. The backend's own `remove_member` handler
 * already refuses to target the `owner` row (`403`) regardless of what
 * this button does, but the caller (Task 12's page) additionally never
 * RENDERS this button for the owner's own row at all -- defense in depth,
 * not reliance on the backend alone. */
export function RemoveMemberButton({ groupId, userId, name }: { groupId: string; userId: string; name: string }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleRemove() {
    setRemoving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/members/${userId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setRemoving(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setRemoving(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Remove
      </Button>
      <Modal opened={opened} onClose={close} title={`Remove ${name} from this group?`}>
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to remove this member</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={removing}>
            Cancel
          </Button>
          <Button color="red" onClick={handleRemove} loading={removing} aria-label="Confirm remove member">
            Remove
          </Button>
        </Group>
      </Modal>
    </>
  );
}
