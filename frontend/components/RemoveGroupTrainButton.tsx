'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Removes a shared train from a group -- the sharer, or any `admin`/
 * `owner`, may click this (the backend enforces which via
 * `groups::remove_train_from_group`'s sharer-or-manager check). The page
 * (`app/groups/[id]/page.tsx`'s `SharedTrainRow`) only renders this when
 * its own `canRemove` mirrors that same check for the current viewer and
 * row, so a plain member never sees it on someone else's shared train --
 * defense in depth, not reliance on the backend alone, same posture as
 * `RemoveMemberButton`'s owner-row gating. Mirrors `DeleteTrainButton.tsx`'s
 * confirm-modal shape. */
export function RemoveGroupTrainButton({ groupId, trainSubscriptionId }: { groupId: string; trainSubscriptionId: number }) {
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
      const response = await fetch(`/api/groups/${groupId}/trains/${trainSubscriptionId}`, { method: 'DELETE' });
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
      <Button variant="subtle" color="red" size="xs" onClick={open}>
        Remove from group
      </Button>
      <Modal opened={opened} onClose={close} title="Remove this train from the group?">
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to remove this train</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={removing}>
            Cancel
          </Button>
          <Button color="red" onClick={handleRemove} loading={removing} aria-label="Confirm remove train from group">
            Remove
          </Button>
        </Group>
      </Modal>
    </>
  );
}
