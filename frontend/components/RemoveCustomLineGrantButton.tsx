'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Stops sharing a custom line with a group -- the member who shared it
 * (always the line's owner), or any `admin`/`owner`, may click this (the
 * backend enforces which, via `groups::remove_custom_line_grant`'s
 * sharer-or-manager check). The page
 * (`app/groups/[id]/page.tsx`'s `SharedCustomLineRow`) only renders this
 * when its own `canRemove` mirrors that same check for the current viewer
 * and row, so a plain member never sees it on someone else's shared line
 * -- defence in depth, not reliance on the backend alone, same posture as
 * `RemoveGroupTrainButton`.
 *
 * Worded "Stop sharing", not "Remove": this only revokes the group's
 * visibility. The line itself, its content and its ownership are entirely
 * untouched, and only its owner can ever delete it. */
export function RemoveCustomLineGrantButton({
  groupId,
  lineId,
  lineName,
}: {
  groupId: string;
  lineId: string;
  lineName: string;
}) {
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
      const response = await fetch(
        `/api/groups/${groupId}/lines/custom/${encodeURIComponent(lineId)}`,
        { method: 'DELETE' },
      );
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
        Stop sharing
      </Button>
      <Modal opened={opened} onClose={close} title="Stop sharing this line with the group?">
        <Text size="sm">
          Group members will no longer be able to see {lineName}. The line itself isn&apos;t
          deleted or changed.
        </Text>
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to stop sharing this line</LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={removing}>
            Cancel
          </Button>
          <Button
            color="red"
            onClick={handleRemove}
            loading={removing}
            aria-label="Confirm stop sharing line with group"
          >
            Stop sharing
          </Button>
        </Group>
      </Modal>
    </>
  );
}
