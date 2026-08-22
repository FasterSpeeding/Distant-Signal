import type { LineStatus, ValidityPeriod } from './types';

/** Every issue lands in exactly one of these, so the tab counts add up. */
export type IssueBucket = 'active' | 'upcoming' | 'ended';

/** `isNow` does NOT mean "this period covers now".
 *
 * `crates/poller-incidents/src/schema.rs` builds it as
 * `is_now: vp.end_time.is_none()` — i.e. "open-ended". The aggregator's
 * `validity_for_output` then does pick the period covering now (via
 * `period_covers_now`) but copies that flag through untouched, so every
 * in-progress planned work with a known end date reaches the frontend as
 * `isNow: false`. Reading only the flag is what produced the incoherent
 * "All (3) / Active (0) / Upcoming (0)" on the line and station pages.
 *
 * So: `isNow` is treated as sufficient but not necessary, with the dates
 * as the real test. An unparseable `toDate` resolves to "still active"
 * rather than silently dropping the issue out of every bucket — the same
 * bias towards surfacing rather than hiding that the rest of this
 * component uses. */
export function periodIsActive(period: ValidityPeriod, now: number): boolean {
  if (period.isNow) return true;
  const from = Date.parse(period.fromDate);
  if (Number.isNaN(from) || from > now) return false;
  if (period.toDate === null) return true;
  const to = Date.parse(period.toDate);
  return Number.isNaN(to) || to >= now;
}

export function periodIsUpcoming(period: ValidityPeriod, now: number): boolean {
  if (periodIsActive(period, now)) return false;
  const from = Date.parse(period.fromDate);
  return !Number.isNaN(from) && from > now;
}

/** Checks every period, not just `validityPeriods[0]` — the previous code
 * read only the first, which stopped being safe once incidents could carry
 * several periods (see
 * `docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md`). */
export function bucketFor(status: LineStatus, now: number): IssueBucket {
  // No validity information at all is not the same as "it's over": the
  // aggregator's own fallback for an incident without a validity period is
  // an open-ended one starting now, so match that.
  if (status.validityPeriods.length === 0) return 'active';
  if (status.validityPeriods.some((period) => periodIsActive(period, now))) return 'active';
  if (status.validityPeriods.some((period) => periodIsUpcoming(period, now))) return 'upcoming';
  return 'ended';
}

/** The period a collapsed row should describe: whichever one covers now,
 * else the soonest one still to come, else the earliest on record. */
export function governingPeriod(status: LineStatus, now: number): ValidityPeriod | undefined {
  const active = status.validityPeriods.find((period) => periodIsActive(period, now));
  if (active) return active;

  const upcoming = status.validityPeriods
    .filter((period) => periodIsUpcoming(period, now))
    .sort((a, b) => Date.parse(a.fromDate) - Date.parse(b.fromDate));
  if (upcoming.length > 0) return upcoming[0];

  return [...status.validityPeriods].sort((a, b) => Date.parse(a.fromDate) - Date.parse(b.fromDate))[0];
}
