import type { LineStatusReport } from './types';
import { severityGroup, severityRank, worstStatus, type SeverityGroup } from './severity';
import { countryForReport, MERGED_TFL_LINE_IDS, type Country } from './modes';

/** Drops the TfL ids that are folded into their National Rail
 * counterpart's own row everywhere a line list is built directly from
 * reports rather than from `/public/lines` (`MERGED_TFL_LINE_IDS`'s own doc
 * comment) -- the exact same exclusion `app/page.tsx`'s
 * `notGoodServiceSummary` already applies, so this dashboard's counts never
 * double-count the same real-world line (e.g. the Elizabeth line reported
 * once under `tfl-elizabeth` AND once under its NR-merged id). */
function realReports(reports: LineStatusReport[]): LineStatusReport[] {
  return reports.filter((report) => !MERGED_TFL_LINE_IDS.includes(report.id));
}

/** Worst-first, then alphabetical -- the exact comparator
 * `notGoodServiceSummary` (`app/page.tsx:144-156`) already uses for its own
 * "Right now" list, reimplemented here as a small, independently-tested,
 * exported function rather than reaching into that page-local one (see this
 * plan's Judgment Call 5 for why `app/page.tsx` itself is left untouched). */
function compareWorstFirst(a: LineStatusReport, b: LineStatusReport): number {
  const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
  return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
}

export interface NetworkStatusOverview {
  /** How many lines fall into each severity bucket right now (deduplicated
   * per Task 2's `realReports`). */
  counts: Record<SeverityGroup, number>;
  /** Every line's report, deduplicated. Not surfaced directly by the
   * dashboard, but used to derive `counts`/`byMode`/`byCountry` and offered
   * here so a caller can compute a total without re-deriving the same
   * exclusion. */
  totalLines: number;
  /** Every affected line (i.e. not in the `'good'` bucket), worst-first
   * then alphabetical -- unbounded, unlike `app/page.tsx`'s
   * `RIGHT_NOW_LIMIT`-capped list (see this plan's Judgment Call 7). */
  worstFirst: LineStatusReport[];
  /** National Rail vs TfL, folding all five TfL-published modes
   * (`tube`/`dlr`/`overground`/`elizabeth-line`/`tram`) into one bucket --
   * mirrors `AllLinesTable`'s own "TfL" operator-filter folding
   * (`expandOperatorForFiltering`), applied to mode instead of operator. */
  byMode: { nationalRail: LineStatusReport[]; tfl: LineStatusReport[] };
  /** `countryForReport`'s three jurisdictions, present as a key only for a
   * country this snapshot actually has a line in -- so a caller doesn't
   * have to invent copy for an always-empty bucket (today, in practice,
   * always just `{ Gb: [...] }`, since `MODE_TO_COUNTRY` is still empty --
   * see `lib/modes.ts`'s own doc comment). */
  byCountry: Partial<Record<Country, LineStatusReport[]>>;
  /** The most recent `computedAt` across every report in this snapshot, or
   * `null` for an empty one -- what `app/status/page.tsx` hands to
   * `LastUpdated` under its own subtitle (2026-09-22 UX review §2.5: "right
   * now" with no timestamp anywhere on the page, despite being served
   * through `withStaleFallback`). Each report carries its own `computedAt`
   * (the aggregator cycle that produced IT), so the network-wide freshest
   * figure is the max across all of them, not any one report's value --
   * different lines can legitimately have been computed at slightly
   * different times within the same aggregator pass. */
  lastUpdated: string | null;
}

/** Builds the whole network-status dashboard's data from an already-fetched
 * `GET /Line/Mode/{mode}/Status` response -- the same payload
 * `app/page.tsx` and `app/lines/page.tsx` already fetch, no new endpoint.
 * Pure and synchronous: no fetch, no React, so `app/status/page.tsx` (Task
 * 3) is a thin Server Component that just calls this and renders the
 * result. */
export function buildNetworkStatusOverview(reports: LineStatusReport[]): NetworkStatusOverview {
  const real = realReports(reports);

  const counts: Record<SeverityGroup, number> = {
    good: 0,
    informational: 0,
    planned: 0,
    mild: 0,
    severe: 0,
  };
  const byMode: NetworkStatusOverview['byMode'] = { nationalRail: [], tfl: [] };
  const byCountry: NetworkStatusOverview['byCountry'] = {};

  for (const report of real) {
    const group = severityGroup(worstStatus(report).statusSeverity);
    counts[group] += 1;

    if (report.modeName === 'national-rail') {
      byMode.nationalRail.push(report);
    } else {
      byMode.tfl.push(report);
    }

    const country = countryForReport(report);
    (byCountry[country] ??= []).push(report);
  }

  const worstFirst = real
    .filter((report) => severityGroup(worstStatus(report).statusSeverity) !== 'good')
    .sort(compareWorstFirst);

  const lastUpdated =
    real.length === 0
      ? null
      : real.reduce((latest, r) => (new Date(r.computedAt) > new Date(latest) ? r.computedAt : latest), real[0].computedAt);

  return { counts, totalLines: real.length, worstFirst, byMode, byCountry, lastUpdated };
}
