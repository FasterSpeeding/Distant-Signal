import { formatDate, formatTime } from './dateFormat';
import { routeLabel, stationLabel } from './stationLabel';

/** The route fields a tracked train's label reads. `trainUid` is optional
 * so a caller with only a pin shape can still pass it. */
export interface TrackedTrainRouteInput {
  pinOriginCrs: string | null;
  pinOriginName: string | null;
  pinDestinationCrs: string | null;
  pinDestinationName: string | null;
  trainUid?: string | null | undefined;
}

/** What a train is still waiting to be named by when it has no route at
 * all: no pin, and no timetable on its shared train row (the API falls
 * back to that row's route, DS-111). */
export const UNIDENTIFIED_TRAIN_LABEL = 'Train not identified yet';

/** A tracked train's route, `"A (AAA) to B (BBB)"` via `routeLabel`.
 *
 * With no origin there is no station to name, which is a normal state for
 * a train tracked by UID before its timetable arrives -- not an unknown
 * station. So this names the train itself ("Train W12345", "Train W12345
 * to York (YRK)") rather than `routeLabel`'s "Unknown station" (DS-111). */
export function trackedTrainRouteLabel(train: TrackedTrainRouteInput): string {
  if (train.pinOriginCrs) {
    return routeLabel(train.pinOriginCrs, train.pinOriginName, train.pinDestinationCrs, train.pinDestinationName);
  }
  const subject = train.trainUid ? `Train ${train.trainUid}` : UNIDENTIFIED_TRAIN_LABEL;
  return train.pinDestinationCrs
    ? `${subject} to ${stationLabel(train.pinDestinationCrs, train.pinDestinationName)}`
    : subject;
}

/** The label a tracked train's row/title should show: the user's own
 * `customName` if they set one, verbatim; otherwise the same
 * route + date(/time) default `TrackedTrainListRow`
 * (`app/journeys/page.tsx`) and `TrainJourney`'s `pinSummary` already
 * compute today -- this function is that computation, extracted so both
 * call sites (and this custom-names feature's own new one) share it rather
 * than each hand-rolling the same `routeLabel(...) + ' · ' + formatDate(...)`
 * shape independently.
 *
 * `pinScheduledDeparture` is optional because `TrackedTrainState`
 * (`lib/types.ts`) has no such field at all -- the backend's own read
 * query for a single tracked train's detail page never selects
 * `pin_scheduled_departure`, only `serviceDate`. When it's absent, this
 * degrades to a date-only default, exactly as `TrainJourney.tsx`'s
 * existing `pinSummary` already does -- not a new gap this feature
 * introduces.
 *
 * Never persisted -- always computed fresh from fields already on the
 * caller's own wire object, at render time. See
 * docs/superpowers/specs/2026-09-05-custom-tracking-names-design.md's
 * Decision 3 for why a stored-at-creation default would go stale the
 * moment a pre-match pin's destination or a slow-to-load station name
 * resolves later. */
export function trackedTrainDisplayName(
  train: TrackedTrainRouteInput & {
    customName: string | null;
    serviceDate: string;
    pinScheduledDeparture?: string | null | undefined;
  },
): string {
  if (train.customName) return train.customName;

  const route = trackedTrainRouteLabel(train);
  const when = train.pinScheduledDeparture
    ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
    : formatDate(train.serviceDate);
  return `${route}, ${when}`;
}
