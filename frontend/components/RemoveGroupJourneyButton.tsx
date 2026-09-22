'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Removes a shared journey from a group -- the sharer, or any `admin`/
 * `owner`, may click this (the backend enforces which via
 * `groups::remove_journey_from_group`'s sharer-or-manager check). Mirrors
 * `RemoveGroupTrainButton.tsx` verbatim -- see that component's own doc
 * comment for the full reasoning, which applies here unchanged. */
export function RemoveGroupJourneyButton({ groupId, journeyId }: { groupId: string; journeyId: number }) {
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
      const response = await fetch(`/api/groups/${groupId}/journeys/${journeyId}`, { method: 'DELETE' });
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
      <Modal opened={opened} onClose={close} title="Remove this journey from the group?">
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to remove this journey</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={removing}>
            Cancel
          </Button>
          <Button color="red" onClick={handleRemove} loading={removing} aria-label="Confirm remove journey from group">
            Remove
          </Button>
        </Group>
      </Modal>
    </>
  );
}
