'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Text } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

// Review §3.2.4: this used to fire straight to `router.refresh()` on
// success, with nothing on screen ever saying the promotion happened --
// the row simply rerendered with `DemoteMemberButton` in this button's
// place a moment later, easy to miss entirely on a page with several rows.
// A short-lived confirmation line fills that gap without a full toast/
// notification system (`@mantine/notifications` isn't a project
// dependency): the refresh is deliberately delayed by this long so the
// confirmation is actually readable before the row changes out from under
// it, rather than flashing and vanishing in the same tick.
const PROMOTED_CONFIRMATION_MS = 900;

/** `owner`-only "Promote to admin" control -- no confirm MODAL (unlike
 * `RemoveMemberButton`/`DeleteTrainButton`): promoting is non-destructive
 * and literally reversible (`DemoteMemberButton` puts an admin the owner
 * regrets promoting straight back to `member`, no removal needed), so a
 * bare click is proportionate, matching this app's existing "confirm only
 * genuinely destructive actions" posture. Demotion, the direction that
 * takes privileges away, does confirm -- see that component.
 *
 * It does now show a brief inline success notice before refreshing (see
 * `PROMOTED_CONFIRMATION_MS` above) -- a lighter-weight fix than a confirm
 * modal, which would contradict this component's own "non-destructive
 * action, no gate" reasoning above; the gap review §3.2.4 actually flagged
 * was the total silence on success, not the lack of a confirm step. */
export function PromoteMemberButton({ groupId, userId }: { groupId: string; userId: string }) {
  const router = useRouter();
  const [promoting, setPromoting] = useState(false);
  const [justPromoted, setJustPromoted] = useState(false);
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
      // Deliberately not `setPromoting(false)` here -- the button stays
      // replaced by the confirmation text below until the delayed refresh
      // below swaps this row for `DemoteMemberButton` outright.
      setJustPromoted(true);
      setTimeout(() => router.refresh(), PROMOTED_CONFIRMATION_MS);
    } catch {
      setError('Request failed.');
      setPromoting(false);
    }
  }

  if (justPromoted) {
    // `aria-live="polite"`: this text appears with no user-initiated
    // navigation or focus change to draw a screen reader's attention to
    // it, unlike the button click itself.
    return (
      <Text size="sm" c="dimmed" aria-live="polite">
        Promoted to admin
      </Text>
    );
  }

  return (
    <>
      <Button variant="subtle" size="xs" onClick={handlePromote} loading={promoting}>
        Promote to admin
      </Button>
      {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
      {needsLoginState.needsLogin && <LoginLink underline="always">Log in to promote this member</LoginLink>}
    </>
  );
}
