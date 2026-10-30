import { cancellationHeadline, cancellationReasonLine, trainCancellation } from './cancellation';
import { formatTime, formatWeekdayShortDate, londonDayKey } from './dateFormat';
import { SNAPSHOT_NOTE } from './linePreview';
import {
  livePreviewImage,
  previewDigest,
  previewWhen,
  type PreviewBandTone,
  type StatusPreviewCard,
} from './livePreview';
import type { PreviewImage } from './pageMetadata';
import { delayLabel } from './serviceStatus';
import { isTimetableOnly, serviceModeLabel } from './serviceMode';
import { trainName } from './trainName';
import type { JourneyStop, PublicTrainState } from './types';

/** A train's live link preview (`/train/[uid]/[date]/card.png`;
 * `lib/livePreview.ts` for what live cards share). Pure: the public train
 * the page reads (`GET /Train/by-uid/{uid}/{date}`, shared rows only:
 * nothing about anyone who tracks it) and `now`.
 *
 * - The band is the train's state in words: "On time", "12 min late",
 *   "Cancelled", "Terminates at Raynes Park", "Arrived 3 min late",
 *   "Planned". Delays take `DelayBadge`'s colours, a cancellation red.
 * - The title is the page's own ("08:42 Woking to London Waterloo").
 * - The lead is where the train is ("Departed Surbiton at 08:58"), why it
 *   was cancelled, or, before it runs, its timetable from end to end.
 * - Then the next stop or the origin's platform, the operator, headcode
 *   and date, and the snapshot note while it is live.
 *
 * A train is live only on its day, while it can still move. A future train,
 * or one whose day is over, gets a static card with no "as of" time and no
 * snapshot note: its timetable, or how it ended (arrived, cancelled, no
 * arrival reported), which no longer changes. A bus or ferry is never
 * tracked live, so its card is always its timetable.
 *
 * No progress strip: a moving dot goes stale in minutes, and a chat app may
 * show the card for hours. The lead names a station and its time instead,
 * which stays true. */

/** The fields the card reads. */
export type TrainPreviewInput = Pick<
  PublicTrainState,
  | 'trainUid'
  | 'serviceDate'
  | 'originCrs'
  | 'originName'
  | 'destinationCrs'
  | 'destinationName'
  | 'scheduledDeparture'
  | 'headcode'
  | 'identity'
  | 'status'
  | 'lastReportedLocation'
  | 'lastEventType'
  | 'delayMinutes'
  | 'delayProvisional'
  | 'journeyStops'
  | 'mayHaveArrived'
  | 'operatorCode'
  | 'operatorName'
  | 'cancelled'
  | 'cancelReasonCode'
  | 'cancelReason'
  | 'cancellation'
  | 'serviceMode'
  | 'liveTracking'
  | 'trackingPhase'
>;

/** How long after its timetabled arrival a train from an earlier day may
 * still be reported (one that left before midnight and runs late), before
 * its card stops being live. */
const OVERNIGHT_GRACE_MS = 3 * 60 * 60 * 1000;

/** "Fri 30 Oct" for a service date ("2026-10-30"), read at midday so no
 * zone can move it to another day. */
export function serviceDateLabel(serviceDate: string): string {
  return formatWeekdayShortDate(`${serviceDate}T12:00:00Z`);
}

/** The page's own title: "08:42 Woking to London Waterloo", else "Train
 * W12345" (or "Rail replacement bus W12345"). */
export function trainPreviewTitle(train: TrainPreviewInput): string {
  return (
    trainName({
      departure: train.scheduledDeparture,
      origin: train.originName ?? train.originCrs,
      destination: train.destinationName ?? train.destinationCrs,
    }) ?? `${serviceModeLabel(train.serviceMode) ?? 'Train'} ${train.trainUid}`
  );
}

function stopName(stop: Pick<JourneyStop, 'name' | 'crs' | 'tiploc'>): string | null {
  return stop.name ?? stop.crs ?? stop.tiploc;
}

/** "Woking 08:42 to London Waterloo 09:13": the timetable, end to end, as
 * far as it is known. Null without both ends. */
export function trainScheduleLine(train: TrainPreviewInput): string | null {
  const stops = train.journeyStops ?? [];
  const first = stops[0];
  const last = stops.length > 1 ? stops[stops.length - 1] : undefined;
  const origin = train.originName ?? train.originCrs ?? (first ? stopName(first) : null);
  const destination = train.destinationName ?? train.destinationCrs ?? (last ? stopName(last) : null);
  if (!origin || !destination) return null;
  const departs = train.scheduledDeparture ?? first?.scheduledDeparture ?? null;
  const arrives = last?.scheduledArrival ?? null;
  const end = (name: string, at: string | null) => (at ? `${name} ${formatTime(at)}` : name);
  return `${end(origin, departs)} to ${end(destination, arrives)}`;
}

/** The last stop a report confirms the train reached (the same rule as
 * `lib/trainPosition.ts`'s `lastReachedIndex`): an actual time, or a pass.
 * -1 before any. */
function lastReachedIndex(stops: readonly JourneyStop[]): number {
  for (let i = stops.length - 1; i >= 0; i--) {
    const stop = stops[i];
    if (!stop) continue;
    if (stop.actualArrival !== null || stop.actualDeparture !== null || stop.lastEventType === 'PASS') return i;
  }
  return -1;
}

/** "Departed Surbiton at 08:58", "Arrived at Surbiton at 08:56", "Passed
 * Surbiton", else the reported location's own words, or null. */
function lastReportedLine(train: TrainPreviewInput): string | null {
  const stops = train.journeyStops ?? [];
  const reached = stops[lastReachedIndex(stops)];
  const name = reached ? stopName(reached) : null;
  if (reached && name) {
    if (reached.actualDeparture) return `Departed ${name} at ${formatTime(reached.actualDeparture)}`;
    if (reached.actualArrival) return `Arrived at ${name} at ${formatTime(reached.actualArrival)}`;
    return `Passed ${name}`;
  }
  if (train.lastReportedLocation) {
    // A code from the movement feed: named when it is one of the stops.
    const code = train.lastReportedLocation;
    const stop = stops.find((s) => s.crs === code || s.tiploc === code);
    return `Last reported at ${(stop && stopName(stop)) ?? code}`;
  }
  return null;
}

/** "Next stop: Wimbledon, expected 09:04" (or "due", from the timetable):
 * the first call after the last report that the train still makes. */
function nextStopLine(train: TrainPreviewInput): string | null {
  const stops = train.journeyStops ?? [];
  const after = lastReachedIndex(stops);
  const next = stops
    .slice(after + 1)
    .find((stop) => stop.kind !== 'Origin' && stop.stopStatus !== 'Skipped' && stop.status !== 'Cancelled');
  const name = next ? stopName(next) : null;
  if (!next || !name) return null;
  if (next.estimatedArrival) return `Next stop: ${name}, expected ${formatTime(next.estimatedArrival)}`;
  if (next.scheduledArrival) return `Next stop: ${name}, due ${formatTime(next.scheduledArrival)}`;
  return `Next stop: ${name}`;
}

/** "Platform 4 at Woking", "Booked platform 4 at Woking" while only the
 * timetable's is known, or null. */
function originPlatformLine(train: TrainPreviewInput): string | null {
  const origin = train.journeyStops?.[0];
  if (origin?.kind !== 'Origin') return null;
  const name = stopName(origin);
  const live = origin.platformStatus === 'cancelled' ? null : origin.platform;
  const booked = origin.bookedPlatform ?? null;
  if (!name) return null;
  if (live && origin.platformBasis !== 'booked') return `Platform ${live} at ${name}`;
  const platform = origin.displayPlatform ?? booked ?? live;
  return platform ? `Booked platform ${platform} at ${name}` : null;
}

/** "South Western Railway · Headcode 1A23", plus the date when given. */
function identityLine(train: TrainPreviewInput, date: string | null): string | null {
  const headcode = train.identity ?? train.headcode ?? null;
  const parts = [
    train.operatorName ?? train.operatorCode ?? null,
    headcode ? `Headcode ${headcode}` : null,
    date,
  ].filter((part): part is string => part !== null && part !== '');
  return parts.length > 0 ? parts.join(' · ') : null;
}

/** Whether the train may still change: its day, or an earlier day's train
 * still within reach of its timetabled arrival. A future day is never live. */
export function trainIsLive(train: TrainPreviewInput, now: number): boolean {
  const today = londonDayKey(new Date(now));
  if (train.serviceDate > today) return false;
  if (train.serviceDate === today) return true;
  const stops = train.journeyStops ?? [];
  const last = stops[stops.length - 1];
  const due = last?.scheduledArrival ?? last?.scheduledDeparture ?? null;
  const dueAt = due ? Date.parse(due) : Number.NaN;
  return !Number.isNaN(dueAt) && now <= dueAt + OVERNIGHT_GRACE_MS;
}

function rows(...lines: (string | null)[]): string[] {
  return lines.filter((line): line is string => line !== null && line !== '');
}

/** The band for a known delay: "12 min late" orange, "3 min early" teal,
 * "On time" green, with `prefix` ("Arrived ") before it. */
function delayBand(delayMinutes: number, provisional: boolean, prefix = ''): { tone: PreviewBandTone; label: string } {
  const delay = delayLabel(delayMinutes, provisional);
  const tone: PreviewBandTone = delay.tone === 'late' ? 'late' : delay.tone === 'early' ? 'early' : 'good';
  const words = prefix === '' ? delay.text : `${prefix}${delay.text.charAt(0).toLowerCase()}${delay.text.slice(1)}`;
  return { tone, label: words };
}

/** The card. `now` decides whether the train is live (and its "as of"
 * time); nothing else reads the clock. */
export function trainPreviewCard(train: TrainPreviewInput, now: number): StatusPreviewCard {
  const title = trainPreviewTitle(train);
  const date = serviceDateLabel(train.serviceDate);
  const schedule = trainScheduleLine(train);
  const live = !isTimetableOnly(train) && trainIsLive(train, now);
  // A live card says when it is from and that it is a snapshot; a static
  // one names the service date in the band instead.
  const aside = live ? `As of ${previewWhen(new Date(now).toISOString())}` : date;
  const identity = identityLine(train, live ? date : null);
  const snapshot = live ? SNAPSHOT_NOTE : null;
  const card = (band: { tone: PreviewBandTone; label: string }, lead: string | null, facts: string[]) => ({
    band: { ...band, aside },
    title,
    lead,
    chips: { names: [], more: 0 },
    rows: facts,
  });

  if (isTimetableOnly(train)) {
    return card(
      { tone: 'informational', label: 'Timetable only' },
      schedule,
      rows(originPlatformLine(train), identity),
    );
  }
  if (train.trackingPhase === 'noTimetable' && train.status === null) {
    return card({ tone: 'informational', label: 'Not in the timetable' }, null, rows(identity, snapshot));
  }

  const cancellation = trainCancellation(train);
  if (cancellation && (train.status === 'cancelled' || cancellation.kind === 'partial')) {
    const reason = cancellationReasonLine(cancellation.reason);
    return card(
      { tone: 'severe', label: cancellationHeadline(cancellation) },
      reason ? `Reason: ${reason}` : schedule,
      rows(reason ? schedule : null, identity, snapshot),
    );
  }

  if (train.status === 'completed') {
    const stops = train.journeyStops ?? [];
    const terminus = stops[stops.length - 1];
    const name = terminus ? stopName(terminus) : (train.destinationName ?? train.destinationCrs);
    const at = terminus?.actualArrival ?? null;
    const lead = name ? `Arrived at ${name}${at ? ` at ${formatTime(at)}` : ''}` : null;
    const band =
      train.delayMinutes === null
        ? { tone: 'good' as const, label: 'Arrived' }
        : delayBand(train.delayMinutes, false, 'Arrived ');
    return card(band, lead, rows(lead === null ? schedule : null, identity, snapshot));
  }

  const started = train.status === 'en_route';
  if (!live) {
    // A future day, or a day that is over without an arrival report.
    if (!started && train.serviceDate > londonDayKey(new Date(now))) {
      return card({ tone: 'informational', label: 'Planned' }, schedule, rows(originPlatformLine(train), identity));
    }
    return started
      ? card(
          { tone: 'informational', label: 'No arrival reported' },
          lastReportedLine(train) ?? schedule,
          rows(identity),
        )
      : card({ tone: 'informational', label: 'No report received' }, schedule, rows(identity));
  }

  if (!started) {
    const label = train.trackingPhase === 'planned' ? 'Planned' : 'Awaiting first report';
    return card({ tone: 'informational', label }, schedule, rows(originPlatformLine(train), identity, snapshot));
  }

  if (train.mayHaveArrived) {
    return card(
      { tone: 'informational', label: 'May have arrived' },
      lastReportedLine(train) ?? schedule,
      rows(identity, snapshot),
    );
  }

  const band =
    train.delayMinutes === null
      ? { tone: 'informational' as const, label: 'Running' }
      : delayBand(train.delayMinutes, train.delayProvisional ?? false);
  return card(band, lastReportedLine(train) ?? schedule, rows(nextStopLine(train), identity, snapshot));
}

/** A digest of what the card says for `?v=`: everything but its "as of"
 * time, so the URL changes when the train's state does (its delay, where
 * it is, a cancellation, going from planned to live to over) and not every
 * minute for a train standing still. `null` (the train couldn't be read)
 * still gives a URL, which changes once it can. */
export function trainPreviewVersion(uid: string, date: string, card: StatusPreviewCard | null): string {
  if (card === null) return previewDigest([uid, date, null]);
  return previewDigest([card.band.tone, card.band.label, card.title, card.lead, card.rows]);
}

/** `/train/{uid}/{date}/card.png?v=...`. */
export function trainPreviewImage(
  uid: string,
  date: string,
  card: StatusPreviewCard | null,
  alt: string,
): PreviewImage {
  return livePreviewImage(
    `/train/${encodeURIComponent(uid)}/${encodeURIComponent(date)}/card.png`,
    trainPreviewVersion(uid, date, card),
    alt,
  );
}
