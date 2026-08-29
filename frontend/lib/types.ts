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

/** `GET /public/auth/session`'s response — always 200, never 401 (an
 * anonymous visitor gets `authenticated: false` with everything else
 * `null`, not an error). `id`/`email`/`name` can all be `null` even when
 * `authenticated` is `true`, depending on what the OIDC provider actually
 * sent back. */
export interface SessionInfo {
  authenticated: boolean;
  id: string | null;
  email: string | null;
  name: string | null;
}

export type ResolutionStatus = 'pending' | 'resolved' | 'unresolved';
export type JourneyStatus = 'awaiting_activation' | 'en_route' | 'cancelled' | 'completed';
export type EtaSource = 'trust-propagated' | 'darwin-estimated';

/** `GET /Train/{trackingId}` and `GET /Train/by-uid/{uid}/{date}`'s shared
 * response shape (`crates/api/src/data/train_tracking.rs`'s
 * `TrackedTrainState`, camelCase on the wire). `status` and every
 * movement field are `null` until `resolutionStatus` is `'resolved'` and
 * `trust-consumer` has written a `train_current_state` row. Note there is
 * no `scheduledDeparture` field -- the backend's read query does not
 * select `pin_scheduled_departure`, only `serviceDate` (a date). See
 * `components/TrainJourney.tsx` for the full per-state rendering rules. */
export interface TrackedTrainState {
  id: number;
  serviceDate: string; // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  trainId: string | null;
  status: JourneyStatus | null;
  lastReportedLocation: string | null;
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  delayMinutes: number | null;
  nextCallingPoint: string | null;
  etaNext: string | null; // RFC3339
  etaSource: EtaSource | null;
}

/** `POST /Train/track`'s request body (`common::TrackPinRequest`). Plain
 * snake_case on the wire -- unlike every other type in this file, which
 * mirrors `crates/api`'s camelCase public JSON, this one matches
 * `crates/common`'s internal-wire-type convention instead. Sent only from
 * `components/TrackTrainForm.tsx`, via the same-origin `/api/Train/track`
 * proxy (`app/api/[...path]/route.ts`). */
export interface TrackPinRequest {
  service_date: string; // "YYYY-MM-DD"
  origin_crs: string;
  scheduled_departure: string; // RFC3339
  destination_crs?: string;
  operator?: string;
}

/** `POST /Train/track`'s response body -- camelCase, like every other
 * `crates/api` public JSON response (only the request body above is
 * snake_case). `resolutionStatus` is always the literal `'pending'` --
 * a newly-created pin has no `train_uid` bound yet. */
export interface TrackPinResponse {
  trackingId: number;
  resolutionStatus: 'pending';
}
