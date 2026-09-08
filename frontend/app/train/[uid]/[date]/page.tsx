import { Stack, Title, Text, Group } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getPublicTrainByUidAndDate, ApiNotFoundError } from '@/lib/api';
import { ShareButton } from '@/components/ShareButton';
import { TrainJourney } from '@/components/TrainJourney';
import { TrackThisTrainButton } from '@/components/TrackThisTrainButton';
import { TextLink } from '@/components/TextLink';
import type { PublicTrainState, TrainJourneyState } from '@/lib/types';

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/** Adapts the PUBLIC, shared-train response into the subset
 * `TrainJourney` renders.
 *
 * Every field here comes from the shared `trains`/`train_current_state`
 * rows. Nothing per-subscriber exists to map: `customName` is always
 * `null` (a custom name belongs to one subscriber, and this page has no
 * subscriber), and there is deliberately no `id` in the result type at
 * all -- `PublicTrainState.trainsId` is a `trains.id`, NOT a
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
function toJourneyState(train: PublicTrainState): TrainJourneyState {
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
  };
}

/** `/train/[uid]/[date]` -- the PUBLIC page for a real-world train,
 * readable by anyone (the backing route dropped its `AuthenticatedUser`
 * extractor and its ownership check as part of the shared-train-identity
 * change).
 *
 * It renders no owner actions. Rename/Delete/tickets all operate on a
 * `train_subscriptions.id`, which this response does not carry and which
 * an anonymous visitor would not have anyway; the backend exposes no
 * "and here's YOUR subscription for this train, if any" hint on this
 * route, so there is nothing to gate them on yet. A subscriber who wants
 * those actions has them on `/train/by-id/[trackingId]` and
 * `/track/mine`, both of which are keyed on a real tracking id.
 *
 * There is likewise no `401` branch: the route is public, so
 * `ApiUnauthorizedError` is not a reachable outcome. The branch that used
 * to be here (a "log in to view this tracked train" prompt) was dead code
 * after that change. */
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

  let train;
  try {
    train = await getPublicTrainByUidAndDate(uid, date);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Train {uid}</Title>
        <Group gap="sm">
          {/* Shown to EVERY visitor, logged in or not -- the shared
              "show the control to everyone, prompt on the real 401"
              posture `PinToggle`/`TrackTrainForm` already establish via
              useNeedsLogin/LoginPromptModal. No `attachTicketId`: this page
              has no `ticketId` query-param convention and inventing one is
              explicitly out of scope
              (docs/superpowers/specs/2026-09-07-train-listing-page-design.md
              §5/§6). Only /trains' own row action attaches tickets.

              A logged-in visitor who ALREADY tracks this train gets no
              special treatment, deliberately: `PublicTrainState` carries no
              "you already have a subscription" hint, by design (it is the
              shared, public train, with nothing per-subscriber on it).
              Clicking again is harmless -- `create_subscription_for_train`
              is idempotent per (user, train) and returns the existing
              subscription, so both clicks land on the same
              /train/by-id/{trackingId}. */}
          <TrackThisTrainButton uid={uid} date={date} />
          <ShareButton />
        </Group>
      </Group>
      <TrainJourney state={toJourneyState(train)} />
      <Text size="sm" c="dimmed">
        This is the public view of this service. Track it above to get updates, or{' '}
        <TextLink href="/trains">Find a train</TextLink> going somewhere else.
      </Text>
    </Stack>
  );
}
