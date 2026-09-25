'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Text } from '@mantine/core';
import type { JourneyLegProposal } from '@/lib/types';

/** `"HH:MM:SS" | null` -> `"HH:MM" | null` -- `TrackTrainForm`'s window-mode
 * time fields take the same "HH:MM" shape `TimeFilterInput` already uses
 * everywhere else in this app (see `trackAgainPrefill.ts`'s own identically
 * named/shaped helper, which this one otherwise duplicates rather than
 * importing: that module's own doc comment scopes it specifically to a
 * `JourneyLegDetail`'s `depart_after`/etc. fields, not this button's
 * `JourneyLegProposal` response). */
function toHHMM(value: string | null): string | null {
  return value ? value.slice(0, 5) : null;
}

/** "Create a journey leg from this ticket" -- the propose-and-confirm entry
 * point onto `GET /Train/tickets/{ticketId}/journey-leg-proposal`
 * (`data::journey_leg_proposal`). Fetches the proposal, then -- same
 * pure-navigation shape as `TrackJourneyAgainButton.tsx`'s own
 * `router.push(href)` -- sends the caller to `/track` with the proposed
 * origin/destination/service-date/departure-window pre-filled as query
 * params, exactly the same deep-link mechanism `TrackJourneyAgainButton`,
 * `TicketEntryForm`'s own "find or track the train this ticket is for"
 * link, and `/stations/[crs]`'s "Track a train from here" shortcut already
 * use.
 *
 * Deliberately does NOT create anything itself, and never can: this
 * component holds no `POST` call of any kind. `/track` mounts
 * `TrackTrainForm` completely unmodified -- pre-filled in window mode, with
 * every field still fully editable, and gated behind that form's own
 * existing validation and its own explicit "Search for a train"/"Track this
 * train" submit button. A caller who never clicks that button has created
 * nothing at all; this button's own job ends the moment the browser
 * navigates. See `data::journey_leg_proposal`'s own module doc comment for
 * the full propose-and-confirm reasoning. */
export function CreateJourneyLegFromTicketButton({ ticketId }: { ticketId: number }) {
  const router = useRouter();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleClick() {
    setLoading(true);
    setError(null);
    try {
      const response = await fetch(`/api/Train/tickets/${ticketId}/journey-leg-proposal`);
      if (!response.ok) {
        setError(
          response.status === 401
            ? 'Log in to create a journey leg from this ticket.'
            : "Couldn't load a proposal for this ticket.",
        );
        return;
      }
      const proposal: JourneyLegProposal = await response.json();

      const params = new URLSearchParams();
      params.set('mode', 'window');
      if (proposal.originCrs) params.set('origin', proposal.originCrs);
      if (proposal.destinationCrs) params.set('destination', proposal.destinationCrs);
      if (proposal.serviceDate) params.set('serviceDate', proposal.serviceDate);
      const departAfter = toHHMM(proposal.departAfter);
      const departBefore = toHHMM(proposal.departBefore);
      if (departAfter) params.set('departAfter', departAfter);
      if (departBefore) params.set('departBefore', departBefore);

      router.push(`/track?${params.toString()}`);
    } catch {
      setError("Couldn't load a proposal for this ticket.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={() => void handleClick()} loading={loading}>
        Create a journey leg from this ticket
      </Button>
      {error && (
        <Text size="xs" c="var(--ds-color-error-text)">
          {error}
        </Text>
      )}
    </>
  );
}
