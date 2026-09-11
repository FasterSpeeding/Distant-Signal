'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** `owner`-only "Delete group" control for `DELETE /groups/{id}` (spec §3:
 * "Delete the group entirely | `owner` only" -- the caller renders this
 * only for an owner, and the backend's `require_role(..,
 * GroupRole::is_owner)` refuses everyone else regardless).
 *
 * Mirrors `LeaveGroupButton.tsx`'s confirm-modal shape rather than a
 * bare button: `groups::delete_group`'s `ON DELETE CASCADE` takes the
 * memberships, the shared-train rows, and the invite links with it, for
 * EVERY member -- strictly more destructive than leaving, so it gets at
 * least the same confirmation weight. Navigates to `/groups` on success,
 * the same "navigate away from a now-gone resource" move
 * `DeleteTrainButton`/`LeaveGroupButton` both make; staying on a detail
 * page for a group that no longer exists would just render a 404. */
export function DeleteGroupButton({ groupId, name }: { groupId: string; name: string }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleDelete() {
    setDeleting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDeleting(false);
        return;
      }
      router.push('/groups');
    } catch {
      setError('Request failed.');
      setDeleting(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" onClick={open}>
        Delete group
      </Button>
      <Modal opened={opened} onClose={close} title={`Delete ${name}?`}>
        <Text>
          This deletes the group for everyone. Every member loses access, every train shared into it stops being
          shared, and the invite link stops working. This can&apos;t be undone.
        </Text>
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to delete this group</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={deleting}>
            Cancel
          </Button>
          <Button color="red" onClick={handleDelete} loading={deleting} aria-label="Confirm delete group">
            Delete
          </Button>
        </Group>
      </Modal>
    </>
  );
}
