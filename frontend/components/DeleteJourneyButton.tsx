'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Group, Modal, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Direct, one-click "delete this journey" action on `/journeys/[id]`
 * (feature request: "journeys should be mutable ... you should be able to
 * ... delete parts or even the whole journey"). Before this component
 * existed, the ONLY way to remove a whole journey was
 * `RemoveJourneyLegButton` repeated once per leg -- `journeys::delete_leg`
 * (`crates/api/src/data/journeys.rs`) only incidentally deletes the whole
 * `journeys` row when the removed leg happened to be the last one
 * remaining. A traveller who wanted to abandon an entire multi-leg journey
 * had no single action for it; this is that action, backed by the new
 * `DELETE /Journeys/{journeyId}`
 * (`crates/api/src/routes/journeys.rs::delete_journey` ->
 * `crates/api/src/data/journeys.rs::delete_journey`), which removes the
 * journey and every one of its legs in one statement (the schema's own
 * `journey_legs.journey_id ... ON DELETE CASCADE`, plus everything that in
 * turn cascades from THAT -- see that data function's own doc comment for
 * the full FK accounting).
 *
 * Deletes via the same-origin `/api/*` proxy, same as every other
 * Client-Component mutation in this codebase (`RemoveJourneyLegButton`,
 * `DeleteTrainButton`, ...) -- this is a Client Component and cannot reach
 * the `api` service directly.
 *
 * Rendered only for `journey.isOwner` (`app/journeys/[id]/page.tsx`) --
 * `delete_journey` 404s "doesn't exist" and "exists but isn't yours"
 * identically (never `403`), so offering this to a non-owning
 * shared-group viewer would only ever produce a dead end, same reasoning
 * as every other owner-only control on that page (`AddJourneyLegButton`,
 * `ShareJourneyButton`, `SaveAsTemplateButton`).
 *
 * Confirm-modal shape (`Modal` + `useDisclosure` + `useNeedsLogin`,
 * `aria-label="Confirm delete journey"` on the modal's own action button
 * so it has a distinct accessible name from this component's trigger once
 * both are in the DOM) is the SAME established pattern
 * `RemoveJourneyLegButton`/`DeleteTrainButton` already use for every other
 * destructive action in this codebase -- deliberately not a bare
 * `window.confirm`.
 *
 * On success, always navigates to `/track/mine` -- unlike
 * `RemoveJourneyLegButton` (which only redirects when the removed leg was
 * the journey's LAST one, and otherwise stays on the page and refreshes),
 * this action always makes `/journeys/{journeyId}` itself 404 on a
 * refresh, so there is nothing to refresh back to. `/track/mine` is the
 * same "back to my trains & journeys" destination this page's own
 * existing back-link already points to. */
export function DeleteJourneyButton({ journeyId }: { journeyId: number }) {
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
      const response = await fetch(`/api/Journeys/${journeyId}`, { method: 'DELETE' });
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
      router.push('/track/mine');
    } catch {
      setError('Request failed.');
      setDeleting(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Delete journey
      </Button>
      <Modal opened={opened} onClose={close} title="Delete this journey?">
        <Text>This removes every leg on this journey. This cannot be undone.</Text>
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to delete this journey</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={deleting}>
            Cancel
          </Button>
          <Button color="red" onClick={handleDelete} loading={deleting} aria-label="Confirm delete journey">
            Delete journey
          </Button>
        </Group>
      </Modal>
    </>
  );
}
