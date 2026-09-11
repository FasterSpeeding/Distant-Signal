'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Self-removal, always allowed for any member (spec §3) -- unlike
 * `RemoveMemberButton`, this targets the CURRENT user's own id, so it
 * needs no `admin`/`owner` gating at the call site: the page renders this
 * for every member's own row unconditionally. On success, navigates back
 * to `/groups` (there's no reason to stay on a group's own detail page
 * once you've left it) -- same "navigate away from a now-gone-to-you
 * resource" reasoning as `DeleteTrainButton`'s own redirect.
 *
 * `willDeleteGroup` should be `true` when the caller is the sole owner of
 * an otherwise-empty group: `groups::remove_member`'s sole-owner-leaves
 * branch deletes the WHOLE group in that case, rather than leaving the
 * caller's own membership row removed -- the confirm copy below says so
 * explicitly instead of the generic "you'll lose access" wording, so this
 * doesn't read as a smaller action than it is. */
export function LeaveGroupButton({
  groupId,
  currentUserId,
  willDeleteGroup = false,
}: {
  groupId: string;
  currentUserId: string;
  willDeleteGroup?: boolean;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [leaving, setLeaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleLeave() {
    setLeaving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/members/${currentUserId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setLeaving(false);
        return;
      }
      router.push('/groups');
    } catch {
      setError('Request failed.');
      setLeaving(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" onClick={open}>
        Leave group
      </Button>
      <Modal opened={opened} onClose={close} title="Leave this group?">
        {willDeleteGroup ? (
          <Text>
            You&apos;re the only member of this group, so leaving will delete it for good -- every shared
            train and the invite link will be gone.
          </Text>
        ) : (
          <Text>
            You&apos;ll lose access to every train shared in this group, and any trains you&apos;ve shared into it
            will be removed for everyone else too.
          </Text>
        )}
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to leave this group</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={leaving}>
            Cancel
          </Button>
          <Button color="red" onClick={handleLeave} loading={leaving} aria-label="Confirm leave group">
            Leave
          </Button>
        </Group>
      </Modal>
    </>
  );
}
