import { formatDate } from './dateFormat';
import { addCalendarDays } from './londonWallClock';
import type { TripPlanQuery } from './tripPlan';

/** How the planner's time is read: the earliest departure, or the latest
 * arrival (`GET /Trips/plan`'s `departAfter` / `arriveBy`). */
export type TimingMode = 'departAfter' | 'arriveBy';

/** An arrive-by time before this (`HH:MM`) is searched on the PREVIOUS
 * day's trains with `dayOffset=1`: "arrive by 00:30 on Saturday" is
 * Friday's late trains, which Saturday's timetable doesn't have. After the
 * last regular late-evening arrivals, before the first morning trains. See
 * docs/superpowers/specs/2026-10-19-trips-plan-arrive-by-design.md §4.2. */
export const OVERNIGHT_CUTOFF = '03:00';

const MINUTES_PER_DAY = 1440;

export function timingMode(query: Pick<TripPlanQuery, 'arriveBy'>): TimingMode {
  return query.arriveBy ? 'arriveBy' : 'departAfter';
}

/** The `GET /Trips/plan` request for a search as the visitor sees it
 * (calendar date and time). A time before [`OVERNIGHT_CUTOFF`], in either
 * mode, moves to the previous service date with `dayOffset: 1` and
 * `acrossMidnight: true`: the API then searches both dates' trains, the
 * previous evening's late ones and this morning's first ones, and merges
 * them (2026-10-20). Everything else is sent as is. */
export function apiQuery(query: TripPlanQuery): TripPlanQuery {
  const { dayOffset: _offset, acrossMidnight: _across, ...rest } = query;
  const time = query.arriveBy ?? query.departAfter;
  if (time && time < OVERNIGHT_CUTOFF) {
    return { ...rest, date: addCalendarDays(query.date, -1), dayOffset: 1, acrossMidnight: true };
  }
  return rest;
}

function clockMinutes(time: string): number {
  const [hours = 0, minutes = 0] = time.split(':').map(Number);
  return hours * 60 + minutes;
}

/** Minutes from the service date's midnight, as the API's
 * `{time, dayOffset}` gives them. */
export function serviceMinutes(at: { time: string; dayOffset: number }): number {
  return at.dayOffset * MINUTES_PER_DAY + clockMinutes(at.time);
}

/** `minutes` from `serviceDate`'s midnight (may be negative or pass a day)
 * as a calendar date and `HH:MM`, the way the form shows them. */
export function calendarAt(serviceDate: string, minutes: number): { date: string; time: string } {
  const days = Math.floor(minutes / MINUTES_PER_DAY);
  const clock = minutes - days * MINUTES_PER_DAY;
  const hh = String(Math.floor(clock / 60)).padStart(2, '0');
  const mm = String(clock % 60).padStart(2, '0');
  return { date: addCalendarDays(serviceDate, days), time: `${hh}:${mm}` };
}

/** `query` with its timing replaced: the other time field dropped. */
export function withTiming(query: TripPlanQuery, mode: TimingMode, at: { date: string; time: string }): TripPlanQuery {
  const { departAfter: _depart, arriveBy: _arrive, dayOffset: _offset, acrossMidnight: _across, ...rest } = query;
  return { ...rest, date: at.date, [mode]: at.time };
}

/** What was searched for, above the results: "Arrive by 09:00, 23 Oct
 * 2026" or "Depart at 08:30, 23 Oct 2026". The visitor's calendar date,
 * not an overnight search's service date. */
export function timingSummary(query: Pick<TripPlanQuery, 'date' | 'departAfter' | 'arriveBy'>): string {
  const date = formatDate(`${query.date}T12:00:00Z`);
  if (query.arriveBy) return `Arrive by ${query.arriveBy}, ${date}`;
  return `Depart at ${query.departAfter || '00:00'}, ${date}`;
}
