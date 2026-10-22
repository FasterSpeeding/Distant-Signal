'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Stack } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginControl } from './LoginControl';
import { describeFailure, failureFromResponse } from '@/lib/failure';

/** "Follow this journey" / "Unfollow" for a journey shared with the caller
 * (DS-146): following lists it among their own journeys, on the home page
 * and `/journeys`, read-only. `POST`/`DELETE /api/Journeys/{id}/follow`.
 *
 * Follows through `groupId`'s share when given, else the share link whose
 * `shareToken` is given (sent in the body, never the URL), else any group
 * the journey is shared into that the caller is in. After a follow it goes
 * to `afterFollowHref` when given (the share-link page, whose journey the
 * caller can now open as theirs), else refreshes; after an unfollow, to
 * `afterUnfollowHref` when given (a journey followed through a link can't
 * be opened once unfollowed), else refreshes. */
export function FollowJourneyButton({
  journeyId,
  following,
  groupId,
  shareToken,
  afterFollowHref,
  afterUnfollowHref,
  size = 'xs',
}: {
  journeyId: number;
  following: boolean;
  groupId?: string;
  shareToken?: string;
  afterFollowHref?: string;
  afterUnfollowHref?: string | undefined;
  size?: 'xs' | 'sm';
}) {
  const router = useRouter();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();
  const action = following ? 'unfollow' : 'follow';

  async function handleClick() {
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    const body = following ? null : groupId ? { groupId } : shareToken ? { shareToken } : null;
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/follow`, {
        method: following ? 'DELETE' : 'POST',
        ...(body === null ? {} : { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          setError(await failureFromResponse(action, 'this journey', response));
        }
        setSubmitting(false);
        return;
      }
      setSubmitting(false);
      const next = following ? afterUnfollowHref : afterFollowHref;
      if (next) {
        router.push(next);
      } else {
        router.refresh();
      }
    } catch {
      setError(describeFailure(action, 'this journey'));
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="xs" align="flex-start">
      <Button variant={following ? 'default' : 'light'} size={size} onClick={handleClick} loading={submitting}>
        {following ? 'Unfollow' : 'Follow this journey'}
      </Button>
      {error && <Alert color="red">{error}</Alert>}
      {needsLoginState.needsLogin && <LoginControl underline="always">Log in to follow this journey</LoginControl>}
    </Stack>
  );
}
