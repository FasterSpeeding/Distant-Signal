import { Stack, Title, Text, Group } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getPublicTrainByUidAndDate, getMyTrackedTrains, ApiNotFoundError } from '@/lib/api';
import { ShareButton } from '@/components/ShareButton';
import { TrainJourney } from '@/components/TrainJourney';
import { TrackThisTrainButton } from '@/components/TrackThisTrainButton';
import { TrackedTrainOwnerControls } from '@/components/TrackedTrainOwnerControls';
import { TicketPanel } from '@/components/TicketPanel';
import { TextLink } from '@/components/TextLink';
import type { PublicTrainState, TrainJourneyState, TrackedTrainListItem } from '@/lib/types';

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/** Adapts the PUBLIC, shared-train response into the subset
 * `TrainJourney` renders.
 *
 * Every field here comes from the shared `trains`/`train_current_state`
 * rows. Nothing per-subscriber exists to map: `customName` is always
 * `null` from this function alone (a custom name belongs to one
 * subscriber, and this function only ever sees the shared, subscriber-less
 * response) -- callers that DO know the current visitor's own subscription
 * (this page's own tracking-overlay branch, once it has a `GET /Train/mine`
 * match) overlay their own `customName` back on top of this result rather
 * than calling this function differently. There is deliberately no `id` in
 * the result type at all -- `PublicTrainState.trainsId` is a `trains.id`, NOT a
 * `train_subscriptions.id`, and this page previously fed it straight into
 * `RenameTrainButton`/`DeleteTrainButton`/`TicketPanel` as a `trackingId`.
 * Both are `BIGSERIAL` starting at 1, so a logged-in visitor could rename
 * or delete an unrelated subscription of their own that happened to share
 * the number.
 *
 * `resolutionStatus` is DERIVED, because the shared row has no such column
 * -- `resolution_status` is per-subscription state. `trains.train_id`
 * (TRUST's own daily identifier) is only ever written by a live-TRUST or
 * backlog resolution, and `origin_crs` only by a schedule match, so the
 * two together give the same three-way distinction `TrainJourney` renders.
 * `'unresolved'` is deliberately unreachable here: that status records a
 * subscriber's pin having been given up on, which is meaningless for a
 * shared train. */
export function toJourneyState(train: PublicTrainState): TrainJourneyState {
  return {
    serviceDate: train.serviceDate,
    pinOriginCrs: train.originCrs,
    pinOriginName: train.originName,
    pinDestinationCrs: train.destinationCrs,
    pinDestinationName: train.destinationName,
    pinScheduledDeparture: train.scheduledDeparture,
    resolutionStatus: train.trainId ? 'resolved' : train.originCrs ? 'schedule_matched' : 'pending',
    trainUid: train.trainUid,
    trainId: train.trainId,
    scheduleDestinationCrs: train.destinationCrs,
    scheduleDestinationName: train.destinationName,
    scheduleCallingPoints: train.callingPoints,
    status: train.status,
    lastReportedLocation: train.lastReportedLocation,
    lastEventType: train.lastEventType,
    delayMinutes: train.delayMinutes,
    nextCallingPoint: train.nextCallingPoint,
    etaNext: train.etaNext,
    etaSource: train.etaSource,
    customName: null,
    journeyStops: train.journeyStops,
  };
}

/** `/train/[uid]/[date]` -- the PUBLIC page for a real-world train,
 * readable by anyone (the backing route dropped its `AuthenticatedUser`
 * extractor and its ownership check as part of the shared-train-identity
 * change).
 *
 * `PublicTrainState` itself carries no owner actions: Rename/Delete/tickets
 * all operate on a `train_subscriptions.id`, which that response doesn't
 * carry. This page now closes that gap itself rather than leaving it to
 * `/train/by-id/[trackingId]`: alongside the public fetch, it also asks
 * `getMyTrackedTrains()` whether the current visitor already tracks this
 * exact `(trainUid, serviceDate)` pair, and if so overlays the same owner
 * controls the by-id page renders (`TrackedTrainOwnerControls`/
 * `TicketPanel`) in place of the generic `TrackThisTrainButton`. An
 * anonymous visitor, or a logged-in one tracking some other train, sees
 * exactly today's read-only view -- see the "tracking overlay" describe
 * block in this file's test for the full case split.
 *
 * Deliberately does NOT also fetch `getTrackedTrainById(match.id)` for a
 * `TrackedTrainState` once a match is found -- an earlier version of this
 * page did, purely to get fields already present on `match` itself
 * (`TrackedTrainListItem`, from `getMyTrackedTrains()`): `id`, `customName`,
 * and the `pin*`/`serviceDate` fields `TrackedTrainOwnerControls` needs.
 * Dropping that redundant fetch also removes the race window it opened (the
 * subscription existing when the list was read but being deleted before a
 * follow-up detail fetch landed) -- there is no second fetch left to race
 * on. The one field `TrackedTrainListItem` genuinely lacks that
 * `TrainJourney` wants -- live movement data (`trainId`,
 * `lastReportedLocation`, `scheduleCallingPoints`, `journeyStops`, ...) --
 * is already fully covered by `train`, the public fetch this page makes
 * unconditionally anyway; see `journeyState` below.
 *
 * There is likewise no `401` branch: the route is public, so
 * `ApiUnauthorizedError` is not a reachable outcome from
 * `getPublicTrainByUidAndDate` itself. The branch that used to be here (a
 * "log in to view this tracked train" prompt) was dead code after that
 * change. */
export default async function TrackedTrainByUidPage({
  params,
}: {
  params: Promise<{ uid: string; date: string }>;
}) {
  const { uid, date } = await params;

  // Validated before the fetch fires, per the same "malformed URL segment
  // 404s directly" rule as the by-id page (Task 8) --
  // docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md's
  // Error handling section.
  if (!DATE_PATTERN.test(date)) {
    notFound();
  }

  let train: PublicTrainState;
  let myTrackedTrains: TrackedTrainListItem[] | null;
  try {
    // Concurrent, not sequential: `getMyTrackedTrains()` has no data
    // dependency on the public fetch, and every visitor pays for both
    // (not just the rare tracking owner) -- running them one after another
    // would add serial latency to this page for everyone.
    //
    // `getMyTrackedTrains().catch(() => null)`: this is an auxiliary "am I
    // tracking this?" check, not this page's primary content -- same
    // fail-closed posture `app/page.tsx` already takes for its own
    // `getMyTrackedTrains()` call. A transient failure of it (network
    // blip, a non-401 backend error) must degrade to the plain public
    // view for every visitor, not take down this whole page.
    //
    // Also worth noting: there is no lighter, single-item "do I track
    // (uid, date)?" endpoint today, so this fetches the visitor's ENTIRE
    // tracked list just to test membership of one pair -- accepted for
    // now (a dedicated endpoint would be a backend change, out of scope
    // for this pass); revisit if this membership check becomes a real
    // cost.
    [train, myTrackedTrains] = await Promise.all([
      getPublicTrainByUidAndDate(uid, date),
      getMyTrackedTrains().catch(() => null),
    ]);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  // `myTrackedTrains` is `null` for an anonymous visitor, or when the
  // fetch above failed and was caught -- `myTrackedTrains?.find(...)`
  // below covers "not logged in", "logged in, nothing matches", and "the
  // check itself failed" identically; all three fall through to the plain
  // public render below.
  const match =
    myTrackedTrains?.find((item) => item.trainUid === uid && item.serviceDate === date) ?? null;

  // `toJourneyState(train)` alone always has `customName: null` (see its
  // own doc comment) -- overlay the visitor's own custom name from `match`
  // once they're the owner. Deliberately not `match` on its own as the
  // whole journey state: `TrackedTrainListItem` has no live-movement
  // fields (`trainId`, `lastReportedLocation`, `scheduleCallingPoints`,
  // `journeyStops`, ...) that `TrainJourney` needs, while `train` (the
  // public fetch above) already has them in full.
  const journeyState: TrainJourneyState = match
    ? { ...toJourneyState(train), customName: match.customName }
    : toJourneyState(train);

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Train {uid}</Title>
        <Group gap="sm">
          {match ? (
            <TrackedTrainOwnerControls train={match} />
          ) : (
            // Shown to EVERY visitor, logged in or not -- the shared
            // "show the control to everyone, prompt on the real 401"
            // posture `PinToggle`/`TrackTrainForm` already establish via
            // useNeedsLogin/LoginPromptModal. No `attachTicketId`: this page
            // has no `ticketId` query-param convention and inventing one is
            // explicitly out of scope
            // (docs/superpowers/specs/2026-09-07-train-listing-page-design.md
            // §5/§6). Only /trains' own row action attaches tickets.
            <TrackThisTrainButton uid={uid} date={date} />
          )}
          {/* Independent of ownership -- shown regardless of which branch
              above rendered. */}
          <ShareButton />
        </Group>
      </Group>
      <TrainJourney state={journeyState} />
      {match && <TicketPanel trackingId={match.id} />}
      {/* `component="div"`, not the default `<p>`: `TextLink` renders its
          own Mantine `<Text>` (a `<p>` by default), so wrapping it in an
          ordinary `<Text>` here would nest a `<p>` inside a `<p>` --
          invalid HTML and a React hydration warning. Same fix, same
          reasoning, as `TrainSearchForm.tsx`'s manual-fallback line. */}
      <Text size="sm" c="dimmed" component="div">
        {match ? (
          <>
            You&apos;re already tracking this service.{' '}
            <TextLink href="/trains">Find a train</TextLink> going somewhere else.
          </>
        ) : (
          <>
            This is the public view of this service. Track it above to get updates, or{' '}
            <TextLink href="/trains">Find a train</TextLink> going somewhere else.
          </>
        )}
      </Text>
    </Stack>
  );
}
