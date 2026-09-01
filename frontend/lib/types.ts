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

/** `GET /Line/{id}/Stats/{from}/to/{to}`'s per-day response shape.
 * `delayRate`/`cancellationRate`/`skipRate` are fractions (0-1) computed
 * server-side from stored sums over DISTINCT trains, deduped by Darwin
 * `service_id` via the aggregator's `dedup::dedup_new_sample_stats` -- see
 * `crates/aggregator/src/queries.rs`'s `record_daily_stats` doc comment --
 * not a share of poll cycles. Each train is counted once per day, using its
 * status the FIRST time it was observed that day; if it's still dwelling in
 * view and its status changes later (e.g. on-time becomes delayed), that
 * later state is never recorded, so these rates can under-report delays
 * that develop mid-visit. `sampleCycles` is the coverage signal the
 * sparse-data gap-rendering in `TrendsResults.tsx` depends on -- render it,
 * don't discard it. */
export interface LineDailyStats {
  day: string; // "YYYY-MM-DD", Europe/London calendar day
  sampleCycles: number;
  total: number;
  delayed: number;
  cancelled: number;
  skipped: number;
  avgDelayMinutes: number;
  delayRate: number;
  cancellationRate: number;
  skipRate: number;
}

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

/** `isOwner` is computed server-side via `OptionalAuthenticatedUser` against
 * the stored `user_id` — `false` for an anonymous visitor, a logged-in
 * non-owner, and a legacy line with no owner at all; `true` only for the
 * real owner. This is the ownership signal `/lines/[id]` uses to hide
 * Edit/Delete for everyone else — see
 * docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md's Policy,
 * Tier 3. */
export interface CustomLineDetail {
  id: string;
  name: string;
  operators: string[];
  stations: string[];
  headcodePrefixes: string[];
  destinationCrsFilter: string[];
  isOwner: boolean;
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

/** `GET /public/history-retention`'s response: how many days of
 * `line_status_history` the backend actually keeps, echoed from the
 * aggregator's own `HISTORY_RETENTION_DAYS` (see
 * `crates/api/src/routes/history_retention.rs`). Used by the
 * `/lines/[id]/history` page to tell a genuinely-pruned range apart from a
 * genuinely-quiet line. */
export interface HistoryRetention {
  historyRetentionDays: number;
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

export type TicketSource = 'manual' | 'pkpass-semantics' | 'pkpass-heuristic' | 'pdf-heuristic';

/** `GET /Train/{trackingId}/tickets`'s per-item response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TrackedTrainTicket`,
 * camelCase). Never includes `userId` -- same posture as
 * `TrackedTrainState`. Nothing caps a tracked train at one ticket; multiple
 * tickets per tracked train are a real, supported case (see
 * `components/TicketPanel.tsx`). */
export interface TrackedTrainTicket {
  id: number;
  trackedTrainId: number;
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  source: TicketSource;
  createdAt: string; // RFC3339
}

/** `POST /Train/{trackingId}/tickets`'s request body
 * (`common::TicketEntryRequest`) -- snake_case, matching `TrackPinRequest`'s
 * own internal-wire-type convention (unlike every other type in this file,
 * which mirrors `crates/api`'s camelCase public JSON). `source` is not
 * optional on this type even though the backend defaults it to `'manual'`
 * -- `components/TicketEntryForm.tsx` always sends it explicitly, since it
 * needs to track the current provenance of the fields it's submitting
 * regardless of which tab produced them. */
export interface TicketEntryRequest {
  operator?: string;
  ticket_type?: string;
  origin_crs?: string;
  destination_crs?: string;
  source: TicketSource;
}

export interface TicketCreatedResponse {
  ticketId: number;
}

/** `POST .../tickets/pkpass` and `POST .../tickets/pdf`'s shared response
 * shape -- every field independently nullable; "not found in this file" is
 * expected, not an error. Never persisted to the database by either upload
 * route -- this is only ever a preview
 * (`components/TicketEntryForm.tsx` pre-fills the manual-entry fields from
 * it and requires a second, separate submit to actually save anything). */
export interface PartialTicket {
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  source: TicketSource;
}

/** Present only inside a non-null `DelayRepayEstimateResponse.estimate`.
 * `disclaimer` here is a DIFFERENT string from
 * `DelayRepayEstimateResponse.disclaimer` (the top-level field) -- see
 * `components/DelayRepayEstimate.tsx`, which renders only the top-level
 * one. */
export interface DelayRepayEstimate {
  scheme: 'DR15' | 'DR30';
  bandMinutes: number;
  percentage: number;
  disclaimer: string;
}

/** `GET .../tickets/{ticketId}/delay-repay`'s response. `claimUrl` and the
 * top-level `disclaimer` are ALWAYS populated, independent of `estimate` --
 * this route never returns a bare percentage with no caveat and no link.
 * `estimate` is `null` whenever any of three things is true (no operator on
 * the ticket, no delay data on the train yet, or a real delay that just
 * didn't clear the matched scheme's lowest band) -- the response gives no
 * signal which of the three applied; see
 * `components/DelayRepayEstimate.tsx` for how this is rendered honestly
 * without inventing a reason the API doesn't give. */
export interface DelayRepayEstimateResponse {
  delayMinutes: number | null;
  estimate: DelayRepayEstimate | null;
  claimUrl: string;
  disclaimer: string;
}
