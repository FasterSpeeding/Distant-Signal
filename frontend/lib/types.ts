export interface ValidityPeriod {
  fromDate: string;
  toDate: string | null;
  isNow: boolean;
}

export interface AffectedRoute {
  from: string;
  to: string;
}

export interface Disruption {
  category: string;
  description: string;
  affectedStops: string[];
  affectedRoutes: AffectedRoute[];
  source: string | null;
}

export interface SampleStats {
  total: number;
  delayed: number;
  cancelled: number;
  skipped: number;
  avgDelayMinutes: number;
}

export interface LineStatus {
  statusSeverity: number;
  statusSeverityDescription: string;
  reason: string;
  dataQuality: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned' | 'tfl';
  validityPeriods: ValidityPeriod[];
  disruption?: Disruption;
  sampleStats?: SampleStats;
}

export interface LineStatusReport {
  $type: string;
  id: string;
  name: string;
  modeName: string;
  operators: string[];
  lineStatuses: LineStatus[];
  computedAt: string;
  /** Present only for a line with a TfL counterpart merged into it (see
   * docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
   * Area 1 -- Elizabeth line today). The counterpart's own current
   * statuses, rendered separately from `lineStatuses` on the line detail
   * page rather than merged into it, since only this report's own
   * `lineStatuses` carries real `sampleStats`. */
  tflStatus?: LineStatus[];
}

export type LineStatusHistoryEntry = LineStatusReport;

export interface Preferences {
  pinnedLines: string[];
  pinnedStations: string[];
}

export interface LineSummary {
  id: string;
  name: string;
  category: string;
  operators: string[];
  source: 'catalogue' | 'custom' | 'tfl';
}

export interface CustomLineDetail {
  id: string;
  name: string;
  operators: string[];
  stations: string[];
  headcodePrefixes: string[];
  destinationCrsFilter: string[];
}

export interface LineDefinitionSummary {
  stations: string[];
  operators: string[];
}

export interface DataFreshness {
  stations: string | null;
  tocs: string | null;
  incidents: string | null;
  tfl: string | null;
}

/** A code/name pair from the `/public/stations` and `/public/tocs`
 * type-ahead endpoints — CRS codes for stations, ATOC codes for
 * operators. */
export interface Suggestion {
  code: string;
  name: string;
}
