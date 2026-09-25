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

/** One row from `GET /public/incidents`. Deliberately lighter than
 * `IncidentDetail` (no description, no validityPeriods, no history, no
 * currentlyAffectsLines) — see
 * docs/superpowers/specs/2026-09-12-incident-archive-design.md Decision 7. */
export interface IncidentSummary {
  incidentId: string;
  summary: string;
  operators: string[];
  /** Always empty in practice: RDM's Knowledgebase Incidents feed carries no
   * station codes, only a free-text route description. Kept on the wire
   * because the column exists and the detail page renders it; `affectedLines`
   * is the field that actually says which railway an incident touches. */
  affectedStations: string[];
  /** Catalogue line ids, as decided by the same matcher that drives the live
   * status pages (`common::matcher`). This is what the Line filter matches
   * on. Empty means "matched no catalogue line" -- which, for a row ingested
   * before the column existed, may just mean "not backfilled yet". */
  affectedLines: string[];
  priority: number;
  isPlanned: boolean;
  isCleared: boolean;
  firstSeenAt: string; // RFC3339
  fetchedAt: string; // RFC3339
}

export interface IncidentSearchResponse {
  results: IncidentSummary[];
  nextCursor: string | null;
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

/** Why `fullCoverageStats` is (or isn't) populated on a given `LineStatus`
 * this cycle -- the full-coverage analog of `SampleAvailability`,
 * deliberately a SIBLING type, not a reuse of it (see
 * `common::FullCoverageAvailability`'s own doc comment,
 * `crates/common/src/lib.rs`, and
 * docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
 * Decision 1). Always present, like `sampleAvailability`. `'not-enabled'` is
 * the default and, as of this app's current line catalogue, the ONLY value
 * this can take -- nothing sets `full_coverage_enabled` on any line yet, and
 * no producer exists to resolve `'pending'`/`'available'`. */
export type FullCoverageAvailability =
  | { state: 'not-enabled' }
  | { state: 'pending' }
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
  /** Full-coverage analog of `sampleStats` -- see `FullCoverageAvailability`'s
   * own doc comment. `undefined` for every line today (nothing produces this
   * yet); permanent, additive scaffolding, not a replacement for
   * `sampleStats` -- see the design doc's Decision 3. */
  fullCoverageStats?: SampleStats;
  fullCoverageAvailability: FullCoverageAvailability;
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
 * docs/superpowers/specs/2026-09-03-per-station-stats-design.md Decision 9,
 * extended per
 * docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md
 * Decision 3. `sampleAvailability.state` is never `'no-coverage'` through
 * this route (a documented invariant of the route handler, not
 * type-enforced). `fullCoverageAvailability` is always present, like
 * `LineStatus`'s own field of the same name -- `'not-enabled'` for every
 * real operator today, since no line has `full_coverage_enabled` set. */
export interface StationOperatorSampleStats {
  operator: string;
  sampleAvailability: SampleAvailability;
  sampleStats?: SampleStats;
  fullCoverageStats?: SampleStats;
  fullCoverageAvailability: FullCoverageAvailability;
}

/** `GET /public/stations/{crs}/accessibility`'s response -- a filtered
 * passthrough of `stations.accessibility` (see
 * docs/superpowers/specs/2026-09-12-station-accessibility-design.md
 * Decision 1 for the exact key allowlist). Every value is `unknown`, not a
 * nested interface, because this codebase has never recorded the RDM
 * feed's field-level shape for any of these keys (design spec
 * Correction 2) -- typing them more precisely here would be inventing a
 * contract this app cannot actually verify. Keys are present only when
 * non-null in the source data; a key with no data is simply absent, not
 * `null`. Deliberately keeps the `accessibility`-named wire vocabulary
 * despite the WCAG "accessibility" naming collision this codebase also
 * uses elsewhere (Correction 4) -- see that section for why this isn't
 * renamed. */
export interface StationAccessibilityData {
  stationAccessibility?: unknown;
  staffAssistance?: unknown;
  toiletsAndChanging?: unknown;
  lifts?: unknown;
  transportLinks?: unknown;
  cycling?: unknown;
  carParks?: unknown;
  dropOffPickUp?: unknown;
  platformFacilities?: unknown;
  stationFacilities?: unknown;
  helpAndSupport?: unknown;
  loungesAndWaiting?: unknown;
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

/** `GET /Line/{id}/Stats/Hourly/{from}/to/{to}`'s per-bucket response shape
 * -- same fields as `LineHalfHourlyStats`, but `bucketStart` in place of
 * `halfHourStart`: this is the start of a 1-hour bucket, derived at READ
 * time by grouping `line_status_half_hourly_stats` rows
 * (`crates/api/src/data/queries.rs`'s `sub_daily_stats_for_range`) --
 * reusing "halfHourStart" for a 1-hour bucket would be a misleading field
 * name. Always an RFC3339 UTC instant; render it through
 * `frontend/lib/dateFormat.ts`'s `formatTime` before display, same
 * convention `LineHalfHourlyStats.halfHourStart` follows. */
export interface LineHourlyStats {
  bucketStart: string; // RFC3339 UTC instant, start of the 1-hour bucket
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

/** `GET /Line/{id}/Stats/SixHourly/{from}/to/{to}`'s per-bucket response
 * shape -- identical to `LineHourlyStats` except the bucket is 6 hours
 * wide, not 1. */
export interface LineSixHourlyStats {
  bucketStart: string; // RFC3339 UTC instant, start of the 6-hour bucket
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

/** `GET /public/operators/{code}/stats/...` and `GET /public/network/stats/...`
 * share the exact same per-bucket response shape the per-line routes
 * already use -- `LineDailyStats` etc. carry no line-specific field, so
 * these are plain aliases for readability at the new call sites, not new
 * structural types. See
 * docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md. */
export type OperatorDailyStats = LineDailyStats;
export type OperatorHalfHourlyStats = LineHalfHourlyStats;
export type OperatorHourlyStats = LineHourlyStats;
export type OperatorSixHourlyStats = LineSixHourlyStats;
export type NetworkDailyStats = LineDailyStats;
export type NetworkHalfHourlyStats = LineHalfHourlyStats;
export type NetworkHourlyStats = LineHourlyStats;
export type NetworkSixHourlyStats = LineSixHourlyStats;

/** `GET /Line/{id}/Stats/Coverage/{from}/to/{to}`'s per-day response shape --
 * the full-coverage sibling of `LineDailyStats` (`resolvedWindows` in place
 * of `sampleCycles`). Rates shown cover every scheduled service on the
 * line, cross-referenced against real train-movement data -- not a sample
 * of live departures at a handful of stations. `resolvedWindows` is the
 * coverage/gap-rendering signal here, the full-coverage analog of
 * `sampleCycles` -- how many cycles this day saw a genuinely resolved
 * (not pending) population, not merely "any raw coverage at all". Always
 * an empty array today: no full-coverage producer exists yet to populate
 * the underlying table. See
 * docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
 * Decision 4. */
export interface LineDailyCoverageStats {
  day: string; // "YYYY-MM-DD", Europe/London calendar day
  resolvedWindows: number;
  total: number;
  delayed: number;
  cancelled: number;
  skipped: number;
  avgDelayMinutes: number;
  delayRate: number;
  cancellationRate: number;
  skipRate: number;
}

/** `GET /Line/{id}/Stats/Coverage/HalfHourly/{from}/to/{to}`'s per-bucket
 * response shape -- half-hourly sibling of `LineDailyCoverageStats`, same
 * relationship `LineHalfHourlyStats` already has to `LineDailyStats`. */
export interface LineHalfHourlyCoverageStats {
  halfHourStart: string; // RFC3339 UTC instant, start of the 30-minute bucket
  resolvedWindows: number;
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
  pinnedOperators: string[];
}

export interface LineSummary {
  id: string;
  name: string;
  category: string;
  operators: string[];
  source: 'catalogue' | 'custom' | 'tfl';
}

/** `GET /public/operators`'s per-item response shape (+ `GET
 * /public/operators/{code}`'s single-item shape) --
 * `crates/api/src/data/operators.rs`'s `OperatorRollup`, hand-serialized
 * camelCase by `crates/api/src/routes/operators.rs`'s `operator_rollup_json`.
 * `code` is either a real ATOC code (`tocs.atoc_code`) or the literal
 * synthetic string `"TfL"`. `lineIds` is the exact "which lines does this
 * operator run" set the rollup was computed from -- reusable by a future
 * per-operator detail/history view without a second request. `sampleStats`
 * is absent when no matching line had a representative status carrying
 * stats yet (always the case for the `"TfL"` row, which never carries
 * sample stats per-line either) -- render with
 * `lib/operatorStats.ts`'s `formatOperatorSampleSummary`, not
 * `lib/sampleStats.ts`'s `formatSampleSummary` (this type has no
 * `sampleAvailability`/`dataQuality` to satisfy `SampleStatsCarrier` with —
 * see docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md's
 * Judgment Call 3 for why). `computedAt` is `null` only in the
 * (never-constructed-in-practice) case of a rollup with zero matching
 * lines. */
export interface OperatorSummary {
  code: string;
  name: string;
  lineIds: string[];
  worstSeverity: number;
  reason: string;
  /** The id/name of the specific line whose status `worstSeverity`/`reason`
   * above came from -- lets a card/page say WHICH of an operator's lines is
   * driving its rollup ("Worst of 4 lines · LNER East Coast Main Line")
   * instead of presenting one line's reason as the whole operator's status
   * with no scope (2026-09-22 UX review [OH] §2.3/I11).
   * `crates/api/src/data/operators.rs`'s `OperatorRollup.worst_line_id`/
   * `worst_line_name`. Optional (rather than always required) so a payload
   * from a server that hasn't rolled this field out yet still satisfies
   * this type -- a caller must treat their absence as "no known worst
   * line" and fall back accordingly, not assume they're always present the
   * way `reason` is. */
  worstLineId?: string;
  worstLineName?: string;
  sampleStats?: SampleStats;
  computedAt: string | null;
}

export interface CustomLineDetail {
  id: string;
  name: string;
  operators: string[];
  stations: string[];
  headcodePrefixes: string[];
  destinationCrsFilter: string[];
  /** Whether the CALLER owns this line. A `200` from
   * `GET /public/lines/{id}` used to prove ownership by itself; custom-line
   * group sharing made that false (a granted group member gets the same
   * full detail), so every edit/delete affordance must gate on this flag
   * rather than on "the fetch succeeded". The backend is still the
   * authority -- `PUT`/`DELETE` remain owner-only and grant-blind -- this
   * is what stops the UI offering a control that can only ever 404. */
  isOwner: boolean;
  /** Every group this line is currently shared into. Populated ONLY when
   * `isOwner` is true; always `[]` for a granted non-owner, so a fellow
   * group member never learns which other groups the owner shared it
   * into. */
  sharedWithGroups: LineGroupRef[];
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
 * genuinely-quiet line.
 *
 * `dailyStatsRetentionDays`/`halfHourlyStatsRetentionHours` (Decision 8 of
 * docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md)
 * extend this same echo to the two other retention ceilings the Trends
 * tab's `GranularityControl` needs -- see `frontend/lib/history.ts`'s
 * `GranularityRetentionCeilings`/`availableGranularities`/
 * `resolveGranularity`. */
export interface HistoryRetention {
  historyRetentionDays: number;
  dailyStatsRetentionDays: number;
  halfHourlyStatsRetentionHours: number;
}

/** A code/name pair from the `/public/stations` and `/public/tocs`
 * type-ahead endpoints — CRS codes for stations, ATOC codes for
 * operators. */
export interface Suggestion {
  code: string;
  name: string;
}

/** A code/name pair from `GET /public/stations/nearby` --
 * (`crates/api/src/data/reference.rs`'s `NearbyStation`), like `Suggestion`
 * but for a "near me" physical-distance lookup rather than text search, and
 * carrying the great-circle distance from the caller's supplied point, in
 * kilometres. Stations with no recorded coordinates never appear in this
 * response at all (the backend excludes them), so there is no nullable
 * distance case to render here. */
export interface NearbyStation {
  code: string;
  name: string;
  distanceKm: number;
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

export type ResolutionStatus = 'pending' | 'schedule_matched' | 'resolved' | 'unresolved';
export type JourneyStatus = 'awaiting_activation' | 'en_route' | 'cancelled' | 'completed';
export type EtaSource = 'trust-propagated' | 'darwin-estimated';

export type ScheduleCallingPointKind = 'Origin' | 'Intermediate' | 'Terminate';

/** One calling point of a `schedule_matched` pin's matched service, as
 * snapshotted at match time (`crates/api/src/data/schedule_matching.rs`'s
 * `ScheduleCallingPointDto`) -- already camelCase on the wire, unlike the
 * Rust `schedule_query::CallingPoint` type it's derived from. */
export interface ScheduleCallingPoint {
  tiploc: string;
  kind: ScheduleCallingPointKind;
  bookedArrival: string | null; // "HH:MM:SS"
  bookedDeparture: string | null;
  isHalfMinuteArrival: boolean;
  isHalfMinuteDeparture: boolean;
}

export type JourneyStopKind = 'Origin' | 'Intermediate' | 'Terminate';

/** `crates/api/src/data/journey.rs`'s `StopStatus`, plain PascalCase on the
 * wire (no `rename_all`) -- same convention as `JourneyStopKind` just
 * above, the field this one sits next to on every `JourneyStop`.
 * `'Unknown'` covers every stop the booked/two-sided-calling-point
 * distinction doesn't apply to at all (an `Origin`/`Terminate` stop, or an
 * `Intermediate` one missing a scheduled time) -- whether such a stop was
 * reached is still answered by `actualArrival`/`actualDeparture` alone,
 * same as before this field existed. */
export type StopStatus = 'Unknown' | 'Scheduled' | 'Called' | 'Skipped';

/** `crates/api/src/data/journey.rs`'s `SkipSource` -- which signal(s)
 * support a `stopStatus: 'Skipped'` verdict, carried as a SIBLING field on
 * `JourneyStop` rather than nested inside `stopStatus` itself, mirroring
 * `EtaBadge.tsx`'s `etaSource` provenance-surfacing convention (see that
 * component's own doc comment). `'Darwin'` is the operator's own explicit
 * per-calling-point cancellation flag -- treat as authoritative.
 * `'Trust'` is inferred purely from a reported TRUST `PASS` event, real
 * running data but an INFERENCE about what a `PASS` message means for a
 * booked stop (see
 * docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md's
 * still-open PASS-mapping caveat) -- word this one more softly than the
 * Darwin case. `'Both'` is the two signals independently agreeing, as
 * confident as `'Darwin'` alone. */
export type SkipSource = 'Darwin' | 'Trust' | 'Both';

/** One calling point of a train's journey, booked schedule merged with the
 * latest reported live data for that location --
 * `crates/api/src/data/journey.rs`'s `JourneyStop`, camelCase on the wire.
 * `null` fields mean "not yet known" (a stop not yet reached has no
 * `actual*`/`delayMinutes`), never a fabricated value -- see
 * docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §2. */
export interface JourneyStop {
  crs: string | null;
  name: string | null;
  tiploc: string | null;
  kind: JourneyStopKind | null;
  scheduledArrival: string | null; // RFC3339
  scheduledDeparture: string | null; // RFC3339
  actualArrival: string | null; // RFC3339
  actualDeparture: string | null; // RFC3339
  // Scheduled time + the train's current overall delay, populated ONLY
  // while the matching `actual*` field above is still `null` -- i.e. only
  // for a stop live movement data hasn't actually reported yet. Never
  // overwrites/coexists meaningfully with a confirmed actual time; see
  // `crates/api/src/data/journey.rs`'s `apply_delay_estimates`.
  estimatedArrival: string | null; // RFC3339
  estimatedDeparture: string | null; // RFC3339
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  variationStatus: string | null;
  // `null` for a `stopStatus: 'Skipped'` stop -- see
  // `crates/api/src/data/journey.rs`'s `apply_stop_status` for why a
  // skipped stop's own delay figure is suppressed rather than shown.
  delayMinutes: number | null;
  stopStatus: StopStatus;
  skipSource: SkipSource | null;
  // Platform is `null` for every stop except (today) the ORIGIN -- Darwin/
  // LDBWS's live departure board only ever reports a station's OWN
  // platform for a service actually departing FROM it, never a
  // per-calling-point platform for the rest of the route, so this codebase
  // genuinely has no platform signal for any other calling point. See
  // `crates/api/src/data/journey.rs`'s `apply_origin_platform` for the full
  // reasoning. `null` here means exactly "not known", never a fabricated
  // value.
  platform: string | null;
  // The EARLIEST platform observed for the origin call -- Darwin has no
  // separate "planned platform" field of its own, so this is reconstructed
  // by `poller-ldbws::platform_history::PlatformHistory` from repeated
  // polls of the origin station's own board. `null` under the same
  // conditions as `platform` above, or when no platform has been observed
  // more than once yet.
  plannedPlatform: string | null;
  // `true` only when both `platform` and `plannedPlatform` are known AND
  // differ -- the non-colour signal to pair with any colour change when
  // showing a changed platform (WCAG 1.4.1). Always `false` when either is
  // `null` -- there is nothing to have changed.
  platformChanged: boolean;
}

/** `GET /Train/{trackingId}`'s response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TrackedTrainState`,
 * camelCase on the wire). NOT `GET /Train/by-uid/{uid}/{date}`'s any
 * more -- that route is public and returns `PublicTrainState`. `status`
 * and every
 * movement field are `null` until `resolutionStatus` is `'resolved'` and
 * `trust-consumer` has written a `train_current_state` row. Note there is
 * no `scheduledDeparture` field -- the backend's read query does not
 * select `pin_scheduled_departure`, only `serviceDate` (a date). See
 * `components/TrainJourney.tsx` for the full per-state rendering rules. */
export interface TrackedTrainState extends TrainJourneyState {
  // The `train_subscriptions.id` every `/Train/{trackingId}` route keys
  // off. Lives here and NOT on `TrainJourneyState`, precisely so a public,
  // subscription-less train can be rendered without one -- see
  // `PublicTrainState` below.
  id: number;
  // How many groups (`GET /groups`) this tracked train is currently shared
  // into -- `0` if it isn't shared anywhere. Lives here, not on
  // `TrainJourneyState`, for the same reason `id` does: a public,
  // subscription-less train has no `group_trains` row to count at all.
  // Read by `DeleteTrainButton`'s confirm modal to warn that deleting this
  // subscription also removes it from every one of those groups (the DB's
  // `group_trains.train_subscription_id ... ON DELETE CASCADE` already does
  // this automatically; this is purely so the UI can warn about it first).
  sharedGroupCount: number;
}

/** Exactly the fields `components/TrainJourney.tsx` reads -- notably NOT
 * `id`. Extracted so that page can render BOTH an owned subscription
 * (`TrackedTrainState`, which extends this) and the public, shared-train
 * view (`PublicTrainState`, adapted into this shape by
 * `app/train/[uid]/[date]/page.tsx`) without either one having to
 * fabricate a `trackingId` it doesn't have. See `PublicTrainState` below
 * for the surrogate-key collision that made that distinction load-bearing
 * rather than cosmetic. */
export interface TrainJourneyState {
  serviceDate: string; // "YYYY-MM-DD"
  // `null` for a subscription created the NR-primary way
  // (`POST /Train/by-uid/{uid}/{date}/track`) against a shared `trains` row
  // that has no schedule data yet -- the DEFAULT outcome of that endpoint,
  // not an edge case. `20260906130000_nullable_pin_columns.sql` dropped
  // this column's `NOT NULL` and the backend read model is `Option<String>`
  // to match.
  pinOriginCrs: string | null;
  pinDestinationCrs: string | null;
  // `null` whenever the backend's `LEFT JOIN stations` found no reference
  // row for the code -- see `lib/stationLabel.ts`'s fallback.
  pinOriginName: string | null;
  pinDestinationName: string | null;
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  trainId: string | null;
  // Populated once `resolutionStatus` is `'schedule_matched'` or later
  // (a schedule match's own destination -- may differ from
  // `pinDestinationCrs`, which is only what the user typed on the
  // tracking form and is optional). `null` until matched, or if the
  // matched schedule's terminus TIPLOC never resolved to a CRS.
  scheduleDestinationCrs: string | null;
  scheduleDestinationName: string | null;
  scheduleCallingPoints: ScheduleCallingPoint[] | null;
  status: JourneyStatus | null;
  lastReportedLocation: string | null;
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  delayMinutes: number | null;
  nextCallingPoint: string | null;
  etaNext: string | null; // RFC3339
  etaSource: EtaSource | null;
  // User-authored display label, or `null` for the computed default -- see
  // `lib/trackingName.ts`'s `trackedTrainDisplayName`. Never inferred from
  // any parsed document (`crates/api/src/data/ticket_extraction.rs`), only
  // ever set via `RenameTrainButton`.
  customName: string | null;
  // Optional here because `TrackedTrainState` genuinely has no such field
  // (the backend's single-train read never selects
  // `pin_scheduled_departure`) -- see `lib/trackingName.ts`.
  pinScheduledDeparture?: string | null;
  // The merged scheduled-timetable + live-overlay stop list -- `null`
  // until `trainUid` is known, or if neither backing source has anything
  // for this train. See `components/JourneyTimeline.tsx` and
  // docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §1.
  journeyStops: JourneyStop[] | null;
  // Server-computed replacement for the old client-only "may have
  // finished" heuristic (`status === 'en_route' && nextCallingPoint ===
  // null`, which fired almost always since `nextCallingPoint` is
  // essentially never populated in practice) -- `true` once now is more
  // than 15 minutes past the ESTIMATED arrival at `journeyStops`'s final
  // calling point. See `crates/api/src/data/journey.rs`'s
  // `may_have_arrived`. Always `false` when there are no `journeyStops` to
  // compute it from.
  mayHaveArrived: boolean;
}

/** `GET /Train/by-uid/{uid}/{date}`'s response shape
 * (`crates/api/src/data/trains.rs`'s `PublicTrainState`, camelCase). This
 * route is PUBLIC and UNSCOPED as of the shared-train-identity change: it
 * describes the shared, real-world train, and carries NO per-subscriber
 * data at all -- no `customName`, no tickets, no notification state.
 *
 * `trainsId` is the shared `trains` row's own surrogate key. It is NOT a
 * tracking id and must never be passed to `RenameTrainButton`,
 * `DeleteTrainButton`, `TicketPanel` or any `/Train/{trackingId}` route:
 * those all read their id as a `train_subscriptions.id`, a different
 * `BIGSERIAL` space that also starts at 1, so the two collide freely. That
 * is exactly the bug this field's old name (`id`) caused on
 * `app/train/[uid]/[date]/page.tsx`. */
export interface PublicTrainState {
  trainsId: number;
  trainUid: string;
  serviceDate: string; // "YYYY-MM-DD"
  originCrs: string | null;
  originName: string | null;
  destinationCrs: string | null;
  destinationName: string | null;
  scheduledDeparture: string | null; // RFC3339
  callingPoints: ScheduleCallingPoint[] | null;
  trainId: string | null;
  status: JourneyStatus | null;
  lastReportedLocation: string | null;
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  delayMinutes: number | null;
  nextCallingPoint: string | null;
  etaNext: string | null; // RFC3339
  etaSource: EtaSource | null;
  journeyStops: JourneyStop[] | null;
  // See `TrainJourneyState.mayHaveArrived`'s own doc comment -- same
  // contract.
  mayHaveArrived: boolean;
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
  // See `TrackedTrainState.pinOriginCrs`'s comment -- same contract, same
  // reason. Both pin fields on this shape are nullable together.
  pinOriginCrs: string | null;
  pinDestinationCrs: string | null;
  // See `TrackedTrainState.pinOriginName`'s comment -- same contract.
  pinOriginName: string | null;
  pinDestinationName: string | null;
  pinScheduledDeparture: string | null; // RFC3339
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  status: JourneyStatus | null;
  delayMinutes: number | null;
  trackedAt: string; // RFC3339 -- list ordering key
  // See `TrackedTrainState.customName`'s comment -- same contract.
  customName: string | null;
  // See `TrackedTrainState.sharedGroupCount`'s comment -- same contract.
  // Needed here too: `/train/[uid]/[date]`'s tracking overlay renders
  // `DeleteTrainButton` off a `TrackedTrainListItem` match (`GET
  // /Train/mine`), not a `TrackedTrainState`.
  sharedGroupCount: number;
}

/** `common::TimeWindow` on the wire -- `crates/common/src/lib.rs`. Both
 * fields `"HH:MM:SS" | null`. */
export interface TimeWindow {
  after: string | null;
  before: string | null;
}

/** One row of `GET /Journeys/mine`
 * (`crates/api/src/data/journeys.rs::JourneyListItem`, camelCase).
 * Deliberately lighter than `JourneyDetail` below -- see that Rust
 * struct's own doc comment for why. */
export interface JourneyListItem {
  id: number;
  customName: string | null;
  createdAt: string;
  legId: number;
  originCrs: string | null;
  destinationCrs: string | null;
  /** The CURRENT leg's own service date ("YYYY-MM-DD") -- when the
   * traveller travels, not `createdAt` (when they set the journey up). */
  serviceDate: string;
  /** Resolved station names for the two CRS codes above, `null` when the
   * code is itself `null` or has no `stations` reference row. Feed both
   * pairs to `lib/stationLabel.ts`'s `routeLabel`, which degrades both
   * ends to bare codes together rather than mixing one name with one
   * code. */
  originName: string | null;
  destinationName: string | null;
  matchMode: 'unmatched' | 'manual' | 'auto';
  trainSubscriptionId: number | null;
  resolutionStatus: string | null;
  status: string | null;
  delayMinutes: number | null;
}

/** `GET /Journeys/{id}`'s per-leg `legSkip` field
 * (`crates/api/src/routes/journeys.rs::LegSkipResponse`, sourced from
 * `crates/api/src/data/station_skip.rs`'s `LegSkipStatus`, camelCase on
 * the wire) -- `null` when the leg has no matched train yet, or no known
 * origin/destination to check (nothing to report, not "checked and
 * clean"). See docs/superpowers/specs/2026-09-22-journey-tracking-design.md §5.2. */
export interface LegSkipStatus {
  originSkipped: boolean;
  destinationSkipped: boolean;
}

/** One leg of `GET /Journeys/{id}`'s response
 * (`crates/api/src/routes/journeys.rs::JourneyLegDetailResponse`).
 * `trackedTrainState` is `null` for an unmatched leg, and otherwise the
 * EXACT SAME shape `GET /Train/{trackingId}` returns -- `TrackedTrainState`
 * is reused verbatim, not a narrower/different type. */
export interface JourneyLegDetail {
  id: number;
  originCrs: string | null;
  /** `null` whenever `originCrs` is `null`, or there is no `stations`
   * reference row for the code -- same `LEFT JOIN stations` mechanism as
   * `TrackedTrainState.pinOriginName`, see that field's own doc comment. */
  originName: string | null;
  destinationCrs: string | null;
  /** See `originName`'s doc comment -- same mechanism, resolved from
   * `destinationCrs`. */
  destinationName: string | null;
  serviceDate: string;
  departAfter: string | null;
  departBefore: string | null;
  arriveAfter: string | null;
  arriveBefore: string | null;
  matchMode: 'unmatched' | 'manual' | 'auto';
  /** Whether this leg was ever created via a window search -- `true` for a
   * `window`-mode leg (even one with all four bounds below left `null`, a
   * deliberate "any train, any time" search) or a template-materialized
   * leg; `false` for a `pin`/`knownTrain`-mode leg, which never had a
   * window to search at all. Drives `JourneyLegCard.tsx`'s `hasWindow`/
   * "Change train" gate -- see that component's own doc comment for why
   * the four bounds above are no longer enough to derive this on their
   * own. */
  windowSearched: boolean;
  trackedTrainState: TrackedTrainState | null;
  legSkip: LegSkipStatus | null;
}

/** `GET /Journeys/{id}`'s full response. */
export interface JourneyDetail {
  id: number;
  customName: string | null;
  createdAt: string;
  legs: JourneyLegDetail[];
  /** Whether the CALLER owns this journey, as opposed to reading it via a
   * group it's been shared into (`journey_readable_by`,
   * `crates/api/src/data/journeys.rs`). Gate every owner-only action on
   * this flag -- the share-journey button
   * (`app/journeys/[id]/page.tsx`) and both owner-only branches of
   * `JourneyLegCard` (the unmatched-leg candidate picker and the
   * matched-leg "Change train" toggle). The backend still refuses all
   * three regardless for a non-owner, but showing them at all to a fellow
   * group member who can only ever get a 404 is its own bug. */
  isOwner: boolean;
  /** The journey's currently active unlisted share link, owner-view only
   * -- always `null` for a non-owner (a group member, or a viewer who
   * reached this journey via the share link itself). See `JourneyShareLink`
   * and `ShareJourneyLinkButton.tsx`. */
  shareLink: JourneyShareLink | null;
}

/** One leg of a `GET /Trips/plan` itinerary
 * (`crates/api/src/data/trip_planning_itinerary.rs::PlannedLeg`,
 * camelCase, discriminated by `kind`). A `transfer` leg has no train
 * identity at all -- it is a walk/tube/bus/ferry hop with no
 * corresponding `journey_legs` row ever created for it (see this plan's
 * own Judgment Call 3). */
export type TripPlanLeg =
  | {
      kind: 'train';
      trainUid: string;
      serviceDate: string; // "YYYY-MM-DD"
      originCrs: string | null;
      destinationCrs: string | null;
      scheduledDeparture: string; // "HH:MM:SS"
      scheduledArrival: string;
      arrivalDayOffset: number;
    }
  | {
      kind: 'transfer';
      mode: string;
      originCrs: string | null;
      destinationCrs: string | null;
      minutes: number;
    };

/** One candidate itinerary for one segment
 * (`crates/api/src/data/trip_planning_itinerary.rs::PlannedItinerary`).
 * `exceedsRecommendedChanges` is only ever present for `results=fastest`
 * (CSA has no interchange-count cap of its own, see that Rust module's
 * own doc comment) -- absent (not `false`) for a `results=options` entry. */
export interface TripPlanItinerary {
  legs: TripPlanLeg[];
  changeCount: number;
  totalDurationMinutes: number;
  exceedsRecommendedChanges?: boolean;
}

/** One origin->destination hop of a (possibly multi-waypoint) plan
 * (`routes::trips::get_trip_plan`'s own `"segments"` array entry). */
export interface TripPlanSegment {
  originCrs: string;
  destinationCrs: string;
  itineraries: TripPlanItinerary[];
  cappedByMaxChanges: boolean;
}

/** `GET /Trips/plan`'s full response. */
export interface TripPlanResponse {
  results: 'fastest' | 'options';
  segments: TripPlanSegment[];
}

/** Body for `POST /Journeys/{journeyId}/legs` (multi-leg chaining, spec
 * §3) -- two of the three shapes `POST /Journeys` already sends for a
 * journey's first leg (no `pin` mode -- spec §3 only offers a direct
 * known-train pick or an open time-window search for "add a leg"),
 * discriminated by `mode` exactly like the backend's own
 * `AddJourneyLegRequest` (`crates/api/src/routes/journeys.rs`).
 *
 * `knownTrain`'s `originCrs`/`destinationCrs` are OPTIONAL overrides for
 * this leg's own boarding/alighting point -- distinct from the matched
 * train's own full route. Omitting both reproduces the historical
 * pin-derived backend behavior (`origin_crs`/`destination_crs` read back
 * off `train_subscriptions.pin_origin_crs`/`pin_destination_crs`, which is
 * itself copied from the train's own full working, start to end);
 * supplying either overrides just that one field, the other still falls
 * back to the pin. `PlanTripFlow.tsx`'s commit step is the one caller that
 * sets them today, using the real per-leg origin/destination already on
 * each `TripPlanLeg` (e.g. boarding a Birmingham->Glasgow service at
 * Crewe, alighting at Preston). `AddJourneyLegButton.tsx` deliberately
 * never sets them -- the origin/destination-override plan's Judgment Call 2:
 * its `knownTrain` mode
 * only ever asks for a train UID + service date, so it has no real
 * per-leg origin/destination of its own to send that would differ from
 * the pin-derived value it's always relied on. */
export type NewJourneyLegRequest =
  | {
      mode: 'knownTrain';
      trainUid: string;
      serviceDate: string; // "YYYY-MM-DD"
      originCrs?: string;
      destinationCrs?: string;
    }
  | {
      mode: 'window';
      originCrs: string;
      destinationCrs: string;
      serviceDate: string; // "YYYY-MM-DD"
      departWindow?: TimeWindow;
      arriveWindow?: TimeWindow;
    };

/** `POST /Journeys/{journeyId}/legs`'s response
 * (`crates/api/src/routes/journeys.rs::AddLegResponse`). `trackingId` is
 * `null` for a `window`-mode leg -- no train bound yet, same convention as
 * `CreateJourneyResponse.trackingId`. */
export interface AddJourneyLegResponse {
  legId: number;
  trackingId: number | null;
}

/** `POST /Journeys`'s response
 * (`crates/api/src/routes/journeys.rs::CreateJourneyResponse`). */
export interface CreateJourneyResponse {
  journeyId: number;
  legId: number;
  trackingId: number | null;
  resolutionStatus: string | null;
}

/** One row of `GET /JourneyTemplates/mine`
 * (`crates/api/src/data/journey_templates.rs::JourneyTemplateListItem`,
 * camelCase). Summarized the same way `app/journeys/[id]/page.tsx`'s
 * `defaultJourneyTitle` computes a journey's own fallback title —
 * first leg's origin to last leg's destination — but computed
 * server-side, since a list row has no per-leg detail to derive it from
 * client-side. `leg_count` can be `0` only for a hand-edited row; every
 * template this app's own UI creates has at least one leg. `active`/
 * `daysOfWeek` are round-tripped for Phase C but unused by anything in
 * this app today — Phase B never sets `daysOfWeek` and every template's
 * `active` is always `true`. */
export interface JourneyTemplateListItem {
  id: number;
  customName: string | null;
  createdAt: string;
  legCount: number;
  firstOriginCrs: string | null;
  firstOriginName: string | null;
  lastDestinationCrs: string | null;
  lastDestinationName: string | null;
  active: boolean;
  daysOfWeek: number | null;
}

/** One leg of `GET /JourneyTemplates/{id}`'s response
 * (`crates/api/src/routes/journey_templates.rs::JourneyTemplateLegDetailResponse`).
 * No `serviceDate`/`matchMode`/`trackedTrainState` — a template leg is
 * date-less and never itself bound to a train; contrast with
 * `JourneyLegDetail`. */
export interface JourneyTemplateLegDetail {
  id: number;
  originCrs: string | null;
  originName: string | null;
  destinationCrs: string | null;
  destinationName: string | null;
  departAfter: string | null;
  departBefore: string | null;
  arriveAfter: string | null;
  arriveBefore: string | null;
}

/** `GET /JourneyTemplates/{id}`'s full response. `daysOfWeek`/`active`/
 * `startsOn`/`endsOn`/`defaultMatchMode`/`autoCommitRule` are real,
 * round-tripped fields (Phase C scaffolding, per
 * docs/superpowers/plans/2026-09-22-reusable-journeys-phaseB-durable-templates-plan.md) —
 * this app's own Phase B UI reads none of them for anything beyond
 * display; see `app/journeys/templates/[id]/page.tsx`'s own scope note. */
export interface JourneyTemplateDetail {
  id: number;
  customName: string | null;
  createdAt: string;
  updatedAt: string;
  daysOfWeek: number | null;
  active: boolean;
  startsOn: string | null;
  endsOn: string | null;
  defaultMatchMode: 'manual' | 'auto';
  autoCommitRule: 'earliest' | 'nearest_to_now' | null;
  legs: JourneyTemplateLegDetail[];
}

/** One leg in a `POST /JourneyTemplates` (`mode: 'manual'`) or
 * `PUT /JourneyTemplates/{id}` request body
 * (`crates/api/src/routes/journey_templates.rs::TemplateLegRequest`). No
 * `serviceDate` — see `JourneyTemplateLegDetail`'s own comment. */
export interface TemplateLegRequest {
  originCrs: string;
  destinationCrs: string;
  departWindow?: TimeWindow;
  arriveWindow?: TimeWindow;
}

/** `POST /JourneyTemplates`'s two mutually-exclusive request shapes
 * (`crates/api/src/routes/journey_templates.rs::CreateJourneyTemplateRequest`).
 * This app's own frontend only ever sends `fromJourney` (see
 * `components/SaveAsTemplateButton.tsx`) — `manual` exists on the wire for
 * a future "start from nothing" UI this plan does not build (see that
 * plan's Judgment Call 5), and because `PUT`'s body needs the identical
 * `TemplateLegRequest` shape regardless. */
export type CreateJourneyTemplateRequest =
  | {
      mode: 'manual';
      customName?: string;
      legs: TemplateLegRequest[];
    }
  | {
      mode: 'fromJourney';
      customName?: string;
      journeyId: number;
    };

/** `PUT /JourneyTemplates/{id}`'s request body — full-resource replace,
 * not a per-field patch (see the Phase B plan's Judgment Calls 1/4).
 * `daysOfWeek`/`active`/`startsOn`/`endsOn`/`defaultMatchMode`/
 * `autoCommitRule` are REQUIRED on every request (Phase C's backend task
 * made them non-optional, with no server-side default) — mirror
 * `JourneyTemplateDetail`'s own field types exactly. */
export interface PutJourneyTemplateRequest {
  customName?: string;
  legs: TemplateLegRequest[];
  daysOfWeek: number | null;
  active: boolean;
  startsOn: string | null;
  endsOn: string | null;
  defaultMatchMode: 'manual' | 'auto';
  autoCommitRule: 'earliest' | 'nearest_to_now' | null;
}

/** `POST /JourneyTemplates`'s response
 * (`crates/api/src/routes/journey_templates.rs::CreateJourneyTemplateResponse`). */
export interface CreateJourneyTemplateResponse {
  templateId: number;
}

/** `POST /JourneyTemplates/{id}/materialize`'s request body — always
 * explicit, never defaulted server-side; the "Run now" button's own date
 * field (`components/RunTemplateNowButton.tsx`) defaults to today
 * client-side and lets the caller change it first. */
export interface MaterializeTemplateRequest {
  serviceDate: string; // "YYYY-MM-DD"
}

/** `POST /JourneyTemplates/{id}/materialize`'s response
 * (`crates/api/src/routes/journey_templates.rs::MaterializeTemplateResponse`).
 * `journeyId` is where `RunTemplateNowButton` navigates on success — the
 * same `/journeys/{id}` detail page any other freshly-created journey
 * lands on. */
export interface MaterializeTemplateResponse {
  journeyId: number;
  legIds: number[];
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
  // CRS codes Darwin's own live departure board reported as skipped today
  // for the specific service being pinned (`DepartureRow.skippedStations`,
  // `TrackTrainForm.tsx`) -- carries that signal past the moment the
  // picker's own live board result expires, so it can end up on the
  // journey timeline's per-stop `skipSource` once a `trains` row exists.
  // Omitted (never sent as `[]`) when there's no such signal to carry --
  // the CIF-picker or manual-entry path, which has no live board at all.
  // See `common::TrackPinRequest.skipped_stations`'s own doc comment.
  skipped_stations?: string[];
  // Same idea as `skipped_stations` immediately above, for Darwin's
  // platform signal instead (`DepartureRow.platform`/`plannedPlatform`) --
  // carries the picked row's platform snapshot through so the journey
  // page's origin stop can show it once a `trains` row exists. See
  // `common::TrackPinRequest.platform`/`planned_platform`'s own doc
  // comments.
  platform?: string;
  planned_platform?: string;
}

/** `POST /Train/track`'s response body -- camelCase, like every other
 * `crates/api` public JSON response (only the request body above is
 * snake_case). `resolutionStatus` is `'pending'` unless a synchronous
 * schedule match succeeded at creation time, in which case it's
 * `'schedule_matched'` -- see
 * docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md
 * Decision 3. */
export interface TrackPinResponse {
  trackingId: number;
  resolutionStatus: ResolutionStatus;
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
  // User-authored display label, or `null` for the computed default -- see
  // `TicketSummary.tsx`. Never inferred from an uploaded `.pkpass`/PDF.
  customName: string | null;
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
  // See `TrackedTrainTicket.customName`'s comment -- same contract.
  customName: string | null;
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

// Shared groups -- see
// docs/superpowers/specs/2026-09-11-shared-groups-design.md. Mirrors
// crates/api/src/data/groups.rs's own wire shapes exactly.

export type GroupRole = 'owner' | 'admin' | 'member';

export interface GroupInviteLink {
  token: string;
  expiresAt: string; // RFC3339
}

/** `POST /Journeys/{id}/share-link`'s response, and the `shareLink` field
 * embedded on `JourneyDetail` for the owner only. `expiresAt` is always
 * `null` today -- journeys choose no forced TTL (design doc
 * docs/superpowers/specs/2026-09-23-unlisted-links-design.md §5) -- kept
 * as `string | null` rather than always-`null` so the type doesn't lie if
 * that choice is ever revisited. */
export interface JourneyShareLink {
  token: string;
  expiresAt: string | null;
}

export interface GroupSummary {
  id: string;
  name: string;
  role: GroupRole;
  memberCount: number;
}

export interface GroupDetail {
  id: string;
  name: string;
  ownerId: string;
  /** Same contract as `GroupMember.displayName`: the owner's own name, or
   * `null` -- never their email address. */
  ownerName: string | null;
  /** Same contract as `GroupMember.displayTag`. */
  ownerTag: string | null;
  memberCount: number;
  role: GroupRole;
  // `null` for a plain `member` -- the invite link is only ever included
  // for an `admin`/`owner` caller (see `routes::groups::get_group`'s own
  // doc comment).
  inviteLink: GroupInviteLink | null;
}

export interface GroupMember {
  userId: string;
  /** The member's own name, or `null` when their identity provider has no
   * name on file for them -- never their email address (the backend
   * deliberately doesn't fall back to one: `crates/api/src/data/users.rs`'s
   * `display_label`). Render `null` as a generic placeholder -- via
   * `lib/memberLabel.ts`'s `memberLabel`, which also appends `displayTag`. */
  displayName: string | null;
  /** Six hex characters that distinguish this member from the other
   * placeholder-rendered members of the same group, and `null` whenever
   * `displayName` is set (a real name is never suffixed). Derived from
   * `userId` and never from an email address -- see
   * `crates/api/src/data/users.rs`'s `MemberDisplay`. Without it, an
   * identity provider whose username claim is the user's email by design
   * (Entra ID's UPN) renders every single member of a group as the same
   * indistinguishable "A member". */
  displayTag: string | null;
  role: GroupRole;
  joinedAt: string; // RFC3339
}

/** A train shared into a group -- deliberately carries no ticket field and
 * no `notificationsEnabled`/exact-`trackedAt` field (spec §4's "Never
 * shown" list is a hard constraint on the backend response this mirrors). */
export interface GroupTrain {
  trainSubscriptionId: number;
  pinOriginCrs: string | null;
  pinDestinationCrs: string | null;
  pinOriginName: string | null;
  pinDestinationName: string | null;
  pinScheduledDeparture: string | null; // RFC3339
  serviceDate: string; // "YYYY-MM-DD"
  resolutionStatus: string;
  trainUid: string | null;
  status: string | null;
  delayMinutes: number | null;
  customName: string | null;
  addedBy: string;
  /** Same contract as `GroupMember.displayName`: the sharer's own name, or
   * `null` -- never their email address. */
  addedByName: string | null;
  /** Same contract as `GroupMember.displayTag`, for the sharer. */
  addedByTag: string | null;
}

/** `GET /public/groups/shared-trains`'s per-item shape
 * (`crates/api/src/data/groups.rs`'s `SharedTrain`): a `GroupTrain` plus
 * the group it was shared into, since this route's rows come from every
 * group the caller belongs to at once rather than one named group.
 *
 * One item per (group, train) pair -- a train shared into two of the
 * caller's groups arrives twice, once per group, so no attribution is
 * lost on the wire; `/track/mine` merges those back into a single row
 * carrying both group tags. Never includes the caller's OWN tracked
 * trains (those are `TrackedTrainListItem`s already), and carries the same
 * "never shown" privacy constraint `GroupTrain` does -- no tickets, no
 * notification state, no exact tracked-at timestamp. */
export interface SharedGroupTrain extends GroupTrain {
  groupId: string;
  groupName: string;
}

/** A custom line granted into a group by its OWNER
 * (`crates/api/src/data/groups.rs`'s `GroupCustomLine`) -- identity and
 * attribution only. The line's definition and live status are read through
 * the ordinary `/lines/{id}` and `/Line/{ids}/Status` routes, which the
 * grant widens for every member of the group, rather than being duplicated
 * onto this shape.
 *
 * A grant conveys READ access only: no group member other than the owner
 * can ever edit, delete, or re-share the line. */
export interface GroupCustomLine {
  lineId: string;
  lineName: string;
  grantedBy: string;
  /** Same contract as `GroupMember.displayName`/`GroupTrain.addedByName`:
   * the sharer's own name, or `null` -- never their email address. */
  grantedByName: string | null;
  /** Same contract as `GroupTrain.addedByTag`: set only when
   * `grantedByName` is `null`, so a group whose IdP can name nobody can
   * still tell one member's shared line from another's. Render through
   * `memberLabel`, never on its own. */
  grantedByTag: string | null;
}

/** `GET /public/groups/shared-custom-lines`'s per-item shape
 * (`crates/api/src/data/groups.rs`'s `SharedCustomLine`): a
 * `GroupCustomLine` plus the group it was granted into, since this route's
 * rows come from every group the caller belongs to at once.
 *
 * One item per (group, line) pair -- a line granted into two of the
 * caller's groups arrives twice, so no attribution is lost on the wire;
 * `lib/sharedCustomLines.ts` merges those back into a single row carrying
 * both group tags. Never includes the caller's OWN custom lines. */
export interface SharedGroupCustomLine extends GroupCustomLine {
  groupId: string;
  groupName: string;
}

/** A journey shared into a group -- `crates/api/src/data/groups.rs`'s
 * `GroupJourney`. Carries the journey's own identity plus its FIRST leg's
 * identity/live-status fields (not a full multi-leg rollup -- see this
 * feature's plan, Judgment Call 2) and a `legCount` so a multi-leg journey
 * at least signals "there's more". Same "never shown" privacy constraint
 * `GroupTrain` documents: no ticket field, no notification state, no
 * exact `addedAt`. */
export interface GroupJourney {
  journeyId: number;
  customName: string | null;
  legCount: number;
  pinOriginCrs: string | null;
  pinDestinationCrs: string | null;
  pinOriginName: string | null;
  pinDestinationName: string | null;
  pinScheduledDeparture: string | null; // RFC3339
  serviceDate: string; // "YYYY-MM-DD"
  resolutionStatus: string | null;
  trainUid: string | null;
  status: string | null;
  delayMinutes: number | null;
  addedBy: string;
  /** Same contract as `GroupMember.displayName`: the sharer's own name, or
   * `null` -- never their email address. */
  addedByName: string | null;
  /** Same contract as `GroupMember.displayTag`, for the sharer. */
  addedByTag: string | null;
}

/** `GET /public/groups/shared-journeys`'s per-item shape
 * (`crates/api/src/data/groups.rs`'s `SharedJourney`): a `GroupJourney`
 * plus the group it was shared into. Not consumed by any page in this
 * phase (see this feature's plan, Judgment Call 4) -- kept for parity with
 * `SharedGroupTrain`. */
export interface SharedGroupJourney extends GroupJourney {
  groupId: string;
  groupName: string;
}

/** One group a custom line is shared into, as its OWNER sees it on their
 * own edit page. Only ever populated for the owner -- see
 * `CustomLineDetail.sharedWithGroups`. */
export interface LineGroupRef {
  id: string;
  name: string;
}

export interface GroupJoinPreview {
  groupId: string;
  groupName: string;
  memberCount: number;
}

export type LineTrainCallingPointKind = 'Origin' | 'Intermediate' | 'Terminate';

/** One calling point inside a `GET /public/lines/{id}/trains?date=` entry's
 * `callingPoints` array (`crates/api/src/routes/lines.rs`'s `get_line_trains`,
 * rendered by `crates/api/src/render.rs`'s `line_train_json`). NOT the same
 * shape as `ScheduleCallingPoint` above (that one is fully camelCase,
 * backed by a different Rust type, `schedule_matching::ScheduleCallingPointDto`).
 * `line_train_json` passes the population entry's `calling_points` array
 * through UNPROCESSED -- only the outer `callingPoints` envelope key is
 * camelCase; see `render.rs`'s own
 * `line_train_json_with_no_live_row_passes_the_population_entry_through_and_nulls_live_status`
 * test, which asserts `json["callingPoints"] == entry["calling_points"]`
 * verbatim. Field names below are therefore this crate's usual accidental
 * exception, not a typo: the real `schedule_query::CallingPoint` snake_case
 * names, the same documented wart `get_line_schedule`'s own doc comment
 * accepts for this identical underlying data. */
export interface LineTrainCallingPoint {
  tiploc: string;
  kind: LineTrainCallingPointKind;
  booked_arrival: string | null; // "HH:MM:SS", CIF/UK-local, no date component
  booked_departure: string | null;
  is_half_minute_arrival: boolean;
  is_half_minute_departure: boolean;
  day_offset: number;
}

/** The `liveStatus` field of a `GET /public/lines/{id}/trains?date=` entry
 * -- `null` whenever no live `trains`/`train_current_state` row exists yet
 * for this UID on this date (an expected, honest gap: this route never
 * triggers a `find_or_create_train` upsert the way
 * `GET /Train/by-uid/{uid}/{date}` does -- see `get_line_trains`'s own doc
 * comment). Deliberately its own type, not a reuse of `PublicTrainState`:
 * `line_train_json` (`render.rs:295-310`) includes only these 14 fields
 * inside `liveStatus`, explicitly omitting `journeyStops`/`callingPoints`/
 * `trainUid`/`serviceDate`/`mayHaveArrived` -- confirmed by that file's
 * `line_train_json_with_a_live_row_attaches_live_status_in_camel_case`
 * test, which asserts all five omitted fields are absent. */
export interface LineTrainLiveStatus {
  trainsId: number;
  trainId: string | null;
  originCrs: string | null;
  originName: string | null;
  destinationCrs: string | null;
  destinationName: string | null;
  scheduledDeparture: string | null; // RFC3339
  status: JourneyStatus | null;
  lastReportedLocation: string | null;
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  delayMinutes: number | null;
  nextCallingPoint: string | null;
  etaNext: string | null; // RFC3339
  etaSource: EtaSource | null;
}

/** One `GET /public/lines/{id}/trains?date=` response entry
 * (`crates/api/src/routes/lines.rs`'s `get_line_trains`) -- every scheduled
 * UID on one line for one rail day, paired with live status where one
 * already exists. See that route's own doc comment and
 * docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md
 * §5.3. The full response is `LineTrainEntry[]`, not wrapped in an
 * envelope -- unlike `GET /public/trains/search`, this route has no cursor
 * of its own; it always returns the whole day's population in one call. */
export interface LineTrainEntry {
  uid: string;
  callingPoints: LineTrainCallingPoint[] | null;
  /** The schedule side's own origin/destination -- the first and last
   * `callingPoints` entries' TIPLOCs, resolved server-side to a CRS and
   * name (`crates/api/src/render.rs`'s `ScheduleRouteEndpoints`). Present
   * (though any individual field may still be `null`) on every entry,
   * regardless of `liveStatus` coverage -- unlike `liveStatus.originCrs`
   * etc., which is `null` whenever a live record has no schedule match of
   * its own. Added so `LineTrainsResults` can always name a route from the
   * schedule when the live side can't (2026-09-22 UX review §4.1). */
  scheduleOriginCrs: string | null;
  scheduleOriginName: string | null;
  scheduleDestinationCrs: string | null;
  scheduleDestinationName: string | null;
  liveStatus: LineTrainLiveStatus | null;
}
