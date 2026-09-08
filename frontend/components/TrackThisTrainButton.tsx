'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Stack } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';

/** The "Track this train" action for a train whose real CIF identity is
 * already known -- a `(train_uid, service_date)` pair. Calls
 * `POST /Train/by-uid/{uid}/{date}/track`
 * (`crates/api/src/routes/train.rs`'s `post_track_by_uid`, the NR-primary
 * tracking entry point), which takes NO request body at all: identity is
 * entirely in the path, so there is no form to fill in and nothing to
 * validate client-side. That is the whole difference from
 * `TrackTrainForm`'s legacy `POST /Train/track` flow, which has to guess an
 * identity from a CRS + time pin.
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
 * Disabling the control while its request is in flight is what does. */
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

  async function track() {
    setBusy(true);
    needsLoginState.reset();
    setError(null);
    try {
      const response = await fetch(
        `/api/Train/by-uid/${encodeURIComponent(uid)}/${encodeURIComponent(date)}/track`,
        { method: 'POST' },
      );

      if (response.ok) {
        const result: { trackingId: number } = await response.json();
        if (attachTicketId !== undefined) {
          // Best-effort, exactly as TrackTrainForm does it -- see this
          // component's own doc comment.
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
        router.push(`/train/by-id/${result.trackingId}`);
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

  return (
    <Stack gap="xs">
      <Button size={size} onClick={track} disabled={busy}>
        {busy ? 'Tracking…' : 'Track this train'}
      </Button>
      {error && (
        <Alert color="red" title="Couldn't track this train">
          {error}
        </Alert>
      )}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this train.
      </LoginPromptModal>
    </Stack>
  );
}
