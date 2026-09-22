'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Stack } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import { TrackDestinationModal } from './TrackDestinationModal';
import { useGroupSummaries } from '@/lib/useGroupSummaries';
import { shareTrackedTrainToGroup } from '@/lib/shareTrackedTrain';
import type { CreateJourneyResponse } from '@/lib/types';

/** The "Track this train" action for a train whose real CIF identity is
 * already known -- a `(train_uid, service_date)` pair. Calls `POST
 * /Journeys` (`crates/api/src/routes/journeys.rs`) with a single
 * `knownTrain`-mode leg (`{ mode: 'knownTrain', trainUid, serviceDate }`):
 * identity is already fully known, so there is no form to fill in and
 * nothing to validate client-side. That is the whole difference from
 * `TrackTrainForm`'s legacy `POST /Train/track` flow, which has to guess an
 * identity from a CRS + time pin. Journey tracking Phase 1 -- see
 * .superpowers/sdd/2026-09-22-journey-tracking-phase1-single-leg-migration-plan
 * -- moved this off the former single-purpose
 * `POST /Train/by-uid/{uid}/{date}/track` route onto the new journeys
 * entry point; the response now carries a `journeyId` alongside the
 * underlying `trackingId`.
 *
 * Two call sites, deliberately different:
 * * `/trains` result rows pass `attachTicketId`, giving the listing page's
 *   action full parity with `/track?ticketId=...`'s existing flow
 *   (docs/superpowers/specs/2026-09-07-train-listing-page-design.md §4).
 * * `/train/[uid]/[date]`'s page-level CTA does NOT -- that page has no
 *   `ticketId` query-param convention and inventing one is explicitly out
 *   of scope (§5 of the same doc).
 *
 * The ticket-attach follow-up mirrors `TrackTrainForm.tsx`'s own
 * (`handleSubmit`, the `attachTicketId !== undefined` block) exactly,
 * including swallowing every failure: tracking the train has ALREADY
 * succeeded by then, so a network blip or a `409 Conflict` (the ticket
 * having since been attached elsewhere) must not block navigation. The
 * ticket simply stays standalone and reattachable from the merged
 * trains/tickets list.
 *
 * Calls the same-origin `/api/*` proxy rather than `lib/api.ts` -- this is
 * a Client Component and cannot read the server-only `API_BASE_URL` env var
 * (same reasoning as `PinToggle` and `TrackTrainForm`).
 *
 * 401 handling is the shared `useNeedsLogin`/`LoginPromptModal` pattern
 * (`useNeedsLogin.ts`'s own doc comment names it). Unlike `TrackTrainForm`,
 * there is no typed input to protect on a 401 -- this control is a single
 * click, so it behaves like `PinToggle`: show the prompt, change nothing
 * else.
 *
 * `disabled={busy}` is load-bearing, not cosmetic:
 * `train_tracking::create_subscription_for_train` is idempotent per
 * `(user_id, trains_id)` as of this feature, but that in-function fix
 * cannot close a genuinely concurrent double-submit under READ COMMITTED.
 * Disabling the control while its request is in flight is what does.
 *
 * Shared-groups follow-up: `useGroupSummaries()` decides, once on mount,
 * whether this user is a member of any group at all. Zero groups (the
 * majority case today, and every anonymous visitor) leaves `track()` wired
 * directly to the button's `onClick`, exactly as before this feature
 * existed -- no prompt, no behavior change, no extra render state. One or
 * more groups instead routes the click through `TrackDestinationModal`
 * (see its own doc comment for why it's safe to reuse this same `track()`
 * function unmodified for the confirm step); either way, `track()` itself
 * is the single place that actually calls `POST /Journeys`, so the
 * zero-groups code path an existing test locks down is untouched. Once
 * tracking succeeds, sharing into the chosen group
 * (`shareTrackedTrainToGroup`) is a second best-effort follow-up, same
 * swallow-every-failure posture as the `attachTicketId` block right below
 * it -- see that helper's own doc comment. */
export function TrackThisTrainButton({
  uid,
  date,
  attachTicketId,
  size = 'sm',
}: {
  uid: string;
  date: string;
  attachTicketId?: number;
  size?: 'xs' | 'sm' | 'md';
}) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();
  const { groups } = useGroupSummaries();
  const [destinationPromptOpened, setDestinationPromptOpened] = useState(false);

  async function track(groupId: string | null) {
    setBusy(true);
    needsLoginState.reset();
    setError(null);
    try {
      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          leg: { mode: 'knownTrain', trainUid: uid, serviceDate: date },
        }),
      });

      if (response.ok) {
        const result: CreateJourneyResponse = await response.json();
        if (attachTicketId !== undefined) {
          // Best-effort, exactly as before -- tracking has already
          // succeeded above. Still keyed on `trackingId`, not the new
          // `journeyId`: ticket attachment is scoped to the underlying
          // `train_subscriptions` row, unchanged by this feature.
          try {
            await fetch(`/api/Train/tickets/${attachTicketId}/attach`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ trackingId: result.trackingId }),
            });
          } catch {
            // Deliberately swallowed.
          }
        }
        if (groupId !== null && result.trackingId !== null) {
          // Best-effort, same posture as the ticket-attach block above.
          // `result.trackingId` is always non-null here -- `knownTrain`
          // mode always binds a train immediately -- but the check keeps
          // this call site honest against `CreateJourneyResponse`'s wider
          // (window-mode-inclusive) type. Still shares into the group via
          // the EXISTING group_trains table, unchanged -- journey-level
          // sharing is Phase 4 (see this plan's own Non-goals).
          await shareTrackedTrainToGroup(groupId, result.trackingId);
        }
        router.push(`/journeys/${result.journeyId}`);
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      setError("Couldn't track this train. Try again.");
    } catch {
      setError("Couldn't track this train. Try again.");
    } finally {
      setBusy(false);
    }
  }

  function handleClick() {
    if (groups.length > 0) {
      setDestinationPromptOpened(true);
      return;
    }
    void track(null);
  }

  return (
    <Stack gap="xs">
      <Button size={size} onClick={handleClick} disabled={busy}>
        {busy ? 'Tracking…' : 'Track this train'}
      </Button>
      {error && (
        <Alert color="red" title="Couldn't track this train">
          {error}
        </Alert>
      )}
      <TrackDestinationModal
        opened={destinationPromptOpened}
        groups={groups}
        onClose={() => setDestinationPromptOpened(false)}
        onConfirm={(groupId) => void track(groupId)}
      />
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this train.
      </LoginPromptModal>
    </Stack>
  );
}
