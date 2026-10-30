import { resolvedStopLabel, type JourneyEndpointNames } from '@/components/JourneyTimeline';
import type { JourneyStop } from './types';

/** The last scheduled calling point with a confirmed reported event -- an
 * ARRIVAL, DEPARTURE, or PASS message TRUST has already sent. This is the
 * ONLY thing "you are here" is allowed to mean in this component.
 *
 * This deliberately walks `actualArrival`/`actualDeparture`/`lastEventType`
 * ONLY -- never `estimatedArrival`/`estimatedDeparture`, which are a forward
 * propagation of the train's current overall delay onto stops nothing has
 * confirmed yet, not a report of anything Network Rail said happened there.
 *
 * The `lastEventType === 'PASS'` arm exists because a PASS at a genuine
 * BOOKED calling point no longer populates `actualArrival`/`actualDeparture`
 * (`crates/api/src/data/journey.rs`'s `overlay_movement_events` -- a PASS
 * there means the train ran through without calling, so showing it as a
 * completed stop in `JourneyTimeline.tsx` was a real display bug). That fix
 * must not regress THIS component: a PASS is still real, reported evidence
 * that the train has physically gone past that point, so it must still
 * advance "you are here" exactly as an ARRIVAL/DEPARTURE would -- it just no
 * longer does so via the `actual*` fields, which now mean "the train called
 * here", a narrower fact than "the train has been confirmed past here".
 *
 * `docs/superpowers/specs/2026-08-28-train-tracking-design.md` already
 * investigated and explicitly rejected Train Describer (TD) / berth-level
 * physical position tracking as a non-goal for this app, reasoning that
 * TRUST's schedule-location events are the right granularity for "where is
 * this train relative to its stops." This function -- and the marker it
 * drives -- is a restatement of that decision, not a new instance of it: it
 * NEVER returns a value that implies the train is between two stops, NEVER
 * advances on a timer, and NEVER changes except when a new confirmed event
 * actually arrives on the next data refresh. A future edit must not
 * "smooth" or animate the marker between two confirmed indices -- there is
 * no data behind such a position, by design, and adding one would silently
 * reopen the TD/GPS non-goal in a different visual form. `-1` (no stop
 * confirmed yet) is a legitimate, common return value, not an error case. */
export function lastReachedIndex(stops: JourneyStop[]): number {
  for (let i = stops.length - 1; i >= 0; i--) {
    const stop = stops[i];
    if (stop && (stop.actualArrival !== null || stop.actualDeparture !== null || stop.lastEventType === 'PASS')) {
      return i;
    }
  }
  return -1;
}

/** The index of the calling point whose own station name/CRS is the one
 * `TrainJourneyState.lastReportedLocation` names -- `-1` when the reported
 * location matches no listed stop, or when there is no reported location
 * at all.
 *
 * This exists to close a self-contradiction, not to invent a position
 * (2026-09-22 UX review, C3). The summary block above this diagram prints
 * "Last reported: Doncaster (departure)" straight off
 * `lastReportedLocation`, while `lastReachedIndex` walks only the
 * per-stop `actualArrival`/`actualDeparture`/`lastEventType` overlay. When
 * a movement report reached `train_current_state` but the per-stop overlay
 * didn't pick it up -- a TIPLOC/CRS key that didn't join, a schedule
 * refresh that dropped the actuals, an ingest ordering gap -- the two
 * disagree, and the page said BOTH "Last reported: Doncaster" and
 * "Last reported at Doncaster -- that report couldn't be matched to a
 * timetabled stop" about a Doncaster sitting in the table below with a
 * real time.
 *
 * The `lastReachedIndex` doc comment's rule is untouched and still
 * governs: a marker may only ever sit ON a calling point, never between
 * two, never on a timer. This adds one more piece of CONFIRMED,
 * report-derived evidence for which calling point that is -- the same
 * report the summary line is already printing -- rather than a second,
 * weaker notion of position. `resolvedStopLabel` (not `journeyStopLabel`)
 * so a reported location can never match the by-index "Stop 3"
 * placeholder; `stop.crs` is compared too so a report that names a CRS
 * matches a row that resolved a full name, and vice versa. Scanned
 * backwards, matching `lastReachedIndex`'s own convention for a route that
 * calls at the same station twice. */
export function reportedLocationIndex(
  stops: JourneyStop[],
  lastReportedLocation: string | null,
  endpointNames: JourneyEndpointNames | undefined,
): number {
  const target = lastReportedLocation?.trim().toLowerCase();
  if (!target) return -1;
  for (let i = stops.length - 1; i >= 0; i--) {
    const stop = stops[i];
    if (!stop) continue;
    const label = resolvedStopLabel(stop, i, stops.length, endpointNames);
    if (label?.trim().toLowerCase() === target) return i;
    if (stop.crs?.trim().toLowerCase() === target) return i;
  }
  return -1;
}

/** Where the train is on `stops`: the "you are here" stop
 * `JourneyProgress` marks, and the hint in a journey leg's collapsed
 * calling points. Two sources for the ONE index, in strict precedence
 * order. The per-stop `actual*`/PASS overlay wins whenever it has
 * anything at all; only when it is empty does the summary block's own
 * `lastReportedLocation` get to name a listed stop instead (C3 -- see
 * `reportedLocationIndex`). `-1` means neither source confirmed a calling
 * point. `fromReport` is `true` when the index came from the report. */
export function trainPosition(
  stops: JourneyStop[],
  lastReportedLocation: string | null,
  endpointNames: JourneyEndpointNames | undefined,
): { index: number; fromReport: boolean } {
  const confirmed = lastReachedIndex(stops);
  if (confirmed !== -1) return { index: confirmed, fromReport: false };
  const reported = reportedLocationIndex(stops, lastReportedLocation, endpointNames);
  return { index: reported, fromReport: reported !== -1 };
}
