'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Group, Modal, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** 2026-09-22 UX review finding I14/2.4: a leg created WITHOUT a search
 * window (a direct `pin`/`knownTrain` pick) has no persisted window to
 * re-search, so `JourneyLegCard.tsx` never offers "Change train" for it --
 * and until `DELETE /Journeys/{journeyId}/legs/{legId}`
 * (`crates/api/src/routes/journeys.rs::delete_journey_leg`) existed, there
 * was no way to remove it either. A traveller who picked the wrong train
 * on this path had no exit from the page at all. This is that exit.
 *
 * `JourneyLegCard` renders this for EVERY owner-viewed leg -- open
 * (unmatched), matched-with-a-window, and matched-without-a-window alike
 * -- alongside "Change train" (offered additionally whenever the leg has
 * a window) rather than instead of it. It used to render this ONLY for a
 * matched, no-window leg, on the theory that a windowed leg's "Change
 * train" was a sufficient substitute for removal; in practice that left
 * most legs on most journeys (anything created via the time-window search
 * flow) with no way to be deleted outright, only re-picked -- the backend
 * route this calls has never cared what created the leg or whether it
 * carries a window, so restricting the button that way was a
 * frontend-only gap with no backend reason behind it.
 *
 * Mirrors `DeleteTrainButton.tsx`'s confirm-modal shape closely (same
 * `Modal` + `useDisclosure` + `useNeedsLogin` pattern, same
 * `aria-label="Confirm ..."` on the modal's own action button so it has a
 * distinct accessible name from this component's trigger once both are in
 * the DOM) -- deliberately not a bare `window.confirm`, for the same
 * reason every other destructive action in this codebase isn't one.
 *
 * `isOnlyLeg` decides what happens after a successful delete:
 * `journeys::delete_leg`'s own doc comment explains that removing a
 * journey's LAST leg deletes the whole (now-empty) journey too, so
 * `/journeys/{journeyId}` would 404 on a plain `router.refresh()` --
 * this instead redirects to `/track/mine`, the same "closest surviving
 * list" target `DeleteTrainButton`'s own default `afterDelete="redirect"`
 * uses for an analogous "this page's own object is gone" case. When a
 * SIBLING leg remains, the journey survives and `router.refresh()` is
 * enough -- the Server Component page re-fetches and this leg's card is
 * simply gone from the list. */
export function RemoveJourneyLegButton({
  journeyId,
  legId,
  isOnlyLeg,
}: {
  journeyId: number;
  legId: number;
  isOnlyLeg: boolean;
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
      const response = await fetch(`/api/Journeys/${journeyId}/legs/${legId}`, { method: 'DELETE' });
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
      if (isOnlyLeg) {
        router.push('/track/mine');
      } else {
        router.refresh();
      }
    } catch {
      setError('Request failed.');
      setRemoving(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Remove leg
      </Button>
      <Modal opened={opened} onClose={close} title="Remove this leg?">
        <Text>
          {isOnlyLeg
            ? 'This is the only leg on this journey, so removing it deletes the whole journey. This cannot be undone.'
            : "This cannot be undone. You can add a new leg afterwards if you'd rather pick a different train."}
        </Text>
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to remove this leg</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={removing}>
            Cancel
          </Button>
          <Button color="red" onClick={handleRemove} loading={removing} aria-label="Confirm remove leg">
            Remove leg
          </Button>
        </Group>
      </Modal>
    </>
  );
}
