import { formatDate, formatTime } from './dateFormat';
import { routeLabel } from './stationLabel';

/** The label a tracked train's row/title should show: the user's own
 * `customName` if they set one, verbatim; otherwise the same
 * route + date(/time) default `TrackedTrainListRow`
 * (`app/track/mine/page.tsx`) and `TrainJourney`'s `pinSummary` already
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
export function trackedTrainDisplayName(train: {
  customName: string | null;
  pinOriginCrs: string;
  pinOriginName: string | null;
  pinDestinationCrs: string | null;
  pinDestinationName: string | null;
  serviceDate: string;
  pinScheduledDeparture?: string;
}): string {
  if (train.customName) return train.customName;

  const route = routeLabel(
    train.pinOriginCrs,
    train.pinOriginName,
    train.pinDestinationCrs,
    train.pinDestinationName,
  );
  const when = train.pinScheduledDeparture
    ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
    : formatDate(train.serviceDate);
  return `${route}, ${when}`;
}
