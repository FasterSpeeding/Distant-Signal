'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** The confirm page's explicit "Join" action (spec §2.3: "Confirm-before-
 * join, never silent auto-join" -- this component is that explicit
 * action). Handles a 401 the same way every other mutating control in
 * this app does, even though the page only renders this for an already-
 * authenticated visitor -- a session can still lapse between page load
 * and this click, the same narrow race `DeleteTrainButton`'s own doc
 * comment already names. */
export function JoinGroupButton({ token, groupId }: { token: string; groupId: string }) {
  const router = useRouter();
  const [joining, setJoining] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleJoin() {
    setJoining(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/join/${token}`, { method: 'POST' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setJoining(false);
        return;
      }
      router.push(`/groups/${groupId}`);
    } catch {
      setError('Request failed.');
      setJoining(false);
    }
  }

  return (
    <>
      {error && <Alert color="red">{error}</Alert>}
      {needsLoginState.needsLogin ? (
        <LoginLink underline="always">Log in to join this group</LoginLink>
      ) : (
        <Button onClick={handleJoin} loading={joining}>
          Join group
        </Button>
      )}
    </>
  );
}
