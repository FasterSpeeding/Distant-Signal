//! Ingestion endpoints for `private_router()`. Each poller POSTs a `Vec<T>`
//! snapshot once per poll cycle; these handlers just deserialize the body
//! (via axum's `Json<T>` extractor — no hand-rolled body validation) and
//! hand it to the matching upsert query.
//!
//! `/tfl-line-status` is the odd one out: its batch is already-computed
//! line status from TfL rather than raw upstream data, so its upsert
//! targets `line_status`/`line_status_history` directly (see
//! `queries::upsert_tfl_line_status`).
//!
//! Each POST route also has a same-path GET counterpart (see `router()`)
//! returning when that table was last successfully populated. Pollers call
//! it once at startup, before their poll loop begins, to skip an
//! immediately-redundant first fetch if the existing data is still fresh —
//! see `common::ingest::time_until_next_poll`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use common::ingest::LastFetchedResponse;
use common::{
    IncidentMessage, LineStatusReport, StationFullCoverageSample, StationReference, StationSample,
    TocReference,
};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::queries::{
    ScheduleCallingPointsFullRow, ScheduleDestinationDeparturesRow, ScheduleNetworkDeparturesRow,
};
use crate::data::train_tracking as queries_train_tracking;

pub fn router() -> Router {
    Router::new()
        .route(
            "/incidents",
            axum::routing::get(get_incidents_last_fetched).post(post_incidents),
        )
        .route(
            "/stations",
            axum::routing::get(get_stations_last_fetched).post(post_stations),
        )
        .route(
            "/tocs",
            axum::routing::get(get_tocs_last_fetched).post(post_tocs),
        )
        .route(
            "/station-samples",
            axum::routing::get(get_station_samples_last_fetched).post(post_station_samples),
        )
        .route(
            "/station-full-coverage-samples",
            axum::routing::get(get_station_full_coverage_samples_last_fetched)
                .post(post_station_full_coverage_samples),
        )
        .route(
            "/tfl-line-status",
            axum::routing::get(get_tfl_line_status_last_fetched).post(post_tfl_line_status),
        )
        .route("/train-events", axum::routing::post(post_train_events))
        .route(
            "/train-forward-signals",
            axum::routing::post(post_train_forward_signals),
        )
        .route(
            "/tracked-trains",
            axum::routing::get(get_active_tracked_trains),
        )
        .route(
            "/trust-event-backlog",
            axum::routing::post(post_trust_event_backlog),
        )
        .route(
            "/schedule-feed-ingests",
            axum::routing::get(get_schedule_feed_last_fetched).post(post_schedule_feed_ingest),
        )
        .route(
            "/schedule-reference-publishes",
            axum::routing::get(get_schedule_reference_last_publish)
                .post(post_schedule_reference_publish),
        )
        .route(
            "/stanox-crs",
            axum::routing::get(get_stanox_crs).post(post_stanox_crs),
        )
        .route("/tiploc-crs", axum::routing::post(post_tiploc_crs))
        .route("/fixed-links", axum::routing::post(post_fixed_links))
        .route(
            "/schedule-line-population",
            axum::routing::get(get_schedule_line_population).post(post_schedule_line_population),
        )
        .route(
            "/full-coverage-stats",
            axum::routing::get(get_full_coverage_stats_last_fetched).post(post_full_coverage_stats),
        )
        .route(
            "/schedule-network-departures",
            axum::routing::post(post_schedule_network_departures),
        )
        .route(
            "/schedule-destination-departures",
            axum::routing::post(post_schedule_destination_departures),
        )
        .route(
            "/schedule-calling-points-full",
            axum::routing::post(post_schedule_calling_points_full),
        )
        .route(
            "/island-of-ireland-stations",
            axum::routing::get(get_island_of_ireland_stations_last_fetched)
                .post(post_island_of_ireland_stations),
        )
        .route(
            "/island-of-ireland-lines",
            axum::routing::get(get_island_of_ireland_lines_last_fetched)
                .post(post_island_of_ireland_lines),
        )
        .route(
            "/island-of-ireland-station-samples",
            axum::routing::get(get_island_of_ireland_station_samples_last_fetched)
                .post(post_island_of_ireland_station_samples),
        )
}

#[derive(Debug, Serialize)]
struct UpsertResponse {
    upserted: u64,
}

async fn get_incidents_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_incidents_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn get_stations_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_stations_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn get_tocs_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_tocs_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn get_station_samples_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_station_samples_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn get_tfl_line_status_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_tfl_line_status_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_incidents(
    State(app): State<App>,
    Json(incidents): Json<Vec<IncidentMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted =
        queries::upsert_incidents(&app.database, &app.redis, &app.line_matcher, &incidents)
            .await
            .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn post_stations(
    State(app): State<App>,
    Json(stations): Json<Vec<StationReference>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_stations(&app.database, &stations)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn post_station_samples(
    State(app): State<App>,
    Json(samples): Json<Vec<StationSample>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_station_samples(&app.database, &samples)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn get_station_full_coverage_samples_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_station_full_coverage_samples_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_station_full_coverage_samples(
    State(app): State<App>,
    Json(samples): Json<Vec<StationFullCoverageSample>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_station_full_coverage_samples(&app.database, &samples)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn post_tocs(
    State(app): State<App>,
    Json(tocs): Json<Vec<TocReference>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_tocs(&app.database, &tocs)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// Unlike the other four ingest routes, this one writes the aggregator's
/// output table directly. That is not a shortcut: TfL publishes finished
/// line status, so there is nothing for the aggregator to infer from
/// incidents or departure boards, and routing it through that crate would
/// mean inventing a second input table for data that is already in its
/// final shape. The two writers stay out of each other's way via
/// `line_status.source` and the `tfl-` line-id prefix.
async fn post_tfl_line_status(
    State(app): State<App>,
    Json(reports): Json<Vec<LineStatusReport>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_tfl_line_status(&app.database, &reports)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `trust-consumer`'s per-poll-cycle batch of TRUST-derived events for
/// tracked trains -- see `queries_train_tracking::upsert_train_event`.
/// Non-transactional across the batch, deliberately, and the returned
/// `upserted` count now reflects that honestly (2026-09-25 Low-severity
/// auth-core review: previously, a per-event error aborted the WHOLE
/// request via `?` on the first failure -- so a caller got either a `500`
/// with no count at all, discarding whatever prefix of the batch had
/// already committed, or (on full success) `events.len()`; there was no
/// response shape that ever reported a genuinely partial result).
/// `upsert_train_event` runs several independent statements per event
/// (legacy-resolution lookups via `flip_legacy_resolution`, the shared
/// `trains`/`train_movement_events`/`train_current_state` writes, etc.),
/// each against `pool: &PgPool` directly rather than a shared transaction
/// handle -- wrapping this whole per-cycle batch in one transaction would
/// mean threading a `Transaction` all the way through
/// `upsert_train_event`/`flip_legacy_resolution` and every one of their
/// own sibling call sites (`data::trust_event_backlog_match`, and this
/// module's own ~15 direct test call sites), a much larger refactor than
/// this fix's own scope justifies for a Low-severity finding. Instead:
/// a per-event failure is logged and skipped -- exactly the same
/// log-and-continue posture `post_trust_event_backlog` just below already
/// takes for its own secondary shared-movement write, for the same
/// reason (one bad event must not sacrifice the rest of an otherwise-good
/// batch) -- and `upserted` reports how many actually committed. The
/// upserts this loop performs are themselves idempotent (keyed
/// `INSERT ... ON CONFLICT`-style writes further down the call chain), so
/// `trust-consumer` retrying a batch that partially failed is safe.
async fn post_train_events(
    State(app): State<App>,
    Json(events): Json<Vec<common::TrainMovementEventMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let mut upserted = 0u64;
    for event in &events {
        match queries_train_tracking::upsert_train_event(&app.database, event).await {
            Ok(()) => upserted += 1,
            Err(err) => {
                tracing::warn!(
                    error = ?err,
                    tracked_train_id = event.tracked_train_id,
                    "failed to upsert train event; continuing with the rest of the batch"
                );
            }
        }
    }
    Ok(Json(UpsertResponse { upserted }))
}

/// `trust-consumer`'s fast-path forwarding signals for the notifier-
/// forwarding queue (Task 17) -- see
/// `crate::data::notifier_forward_queue::insert_forward_signals`. Purely
/// additive: notifier's own unchanged cooldown/escalation logic is still
/// the sole gatekeeper for whether a push is actually sent.
async fn post_train_forward_signals(
    State(app): State<App>,
    Json(signals): Json<Vec<common::TrainForwardSignalMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let inserted =
        crate::data::notifier_forward_queue::insert_forward_signals(&app.database, &signals)
            .await
            .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted: inserted }))
}

/// `trust-backlog-consumer`'s per-cycle batch of key-journey-point TRUST
/// events, scoped to catalogued-line CRSs -- see
/// `crate::data::trust_event_backlog::upsert_trust_event_backlog_batch`.
///
/// **A partially rejected batch is still a 200.** Rows refused for a data
/// error (a constraint violation or invalid input) are listed in the
/// response's `rejected` field, and every other row is inserted. Returning
/// an error status instead would make the consumer retry a batch that can
/// never fully succeed -- the production failure this shape exists to end
/// (one bad row in a batch, replayed every 30 seconds for days, with over a
/// thousand good rows stuck behind it). A 200 keeps the route
/// backward-compatible too: a consumer that predates `rejected` ACKs the
/// batch and moves on. Because such a consumer never looks at `rejected`,
/// this route logs every rejected row itself (with the whole event, so it
/// can be recovered by hand) and counts it in
/// `distant_signal_api_trust_event_backlog_rejected_rows_total{reason}`.
///
/// A transient failure (connection, pool timeout, serialization failure,
/// lock or statement timeout, or any unexpected SQLSTATE) is still a 500,
/// so the consumer keeps retrying.
async fn post_trust_event_backlog(
    State(app): State<App>,
    Json(events): Json<Vec<common::TrustBacklogEventMessage>>,
) -> Result<Json<common::TrustBacklogIngestResponse>, (StatusCode, String)> {
    let outcome =
        crate::data::trust_event_backlog::upsert_trust_event_backlog_batch(&app.database, &events)
            .await
            .map_err(internal_error)?;
    for rejected in &outcome.rejected {
        tracing::warn!(
            index = rejected.index,
            dedup_key = %rejected.dedup_key,
            sqlstate = %rejected.sqlstate,
            reason = %rejected.reason,
            constraint = ?rejected.constraint,
            message = %rejected.message,
            event = ?events.get(rejected.index),
            "rejected trust-event-backlog row; inserted the rest of its batch"
        );
        metrics::counter!(
            common::metrics::metric_name("api_trust_event_backlog_rejected_rows_total"),
            "reason" => rejected.reason.clone()
        )
        .increment(1);
    }

    // Additional, parallel write onto the shared trains/train_movement_events/
    // train_current_state tables -- see ingest_shared_movements_batch's own
    // doc comment (in particular, why this can safely call the whole
    // batch through in one shot rather than looping call-per-event the way
    // this used to). A per-event failure here is logged and skipped, never
    // propagated: this route's own contract (backlog archival) must not
    // start failing because of a problem in the newer, separate shared-store
    // write path.
    let shared_movement_results =
        crate::data::trust_event_backlog::ingest_shared_movements_batch(&app.database, &events)
            .await;
    for (event, result) in events.iter().zip(shared_movement_results) {
        if let Err(err) = result {
            tracing::warn!(error = ?err, train_id = %event.train_id, "failed to ingest shared movement");
        }
    }

    Ok(Json(common::TrustBacklogIngestResponse {
        upserted: outcome.inserted,
        rejected: outcome.rejected,
    }))
}

/// `trust-consumer`'s periodic reference reload -- pending and
/// resolved-but-not-completed tracked trains, so it can recognize incoming
/// TRUST messages against them after a restart. See
/// `queries_train_tracking::list_active_tracked_trains`.
async fn get_active_tracked_trains(
    State(app): State<App>,
) -> Result<Json<Vec<common::TrackedTrainRef>>, (StatusCode, String)> {
    let rows = queries_train_tracking::list_active_tracked_trains(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}

/// `schedule-ingest`'s per-delivery record of one successfully-verified CIF
/// SCHEDULE feed delivery. Unlike the other ingest routes this isn't a
/// per-poll-cycle batch of reference data -- it's one row per delivery,
/// recorded once a stable `.zip` delivery has been extracted (see
/// `crates/schedule-ingest`).
///
/// `delivered_at` is the delivery zip's own mtime -- the real identity of
/// "which delivery is this" now that there is no sequence number (see
/// `docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`).
/// `ingested_at` is when this process actually happened to be processed,
/// kept only as separate observability data.
#[derive(Debug, Deserialize)]
struct ScheduleFeedIngestRequest {
    delivered_at: chrono::DateTime<chrono::Utc>,
    ingested_at: chrono::DateTime<chrono::Utc>,
    files: Vec<ScheduleFeedFile>,
}

/// One file observed as part of a schedule-feed delivery. `bytes` is the
/// size `schedule-ingest` itself observed on disk once stable, not a
/// manifest-declared size -- the real manifest format has no such field.
#[derive(Debug, Deserialize, Serialize)]
struct ScheduleFeedFile {
    name: String,
    bytes: u64,
}

async fn get_schedule_feed_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_schedule_feed_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_schedule_feed_ingest(
    State(app): State<App>,
    Json(req): Json<ScheduleFeedIngestRequest>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let files = serde_json::to_value(&req.files).map_err(|e| internal_error(e.into()))?;
    queries::insert_schedule_feed_ingest(&app.database, req.delivered_at, req.ingested_at, &files)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted: 1 }))
}

/// `crates/schedule-reference`'s OWN per-delivery completion marker -- the
/// one route in this file where that producer both writes and reads back its
/// own state, because the state IS its own (see
/// `queries::insert_schedule_reference_publish`).
///
/// Distinct from `/schedule-feed-ingests` above, and the distinction is
/// load-bearing: that route is `schedule-ingest`'s record of having
/// EXTRACTED a delivery; this one is `schedule-reference`'s record of having
/// PUBLISHED everything it derives from that delivery. `schedule-reference`
/// used to seed its restart dedup from the former, which meant a restart
/// mid-processing made it skip a delivery it had never actually published
/// -- see `20260925130000_schedule_reference_publishes.sql`.
async fn post_schedule_reference_publish(
    State(app): State<App>,
    Json(req): Json<common::ingest::ScheduleReferencePublishRequest>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    queries::insert_schedule_reference_publish(&app.database, &req.delivery)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted: 1 }))
}

/// The GET half of `/schedule-reference-publishes` -- read once at startup by
/// `schedule-reference::main::seed_last_processed_delivery`. Returns the
/// delivery directory name, NOT a timestamp, unlike every `last_*_fetch` GET
/// in this file: see `common::ingest::ScheduleReferencePublishRequest`'s own
/// doc comment for why the marker is the directory name verbatim.
async fn get_schedule_reference_last_publish(
    State(app): State<App>,
) -> Result<Json<common::ingest::LastCompletedPublishResponse>, (StatusCode, String)> {
    let delivery = queries::last_completed_schedule_reference_publish(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(common::ingest::LastCompletedPublishResponse {
        delivery,
    }))
}

/// `crates/schedule-reference`'s per-sequence batch of resolved
/// STANOX/CRS rows -- see `queries::upsert_stanox_crs`.
///
/// Also prunes (`queries::prune_stanox_crs_not_in`), in the same request,
/// right after the upsert: this whole POST body IS one delivery's complete
/// STANOX set (see `upsert_stanox_crs`'s own migration-comment-cited "every
/// daily delivery is a full refresh"), so any row this call did NOT just
/// upsert is stale as of this cycle and must go -- see that function's own
/// doc comment for why this cleanup is a route-level step rather than baked
/// into the upsert itself (Signal Box Audit Low finding, 2026-09-25).
async fn post_stanox_crs(
    State(app): State<App>,
    Json(records): Json<Vec<common::StanoxCrsRecord>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_stanox_crs(&app.database, &records)
        .await
        .map_err(internal_error)?;
    let keep_stanoxes: Vec<String> = records.iter().map(|r| r.stanox.clone()).collect();
    queries::prune_stanox_crs_not_in(&app.database, &keep_stanoxes)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `crates/schedule-reference`'s per-sequence batch of directly-resolved
/// TIPLOC->CRS rows -- see `queries::upsert_tiploc_crs`.
///
/// Also prunes (`queries::prune_tiploc_crs_not_in`), same reasoning and same
/// same-request timing as `post_stanox_crs`'s own doc comment directly above.
async fn post_tiploc_crs(
    State(app): State<App>,
    Json(records): Json<Vec<common::TiplocCrsRecord>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_tiploc_crs(&app.database, &records)
        .await
        .map_err(internal_error)?;
    let keep_tiplocs: Vec<String> = records.iter().map(|r| r.tiploc.clone()).collect();
    queries::prune_tiploc_crs_not_in(&app.database, &keep_tiplocs)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `crates/schedule-reference`'s per-cycle fixed-links batch -- see
/// `queries::upsert_fixed_links`.
async fn post_fixed_links(
    State(app): State<App>,
    Json(records): Json<Vec<common::FixedLinkRecord>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_fixed_links(&app.database, &records)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `trust-consumer`'s periodic live-table reload -- returns the full
/// current table, not a freshness timestamp (see `queries::list_stanox_crs`'s
/// own doc comment for why this route differs from every `last_*_fetch`
/// GET elsewhere in this file).
async fn get_stanox_crs(
    State(app): State<App>,
) -> Result<Json<Vec<common::StanoxCrsRecord>>, (StatusCode, String)> {
    let rows = queries::list_stanox_crs(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}

/// `crates/schedule-reference`'s per-line CIF SCHEDULE population publish
/// (POST, its own existing writer credential) and
/// `crates/full-coverage-consumer`'s reload (GET, a new credential) --
/// see `queries::{upsert,get}_schedule_line_population`. Unlike every
/// other GET in this file, this one returns the actual current row for one
/// `(line_id, service_date)`, not a freshness timestamp -- its real reader
/// needs the rows themselves, mirroring `/stanox-crs`'s shape (see
/// docs/superpowers/plans/2026-09-04-option-b-live-consumer-plan.md's
/// Correction 2).
#[derive(Debug, Deserialize)]
struct SchedulePopulationParams {
    line_id: String,
    service_date: chrono::NaiveDate,
}

/// `population` is a `Box<RawValue>`, not a `serde_json::Value`: serde_json
/// still validates that it is well-formed JSON while scanning the body (so
/// a malformed body gets exactly the same `Json` extractor rejection as
/// before), but keeps it as one contiguous string instead of building a
/// `Value` tree several times the size of a body that reaches 31 MB of JSON
/// text for the biggest line. The text is then bound straight into the
/// upsert as `$3::jsonb` -- see `queries::upsert_schedule_line_population`
/// for the OOM this fixed. The wire format is unchanged.
#[derive(Debug, Deserialize)]
struct SchedulePopulationBody {
    line_id: String,
    service_date: chrono::NaiveDate,
    population: Box<serde_json::value::RawValue>,
}

async fn post_schedule_line_population(
    State(app): State<App>,
    Json(body): Json<SchedulePopulationBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    queries::upsert_schedule_line_population(
        &app.database,
        &body.line_id,
        body.service_date,
        body.population.get(),
    )
    .await
    .map_err(internal_error)?;
    Ok(StatusCode::OK)
}

/// Prefix of `GET /private/schedule-line-population`'s `ETag` values:
/// `"slp-<updated_at as Unix microseconds>"`. The prefix only exists so a
/// validator minted by something else can never parse as one of ours.
const POPULATION_ETAG_PREFIX: &str = "slp-";

/// The `ETag` for a `schedule_line_population` row last changed at
/// `updated_at` -- see `queries::get_schedule_line_population_conditional`
/// for why `updated_at` is a sound content version.
fn population_etag(updated_at: chrono::DateTime<chrono::Utc>) -> String {
    format!(
        "\"{POPULATION_ETAG_PREFIX}{}\"",
        updated_at.timestamp_micros()
    )
}

/// What a request's `If-None-Match` asks about, in terms
/// `queries::get_schedule_line_population_conditional` understands.
#[derive(Debug, Default, PartialEq, Eq)]
struct PopulationIfNoneMatch {
    /// `If-None-Match: *` -- any current representation matches.
    any: bool,
    /// Every entity tag in the header(s) that parses as one of
    /// [`population_etag`]'s, decoded back to its `updated_at`. Anything
    /// else (another server's tag, garbage) is ignored, i.e. treated as
    /// "doesn't match", which is always safe: the client just gets a 200.
    versions: Vec<chrono::DateTime<chrono::Utc>>,
}

/// Parses `If-None-Match` (RFC 9110 §13.1.2: a comma-separated list, or
/// `*`, possibly split across several header lines). Weak comparison, as
/// that section requires for `If-None-Match`, so a `W/` prefix is ignored.
fn parse_population_if_none_match(headers: &axum::http::HeaderMap) -> PopulationIfNoneMatch {
    let mut parsed = PopulationIfNoneMatch::default();
    for value in headers.get_all(axum::http::header::IF_NONE_MATCH) {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for tag in value.split(',').map(str::trim) {
            if tag == "*" {
                parsed.any = true;
                continue;
            }
            let tag = tag.strip_prefix("W/").unwrap_or(tag);
            let Some(micros) = tag
                .strip_prefix('"')
                .and_then(|t| t.strip_suffix('"'))
                .and_then(|t| t.strip_prefix(POPULATION_ETAG_PREFIX))
                .and_then(|t| t.parse::<i64>().ok())
            else {
                continue;
            };
            if let Some(version) = chrono::DateTime::from_timestamp_micros(micros) {
                parsed.versions.push(version);
            }
        }
    }
    parsed
}

/// Returns the stored population as Postgres's JSON text, verbatim, with
/// `Content-Type: application/json` -- no `serde_json::Value` round-trip
/// (that decode was several times a 31 MB body, per request, under
/// `full-coverage-consumer`'s 450-675-GETs-per-2-minutes reload bursts).
/// The JSON is the same value as before; only whitespace and object key
/// order can differ, which no JSON reader (in particular
/// `full-coverage-consumer`'s `Vec<LinePopulationEntry>` deserialize)
/// depends on. A missing row is still `200 null`.
///
/// **Conditional GET (2026-09-26).** Every 200 carries an `ETag` derived
/// from the row's `updated_at`, and a request whose `If-None-Match`
/// matches it gets `304 Not Modified` with no body -- without Postgres
/// even decompressing the blob. `full-coverage-consumer` sends back the
/// last `ETag` it saw, so its every-300s reload of every line re-downloads
/// only the populations that actually changed. Both directions of version
/// skew are safe: an older consumer sends no `If-None-Match` and gets
/// exactly the 200 it always did (the extra header is ignored), and a newer
/// consumer against an older `api` gets 200s with no `ETag`, so it never
/// has a validator to send.
async fn get_schedule_line_population(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(params): axum::extract::Query<SchedulePopulationParams>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    use axum::http::header::{CONTENT_TYPE, ETAG};
    use axum::response::IntoResponse;

    let if_none_match = parse_population_if_none_match(&headers);
    let population = queries::get_schedule_line_population_conditional(
        &app.database,
        &params.line_id,
        params.service_date,
        if_none_match.any,
        &if_none_match.versions,
    )
    .await
    .map_err(internal_error)?;

    Ok(match population {
        None => ([(CONTENT_TYPE, "application/json")], "null").into_response(),
        Some(queries::ConditionalPopulation::NotModified { updated_at }) => (
            StatusCode::NOT_MODIFIED,
            [(ETAG, population_etag(updated_at))],
        )
            .into_response(),
        Some(queries::ConditionalPopulation::Modified {
            updated_at,
            population,
        }) => (
            [
                (CONTENT_TYPE, "application/json".to_string()),
                (ETAG, population_etag(updated_at)),
            ],
            population,
        )
            .into_response(),
    })
}

/// Query parameters shared by `/schedule-destination-departures` and
/// `/schedule-calling-points-full` -- both are whole-day publishes that
/// `schedule-reference` splits across several calls per service date
/// (`post_date_scoped_rows_in_chunks`, `PUBLISH_CHUNK_ROWS` rows each).
///
/// Two protocols, chosen by whether `publish_id` is present:
///
/// * **Diff protocol (`publish_id` present, 2026-09-26+).** Every chunk of
///   one publish carries the same `publish_id`; the final chunk also carries
///   `last_chunk=true` and `total_rows=<rows across all chunks>`. Each chunk
///   upserts its rows without rewriting unchanged ones; the final chunk
///   deletes the rows the publish did not carry. See
///   `queries::SchedulePublishPart` for the full protocol. `first_chunk` is
///   still sent (and used, to discard an abandoned earlier publish's staged
///   keys) because an OLDER `api` only understands `first_chunk` -- it
///   ignores the unknown new parameters and falls back to its own
///   delete-then-insert behavior, which is correct for the same sequence of
///   calls.
/// * **Legacy protocol (no `publish_id`).** Exactly the pre-2026-09-26
///   behavior, for an older `schedule-reference` during a rolling deploy:
///   `first_chunk=true` (the default when omitted, so an unaware caller gets
///   whole-day-replace rather than silently becoming insert-only) clears
///   the touched dates before inserting; `first_chunk=false` only inserts.
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct ScheduleChunkParams {
    #[serde(default = "default_true")]
    first_chunk: bool,
    publish_id: Option<String>,
    #[serde(default)]
    last_chunk: bool,
    total_rows: Option<u64>,
}

/// Longest `publish_id` accepted -- `schedule-reference`'s own ids are well
/// under this; the bound only stops a malformed caller staging arbitrarily
/// large keys.
const MAX_PUBLISH_ID_LEN: usize = 128;

/// Which publish protocol one `ScheduleChunkParams` selects -- see that
/// struct's doc comment.
enum ScheduleChunkMode<'a> {
    Legacy { first_chunk: bool },
    Diff(queries::SchedulePublishPart<'a>),
}

impl ScheduleChunkParams {
    fn mode(&self) -> Result<ScheduleChunkMode<'_>, (StatusCode, String)> {
        let Some(publish_id) = self.publish_id.as_deref() else {
            return Ok(ScheduleChunkMode::Legacy {
                first_chunk: self.first_chunk,
            });
        };
        if publish_id.is_empty() || publish_id.len() > MAX_PUBLISH_ID_LEN {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("publish_id must be 1-{MAX_PUBLISH_ID_LEN} characters"),
            ));
        }
        let final_total_rows = match (self.last_chunk, self.total_rows) {
            (true, Some(total)) => Some(total),
            (true, None) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "last_chunk=true requires total_rows".to_string(),
                ));
            }
            (false, _) => None,
        };
        Ok(ScheduleChunkMode::Diff(queries::SchedulePublishPart {
            publish_id,
            first_chunk: self.first_chunk,
            final_total_rows,
        }))
    }
}

/// `crates/schedule-reference`'s per-cycle batch of CIF-derived per-station
/// departures -- see `queries::upsert_schedule_network_departures`. POST
/// only: unlike `/schedule-line-population`, no service reads this table
/// back over HTTP -- `api` serves it straight off Postgres via
/// `routes::departures::get_station_schedule_departures`. See
/// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
/// Decision 1.
async fn post_schedule_network_departures(
    State(app): State<App>,
    Json(rows): Json<Vec<ScheduleNetworkDeparturesRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_schedule_network_departures(&app.database, &rows)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `crates/schedule-reference`'s per-DELIVERY batch of CIF-derived
/// per-DESTINATION departures -- the destination-keyed sibling of
/// `post_schedule_network_departures` directly above, and the write side of
/// the calling-point-first train search
/// (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
/// Approach B, as revised by
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md,
/// Approach C). POST only: no service reads this table back over HTTP --
/// `api` serves it straight off Postgres via
/// `routes::trains::get_trains_search`.
///
/// The body is FLAT -- one element per departure, ~377,000 of them, ~30MB
/// -- not one element per destination with an array inside it, split by
/// `schedule-reference` into several calls per service date. See
/// `ScheduleChunkParams` for the diff (`?publish_id=`) and legacy
/// (`?first_chunk=` only) chunk protocols this dispatches between; this
/// handler adds no logic of its own beyond that dispatch, deliberately.
async fn post_schedule_destination_departures(
    State(app): State<App>,
    axum::extract::Query(params): axum::extract::Query<ScheduleChunkParams>,
    Json(rows): Json<Vec<ScheduleDestinationDeparturesRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = match params.mode()? {
        ScheduleChunkMode::Legacy { first_chunk } => {
            queries::upsert_schedule_destination_departures_chunk(&app.database, &rows, first_chunk)
                .await
        }
        ScheduleChunkMode::Diff(part) => {
            queries::upsert_schedule_destination_departures_publish_part(&app.database, &rows, part)
                .await
        }
    }
    .map_err(schedule_publish_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// Dynamic Trip Planning Phase 2's whole-network, un-bucketed
/// calling-point publish -- POST-only, no GET pair, same shape as
/// `/schedule-destination-departures` and `/fixed-links` directly above,
/// reusing the same `schedule-reference` writer credential (see
/// `app.rs`'s route-group table). See
/// `queries::upsert_schedule_calling_points_full_publish_part` for the
/// diff-publish transaction shape, and `ScheduleChunkParams` for the
/// chunk protocols this route shares with
/// `post_schedule_destination_departures` directly above.
async fn post_schedule_calling_points_full(
    State(app): State<App>,
    axum::extract::Query(params): axum::extract::Query<ScheduleChunkParams>,
    Json(rows): Json<Vec<ScheduleCallingPointsFullRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = match params.mode()? {
        ScheduleChunkMode::Legacy { first_chunk } => {
            queries::upsert_schedule_calling_points_full_chunk(&app.database, &rows, first_chunk)
                .await
        }
        ScheduleChunkMode::Diff(part) => {
            queries::upsert_schedule_calling_points_full_publish_part(&app.database, &rows, part)
                .await
        }
    }
    .map_err(schedule_publish_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `full-coverage-consumer`'s own periodic snapshot write/read-back --
/// unlike `/schedule-line-population`, both methods here share the SAME
/// group (`internal_oauth_group_full_coverage`), matching `/incidents`'s
/// "one producer, one group, both methods" shape rather than
/// `/stanox-crs`'s split, since this GET is only ever this producer
/// re-checking its own last write, not a second, different caller (see
/// Correction 2).
async fn post_full_coverage_stats(
    State(app): State<App>,
    Json(rows): Json<Vec<common::FullCoverageLineStatsRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_full_coverage_line_stats(&app.database, &rows)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn get_full_coverage_stats_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_full_coverage_line_stats_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

/// `poller-irish-rail-gtfs`'s per-poll-cycle station/line catalogue batch --
/// see `crate::data::island_of_ireland::{upsert_stations,upsert_lines}`.
/// Tier A of docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md.
async fn get_island_of_ireland_stations_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = crate::data::island_of_ireland::last_stations_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_island_of_ireland_stations(
    State(app): State<App>,
    Json(stations): Json<Vec<common::island_of_ireland::IslandOfIrelandStation>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = crate::data::island_of_ireland::upsert_stations(&app.database, &stations)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn get_island_of_ireland_lines_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = crate::data::island_of_ireland::last_lines_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_island_of_ireland_lines(
    State(app): State<App>,
    Json(lines): Json<Vec<common::island_of_ireland::IslandOfIrelandLineDefinition>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = crate::data::island_of_ireland::upsert_lines(&app.database, &lines)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `poller-irish-rail-live`'s per-poll-cycle raw departure-board batch --
/// see `crate::data::island_of_ireland::upsert_station_samples`. Tier B of
/// docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md.
async fn get_island_of_ireland_station_samples_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = crate::data::island_of_ireland::last_station_samples_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_island_of_ireland_station_samples(
    State(app): State<App>,
    Json(samples): Json<Vec<common::island_of_ireland::IslandOfIrelandStationSample>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = crate::data::island_of_ireland::upsert_station_samples(&app.database, &samples)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// Error mapping for the two chunked schedule publish routes -- see
/// `queries::finish_publish_part`. Both non-500 cases mean "the final
/// chunk's delete did not run, nothing was deleted, try the whole publish
/// again LATER"; `schedule-reference` does not retry either within a cycle:
///
/// * 409 Conflict -- another final chunk of the same product is still
///   deleting (`queries::SchedulePublishBusy`).
/// * 503 Service Unavailable -- the delete phase hit
///   `PUBLISH_DELETE_STATEMENT_TIMEOUT` (SQLSTATE 57014) and rolled back.
fn schedule_publish_error(err: anyhow::Error) -> (StatusCode, String) {
    if let Some(busy) = err.downcast_ref::<queries::SchedulePublishBusy>() {
        tracing::warn!(error = %busy, "schedule publish final chunk refused: delete already running");
        return (StatusCode::CONFLICT, busy.to_string());
    }
    if queries::is_statement_timeout(&err) {
        tracing::error!(error = ?err, "schedule publish final chunk hit its statement timeout; rolled back");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "schedule publish delete timed out and was rolled back".to_string(),
        );
    }
    internal_error(err)
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "ingestion upsert failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "ingestion failed".to_string(),
    )
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    const FIXTURE_LINE_ID: &str = "ZTEST";

    /// Copied from `routes::station_stats::db_tests::test_app` (that
    /// module's own doc comment: colocated per-file rather than shared,
    /// until a third file needs it too). Every field an inert placeholder
    /// except `database`, which the caller supplies -- these tests touch
    /// nothing else on `App`.
    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            metrics_port: 9091,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
            past_travel_retention_days: 548,
            stale_push_subscription_days: 365,
            inactive_account_retention_days: 0,
        };

        std::sync::Arc::new(AppState {
            // Built from the same catalogue the real `AppState::init`
            // builds it from, so a test never gets a matcher that
            // disagrees with its own `config.lines`.
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            config,
            database: pool,
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn delete_fixture(pool: &PgPool, crs: &str, operator: &str) {
        sqlx::query("DELETE FROM station_full_coverage_samples WHERE crs = $1 AND operator = $2")
            .bind(crs)
            .bind(operator)
            .execute(pool)
            .await
            .expect("cleanup fixture station_full_coverage_samples row");
    }

    /// Distinct name from `delete_fixture` above -- same "reserved
    /// fixture namespace" spirit, applied to `schedule_line_population`'s
    /// own `line_id` key instead of a (crs, operator) pair.
    /// `get_schedule_line_population` returns JSON text; compare it as JSON
    /// (Postgres's jsonb rendering normalises whitespace and key order).
    fn parse_population(text: Option<String>) -> Option<Value> {
        text.map(|t| serde_json::from_str(&t).expect("stored population is valid JSON"))
    }

    async fn delete_population_fixture(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM schedule_line_population WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_line_population rows");
    }

    fn sample_body(crs: &str, operator: &str, resolved_at: chrono::DateTime<chrono::Utc>) -> Value {
        json!([{
            "crs": crs,
            "operator": operator,
            "resolved_at": resolved_at,
            "stats": {
                "total": 10, "delayed": 2, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 3.5
            }
        }])
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_full_coverage_samples -- --ignored --test-threads=1`"]
    async fn station_full_coverage_samples_post_one_row_upserts_and_lands_with_the_right_shape() {
        let pool = connect().await;
        delete_fixture(&pool, "ZFA", "ZA").await;

        let resolved_at = chrono::Utc::now();
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/station-full-coverage-samples")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&sample_body("ZFA", "ZA", resolved_at)).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"upserted": 1}));

        let stats: serde_json::Value = sqlx::query_scalar(
            "SELECT stats FROM station_full_coverage_samples WHERE crs = 'ZFA' AND operator = 'ZA'",
        )
        .fetch_one(&pool)
        .await
        .expect("row landed");
        assert_eq!(
            stats,
            serde_json::json!({
                "total": 10, "delayed": 2, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 3.5
            })
        );

        delete_fixture(&pool, "ZFA", "ZA").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_full_coverage_samples -- --ignored --test-threads=1`"]
    async fn station_full_coverage_samples_repeat_post_updates_in_place_not_duplicated() {
        let pool = connect().await;
        delete_fixture(&pool, "ZFB", "ZB").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        let first_resolved_at = chrono::Utc::now();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/station-full-coverage-samples")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&sample_body("ZFB", "ZB", first_resolved_at)).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let second_resolved_at = first_resolved_at + chrono::Duration::minutes(1);
        let second_body = json!([{
            "crs": "ZFB",
            "operator": "ZB",
            "resolved_at": second_resolved_at,
            "stats": {
                "total": 20, "delayed": 5, "cancelled": 0, "skipped": 1, "avg_delay_minutes": 1.0
            }
        }]);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/station-full-coverage-samples")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&second_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT crs, operator FROM station_full_coverage_samples WHERE crs = 'ZFB' AND operator = 'ZB'",
        )
        .fetch_all(&pool)
        .await
        .expect("query rows");
        assert_eq!(
            rows.len(),
            1,
            "row should be updated in place, not duplicated"
        );

        let stats: serde_json::Value = sqlx::query_scalar(
            "SELECT stats FROM station_full_coverage_samples WHERE crs = 'ZFB' AND operator = 'ZB'",
        )
        .fetch_one(&pool)
        .await
        .expect("row present");
        assert_eq!(
            stats,
            serde_json::json!({
                "total": 20, "delayed": 5, "cancelled": 0, "skipped": 1, "avg_delay_minutes": 1.0
            })
        );

        delete_fixture(&pool, "ZFB", "ZB").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_full_coverage_samples -- --ignored --test-threads=1`"]
    async fn station_full_coverage_samples_get_last_fetched_after_seeding_is_not_null() {
        let pool = connect().await;
        delete_fixture(&pool, "ZFC", "ZC").await;

        let resolved_at = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO station_full_coverage_samples (crs, operator, resolved_at, stats) \
             VALUES ('ZFC', 'ZC', $1, '{\"total\":1,\"delayed\":0,\"cancelled\":0,\"skipped\":0,\"avg_delay_minutes\":0.0}')",
        )
        .bind(resolved_at)
        .execute(&pool)
        .await
        .expect("seed fixture row");

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/station-full-coverage-samples")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let fetched_at = json["fetchedAt"]
            .as_str()
            .expect("fetchedAt should be a non-null timestamp string");
        let fetched_at: chrono::DateTime<chrono::Utc> = fetched_at.parse().unwrap();
        assert!(
            (fetched_at - resolved_at).num_seconds().abs() < 5,
            "fetchedAt {fetched_at} should be close to the seeded resolved_at {resolved_at}"
        );

        delete_fixture(&pool, "ZFC", "ZC").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_full_coverage_samples -- --ignored --test-threads=1`"]
    async fn station_full_coverage_samples_get_last_fetched_on_an_empty_table_is_null() {
        // No fixture row is seeded by this test at all, on either the CRS
        // this test uses or otherwise -- `last_station_full_coverage_samples_fetch`
        // is a bare `MAX(resolved_at)` over the whole table (unlike every
        // other query in this module, it isn't scoped by CRS), so this
        // assertion relies on the plan's own binding Non-goal that no real
        // producer writes any row into this table yet (see the plan's
        // Non-goals section) -- in any test/CI database this table is
        // therefore expected to be genuinely empty, not forced empty by a
        // destructive TRUNCATE against a real deployment's table.
        let pool = connect().await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/station-full-coverage-samples")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"fetchedAt": null}));
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn post_then_get_round_trips_the_exact_population_json() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;

        let service_date: chrono::NaiveDate = "2026-09-04".parse().unwrap();
        let population = serde_json::json!([
            {"uid": "C11052", "calling_points": []},
        ]);
        queries::upsert_schedule_line_population(
            &pool,
            FIXTURE_LINE_ID,
            service_date,
            &population.to_string(),
        )
        .await
        .expect("seed population");

        let fetched = queries::get_schedule_line_population(&pool, FIXTURE_LINE_ID, service_date)
            .await
            .expect("fetch population");
        assert_eq!(parse_population(fetched), Some(population));

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn a_second_post_for_the_same_key_wholesale_replaces_not_merges() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;

        let service_date: chrono::NaiveDate = "2026-09-04".parse().unwrap();
        let first = serde_json::json!([{"uid": "C11052", "calling_points": []}]);
        let second = serde_json::json!([{"uid": "C99999", "calling_points": []}]);

        queries::upsert_schedule_line_population(
            &pool,
            FIXTURE_LINE_ID,
            service_date,
            &first.to_string(),
        )
        .await
        .expect("seed first population");
        queries::upsert_schedule_line_population(
            &pool,
            FIXTURE_LINE_ID,
            service_date,
            &second.to_string(),
        )
        .await
        .expect("seed second population");

        let rows: Vec<(serde_json::Value,)> = sqlx::query_as(
            "SELECT population FROM schedule_line_population WHERE line_id = $1 AND service_date = $2",
        )
        .bind(FIXTURE_LINE_ID)
        .bind(service_date)
        .fetch_all(&pool)
        .await
        .expect("select fixture rows");

        assert_eq!(rows.len(), 1, "wholesale replace, not a second row");
        assert_eq!(rows[0].0, second);

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    /// Guards the `IS DISTINCT FROM` in `upsert_schedule_line_population`:
    /// republishing an equal population must not write a new row version
    /// (a whole new ~0.5 MB TOAST copy per line per day in production),
    /// while a changed one still replaces it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn an_identical_republish_leaves_the_row_untouched_but_a_change_still_writes() {
        async fn row_version(pool: &PgPool, service_date: chrono::NaiveDate) -> (String, String) {
            sqlx::query_as(
                "SELECT xmin::text, updated_at::text FROM schedule_line_population \
                 WHERE line_id = $1 AND service_date = $2",
            )
            .bind(FIXTURE_LINE_ID)
            .bind(service_date)
            .fetch_one(pool)
            .await
            .expect("read fixture row version")
        }

        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;

        let service_date: chrono::NaiveDate = "2026-09-04".parse().unwrap();
        let original = serde_json::json!([{"uid": "C11052", "calling_points": []}]);
        queries::upsert_schedule_line_population(
            &pool,
            FIXTURE_LINE_ID,
            service_date,
            &original.to_string(),
        )
        .await
        .expect("seed population");
        let (xmin_before, updated_at_before) = row_version(&pool, service_date).await;

        // Same content with the keys in a different order: jsonb equality,
        // not text equality, decides "unchanged".
        let reordered = serde_json::json!([{"calling_points": [], "uid": "C11052"}]);
        queries::upsert_schedule_line_population(
            &pool,
            FIXTURE_LINE_ID,
            service_date,
            &reordered.to_string(),
        )
        .await
        .expect("republish identical population");
        let (xmin_after_same, updated_at_after_same) = row_version(&pool, service_date).await;
        assert_eq!(
            xmin_after_same, xmin_before,
            "an identical republish must not write a new row version"
        );
        assert_eq!(updated_at_after_same, updated_at_before);

        let changed = serde_json::json!([{"uid": "C99999", "calling_points": []}]);
        queries::upsert_schedule_line_population(
            &pool,
            FIXTURE_LINE_ID,
            service_date,
            &changed.to_string(),
        )
        .await
        .expect("publish changed population");
        let (xmin_after_change, _) = row_version(&pool, service_date).await;
        assert_ne!(
            xmin_after_change, xmin_before,
            "a changed population must still be written"
        );
        let fetched = queries::get_schedule_line_population(&pool, FIXTURE_LINE_ID, service_date)
            .await
            .expect("fetch population");
        assert_eq!(parse_population(fetched), Some(changed));

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn get_for_a_key_never_posted_is_none_not_an_error() {
        let pool = connect().await;
        delete_population_fixture(&pool, "ZNEVER").await;

        let service_date: chrono::NaiveDate = "2026-09-04".parse().unwrap();
        let fetched = queries::get_schedule_line_population(&pool, "ZNEVER", service_date)
            .await
            .expect("query should succeed even with no row");
        assert_eq!(fetched, None);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn http_post_then_get_round_trip_through_the_router() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        let post_body = serde_json::json!({
            "line_id": FIXTURE_LINE_ID,
            "service_date": "2026-09-04",
            "population": [{"uid": "C11052", "calling_points": []}],
        });
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/schedule-line-population")
                    .header("content-type", "application/json")
                    .body(Body::from(post_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/schedule-line-population?line_id={FIXTURE_LINE_ID}&service_date=2026-09-04"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{"uid": "C11052", "calling_points": []}])
        );

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    /// A multi-MB population shaped like a real one (thousands of entries,
    /// a dozen-plus calling points each, every field type the real
    /// `LinePopulationEntry` carries).
    fn large_population(entries: usize) -> Value {
        Value::Array(
            (0..entries)
                .map(|i| {
                    json!({
                        "uid": format!("L{i:05}"),
                        "calling_points": (0..16).map(|j| json!({
                            "tiploc": format!("TPL{j:03}"),
                            "kind": if j == 0 { "Origin" } else if j == 15 { "Terminate" } else { "Intermediate" },
                            "booked_arrival": if j == 0 { Value::Null } else { json!("08:15:30") },
                            "booked_departure": if j == 15 { Value::Null } else { json!("08:16:00") },
                            "is_half_minute_arrival": j % 2 == 0,
                            "is_half_minute_departure": false,
                            "day_offset": 0,
                            "activity": "T ",
                            "public_arrival": null,
                            "public_departure": "08:16:00",
                            "unicode": "Kings Cross \u{00e9}\u{2014}\"quoted\"\\",
                            "float": 1.5,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect(),
        )
    }

    /// With the same 100 MB body limit `routes::private_router` layers over
    /// the real `/private/*` routes (axum's own 2 MB default would 413 the
    /// multi-MB fixture).
    fn population_router(pool: &PgPool) -> axum::Router {
        crate::app::Router::new()
            .merge(router())
            .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
            .with_state(test_app(pool.clone()))
    }

    async fn post_population(
        router: &axum::Router,
        line_id: &str,
        population: &Value,
    ) -> StatusCode {
        let body = json!({
            "line_id": line_id,
            "service_date": "2026-09-04",
            "population": population,
        });
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/schedule-line-population")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    /// `(status, etag, content-type, body)` of one GET, optionally with an
    /// `If-None-Match`.
    async fn get_population(
        router: &axum::Router,
        line_id: &str,
        if_none_match: Option<&str>,
    ) -> (StatusCode, Option<String>, Option<String>, Vec<u8>) {
        let mut request = Request::builder().uri(format!(
            "/schedule-line-population?line_id={line_id}&service_date=2026-09-04"
        ));
        if let Some(tag) = if_none_match {
            request = request.header("if-none-match", tag);
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let header = |name| {
            response
                .headers()
                .get(name)
                .map(|v: &axum::http::HeaderValue| v.to_str().unwrap().to_string())
        };
        let etag = header(axum::http::header::ETAG);
        let content_type = header(axum::http::header::CONTENT_TYPE);
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, etag, content_type, body)
    }

    /// The memory fix must not change what is stored or served: a
    /// multi-MB population POSTed through the `RawValue`/`$3::jsonb` path
    /// is jsonb-equal to the same population bound the OLD way (as a
    /// `serde_json::Value`), the GET body parses back to the identical
    /// value, and re-POSTing the GET body verbatim is a no-op.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn a_multi_mb_population_round_trips_post_and_get_unchanged() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
        let router = population_router(&pool);

        let population = large_population(6000);
        let text_len = population.to_string().len();
        assert!(text_len > 5_000_000, "fixture is only {text_len} bytes");

        assert_eq!(
            post_population(&router, FIXTURE_LINE_ID, &population).await,
            StatusCode::OK
        );

        // Equal (jsonb equality) to what binding a `Value` -- the old
        // code path -- stores.
        let (equal_to_value_bind, xmin_before): (bool, String) = sqlx::query_as(
            "SELECT population = $3, xmin::text FROM schedule_line_population \
             WHERE line_id = $1 AND service_date = $2",
        )
        .bind(FIXTURE_LINE_ID)
        .bind(chrono::NaiveDate::from_ymd_opt(2026, 9, 4).unwrap())
        .bind(&population)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(equal_to_value_bind);

        let (status, etag, content_type, body) =
            get_population(&router, FIXTURE_LINE_ID, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.as_deref(), Some("application/json"));
        assert!(etag.is_some());
        let served: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(served, population);
        // And it still deserializes into the consumer's own wire type.
        let _: Vec<schedule_query::LinePopulationEntry> = serde_json::from_slice(&body).unwrap();

        // Re-publishing the served text verbatim changes nothing.
        let served_raw: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            post_population(&router, FIXTURE_LINE_ID, &served_raw).await,
            StatusCode::OK
        );
        let (xmin_after,): (String,) =
            sqlx::query_as("SELECT xmin::text FROM schedule_line_population WHERE line_id = $1")
                .bind(FIXTURE_LINE_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            xmin_after, xmin_before,
            "identical republish must be a no-op"
        );
        let (_, etag_after, _, _) = get_population(&router, FIXTURE_LINE_ID, None).await;
        assert_eq!(etag_after, etag, "an unchanged population keeps its ETag");

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    /// Conditional GET, both compatibility directions on the api side: a
    /// client with no `If-None-Match` (every consumer predating this
    /// change) gets the full 200 exactly as before; a client echoing the
    /// `ETag` gets a bodyless 304 until the population changes, then a 200
    /// with a new `ETag`. Foreign/garbage validators are ignored (200).
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn get_honours_if_none_match_and_still_serves_clients_without_it() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
        let router = population_router(&pool);

        // Unpublished: still `200 null`, with or without a validator.
        for tag in [None, Some("\"slp-1\""), Some("*")] {
            let (status, etag, _, body) = get_population(&router, FIXTURE_LINE_ID, tag).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(etag, None);
            assert_eq!(body, b"null");
        }

        let first = large_population(50);
        post_population(&router, FIXTURE_LINE_ID, &first).await;

        let (status, etag, _, body) = get_population(&router, FIXTURE_LINE_ID, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), first);
        let etag = etag.expect("a 200 for a published row carries an ETag");

        for matching in [
            etag.clone(),
            format!("W/{etag}"),
            format!("\"other\", {etag}"),
            "*".to_string(),
        ] {
            let (status, echoed, _, body) =
                get_population(&router, FIXTURE_LINE_ID, Some(&matching)).await;
            assert_eq!(
                status,
                StatusCode::NOT_MODIFIED,
                "If-None-Match: {matching}"
            );
            assert_eq!(echoed.as_deref(), Some(etag.as_str()));
            assert!(body.is_empty());
        }
        for not_matching in ["\"slp-1\"", "\"something-else\"", "garbage"] {
            let (status, _, _, body) =
                get_population(&router, FIXTURE_LINE_ID, Some(not_matching)).await;
            assert_eq!(status, StatusCode::OK, "If-None-Match: {not_matching}");
            assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), first);
        }

        // A changed population invalidates the old validator.
        let second = large_population(51);
        post_population(&router, FIXTURE_LINE_ID, &second).await;
        let (status, new_etag, _, body) =
            get_population(&router, FIXTURE_LINE_ID, Some(&etag)).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(new_etag.as_deref(), Some(etag.as_str()));
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), second);

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    /// The POST keeps the old request-format validation: malformed JSON,
    /// a missing `population`, and a bad date are still rejected by the
    /// `Json` extractor, and nothing is written.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn post_still_rejects_malformed_bodies() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
        let router = population_router(&pool);

        let cases = [
            (
                r#"{"line_id": "ZTEST", "service_date": "2026-09-04", "population": [{"uid": }]}"#,
                StatusCode::BAD_REQUEST,
            ),
            (
                r#"{"line_id": "ZTEST", "service_date": "2026-09-04"}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                r#"{"line_id": "ZTEST", "service_date": "not-a-date", "population": []}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
        ];
        for (body, expected) in cases {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/schedule-line-population")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "body: {body}");
        }
        let (count,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM schedule_line_population WHERE line_id = $1")
                .bind(FIXTURE_LINE_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }

    /// `get_schedule_line_population_entries`' in-SQL uid filter keeps the
    /// published order and matches the old in-Rust filter.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn population_entries_filter_by_uid_in_sql_preserving_order() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
        let population = large_population(5);
        let mut population_array = population.as_array().unwrap().clone();
        // A second, later entry for the same uid: both must come back, in order.
        let mut dup = population_array[1].clone();
        dup["calling_points"] = json!([]);
        population_array.push(dup);
        let population = Value::Array(population_array);
        queries::upsert_schedule_line_population(
            &pool,
            FIXTURE_LINE_ID,
            date,
            &population.to_string(),
        )
        .await
        .unwrap();

        let all: Vec<schedule_query::LinePopulationEntry> =
            serde_json::from_value(population.clone()).unwrap();
        let fetched_all =
            queries::get_schedule_line_population_entries(&pool, FIXTURE_LINE_ID, date, None)
                .await
                .unwrap();
        assert_eq!(fetched_all.as_ref(), Some(&all));

        let only = queries::get_schedule_line_population_entries(
            &pool,
            FIXTURE_LINE_ID,
            date,
            Some("L00001"),
        )
        .await
        .unwrap()
        .unwrap();
        let expected: Vec<_> = all.iter().filter(|e| e.uid == "L00001").cloned().collect();
        assert_eq!(expected.len(), 2);
        assert_eq!(only, expected);

        let none = queries::get_schedule_line_population_entries(
            &pool,
            FIXTURE_LINE_ID,
            date,
            Some("NOPE"),
        )
        .await
        .unwrap();
        assert_eq!(none, Some(vec![]));
        let missing =
            queries::get_schedule_line_population_entries(&pool, "ZNEVER", date, Some("L00001"))
                .await
                .unwrap();
        assert_eq!(missing, None);

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[test]
    fn if_none_match_parsing_accepts_lists_weak_tags_and_star_and_ignores_the_rest() {
        let mut headers = axum::http::HeaderMap::new();
        let version = chrono::DateTime::from_timestamp_micros(1_790_000_000_123_456).unwrap();
        let etag = population_etag(version);
        assert_eq!(etag, "\"slp-1790000000123456\"");
        headers.append(
            axum::http::header::IF_NONE_MATCH,
            format!("\"foreign\", W/{etag}, garbage").parse().unwrap(),
        );
        headers.append(axum::http::header::IF_NONE_MATCH, "*".parse().unwrap());
        assert_eq!(
            parse_population_if_none_match(&headers),
            PopulationIfNoneMatch {
                any: true,
                versions: vec![version],
            }
        );
        assert_eq!(
            parse_population_if_none_match(&axum::http::HeaderMap::new()),
            PopulationIfNoneMatch::default()
        );
    }

    async fn delete_full_coverage_fixture(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM full_coverage_line_stats WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture full_coverage_line_stats row");
    }

    fn fixture_row(line_id: &str, availability: &str) -> common::FullCoverageLineStatsRow {
        common::FullCoverageLineStatsRow {
            line_id: line_id.to_string(),
            service_date: "2026-09-04".parse().unwrap(),
            availability: availability.to_string(),
            stats: common::SampleStats {
                total: 10,
                delayed: 2,
                cancelled: 1,
                skipped: 0,
                avg_delay_minutes: 3.5,
            },
            partial: false,
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_line_stats -- --ignored --test-threads=1`"]
    async fn post_then_last_fetch_is_non_null_and_recent() {
        let pool = connect().await;
        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;

        let before = queries::last_full_coverage_line_stats_fetch(&pool)
            .await
            .expect("query last fetch");

        queries::upsert_full_coverage_line_stats(&pool, &[fixture_row(FIXTURE_LINE_ID, "pending")])
            .await
            .expect("seed full_coverage_line_stats row");

        let after = queries::last_full_coverage_line_stats_fetch(&pool)
            .await
            .expect("query last fetch")
            .expect("a row now exists");
        if let Some(before) = before {
            assert!(after >= before);
        }
        assert!(chrono::Utc::now() - after < chrono::Duration::minutes(1));

        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    /// Since 2026-09-27 the table keeps one row per line PER DAY: the next
    /// day's row is added beside the previous day's, not over it, and the
    /// readers take the date.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_line_stats -- --ignored --test-threads=1`"]
    async fn full_coverage_line_stats_keeps_a_row_per_day_and_reads_by_date() {
        let pool = connect().await;
        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;
        let day1: chrono::NaiveDate = "2026-09-04".parse().unwrap();
        let day2: chrono::NaiveDate = "2026-09-05".parse().unwrap();

        let closed = fixture_row(FIXTURE_LINE_ID, "available");
        let mut today = fixture_row(FIXTURE_LINE_ID, "pending");
        today.service_date = day2;
        today.partial = true;
        today.stats.cancelled = 0;
        queries::upsert_full_coverage_line_stats(&pool, &[closed.clone(), today.clone()])
            .await
            .expect("upsert two days");

        let latest = queries::get_full_coverage_line_stats(&pool, FIXTURE_LINE_ID, None)
            .await
            .unwrap()
            .expect("a row");
        assert_eq!(latest.service_date, day2, "no date: the most recent day");
        assert!(latest.partial, "partial round-trips");
        assert_eq!(latest.availability, "pending");

        let first = queries::get_full_coverage_line_stats(&pool, FIXTURE_LINE_ID, Some(day1))
            .await
            .unwrap()
            .expect("the closed day survives the next day's write");
        assert_eq!(first.availability, "available");
        assert!(!first.partial);
        assert_eq!(first.stats.cancelled, 1);

        let history =
            queries::full_coverage_line_stats_for_range(&pool, FIXTURE_LINE_ID, day1, day2)
                .await
                .unwrap();
        assert_eq!(
            history.iter().map(|r| r.service_date).collect::<Vec<_>>(),
            vec![day1, day2]
        );
        assert!(
            queries::get_full_coverage_line_stats(
                &pool,
                FIXTURE_LINE_ID,
                Some("2026-09-06".parse().unwrap())
            )
            .await
            .unwrap()
            .is_none()
        );

        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    /// DB review F3: the consumer re-posts every line every minute; an
    /// identical row must not be rewritten (no dead tuple, `updated_at`
    /// untouched), while a real change still is.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_line_stats -- --ignored --test-threads=1`"]
    async fn an_unchanged_full_coverage_row_is_not_rewritten() {
        let pool = connect().await;
        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;
        let row = fixture_row(FIXTURE_LINE_ID, "pending");
        assert_eq!(
            queries::upsert_full_coverage_line_stats(&pool, std::slice::from_ref(&row))
                .await
                .unwrap(),
            1
        );
        let updated_at = || async {
            sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
                "SELECT updated_at FROM full_coverage_line_stats WHERE line_id = $1",
            )
            .bind(FIXTURE_LINE_ID)
            .fetch_one(&pool)
            .await
            .unwrap()
        };
        let first = updated_at().await;

        assert_eq!(
            queries::upsert_full_coverage_line_stats(&pool, std::slice::from_ref(&row))
                .await
                .unwrap(),
            0,
            "an identical post writes nothing"
        );
        assert_eq!(updated_at().await, first);

        let mut changed = row.clone();
        changed.stats.delayed += 1;
        assert_eq!(
            queries::upsert_full_coverage_line_stats(&pool, &[changed])
                .await
                .unwrap(),
            1
        );
        let mut now_partial = row;
        now_partial.stats.delayed += 1;
        now_partial.partial = true;
        assert_eq!(
            queries::upsert_full_coverage_line_stats(&pool, &[now_partial])
                .await
                .unwrap(),
            1,
            "a change of the partial flag alone is a change"
        );

        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_line_stats -- --ignored --test-threads=1`"]
    async fn a_second_post_for_the_same_line_and_day_updates_the_row_in_place() {
        let pool = connect().await;
        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;

        queries::upsert_full_coverage_line_stats(&pool, &[fixture_row(FIXTURE_LINE_ID, "pending")])
            .await
            .expect("seed first row");
        queries::upsert_full_coverage_line_stats(
            &pool,
            &[fixture_row(FIXTURE_LINE_ID, "available")],
        )
        .await
        .expect("seed second row");

        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT availability FROM full_coverage_line_stats WHERE line_id = $1")
                .bind(FIXTURE_LINE_ID)
                .fetch_all(&pool)
                .await
                .expect("select fixture rows");

        assert_eq!(rows.len(), 1, "wholesale replace, not a second row");
        assert_eq!(rows[0].0, "available");

        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_line_stats -- --ignored --test-threads=1`"]
    async fn last_fetch_against_an_empty_table_is_null() {
        let pool = connect().await;
        sqlx::query("DELETE FROM full_coverage_line_stats")
            .execute(&pool)
            .await
            .expect("clear the whole table for this test");

        let fetched_at = queries::last_full_coverage_line_stats_fetch(&pool)
            .await
            .expect("query should succeed against an empty table");
        assert_eq!(fetched_at, None);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                post_stanox_crs -- --ignored --test-threads=1`"]
    async fn a_second_post_stanox_crs_prunes_stanoxes_absent_from_the_new_delivery() {
        // End-to-end proof of the Signal Box Audit Low finding this closes:
        // a real delivery cycle (one POST = one whole delivery's STANOX set,
        // per `upsert_stanox_crs`'s own doc comment) that stops mentioning a
        // previously-published STANOX must remove it, not leave it to
        // accumulate forever.
        let pool = connect().await;
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-ROUTE-PRUNE-%'")
            .execute(&pool)
            .await
            .ok();

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        let first_body = json!([
            {
                "stanox": "TEST-ROUTE-PRUNE-KEEP",
                "crs": "EUS",
                "tiploc": "TEST-RP-EUSTON",
                "station_name": "EUSTON",
                "source_sequence": 1,
                "change_time_minutes": null,
            },
            {
                "stanox": "TEST-ROUTE-PRUNE-STALE",
                "crs": "CRE",
                "tiploc": "TEST-RP-CREWE",
                "station_name": "CREWE",
                "source_sequence": 1,
                "change_time_minutes": null,
            },
        ]);
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/stanox-crs")
                    .header("content-type", "application/json")
                    .body(Body::from(first_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let after_first: Vec<(String,)> =
            sqlx::query_as("SELECT stanox FROM stanox_crs WHERE stanox LIKE 'TEST-ROUTE-PRUNE-%'")
                .fetch_all(&pool)
                .await
                .expect("read back after first delivery");
        assert_eq!(
            after_first.len(),
            2,
            "both rows land from the first delivery"
        );

        // The next delivery no longer mentions TEST-ROUTE-PRUNE-STALE at
        // all (e.g. a decommissioned STANOX).
        let second_body = json!([
            {
                "stanox": "TEST-ROUTE-PRUNE-KEEP",
                "crs": "EUS",
                "tiploc": "TEST-RP-EUSTON",
                "station_name": "EUSTON",
                "source_sequence": 2,
                "change_time_minutes": null,
            },
        ]);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/stanox-crs")
                    .header("content-type", "application/json")
                    .body(Body::from(second_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let after_second: Vec<(String,)> =
            sqlx::query_as("SELECT stanox FROM stanox_crs WHERE stanox LIKE 'TEST-ROUTE-PRUNE-%'")
                .fetch_all(&pool)
                .await
                .expect("read back after second delivery");
        assert_eq!(
            after_second.len(),
            1,
            "the STANOX the second delivery stopped mentioning must be pruned"
        );
        assert_eq!(after_second[0].0, "TEST-ROUTE-PRUNE-KEEP");

        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-ROUTE-PRUNE-%'")
            .execute(&pool)
            .await
            .ok();
    }

    async fn delete_network_departures_fixture(pool: &PgPool, crs: &str) {
        sqlx::query("DELETE FROM schedule_network_departures WHERE crs = $1")
            .bind(crs)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_network_departures rows");
    }

    fn network_departures_body(crs: &str, service_date: &str) -> Value {
        json!([{
            "crs": crs,
            "service_date": service_date,
            "departures": [{"uid": "C11052", "scheduled": "08:22:00", "destination_crs": "CRE"}],
        }])
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_network_departures -- --ignored --test-threads=1`"]
    async fn post_schedule_network_departures_upserts_the_row() {
        let pool = connect().await;
        delete_network_departures_fixture(&pool, "ZQV").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/schedule-network-departures")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        network_departures_body("ZQV", "2026-09-04").to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"upserted": 1}));

        let departures: serde_json::Value = sqlx::query_scalar(
            "SELECT departures FROM schedule_network_departures WHERE crs = 'ZQV' AND service_date = '2026-09-04'",
        )
        .fetch_one(&pool)
        .await
        .expect("row landed");
        assert_eq!(departures[0]["uid"], "C11052");

        delete_network_departures_fixture(&pool, "ZQV").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_network_departures -- --ignored --test-threads=1`"]
    async fn a_second_network_departures_post_for_the_same_key_wholesale_replaces_not_merges() {
        let pool = connect().await;
        delete_network_departures_fixture(&pool, "ZQW").await;

        queries::upsert_schedule_network_departures(
            &pool,
            &[ScheduleNetworkDeparturesRow {
                crs: "ZQW".to_string(),
                service_date: "2026-09-04".parse().unwrap(),
                departures: serde_json::json!([{"uid": "C11052", "scheduled": "08:22:00", "destination_crs": "CRE"}]),
            }],
        )
        .await
        .expect("seed first row");
        queries::upsert_schedule_network_departures(
            &pool,
            &[ScheduleNetworkDeparturesRow {
                crs: "ZQW".to_string(),
                service_date: "2026-09-04".parse().unwrap(),
                departures: serde_json::json!([{"uid": "C99999", "scheduled": "09:00:00", "destination_crs": null}]),
            }],
        )
        .await
        .expect("seed second row");

        let rows: Vec<(serde_json::Value,)> = sqlx::query_as(
            "SELECT departures FROM schedule_network_departures WHERE crs = 'ZQW' AND service_date = '2026-09-04'",
        )
        .fetch_all(&pool)
        .await
        .expect("select fixture rows");
        assert_eq!(rows.len(), 1, "wholesale replace, not a second row");
        assert_eq!(rows[0].0[0]["uid"], "C99999");

        delete_network_departures_fixture(&pool, "ZQW").await;
    }

    /// Day-scoped, like the upsert itself -- the unit of replacement for
    /// this table is a whole `service_date`, not one destination's rows.
    /// The fixture date is in 2099 for the same reason Task 5's are: it
    /// must not collide with a real published day in a shared development
    /// database. See `queries::schedule_destination_departures_query_tests`'
    /// own module doc comment.
    async fn delete_destination_departures_fixture(pool: &PgPool, service_date: &str) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1::date")
            .bind(service_date)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_destination_departures rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn post_schedule_destination_departures_upserts_the_rows() {
        let pool = connect().await;
        delete_destination_departures_fixture(&pool, "2099-02-01").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        // Flat: one JSON object per DEPARTURE, exactly as
        // schedule-reference's `schedule_destination_departures_rows`
        // emits them (Task 4) and exactly as
        // `queries::ScheduleDestinationDeparturesRow` deserializes them.
        // Two rows, so "one row per departure" is actually discriminated.
        let body = serde_json::json!([
            {
                "service_date": "2099-02-01",
                "destination_crs": "ZRB",
                "scheduled": "08:22:00",
                "train_uid": "C10001",
                "origin_crs": "EUS",
                "true_origin_crs": "PAD",
                "destination_arrival": "11:30:00"
            },
            {
                "service_date": "2099-02-01",
                "destination_crs": "ZRB",
                "scheduled": "10:05:00",
                "train_uid": "C10002",
                "origin_crs": "CRE",
                "true_origin_crs": null
            }
        ]);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/schedule-destination-departures")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(json["upserted"], 2);

        // destination_crs, scheduled, train_uid, origin_crs, true_origin_crs,
        // destination_arrival.
        type StoredDepartureRow = (
            String,
            chrono::NaiveTime,
            String,
            String,
            Option<String>,
            Option<chrono::NaiveTime>,
        );
        let stored: Vec<StoredDepartureRow> = sqlx::query_as(
            "SELECT destination_crs, scheduled, train_uid, origin_crs, true_origin_crs, destination_arrival \
             FROM schedule_destination_departures \
             WHERE service_date = '2099-02-01' \
             ORDER BY scheduled",
        )
        .fetch_all(&pool)
        .await
        .expect("read back the upserted rows");

        assert_eq!(stored.len(), 2, "one stored row per posted departure");
        assert_eq!(stored[0].0, "ZRB");
        assert_eq!(
            stored[0].1,
            chrono::NaiveTime::from_hms_opt(8, 22, 0).unwrap()
        );
        assert_eq!(stored[0].2, "C10001");
        assert_eq!(stored[0].3, "EUS");
        assert_eq!(stored[0].4, Some("PAD".to_string()));
        assert_eq!(
            stored[0].5,
            Some(chrono::NaiveTime::from_hms_opt(11, 30, 0).unwrap())
        );
        assert_eq!(stored[1].2, "C10002");
        assert_eq!(stored[1].3, "CRE");
        assert_eq!(stored[1].4, None);
        assert_eq!(
            stored[1].5, None,
            "an absent destination_arrival key must deserialize as None, not fail or default to a real time"
        );

        delete_destination_departures_fixture(&pool, "2099-02-01").await;
    }

    /// The route-level contract for a partially rejected batch: still a
    /// 200 (so a consumer that predates `rejected` ACKs it), the valid row
    /// lands, and the bad row is named in `rejected`.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                trust_event_backlog_post_with_one_bad_row -- --ignored --test-threads=1`"]
    async fn trust_event_backlog_post_with_one_bad_row_is_a_200_that_reports_it() {
        let pool = connect().await;
        let cleanup = "DELETE FROM trust_event_backlog WHERE dedup_key LIKE 'test-route-poison-%'";
        sqlx::query(cleanup)
            .execute(&pool)
            .await
            .expect("pre-clean");
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let row = |msg_type: &str, dedup_key: &str| {
            json!({
                "train_id": "TEST-ROUTE-POISON",
                "service_date": "2026-09-05",
                "msg_type": msg_type,
                "dedup_key": dedup_key,
            })
        };
        let body = json!([
            row("0003", "test-route-poison-good"),
            row("0009", "test-route-poison-bad"),
        ]);

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/trust-event-backlog")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let landed: Vec<String> = sqlx::query_scalar(
            "SELECT dedup_key FROM trust_event_backlog WHERE dedup_key LIKE 'test-route-poison-%'",
        )
        .fetch_all(&pool)
        .await
        .expect("read back");
        sqlx::query(cleanup).execute(&pool).await.expect("cleanup");

        assert_eq!(status, StatusCode::OK);
        assert_eq!(landed, vec!["test-route-poison-good".to_string()]);
        let parsed: common::TrustBacklogIngestResponse =
            serde_json::from_slice(&body).expect("response parses");
        assert_eq!(parsed.upserted, 1);
        assert_eq!(parsed.rejected.len(), 1);
        assert_eq!(parsed.rejected[0].index, 1);
        assert_eq!(parsed.rejected[0].dedup_key, "test-route-poison-bad");
        assert_eq!(parsed.rejected[0].reason, "check_violation");
        // The old wire field is still there for old consumers.
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["upserted"], 1);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                post_train_forward_signals -- --ignored --test-threads=1`"]
    async fn post_train_forward_signals_through_the_router_lands_in_the_queue() {
        let pool = connect().await;
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-FORWARD-SIGNALS-ROUTE-UID",
            "2026-09-06".parse().unwrap(),
        )
        .await
        .expect("find_or_create_train");

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        let body = serde_json::json!([{
            "trains_id": trains_id,
            "event_summary": "en_route at WAT",
        }]);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/train-forward-signals")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"upserted": 1}));

        let event_summary: String = sqlx::query_scalar(
            "SELECT event_summary FROM notifier_forward_queue WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("row landed");
        assert_eq!(event_summary, "en_route at WAT");

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                train_events_batch_reports_an_accurate_upserted_count -- --ignored --test-threads=1`"]
    async fn train_events_batch_reports_an_accurate_upserted_count_when_one_event_fails_mid_batch()
    {
        // The regression this test exists to pin (2026-09-25 Low-severity
        // auth-core review, "ingest batch writes are non-transactional"):
        // before the fix, `post_train_events` used `?` to abort the WHOLE
        // request on the first per-event error, so a caller either got a
        // bare `500` (no count reported at all, silently discarding
        // whatever prefix of the batch had already committed) or, on full
        // success, `events.len()` -- there was no response shape that ever
        // reported a genuinely partial result. This drives one real,
        // deliberately-invalid event (a `status` value the DB's own CHECK
        // constraint on `train_current_state.status` rejects) through the
        // real router, alongside two harmless ones, and asserts:
        //  1. the request still succeeds overall (`200`, not `500`) --
        //     one bad event no longer sacrifices the rest of the batch;
        //  2. `upserted` counts only the two that actually succeeded, not
        //     all three; and
        //  3. the failing event's OWN partial application is visible: its
        //     `train_movement_events` row landed (that INSERT ran and
        //     committed before the CHECK-violating one) even though its
        //     `train_current_state` row never did -- the literal
        //     "non-transactional, partial application" shape this finding
        //     names, now at least honestly reported via the count.
        let pool = connect().await;
        let user_id = "TEST-INGEST-TRAIN-EVENTS-BATCH-USER";
        sqlx::query("INSERT INTO users (id) VALUES ($1) ON CONFLICT (id) DO NOTHING")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("seed fixture user");
        let subscription_id: i64 = sqlx::query_scalar(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, '2099-03-01', 'ZFA', '2099-03-01T08:00:00Z') \
             RETURNING id",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("seed fixture train_subscriptions row");

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        let events = json!([
            {
                // Resolves a real trains_id (via flip_legacy_resolution's
                // (None, Some(train_uid)) arm) and so actually reaches
                // upsert_train_movement -- but with an invalid `status`
                // that violates train_current_state's own CHECK
                // constraint, forcing a genuine DB error on this one
                // event only.
                "tracked_train_id": subscription_id,
                "resolved_train_uid": "TEST-INGEST-BATCH-PARTIAL-UID",
                "resolved_train_id": "T99999",
                "dedup_key": "test-ingest-batch-partial-dedup",
                "msg_type": "0003",
                "raw_body": {},
                "status": "not-a-real-status-value"
            },
            {
                // No identity resolvable at all (an unknown
                // tracked_train_id) -- upsert_train_event drops this as a
                // logged no-op and returns Ok(()), same as before this
                // fix; included to prove the batch keeps processing past
                // the failing event above.
                "tracked_train_id": -9_123_456_001i64,
                "dedup_key": "test-ingest-batch-harmless-1",
                "msg_type": "0003",
                "raw_body": {},
                "status": "en_route"
            },
            {
                "tracked_train_id": -9_123_456_002i64,
                "dedup_key": "test-ingest-batch-harmless-2",
                "msg_type": "0003",
                "raw_body": {},
                "status": "en_route"
            }
        ]);

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/train-events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&events).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "one bad event in the batch must not fail the whole request"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"upserted": 2}),
            "upserted must count only the two events that actually succeeded, not all three"
        );

        let fixture_service_date: chrono::NaiveDate = "2099-03-01".parse().unwrap();
        let trains_id: i64 =
            sqlx::query_scalar("SELECT id FROM trains WHERE train_uid = $1 AND service_date = $2")
                .bind("TEST-INGEST-BATCH-PARTIAL-UID")
                .bind(fixture_service_date)
                .fetch_one(&pool)
                .await
                .expect("the failing event's own resolution still created a trains row");

        let movement_dedup_key: String =
            sqlx::query_scalar("SELECT dedup_key FROM train_movement_events WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect(
                    "the failing event's train_movement_events write committed before the \
             CHECK-violating train_current_state write ran -- exactly the partial-application \
             shape this finding names",
                );
        assert_eq!(movement_dedup_key, "test-ingest-batch-partial-dedup");

        let current_state_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM train_current_state WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("count train_current_state rows");
        assert_eq!(
            current_state_count, 0,
            "the CHECK-violating insert must not have landed a train_current_state row"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(subscription_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// The diff chunk protocol end to end through the router, exactly as
    /// `schedule-reference`'s `post_date_scoped_rows_in_chunks` sends it: two
    /// chunks sharing a `publish_id`, the last carrying
    /// `last_chunk=true&total_rows=`. The date ends with exactly the union of
    /// the two chunks, and the previous publish's row that neither carried
    /// is gone.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                post_schedule_calling_points_full -- --ignored --test-threads=1`"]
    async fn post_schedule_calling_points_full_diff_protocol_publishes_the_union_of_its_chunks() {
        let pool = connect().await;
        let date = "2099-07-20";
        let clear = || async {
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE service_date = $1::date")
                .bind(date)
                .execute(&pool)
                .await
                .expect("cleanup fixture rows");
        };
        clear().await;
        let calling_point = |uid: &str| {
            json!({
                "service_date": date,
                "uid": uid,
                "seq": 0,
                "tiploc": "EUSTON",
                "kind": "origin",
                "booked_arrival": null,
                "booked_departure": "08:00:00",
                "day_offset": 0
            })
        };
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let post = |query: &'static str, body: Value| {
            let router = router.clone();
            async move {
                router
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(format!("/schedule-calling-points-full?{query}"))
                            .header("content-type", "application/json")
                            .body(Body::from(serde_json::to_vec(&body).unwrap()))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
            }
        };

        // The previous publish (legacy protocol, as an older publisher sends it).
        assert_eq!(
            post("first_chunk=true", json!([calling_point("STALE")])).await,
            StatusCode::OK
        );
        assert_eq!(
            post(
                "first_chunk=true&publish_id=route-test",
                json!([calling_point("CHUNK1")])
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(
            post(
                "first_chunk=false&publish_id=route-test&last_chunk=true&total_rows=2",
                json!([calling_point("CHUNK2")])
            )
            .await,
            StatusCode::OK
        );

        let uids: Vec<String> = sqlx::query_scalar(
            "SELECT uid FROM schedule_calling_points_full WHERE service_date = $1::date ORDER BY uid",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back");
        assert_eq!(uids, vec!["CHUNK1".to_string(), "CHUNK2".to_string()]);

        clear().await;
    }
}

#[cfg(test)]
mod schedule_chunk_params_tests {
    use super::*;

    fn params(query: &str) -> ScheduleChunkParams {
        let uri: axum::http::Uri = format!("/x?{query}").parse().expect("valid uri");
        axum::extract::Query::<ScheduleChunkParams>::try_from_uri(&uri)
            .expect("valid query string")
            .0
    }

    /// An older `schedule-reference` sends only `first_chunk` (or nothing):
    /// it must get exactly the legacy delete-then-insert contract.
    #[test]
    fn no_publish_id_selects_the_legacy_protocol() {
        assert!(matches!(
            params("").mode(),
            Ok(ScheduleChunkMode::Legacy { first_chunk: true })
        ));
        assert!(matches!(
            params("first_chunk=false").mode(),
            Ok(ScheduleChunkMode::Legacy { first_chunk: false })
        ));
    }

    #[test]
    fn a_publish_id_selects_the_diff_protocol_and_only_the_last_chunk_finalizes() {
        let middle = params("first_chunk=false&publish_id=p1");
        let Ok(ScheduleChunkMode::Diff(part)) = middle.mode() else {
            panic!("expected the diff protocol");
        };
        assert_eq!(
            (part.publish_id, part.first_chunk, part.final_total_rows),
            ("p1", false, None)
        );

        let last = params("first_chunk=false&publish_id=p1&last_chunk=true&total_rows=120000");
        let Ok(ScheduleChunkMode::Diff(part)) = last.mode() else {
            panic!("expected the diff protocol");
        };
        assert_eq!(part.final_total_rows, Some(120_000));
    }

    /// A final chunk without its row total cannot be verified, so it is
    /// rejected rather than guessed at.
    #[test]
    fn a_last_chunk_without_total_rows_is_a_bad_request() {
        let Err((status, _)) = params("publish_id=p1&last_chunk=true").mode() else {
            panic!("expected a rejection");
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn an_empty_or_oversized_publish_id_is_a_bad_request() {
        for query in [
            "publish_id=".to_string(),
            format!("publish_id={}", "x".repeat(MAX_PUBLISH_ID_LEN + 1)),
        ] {
            let Err((status, _)) = params(&query).mode() else {
                panic!("expected a rejection for {query}");
            };
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
    }
}
