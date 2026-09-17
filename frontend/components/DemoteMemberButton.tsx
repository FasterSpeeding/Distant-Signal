'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** `owner`-only "Demote to member" control -- the inverse of
 * `PromoteMemberButton`, gated the same way (the backend's `demote_member`
 * handler checks `GroupRole::is_owner`, not `can_manage`, so an admin who
 * clicked this would only ever get a 403).
 *
 * Unlike `PromoteMemberButton`, this DOES confirm first, following
 * `RemoveMemberButton`/`LeaveGroupButton`'s modal shape. Promotion's own
 * comment reasons that a bare click is proportionate because promoting is
 * non-destructive and the owner can undo it; demotion is the direction
 * that actually takes something away -- it silently strips a member's
 * invite-link, rename and member-removal powers, with no notification to
 * them -- and it sits one row apart from "Remove" in a list of member
 * rows, where a misclick lands on the wrong person. That is the same
 * "confirm genuinely consequential actions" line this app already draws,
 * read in the direction the loss of privilege actually runs.
 *
 * The member keeps their membership and everything they've shared into the
 * group -- the confirm copy says so, so this doesn't read as a removal. */
export function DemoteMemberButton({
  groupId,
  userId,
  name,
}: {
  groupId: string;
  userId: string;
  name: string;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [demoting, setDemoting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleDemote() {
    setDemoting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/members/${userId}/demote`, { method: 'POST' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDemoting(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setDemoting(false);
    }
  }

  return (
    <>
      <Button variant="subtle" size="xs" onClick={open}>
        Demote to member
      </Button>
      <Modal opened={opened} onClose={close} title={`Demote ${name} to member?`}>
        <Text>
          They&apos;ll stay in the group and keep everything they&apos;ve shared into it, but they&apos;ll no
          longer be able to manage the invite link, rename the group, or remove other members.
        </Text>
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to demote this member</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={demoting}>
            Cancel
          </Button>
          <Button onClick={handleDemote} loading={demoting} aria-label="Confirm demote member">
            Demote
          </Button>
        </Group>
      </Modal>
    </>
  );
}
