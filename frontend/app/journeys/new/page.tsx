import { Stack, Title, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { JourneyCreationFlow } from '@/components/JourneyCreationFlow';

/** `/journeys/new` -- the app's primary, nav-linked entry point for
 * tracking something (`lib/navLinks.ts`'s `TRACK_JOURNEY_DESTINATION`).
 * Journey tracking is the app's main feature going forward; tracking one
 * train is the simple, one-leg case of a journey, not a separate concept
 * -- this page is where that gets built, whether the visitor stops after
 * one leg or keeps going to add a change of trains.
 *
 * DESIGN DECISION (the question this page exists to answer: does the whole
 * journey get composed client-side and submitted in one shot, or does leg 1
 * get created immediately and further legs get chained on one at a time?):
 *
 * This page creates leg 1 immediately, via the EXISTING `POST /Journeys`
 * (unchanged -- see `TrackTrainForm.tsx`'s `submitTrack`/`submitWindow`),
 * and then lets the visitor chain on leg 2, 3, ... one at a time via the
 * EXISTING `POST /Journeys/{id}/legs` (unchanged -- see
 * `AddJourneyLegButton.tsx`), all without ever leaving this page. It does
 * NOT compose a whole multi-leg draft client-side first and submit it
 * atomically in one new call.
 *
 * Why: this is the pragmatic choice, not a compromise --
 *
 * 1. Zero new backend surface. Both routes above already exist, are
 *    already tested, and are already exactly what the pre-existing
 *    "create at /track, then add legs one at a time from the journey
 *    page" flow used -- this page is a UI-only change that keeps the user
 *    on ONE page throughout, rather than a new create-a-whole-journey
 *    endpoint with its own request/response shape and validation rules to
 *    design and land.
 * 2. An atomic "submit N legs at once" endpoint has a real failure-mode
 *    question an incremental one never faces: what happens when leg 2 of
 *    3 fails to create (a bad time window, a train that no longer
 *    exists)? Roll back the whole journey the user already spent time
 *    building? Partially commit and report which legs failed? Both are
 *    real design/validation complexity for comparatively little user
 *    benefit here.
 * 3. A journey with only its first leg committed is not a degraded or
 *    half-finished state that needs hiding -- it is already a complete,
 *    useful, first-class journey (every other part of this app already
 *    treats a 1-leg journey this way). So there is no correctness reason
 *    to withhold it from the database until every leg the user MIGHT add
 *    is known; committing leg 1 the moment it exists is simply honest.
 * 4. It reuses `TrackTrainForm` and `AddJourneyLegButton` verbatim (both
 *    gained one small optional prop -- `onCreated`/`onAdded` -- that lets a
 *    caller take over what happens after a successful submit instead of
 *    the pre-existing unconditional redirect/refresh; every other caller
 *    of either component is completely unaffected). Single-train tracking
 *    -- the common case -- is untouched: filling in leg 1 and stopping
 *    there is exactly as fast as `/track` always was, with one visible
 *    difference (an inline "Add a leg" appears once the journey exists,
 *    which a single-leg tracker is free to ignore and click Done).
 *
 * See `JourneyCreationFlow.tsx`'s own doc comment for exactly how the two
 * states (before/after leg 1 exists) are rendered.
 *
 * Anonymous/logged-out handling matches `/track` exactly, for the same
 * reason: `TrackTrainForm`'s "Track this train"/"Search for a train"
 * button is shown to everyone regardless of session (the Tier-2 "show the
 * control, gate on the real 401" pattern `useNeedsLogin.ts` documents),
 * with the same account hint below telling a visitor up front what happens
 * if they aren't signed in yet. `AddJourneyLegButton` (offered once the
 * journey exists) already has its own equivalent 401 handling too -- this
 * page adds no login-prompt logic of its own; both reused components
 * bring their own. */
const METADATA_TITLE = 'Track a Journey — Distant Signal';
const METADATA_DESCRIPTION =
  'Track a whole journey, start to finish — pin a specific train or search a time window for leg 1, then add another leg right here if your trip involves a change of trains. A single train is already a complete journey; stop whenever you like.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

export default function JourneysNewPage() {
  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Track a Journey</Title>
      <Text c="dimmed">
        Pin a specific train, or search a time window if you&apos;re not sure which one yet. Once it&apos;s tracked
        you can add another leg right here — for a journey with a change of trains — or stop now; a single train is
        already a complete journey.
      </Text>
      <Text size="sm" c="dimmed">
        Tracking a journey needs a Distant Signal account — you&apos;ll be sent to log in when you save if you
        aren&apos;t already signed in.
      </Text>
      <JourneyCreationFlow />
    </Stack>
  );
}
