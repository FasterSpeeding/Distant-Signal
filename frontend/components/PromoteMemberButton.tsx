'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Text } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** `owner`-only "Promote to admin" control -- no confirm modal (unlike
 * `RemoveMemberButton`/`DeleteTrainButton`): promoting is non-destructive
 * and reversible in spirit (an owner can always remove an admin they
 * regret promoting), so a bare click is proportionate, matching this
 * app's existing "confirm only genuinely destructive actions" posture. */
export function PromoteMemberButton({ groupId, userId }: { groupId: string; userId: string }) {
  const router = useRouter();
  const [promoting, setPromoting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handlePromote() {
    setPromoting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/members/${userId}/promote`, { method: 'POST' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setPromoting(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Request failed.');
      setPromoting(false);
    }
  }

  return (
    <>
      <Button variant="subtle" size="xs" onClick={handlePromote} loading={promoting}>
        Promote to admin
      </Button>
      {error && <Text c="red">{error}</Text>}
      {needsLoginState.needsLogin && <LoginLink underline="always">Log in to promote this member</LoginLink>}
    </>
  );
}
