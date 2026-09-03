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
  impactType: string | null;
}

export interface IncidentLineRef {
  id: string;
  name: string;
}

export interface IncidentHistoryEntry {
  summary: string;
  description: string;
  operators: string[];
  affectedStations: string[];
  priority: number;
  validityPeriods: ValidityPeriod[];
  isPlanned: boolean;
  isCleared: boolean;
  recordedAt: string; // RFC3339
}

/** `GET /public/incidents/{incidentId}`'s response
 * (`crates/api/src/routes/incidents.rs`). `description` is raw HTML —
 * sanitize with `sanitizeDescription` (`frontend/lib/sanitizeHtml.ts`)
 * before rendering, same as `DisruptionDetail`. `currentlyAffectsLines`
 * is computed fresh per request — can be empty for a cleared or
 * no-longer-matched incident, which is a normal outcome, not an error. */
export interface IncidentDetail {
  incidentId: string;
  summary: string;
  description: string;
  operators: string[];
  affectedStations: string[];
  priority: number;
  validityPeriods: ValidityPeriod[];
  isPlanned: boolean;
  isCleared: boolean;
  firstSeenAt: string; // RFC3339
  fetchedAt: string; // RFC3339
  currentlyAffectsLines: IncidentLineRef[];
  history: IncidentHistoryEntry[];
}

export interface SampleStats {
  total: number;
  delayed: number;
  cancelled: number;
  skipped: number;
  avgDelayMinutes: number;
}

/** Why `sampleStats` is (or isn't) populated on a given `LineStatus` this
 * cycle -- see `common::SampleAvailability`
 * (`crates/common/src/lib.rs`) and
 * docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md
 * Decision 2. Always present, unlike `sampleStats` itself. Read this only
 * through `sampleUnavailableReason`/`formatSampleSummary`
 * (`lib/sampleStats.ts`) -- it is not a meaningful signal on its own for a
 * TfL-quality status (see that module's precedence-order doc comment). */
export type SampleAvailability =
  | { state: 'no-coverage' }
  | { state: 'below-threshold'; observed: number; required: number }
  | { state: 'available' };

export interface LineStatus {
  statusSeverity: number;
  statusSeverityDescription: string;
  reason: string;
  dataQuality: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned' | 'tfl';
  validityPeriods: ValidityPeriod[];
  disruption?: Disruption;
  sampleStats?: SampleStats;
  sampleAvailability: SampleAvailability;
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

/** One operator's row from `GET /public/stations/{crs}/sample-stats`
 * (`crates/api/src/routes/station_stats.rs`) --
 * docs/superpowers/specs/2026-09-03-per-station-stats-design.md Decision 9.
 * `sampleAvailability.state` is never `'no-coverage'` through this route (a
 * documented invariant of the route handler, not type-enforced -- see that
 * design doc's Open Questions #4). */
export interface StationOperatorSampleStats {
  operator: string;
  sampleAvailability: SampleAvailability;
  sampleStats?: SampleStats;
}

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

/** `GET /Line/{id}/Stats/HalfHourly/{from}/to/{to}`'s per-bucket response
 * shape -- half-hourly sibling of `LineDailyStats`. `halfHourStart` is an
 * RFC3339 UTC instant (the start of the 30-minute bucket -- :00 or :30),
 * not a calendar day -- always render it through
 * `frontend/lib/dateFormat.ts`'s `formatTime` before display, same
 * convention `LineDailyStats.day` follows for its own rendering. Same
 * dedup/attribution caveat as `LineDailyStats` applies, reworded for "that
 * half hour" instead of "that day". Originally `LineHourlyStats` with an
 * `hourStart` field (1-hour buckets); renamed when the trend chart's
 * granularity was doubled -- see git history for the hourly-era version. */
export interface LineHalfHourlyStats {
  halfHourStart: string; // RFC3339 UTC instant, start of the 30-minute bucket
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
  // Deliberately snake_case, unlike every other field in this file's
  // camelCase types: `crates/api/src/routes/freshness.rs`'s `DataFreshness`
  // has no `#[serde(rename_all = ...)]`, so this field serializes on the
  // wire as literally `schedule_feed` -- when a CIF SCHEDULE feed delivery
  // was last recorded by `schedule-ingest`'s push to
  // `/private/schedule-feed-ingests`.
  schedule_feed: string | null;
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
  // `null` whenever the backend's `LEFT JOIN stations` found no reference
  // row for the code -- see `lib/stationLabel.ts`'s fallback.
  pinOriginName: string | null;
  pinDestinationName: string | null;
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

/** `GET /Train/mine`'s per-item response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TrackedTrainListItem`,
 * camelCase). A deliberately lighter shape than `TrackedTrainState` --
 * excludes live movement detail (train id, last reported location, next
 * calling point, ETA), appropriate for one train's detail page, not a
 * multi-row list. `pinScheduledDeparture` is new: neither
 * `TrackedTrainState` nor any other existing route exposes it. */
export interface TrackedTrainListItem {
  id: number;
  serviceDate: string; // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  // See `TrackedTrainState.pinOriginName`'s comment -- same contract.
  pinOriginName: string | null;
  pinDestinationName: string | null;
  pinScheduledDeparture: string; // RFC3339
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  status: JourneyStatus | null;
  delayMinutes: number | null;
  trackedAt: string; // RFC3339 -- list ordering key
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
 * `components/TicketPanel.tsx`). `trackedTrainId` is `number | null` --
 * `null` for a STANDALONE ticket (uploaded/entered before a tracked train
 * exists for it, per the upload-first flow) that hasn't been attached to
 * one yet. Every row `GET /Train/{trackingId}/tickets` itself returns is
 * always attached (that route is scoped BY a `tracked_train_id`), so this
 * only ever reads `null` when the same wire shape is reused for a ticket
 * fetched a different way (e.g. `get_ticket_owned`, used internally by the
 * attach flow) -- callers of THIS route can treat it as always non-null in
 * practice, but the type stays honest about the shape. */
export interface TrackedTrainTicket {
  id: number;
  trackedTrainId: number | null;
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  // Joined on THIS ticket's own origin/destination, not the pin route --
  // see `lib/stationLabel.ts`'s fallback.
  originName: string | null;
  destinationName: string | null;
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
 * regardless of which tab produced them. Also the request body for
 * `POST /Train/tickets` (no `trackingId` in the path at all) -- the
 * upload-first, standalone-ticket-creation route; the body shape is
 * identical, only the URL and the resulting ticket's `trackedTrainId`
 * differ. */
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

/** `POST /Train/tickets/{ticketId}/attach`'s request/response shapes --
 * attaches an existing standalone ticket (one created via
 * `POST /Train/tickets`, still `trackedTrainId: null`) to a tracked train
 * the caller owns, once they've found or created the one it's actually
 * for. `404` (ticket or tracked train doesn't exist / isn't the caller's --
 * this app's universal "never 403" convention) and `409` (the ticket is
 * already attached to something) are both real, distinct outcomes a caller
 * needs to handle -- see `components/AttachTicketAction.tsx`. */
export interface AttachTicketRequest {
  trackingId: number;
}

export interface AttachTicketResponse {
  ticketId: number;
  trackedTrainId: number;
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

/** `GET /Train/tickets/mine`'s per-item response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TicketListItem`, camelCase).
 * The last four fields are deliberately shaped identically to
 * `DelayRepayEstimateResponse` so a `TicketListItem` can be passed
 * straight into `<DelayRepayEstimate>` with no adapter -- see
 * docs/superpowers/specs/2026-08-31-tickets-list-design.md's Finding 7 /
 * Decision 1.
 *
 * `trackedTrainId` and every train-context field (`serviceDate`,
 * `pinOriginCrs`, `pinDestinationCrs`, `pinScheduledDeparture`,
 * `resolutionStatus`, `trainUid`, `status`) are now nullable -- all `null`
 * together for a STANDALONE ticket (uploaded/entered before a tracked
 * train exists for it) that hasn't been attached to one yet. `estimate`/
 * `delayMinutes` are already nullable and stay `null` for the same row, by
 * construction (no train means no delay data to estimate against) --
 * `claimUrl`/`disclaimer` stay unconditionally populated regardless, same
 * invariant as an attached ticket whose train hasn't reported a delay yet. */
export interface TicketListItem {
  id: number;
  trackedTrainId: number | null;
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  // See `TrackedTrainTicket.originName`'s comment -- same contract.
  originName: string | null;
  destinationName: string | null;
  source: TicketSource;
  createdAt: string; // RFC3339 -- list ordering key
  serviceDate: string | null; // "YYYY-MM-DD"
  pinOriginCrs: string | null;
  pinDestinationCrs: string | null;
  pinScheduledDeparture: string | null; // RFC3339
  resolutionStatus: ResolutionStatus | null;
  trainUid: string | null;
  status: JourneyStatus | null;
  delayMinutes: number | null;
  estimate: DelayRepayEstimate | null;
  claimUrl: string;
  disclaimer: string;
}

// ---------------------------------------------------------------------------
// Chat (embedded-chatbot-option-b-client-side-tokens plan) -- ChatPanel's
// own browser-side tool-calling loop, not a server-side orchestrator
// (that was removed, see that plan's Task 5).
// ---------------------------------------------------------------------------

/** Mirrors `distant-signal-mcp`'s own `StationRef`
 * (`src/tools/plan-journey.ts`) -- ported here (not imported: a separate
 * repository, no shared package) only as far as `ChatPanel` actually needs
 * for the "track this leg" deep-link. */
export interface RenderedStationRef {
  tiploc: string;
  name: string | null;
  crs: string | null;
}

/** Ported from `distant-signal-mcp`'s own `RenderedTrainLeg`
 * (`src/tools/plan-journey.ts:160-179`, per the chatbot MCP-integration
 * research doc's own citation) -- only the fields `ChatPanel`'s "track
 * this leg" card actually reads. A `plan_journey` tool-result's
 * `structuredContent` carries the full shape (including `RenderedTransferLeg`
 * siblings this app never renders a card for); this type is intentionally
 * a subset, not a 1:1 port of every field distant-signal-mcp defines. */
export interface RenderedTrainLeg {
  kind: 'train';
  from: RenderedStationRef;
  to: RenderedStationRef;
  departure: string;
  arrival: string;
  departureAt: string | null;
  arrivalAt: string | null;
  operator: string | null;
  uid: string;
}
