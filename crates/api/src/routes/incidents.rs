//! `GET /public/incidents/{incidentId}` -- a single Knowledgebase incident's
//! full detail: description, affected stations, every validity period,
//! which lines currently report it, and its own change history.
//! Unauthenticated, matching every other read in `public_router()` -- see
//! docs/superpowers/specs/2026-08-31-incident-detail-page-design.md's
//! "Public read-route convention" finding: every field this returns is
//! already fully public today via `GET /Line/{ids}/Status?detail=true`.
//!
//! One correction to that finding: `currentlyAffectsLines` was NOT already
//! fully public. It reads `line_status`, which also holds private
//! custom-line rows, and `GET /Line/{ids}/Status` gates those rows while
//! this route did not -- see `get_incident`'s own doc comment. "Never
//! requires a session" is not the same as "returns the same bytes to
//! everyone," and this route is now the latter kind of public.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::auth::OptionalAuthenticatedUser;
use crate::data::config::LineCatalogue;
use crate::data::custom_lines;
use crate::data::queries;
use crate::data::recently_cleared::{self, ExpectedEnd, Placement};

pub fn router() -> Router {
    Router::new()
        .route("/incidents", axum::routing::get(search_incidents))
        .route(
            "/incidents/recently-cleared",
            axum::routing::get(get_recently_cleared),
        )
        .route("/incidents/{incidentId}", axum::routing::get(get_incident))
}

/// `knowledgebase-incident-{incidentId}` is the ONLY provenance-string
/// format that names a real `incidents` row -- see the design spec's
/// Correction 1. Reconstructing it here (rather than storing/returning the
/// bare `incident_id` as `disruption.source`) is what lets
/// `lines_currently_reporting_incident` reach into `line_status.statuses`'
/// JSONB and find this exact incident.
fn knowledgebase_source(incident_id: &str) -> String {
    format!("knowledgebase-incident-{incident_id}")
}

/// Page size when the caller does not ask for one. Same numeric value as
/// `routes::trains::DEFAULT_SEARCH_LIMIT` -- no reason for this route's
/// page size to differ -- but declared as its own constant, not shared,
/// since the two routes have no reason to be coupled (Decision 4 of the
/// design spec).
const DEFAULT_INCIDENT_SEARCH_LIMIT: i64 = 50;

/// Hard ceiling on one page, clamped server-side rather than rejected --
/// same rationale and same numeric value as
/// `routes::trains::MAX_SEARCH_LIMIT`.
const MAX_INCIDENT_SEARCH_LIMIT: i64 = 200;

/// `#[serde(deny_unknown_fields)]` for the same reason
/// `trains.rs::TrainSearchParams` has it -- see this crate's established
/// posture: a misspelled filter name must 400, not silently no-op. See
/// Correction 3 of the design spec.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncidentSearchParams {
    /// Optional. Comma-separated ATOC codes, e.g. `operator=SW,VT` --
    /// matches this codebase's existing multi-value convention
    /// (`GET /Line/{ids}/Status`'s `ids`), not repeated query keys.
    /// Matches if the incident's `operators` array overlaps this set at
    /// all (an "any of" filter).
    operator: Option<String>,
    /// Optional. A single catalogue line id (`app.config.lines`, never a
    /// custom line), matched against `incidents.affected_lines` -- the
    /// stored output of `common::matcher`, the same function that decides
    /// which lines report an incident on the live status pages. An id that
    /// doesn't resolve in `app.config.lines` is a `400` ("unknown line"),
    /// not a 404 or a silently-empty result.
    line: Option<String>,
    /// Optional, RFC3339. Inclusive lower bound on `first_seen_at`.
    from: Option<String>,
    /// Optional, RFC3339. Inclusive upper bound on `first_seen_at`.
    to: Option<String>,
    /// Optional. `true` = planned works only, `false` = unplanned only,
    /// omitted = either.
    planned: Option<bool>,
    /// Optional, legacy spelling of `state`: `true` = `state=cleared`,
    /// `false` = `state=active`, omitted = any. Since 2026-10-06 `false`
    /// no longer includes incidents the feed stopped listing without
    /// clearing ("ended"), and since 2026-10-20 nor ones outside their
    /// validity windows. Deliberately not a hidden default filter. A
    /// `400` alongside `state`.
    cleared: Option<bool>,
    /// Optional. `active` (not cleared, still listed, a validity window
    /// covers now), `upcoming` (not cleared, still listed, no window has
    /// started yet), `cleared` (RDM cleared it) or `ended` (no longer
    /// listed by the feed, or every window has finished); omitted = any.
    /// Anything else is a `400`. See `queries::IncidentState`,
    /// docs/superpowers/specs/2026-10-06-incident-source-removal-design.md
    /// and docs/superpowers/specs/2026-10-20-incident-validity-state-design.md.
    state: Option<String>,
    /// Optional. Inclusive lower bound on the raw `priority` integer. No
    /// documented "major"/"minor" mapping exists -- this is a raw numeric
    /// range over an unexplained feed value.
    priority_min: Option<i32>,
    /// Optional. Inclusive upper bound. A `400` if both bounds are given
    /// and `priority_min > priority_max`.
    priority_max: Option<i32>,
    /// Optional page size, 1..=`MAX_INCIDENT_SEARCH_LIMIT`. Over-large is
    /// clamped, not rejected; non-positive or unparseable is a `400`.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`.
    after: Option<String>,
    /// Optional free text, matched against each incident's summary and
    /// description (added 2026-10-14). Every word must match, as a prefix
    /// ("sig fail" finds "Signalling failure"), with English stemming;
    /// punctuation only separates words. Blank is no filter; over
    /// `MAX_INCIDENT_SEARCH_QUERY_CHARS` is a `400`. See
    /// `queries::incident_search_tsquery`.
    q: Option<String>,
}

/// Longest `q` accepted, in characters. Generous for any real search, and
/// keeps an absurd one from reaching the database.
const MAX_INCIDENT_SEARCH_QUERY_CHARS: usize = 200;

/// Parses `q` into the tsquery `queries::search_incidents` binds: `None`
/// for absent, blank or punctuation-only, a `400` when too long.
fn normalize_text_query(raw: Option<&str>) -> Result<Option<String>, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if raw.chars().count() > MAX_INCIDENT_SEARCH_QUERY_CHARS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("q must be at most {MAX_INCIDENT_SEARCH_QUERY_CHARS} characters"),
        ));
    }
    Ok(queries::incident_search_tsquery(raw))
}

/// Parses and bounds the page size -- same shape as
/// `routes::trains::normalize_limit`, kept as its own local copy since the
/// two routes' constants are deliberately not shared.
fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_INCIDENT_SEARCH_LIMIT);
    };
    let parsed: i64 = raw.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        )
    })?;
    if parsed < 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        ));
    }
    Ok(parsed.min(MAX_INCIDENT_SEARCH_LIMIT))
}

/// The archive's lifecycle filter: `state` (`active`/`upcoming`/`cleared`/`ended`),
/// or its legacy boolean spelling `cleared`. Both at once is a `400` rather
/// than a precedence rule a caller would have to know about; so is an
/// unknown `state` (this route's "a typo must 400, not no-op" posture).
fn normalize_state(
    state: Option<&str>,
    cleared: Option<bool>,
) -> Result<Option<queries::IncidentState>, (StatusCode, String)> {
    let state = state.map(str::trim).filter(|s| !s.is_empty());
    match (state, cleared) {
        (Some(_), Some(_)) => Err((
            StatusCode::BAD_REQUEST,
            "use either state or cleared, not both".to_string(),
        )),
        (Some(raw), None) => queries::IncidentState::parse(raw).map(Some).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "state must be one of active, upcoming, cleared, ended".to_string(),
            )
        }),
        (None, Some(true)) => Ok(Some(queries::IncidentState::Cleared)),
        (None, Some(false)) => Ok(Some(queries::IncidentState::Active)),
        (None, None) => Ok(None),
    }
}

/// Parses a caller-supplied RFC3339 timestamp for `from`/`to`.
fn normalize_rfc3339(
    label: &str,
    raw: &str,
) -> Result<chrono::DateTime<chrono::Utc>, (StatusCode, String)> {
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("{label} must be an RFC3339 timestamp"),
            )
        })
}

/// Renders a keyset cursor for the wire: base64url-without-padding of
/// `"{RFC3339 first_seen_at}|{incident_id}"` -- same opaque-token shape as
/// `routes::trains::encode_cursor`. Not signed: it names a public row on
/// an unauthenticated route.
fn encode_cursor(cursor: &queries::IncidentSearchCursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}|{}",
        cursor.first_seen_at.to_rfc3339(),
        cursor.incident_id
    ))
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never
/// silently dropped -- dropping it would restart the caller at page 1
/// while their UI appended the response as page 2, duplicating rows.
fn decode_cursor(raw: &str) -> Result<queries::IncidentSearchCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let parts: Vec<&str> = decoded.split('|').collect();
    let [first_seen_at, incident_id] = parts.as_slice() else {
        return Err(invalid());
    };
    let first_seen_at = chrono::DateTime::parse_from_rfc3339(first_seen_at)
        .map_err(|_| invalid())?
        .with_timezone(&chrono::Utc);
    Ok(queries::IncidentSearchCursor {
        first_seen_at,
        incident_id: (*incident_id).to_string(),
    })
}

/// `state`, `startsAt` and `validityEndedAt` (2026-10-20), shared by a list
/// row and the detail so the two can't disagree. See
/// `queries::IncidentState` for the rule.
fn timing_json(timing: &queries::IncidentTiming) -> [(&'static str, Value); 3] {
    [
        ("state", Value::from(timing.state.as_str())),
        (
            "startsAt",
            timing
                .starts_at
                .map_or(Value::Null, |t| Value::from(t.to_rfc3339())),
        ),
        (
            "validityEndedAt",
            timing
                .validity_ended_at
                .map_or(Value::Null, |t| Value::from(t.to_rfc3339())),
        ),
    ]
}

/// Adds [`timing_json`]'s fields to a rendered incident object.
fn with_timing(mut object: Value, timing: &queries::IncidentTiming) -> Value {
    if let Value::Object(map) = &mut object {
        for (key, value) in timing_json(timing) {
            map.insert(key.to_string(), value);
        }
    }
    object
}

/// Renders one `IncidentSummaryRow` as camelCase JSON -- the `IncidentSummary`
/// shape `frontend/lib/types.ts` declares. Its state is judged at `now`,
/// the instant the search filtered at.
fn incident_summary_json(
    row: &queries::IncidentSummaryRow,
    now: chrono::DateTime<chrono::Utc>,
) -> Value {
    let timing = queries::IncidentTiming::derive(
        row.is_cleared,
        row.source_removed_at,
        &queries::parse_validity_periods(&row.validity_periods),
        now,
    );
    with_timing(
        json!({
            "incidentId": row.incident_id,
            "summary": row.summary,
            "operators": row.operators,
            "affectedStations": row.affected_stations,
            "affectedLines": row.affected_lines,
            "priority": row.priority,
            "isPlanned": row.is_planned,
            "isCleared": row.is_cleared,
            "firstSeenAt": row.first_seen_at.to_rfc3339(),
            "fetchedAt": row.fetched_at.to_rfc3339(),
            "sourceRemovedAt": row.source_removed_at.map(|t| t.to_rfc3339()),
        }),
        &timing,
    )
}

/// `GET /public/incidents` -- see
/// docs/superpowers/specs/2026-09-12-incident-archive-design.md Decisions
/// 1-5. Unauthenticated, per this file's own established public-read
/// convention.
///
/// The `line` filter matches `incidents.affected_lines`, which
/// `queries::upsert_incidents` fills by running `common::matcher` -- the
/// aggregator's own matcher -- over the incident at ingest. So the archive
/// and the live status pages agree by construction about which lines an
/// incident affects, including the `KeywordOnly`/`OperatorOnly` tiers.
///
/// It used to resolve the line to its CRS list and apply a station-overlap
/// filter instead (the design spec's Decision 2 / Correction 1
/// approximation). That returned zero rows for every line in production,
/// because `incidents.affected_stations` is never written by anything:
/// RDM's Incidents XML has no CRS field. See
/// docs/superpowers/specs/2026-09-16-tfl-incident-archive-design.md 1c.
/// The one caveat that remains is scope, not emptiness: rows ingested
/// before `20260917090000_incidents_affected_lines.sql` match no line until
/// `writer-maintenance backfill-incident-lines` has been run (see
/// docs/incident-affected-lines-backfill.md).
///
/// `total` (added 2026-10-14) is how many incidents match in all, for "Show
/// 12 incidents" on /incidents. It is on the first page only and `null` on
/// a page fetched with `after`, which belongs to a search whose total the
/// caller already has.
async fn search_incidents(
    State(app): State<App>,
    Query(params): Query<IncidentSearchParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let operators: Option<Vec<String>> = params.operator.as_deref().and_then(|raw| {
        let list: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if list.is_empty() { None } else { Some(list) }
    });

    // Validated against the catalogue here rather than simply passed
    // through: a line id that does not exist must be a 400, not a silently
    // empty result set. That distinction is the whole reason this defect
    // was hard to notice -- an empty page reads as "nothing archived for
    // this line", which is exactly the wrong conclusion.
    let line: Option<String> = match params
        .line
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(line_id) => {
            if !app.line_matcher.knows_line(line_id) {
                return Err((StatusCode::BAD_REQUEST, "unknown line".to_string()));
            }
            Some(line_id.to_string())
        }
        None => None,
    };

    let from = params
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_rfc3339("from", s))
        .transpose()?;
    let to = params
        .to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_rfc3339("to", s))
        .transpose()?;

    if let (Some(from_bound), Some(to_bound)) = (from, to)
        && from_bound > to_bound
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "from must not be after to".to_string(),
        ));
    }

    if let (Some(min), Some(max)) = (params.priority_min, params.priority_max)
        && min > max
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "priority_min must not exceed priority_max".to_string(),
        ));
    }

    let state = normalize_state(params.state.as_deref(), params.cleared)?;
    let text = normalize_text_query(params.q.as_deref())?;

    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;

    // One instant for the `state` filter and every row's `state`.
    let now = chrono::Utc::now();
    let page = queries::search_incidents_at(
        &app.database,
        operators,
        line,
        params.planned,
        state,
        params.priority_min,
        params.priority_max,
        from,
        to,
        text.as_deref(),
        after.as_ref(),
        limit,
        now,
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(json!({
        "results": page.results.iter().map(|row| incident_summary_json(row, now)).collect::<Vec<Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(encode_cursor),
        "total": page.total,
    })))
}

/// `GET /public/incidents/recently-cleared` query: at most one of a
/// catalogue `line` id or a `station` code; neither is every item (the
/// status page).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecentlyClearedParams {
    line: Option<String>,
    station: Option<String>,
}

/// Where the caller wants items from (`recently_cleared::Placement`). A
/// line must be a catalogue line (`affected_lines` holds only those): an
/// unknown, `tfl-` or `custom-` ID is a `400`, as on `GET /public/incidents`. A
/// station is placed through every catalogue line that calls there (as
/// `GET /StopPoint/{crs}/Disruption` places live disruptions) plus its own
/// code in `affected_stations`.
fn recently_cleared_placement(
    params: &RecentlyClearedParams,
    lines: &LineCatalogue,
) -> Result<Placement, (StatusCode, String)> {
    let non_empty = |raw: &Option<String>| {
        raw.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    match (non_empty(&params.line), non_empty(&params.station)) {
        (Some(_), Some(_)) => Err((
            StatusCode::BAD_REQUEST,
            "use either line or station, not both".to_string(),
        )),
        (Some(line_id), None) => {
            if lines.iter().any(|line| line.id == line_id) {
                Ok(Placement::Matching {
                    line_ids: vec![line_id],
                    station: None,
                })
            } else {
                Err((StatusCode::BAD_REQUEST, "unknown line".to_string()))
            }
        }
        (None, Some(station)) => {
            if station.len() != 3 || !station.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "station must be a three-letter station code".to_string(),
                ));
            }
            let crs = station.to_ascii_uppercase();
            Ok(Placement::Matching {
                line_ids: lines
                    .iter()
                    .filter(|line| line.has_station(&crs))
                    .map(|line| line.id.clone())
                    .collect(),
                station: Some(crs),
            })
        }
        (None, None) => Ok(Placement::Everywhere),
    }
}

/// One item of `GET /public/incidents/recently-cleared`; see
/// docs/api-changelog.md (2026-10-12) for the shape. `affectedLines` names
/// catalogue lines only (all `affected_lines` can hold), so no private
/// custom line can appear.
fn recently_cleared_json(
    row: &recently_cleared::RecentlyClearedRow,
    lines: &LineCatalogue,
) -> Value {
    let expected = match (
        row.prior_summary.as_deref(),
        row.prior_description.as_deref(),
        row.prior_recorded_at,
    ) {
        (Some(summary), Some(description), Some(published_at)) => {
            recently_cleared::expected_end(summary, description, published_at, row.cleared_at)
        }
        _ => None,
    };
    json!({
        "incidentId": row.incident_id,
        "summary": row.summary,
        "description": row.description,
        "operators": row.operators,
        "affectedLines": row
            .affected_lines
            .iter()
            .filter_map(|id| lines.iter().find(|line| &line.id == id))
            .map(|line| json!({ "id": line.id, "name": line.name }))
            .collect::<Vec<_>>(),
        "clearedAt": row.cleared_at.to_rfc3339(),
        "lastListedAt": row.fetched_at.to_rfc3339(),
        "expectedUntil": match expected {
            Some(ExpectedEnd::At(at)) => Some(at.to_rfc3339()),
            _ => None,
        },
        "expectedUntilEndOfDay": expected == Some(ExpectedEnd::EndOfDay),
    })
}

/// `GET /public/incidents/recently-cleared`: cleared incidents the feed
/// still lists, cleared since today's 02:00, newest clear first
/// (`data::recently_cleared`). Info only: nothing here changes a line's
/// status. Unauthenticated and the same for every caller.
async fn get_recently_cleared(
    State(app): State<App>,
    Query(params): Query<RecentlyClearedParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let placement = recently_cleared_placement(&params, &app.config.lines)?;
    let rows = recently_cleared::recently_cleared(&app.database, chrono::Utc::now(), &placement)
        .await
        .map_err(internal_error)?;
    Ok(Json(json!({
        "results": rows
            .iter()
            .map(|row| recently_cleared_json(row, &app.config.lines))
            .collect::<Vec<Value>>(),
    })))
}

/// Unauthenticated in the sense that it never *requires* a session, but
/// it does take an `OptionalAuthenticatedUser`: `currentlyAffectsLines`
/// reads `line_status`, which holds private custom-line rows alongside the
/// public catalogue/TfL ones, so the response has to know who is asking
/// before it can decide which of those rows the caller may see. Same
/// extractor and same posture as `routes::line_status`'s own handlers --
/// an anonymous caller gets a normal `200` with fewer lines listed, never
/// a `401`.
///
/// Before this gate existed, the route disclosed any other user's private
/// custom-line id and name to an anonymous caller, whenever the matcher
/// had attached this incident to that line
/// (docs/superpowers/specs/2026-09-16-custom-lines-in-incident-archive-filter-research.md
/// §5c).
async fn get_incident(
    State(app): State<App>,
    Path(incident_id): Path<String>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<([(header::HeaderName, &'static str); 2], Json<Value>), (StatusCode, String)> {
    let Some(incident) = queries::incident_by_id(&app.database, &incident_id)
        .await
        .map_err(internal_error)?
    else {
        return Err((StatusCode::NOT_FOUND, "incident not found".to_string()));
    };
    let history = queries::incident_history_for_id(&app.database, &incident_id)
        .await
        .map_err(internal_error)?;
    let source = knowledgebase_source(&incident_id);
    let lines = queries::lines_currently_reporting_incident(&app.database, &source)
        .await
        .map_err(internal_error)?;
    // `lines_currently_reporting_incident` is a raw, ungated read of
    // `line_status` -- the privacy gate is here, exactly as
    // `routes::line_status::filter_private_custom_rows` applies the same
    // shared helper to that table's other readers.
    let lines = custom_lines::retain_readable_custom_rows(
        &app.database,
        lines,
        user.as_ref().map(|caller| caller.id.as_str()),
        |row| row.line_id.as_str(),
    )
    .await
    .map_err(internal_error)?;

    // This body now varies by session (the custom-line rows above), while
    // the path still looks like a plain public read. Say so explicitly
    // rather than relying on no edge rule ever caching `/public/incidents/*`
    // -- see the research doc's §5d: closing the leak in the application
    // and re-opening it at the CDN would be the same bug again. `Vary` is
    // strictly redundant next to `no-store` for a conforming cache; it is
    // here for the non-conforming intermediary that honours one and not
    // the other, which is the whole scenario §5d is about.
    //
    // This is the only `Cache-Control` the API sets anywhere, and the
    // asymmetry is deliberate rather than an oversight: `/Line/{ids}/Status`,
    // `/Line/Mode/{modes}/Status` and the seven `/Line/{id}/...` routes in
    // `routes::line_status` are equally session-dependent and equally
    // bare-headed. §5d asked for it on this route specifically (it measured
    // this route in production); widening it to the rest is a separate,
    // uniform change, not something to half-do here.
    Ok((
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Cookie"),
        ],
        Json(to_incident_detail_json(
            incident,
            history,
            lines,
            chrono::Utc::now(),
        )),
    ))
}

/// Renders `serde_json::Value` field-by-field via `json!()`, exactly like
/// `crates/api/src/render.rs::status_to_json` -- deliberately NOT
/// `#[derive(Serialize)] #[serde(rename_all = "camelCase")]` on a struct
/// that embeds `validity_periods` directly, because `rename_all` is not
/// inherited into a nested type. See this plan's Status note Correction A
/// and Global Constraints for the concrete failure mode that would produce
/// (a response that's camelCase at the top level but `snake_case` inside
/// every validity period). Pure function, no I/O -- unit-testable without
/// a database, matching `to_tfl_shape`'s own testable shape in `render.rs`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "callers hand over values they no longer need"
)]
fn to_incident_detail_json(
    incident: queries::IncidentRow,
    history: Vec<queries::IncidentHistoryRow>,
    lines: Vec<queries::IncidentLineRefRow>,
    now: chrono::DateTime<chrono::Utc>,
) -> Value {
    let timing = queries::IncidentTiming::derive(
        incident.is_cleared,
        incident.source_removed_at,
        &queries::parse_validity_periods(&incident.validity_periods),
        now,
    );
    with_timing(
        json!({
            "incidentId": incident.incident_id,
            "summary": incident.summary,
            "description": incident.description,
            "operators": incident.operators,
            "affectedStations": incident.affected_stations,
            "priority": incident.priority,
            "validityPeriods": render_validity_periods(&incident.validity_periods),
            "isPlanned": incident.is_planned,
            "isCleared": incident.is_cleared,
            "firstSeenAt": incident.first_seen_at.to_rfc3339(),
            "fetchedAt": incident.fetched_at.to_rfc3339(),
            "sourceRemovedAt": incident.source_removed_at.map(|t| t.to_rfc3339()),
            "currentlyAffectsLines": lines.iter().map(|l| json!({
                "id": l.line_id,
                "name": l.name,
            })).collect::<Vec<_>>(),
            "history": history.iter().map(|h| json!({
                "summary": h.summary,
                "description": h.description,
                "operators": h.operators,
                "affectedStations": h.affected_stations,
                "priority": h.priority,
                "validityPeriods": render_validity_periods(&h.validity_periods),
                "isPlanned": h.is_planned,
                "isCleared": h.is_cleared,
                "recordedAt": h.recorded_at.to_rfc3339(),
            })).collect::<Vec<_>>(),
        }),
        &timing,
    )
}

/// `validity_periods` comes back from `queries::incident_by_id`/
/// `incident_history_for_id` as raw `serde_json::Value` (the column's own
/// stored JSONB, snake-case field names -- `from_date`/`to_date`/`is_now`,
/// since `common::ValidityPeriod` has no `rename_all`). Deserializes into
/// the real Rust type first, rather than this function re-implementing
/// JSONB field access by hand. A malformed row does NOT fail loudly: it
/// degrades silently to an empty validity-periods array, with a
/// `tracing::warn!` as the only signal.
fn render_validity_periods(raw: &Value) -> Value {
    Value::Array(
        queries::parse_validity_periods(raw)
            .into_iter()
            .map(|p| {
                json!({
                    "fromDate": p.from_date.to_rfc3339(),
                    "toDate": p.to_date.map(|d| d.to_rfc3339()),
                    "isNow": p.is_now,
                })
            })
            .collect(),
    )
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used as a map_err callback, which passes the error by value"
)]
fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    if let Some(unavailable) = crate::unavailable::response_for(&err) {
        return unavailable;
    }
    tracing::error!(error = ?err, "incident lookup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn sample_incident() -> queries::IncidentRow {
        queries::IncidentRow {
            incident_id: "12345".to_string(),
            summary: "Signal failure at Woking".to_string(),
            description: "<p>Delays expected</p>".to_string(),
            operators: vec!["VT".to_string()],
            affected_stations: vec!["WOK".to_string(), "WAT".to_string()],
            priority: 3,
            validity_periods: serde_json::json!([
                {"from_date": "2026-08-30T09:00:00Z", "to_date": null, "is_now": true}
            ]),
            is_planned: false,
            is_cleared: false,
            first_seen_at: Utc.with_ymd_and_hms(2026, 8, 30, 9, 0, 0).unwrap(),
            fetched_at: Utc.with_ymd_and_hms(2026, 8, 31, 10, 15, 0).unwrap(),
            source_removed_at: None,
        }
    }

    /// 2026-10-20: the detail's `state` comes from the validity windows at
    /// the given instant, with `startsAt`/`validityEndedAt` explaining it.
    #[test]
    fn renders_the_state_the_validity_windows_give() {
        let mut incident = sample_incident();
        incident.is_planned = true;
        incident.validity_periods = serde_json::json!([
            {"from_date": "2026-10-24T23:00:00Z", "to_date": "2026-10-25T23:59:00Z", "is_now": false}
        ]);
        let before = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap();
        let json = to_incident_detail_json(incident.clone(), vec![], vec![], before);
        assert_eq!(json["state"], "upcoming");
        assert_eq!(json["startsAt"], "2026-10-24T23:00:00+00:00");
        assert!(json["validityEndedAt"].is_null());

        let during = Utc.with_ymd_and_hms(2026, 10, 25, 1, 30, 0).unwrap();
        let json = to_incident_detail_json(incident.clone(), vec![], vec![], during);
        assert_eq!(json["state"], "active");
        assert!(json["startsAt"].is_null());

        let after = Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap();
        let json = to_incident_detail_json(incident.clone(), vec![], vec![], after);
        assert_eq!(json["state"], "ended");
        assert_eq!(json["validityEndedAt"], "2026-10-25T23:59:00+00:00");

        incident.is_cleared = true;
        let json = to_incident_detail_json(incident, vec![], vec![], before);
        assert_eq!(json["state"], "cleared", "cleared beats the windows");
    }

    #[test]
    fn renders_top_level_fields_as_camel_case() {
        let json = to_incident_detail_json(sample_incident(), vec![], vec![], Utc::now());
        assert_eq!(json["incidentId"], "12345");
        assert_eq!(json["summary"], "Signal failure at Woking");
        assert_eq!(json["affectedStations"][0], "WOK");
        assert_eq!(json["isPlanned"], false);
        assert_eq!(json["isCleared"], false);
        assert!(
            json["sourceRemovedAt"].is_null(),
            "a still-listed incident has an explicit null, not a missing key"
        );
    }

    #[test]
    fn an_ended_incident_renders_when_the_feed_last_listed_it() {
        let mut incident = sample_incident();
        incident.source_removed_at = Some(Utc.with_ymd_and_hms(2026, 10, 5, 22, 55, 0).unwrap());
        let json = to_incident_detail_json(incident, vec![], vec![], Utc::now());
        assert_eq!(json["sourceRemovedAt"], "2026-10-05T22:55:00+00:00");
        assert_eq!(json["isCleared"], false, "ended is not cleared");
    }

    #[test]
    fn the_state_filter_and_its_legacy_cleared_spelling() {
        use queries::IncidentState;
        assert_eq!(normalize_state(None, None), Ok(None));
        assert_eq!(normalize_state(Some(" "), None), Ok(None));
        assert_eq!(
            normalize_state(Some("active"), None),
            Ok(Some(IncidentState::Active))
        );
        assert_eq!(
            normalize_state(Some("ended"), None),
            Ok(Some(IncidentState::Ended))
        );
        assert_eq!(
            normalize_state(Some("cleared"), None),
            Ok(Some(IncidentState::Cleared))
        );
        assert_eq!(
            normalize_state(None, Some(true)),
            Ok(Some(IncidentState::Cleared))
        );
        assert_eq!(
            normalize_state(None, Some(false)),
            Ok(Some(IncidentState::Active)),
            "cleared=false is active only, never ended rows"
        );
        assert_eq!(
            normalize_state(Some("gone"), None).map_err(|(status, _)| status),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            normalize_state(Some("active"), Some(false)).map_err(|(status, _)| status),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn validity_periods_render_as_camel_case_not_snake_case() {
        // The direct regression test for Correction A -- proves this
        // function does not fall back to a derived Serialize impl that
        // would leak `from_date`/`to_date`/`is_now` through unrenamed.
        let json = to_incident_detail_json(sample_incident(), vec![], vec![], Utc::now());
        let period = &json["validityPeriods"][0];
        assert_eq!(period["fromDate"], "2026-08-30T09:00:00+00:00");
        assert!(period["toDate"].is_null());
        assert_eq!(period["isNow"], true);
        assert!(
            period.get("from_date").is_none(),
            "must not leak the raw snake_case JSONB field name"
        );
    }

    #[test]
    fn currently_affects_lines_is_empty_array_not_null_when_no_lines_match() {
        let json = to_incident_detail_json(sample_incident(), vec![], vec![], Utc::now());
        assert!(json["currentlyAffectsLines"].is_array());
        assert_eq!(json["currentlyAffectsLines"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn history_renders_every_entry_newest_first_order_preserved() {
        let history = vec![
            queries::IncidentHistoryRow {
                summary: "v2".to_string(),
                description: "d".to_string(),
                operators: vec![],
                affected_stations: vec![],
                priority: 2,
                validity_periods: serde_json::json!([]),
                is_planned: false,
                is_cleared: false,
                recorded_at: Utc.with_ymd_and_hms(2026, 8, 31, 9, 0, 0).unwrap(),
            },
            queries::IncidentHistoryRow {
                summary: "v1".to_string(),
                description: "d".to_string(),
                operators: vec![],
                affected_stations: vec![],
                priority: 1,
                validity_periods: serde_json::json!([]),
                is_planned: false,
                is_cleared: false,
                recorded_at: Utc.with_ymd_and_hms(2026, 8, 30, 9, 0, 0).unwrap(),
            },
        ];
        let json = to_incident_detail_json(sample_incident(), history, vec![], Utc::now());
        assert_eq!(json["history"][0]["summary"], "v2");
        assert_eq!(json["history"][1]["summary"], "v1");
    }

    #[test]
    fn currently_affects_lines_renders_id_and_name() {
        let lines = vec![queries::IncidentLineRefRow {
            line_id: "south-western".to_string(),
            name: "South Western Main Line".to_string(),
        }];
        let json = to_incident_detail_json(sample_incident(), vec![], lines, Utc::now());
        assert_eq!(json["currentlyAffectsLines"][0]["id"], "south-western");
        assert_eq!(
            json["currentlyAffectsLines"][0]["name"],
            "South Western Main Line"
        );
    }

    fn catalogue() -> LineCatalogue {
        let line = |id: &str, name: &str, stations: &[&str]| common::LineDefinition {
            id: id.to_string(),
            name: name.to_string(),
            mode: "train".to_string(),
            category: "main".to_string(),
            operators: vec!["SW".to_string()],
            stations: stations
                .iter()
                .map(|crs| common::Station {
                    crs: (*crs).to_string(),
                    tiploc: None,
                    role: "principal".to_string(),
                    segment: None,
                })
                .collect(),
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: std::collections::HashMap::new(),
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
            pass_through: Vec::new(),
            crs_aliases: std::collections::BTreeMap::new(),
            trunk_for: Vec::new(),
        };
        LineCatalogue(vec![
            line("swml", "South West Main Line", &["WAT", "WOK"]),
            line("portsmouth", "Portsmouth Direct", &["WOK", "GLD", "PMS"]),
        ])
    }

    fn params(line: Option<&str>, station: Option<&str>) -> RecentlyClearedParams {
        RecentlyClearedParams {
            line: line.map(str::to_string),
            station: station.map(str::to_string),
        }
    }

    #[test]
    fn recently_cleared_placement_by_line_station_or_everywhere() {
        let lines = catalogue();
        assert_eq!(
            recently_cleared_placement(&params(None, None), &lines),
            Ok(Placement::Everywhere)
        );
        assert_eq!(
            recently_cleared_placement(&params(Some(" "), Some("")), &lines),
            Ok(Placement::Everywhere)
        );
        assert_eq!(
            recently_cleared_placement(&params(Some("swml"), None), &lines),
            Ok(Placement::Matching {
                line_ids: vec!["swml".to_string()],
                station: None
            })
        );
        assert_eq!(
            recently_cleared_placement(&params(None, Some("wok")), &lines),
            Ok(Placement::Matching {
                line_ids: vec!["swml".to_string(), "portsmouth".to_string()],
                station: Some("WOK".to_string())
            }),
            "every catalogue line through the station, and the code itself"
        );
        assert_eq!(
            recently_cleared_placement(&params(None, Some("ZZZ")), &lines),
            Ok(Placement::Matching {
                line_ids: vec![],
                station: Some("ZZZ".to_string())
            })
        );
        for bad in [
            params(Some("tfl-victoria"), None),
            params(Some("custom-mine"), None),
            params(None, Some("WOKI")),
            params(None, Some("W1K")),
            params(Some("swml"), Some("WOK")),
        ] {
            assert_eq!(
                recently_cleared_placement(&bad, &lines).map_err(|(status, _)| status),
                Err(StatusCode::BAD_REQUEST),
                "{bad:?}"
            );
        }
    }

    fn cleared_row(prior_summary: Option<&str>) -> recently_cleared::RecentlyClearedRow {
        recently_cleared::RecentlyClearedRow {
            incident_id: "0071A58B".to_string(),
            summary: "CLEARED: Residual disruption between Brockenhurst and Bournemouth"
                .to_string(),
            description: "<p>Disruption has now ended.</p>".to_string(),
            operators: vec!["SW".to_string()],
            affected_lines: vec!["portsmouth".to_string(), "not-in-catalogue".to_string()],
            cleared_at: Utc.with_ymd_and_hms(2026, 10, 2, 20, 26, 0).unwrap(),
            fetched_at: Utc.with_ymd_and_hms(2026, 10, 2, 22, 55, 0).unwrap(),
            prior_summary: prior_summary.map(str::to_string),
            prior_description: prior_summary.map(|_| "<p>Trains may be delayed.</p>".to_string()),
            prior_recorded_at: prior_summary
                .map(|_| Utc.with_ymd_and_hms(2026, 10, 2, 19, 0, 0).unwrap()),
        }
    }

    #[test]
    fn recently_cleared_renders_the_clear_and_the_stated_end() {
        let lines = catalogue();
        let json = recently_cleared_json(
            &cleared_row(Some("Residual disruption expected until 22:30")),
            &lines,
        );
        assert_eq!(json["incidentId"], "0071A58B");
        assert_eq!(json["description"], "<p>Disruption has now ended.</p>");
        assert_eq!(json["clearedAt"], "2026-10-02T20:26:00+00:00");
        assert_eq!(json["lastListedAt"], "2026-10-02T22:55:00+00:00");
        assert_eq!(
            json["expectedUntil"], "2026-10-02T21:30:00+00:00",
            "22:30 BST"
        );
        assert_eq!(json["expectedUntilEndOfDay"], false);
        assert_eq!(
            json["affectedLines"],
            serde_json::json!([{ "id": "portsmouth", "name": "Portsmouth Direct" }]),
            "catalogue lines only, by name"
        );

        let json = recently_cleared_json(
            &cleared_row(Some("Disruption expected until the end of the day")),
            &lines,
        );
        assert!(json["expectedUntil"].is_null());
        assert_eq!(json["expectedUntilEndOfDay"], true);

        for row in [
            cleared_row(None),
            cleared_row(Some("Signal failure at Woking")),
        ] {
            let json = recently_cleared_json(&row, &lines);
            assert!(json["expectedUntil"].is_null(), "an explicit null");
            assert_eq!(json["expectedUntilEndOfDay"], false);
        }
    }

    #[test]
    fn knowledgebase_source_matches_the_exact_format_correction_1_verified() {
        assert_eq!(
            knowledgebase_source("12345"),
            "knowledgebase-incident-12345"
        );
    }
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::hash_session_token;
    use crate::auth::internal_oauth::ServiceTokenVerifier;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::custom_lines::NewCustomLine;
    use crate::data::users::insert_session;

    /// Local to this test module, matching `routes::trains`'s own
    /// `test_app` -- this codebase's convention is one small
    /// per-route-test-module copy of this fixture builder, not a shared
    /// helper (grepped: `stanox_crs.rs`, `departures.rs`,
    /// `station_stats.rs`, `trains.rs`, `groups.rs`,
    /// `ingest.rs`, `train.rs` each define their own).
    fn test_app(pool: PgPool, lines: Vec<common::LineDefinition>) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            migration_database_url: None,
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_mcp: "srv-ds-mcp".to_string(),
            transit_networks: Vec::new(),
            admin_group: String::new(),
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
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
            background_loops: true,
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
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            delegated: std::sync::Arc::default(),
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

    async fn delete_fixtures(pool: &PgPool) {
        sqlx::query("DELETE FROM incidents WHERE incident_id LIKE 'route-test-%'")
            .execute(pool)
            .await
            .expect("cleanup fixture incidents rows");
    }

    /// Seeds a row with an explicit `affected_lines` array -- the column
    /// the Line filter matches on, normally written by the matcher inside
    /// `upsert_incidents`.
    async fn seed_incident_with_lines(
        pool: &PgPool,
        incident_id: &str,
        operators: &[&str],
        affected_lines: &[&str],
    ) {
        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, \
                 affected_lines, priority, is_planned, is_cleared) \
             VALUES ($1, $2, '', $3, '{}', $4, 1, false, false)",
        )
        .bind(incident_id)
        .bind(format!("Fixture incident {incident_id}"))
        .bind(operators)
        .bind(affected_lines)
        .execute(pool)
        .await
        .expect("seed fixture incidents row");
    }

    async fn seed_incident(
        pool: &PgPool,
        incident_id: &str,
        operators: &[&str],
        affected_stations: &[&str],
        priority: i32,
        is_planned: bool,
        is_cleared: bool,
    ) {
        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, priority, \
                 is_planned, is_cleared) \
             VALUES ($1, $2, '', $3, $4, $5, $6, $7)",
        )
        .bind(incident_id)
        .bind(format!("Fixture incident {incident_id}"))
        .bind(operators)
        .bind(affected_stations)
        .bind(priority)
        .bind(is_planned)
        .bind(is_cleared)
        .execute(pool)
        .await
        .expect("seed fixture incidents row");
    }

    async fn get(
        pool: &PgPool,
        lines: Vec<common::LineDefinition>,
        uri: &str,
    ) -> (StatusCode, String) {
        let router: axum::Router = Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), lines));
        let response = router
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    fn results(body: &str) -> Vec<Value> {
        let json: Value = serde_json::from_str(body).unwrap();
        assert!(
            json.is_object() && json.get("results").is_some() && json.get("nextCursor").is_some(),
            "the body is an envelope with exactly `results` and `nextCursor`: {json}"
        );
        json["results"].as_array().cloned().unwrap()
    }

    fn next_cursor(body: &str) -> Option<String> {
        let json: Value = serde_json::from_str(body).unwrap();
        json["nextCursor"].as_str().map(str::to_string)
    }

    /// Drives `GET /incidents/{id}` end-to-end through the real router,
    /// optionally as a logged-in caller (`raw_token` is the raw session
    /// token `seed_session` hands back, sent exactly as the browser would).
    /// Returns the response headers too -- this route's `Cache-Control`
    /// is load-bearing now that its body varies by session.
    async fn get_detail(
        pool: &PgPool,
        incident_id: &str,
        raw_token: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let router: axum::Router = Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), vec![]));
        let mut builder = Request::builder().uri(format!("/incidents/{incident_id}"));
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let response = router
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value = serde_json::from_slice(&body).unwrap_or_else(|_| {
            Value::String(String::from_utf8(body.to_vec()).expect("body is valid utf8"))
        });
        (status, headers, value)
    }

    /// Every line id in a detail response's `currentlyAffectsLines`.
    fn affected_line_ids(body: &Value) -> Vec<String> {
        body["currentlyAffectsLines"]
            .as_array()
            .expect("currentlyAffectsLines is an array")
            .iter()
            .map(|l| l["id"].as_str().expect("id is a string").to_string())
            .collect()
    }

    /// Seeds a real, resolvable session for `user_id` (creating the user if
    /// needed) and returns the RAW token -- same helper, same contract, as
    /// `routes::line_status::db_tests::seed_session`.
    async fn seed_session(pool: &PgPool, user_id: &str) -> String {
        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(user_id)
            .bind(user_id)
            .execute(pool)
            .await
            .expect("seed fixture user");

        let raw_token = format!("test-raw-session-token-for-{user_id}");
        insert_session(pool, &hash_session_token(&raw_token), user_id, 14)
            .await
            .expect("seed fixture session");
        raw_token
    }

    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    /// Seeds one `line_status` row carrying a single status whose
    /// `disruption.source` is the exact `knowledgebase-incident-{id}`
    /// string `get_incident` reconstructs -- i.e. the row shape that makes
    /// this line show up in that incident's `currentlyAffectsLines`. The
    /// JSONB mirrors `queries::db_tests`' own fixture for this query.
    async fn seed_line_reporting(pool: &PgPool, line_id: &str, incident_id: &str) {
        let statuses = format!(
            "[{{\"severity\":9,\"reason\":\"x\",\"validity\":{{\"from_date\":\"2026-01-01T00:00:00Z\",\
               \"to_date\":null,\"is_now\":true}},\"data_quality\":\"knowledgebase\",\
               \"disruption\":{{\"category\":\"RealTime\",\"description\":\"x\",\"affected_stops\":[],\
               \"affected_routes\":[],\"source\":\"knowledgebase-incident-{incident_id}\"}}}}]"
        );
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, computed_at) \
             VALUES ($1, $2, 'national-rail', '{SW}', $3::jsonb, NOW()) \
             ON CONFLICT (line_id) DO UPDATE SET statuses = EXCLUDED.statuses, \
                computed_at = EXCLUDED.computed_at",
        )
        .bind(line_id)
        .bind(format!("Name of {line_id}"))
        .bind(statuses)
        .execute(pool)
        .await
        .expect("seed fixture line_status row");
    }

    async fn cleanup_line_status(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM line_status WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture line_status row");
    }

    /// The route end to end: placement by line and station, and only the
    /// listed cleared rows. The window itself is
    /// `data::recently_cleared::db_tests`'.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                recently_cleared -- --ignored --test-threads=1`"]
    async fn recently_cleared_route_places_items_by_line_and_station() {
        let pool = connect().await;
        let delete_history = "DELETE FROM incident_history WHERE incident_id LIKE 'route-test-%'";
        sqlx::query(delete_history)
            .execute(&pool)
            .await
            .expect("cleanup history");
        delete_fixtures(&pool).await;
        let matcher = common::matcher::LineMatcher::new(&[]);
        let cleared = |id: &str, summary: &str| common::IncidentMessage {
            incident_id: id.to_string(),
            summary: summary.to_string(),
            description: format!("<p>{summary}.</p>"),
            operators: vec!["VT".to_string()],
            affected_stations: vec![],
            priority: 2,
            validity: vec![],
            is_planned: false,
            is_cleared: true,
        };
        let batch = vec![
            cleared("route-test-cleared-1", "CLEARED: Disruption at Woking"),
            cleared("route-test-cleared-2", "CLEARED: Disruption elsewhere"),
        ];
        ds_store::incidents::apply_snapshot(
            &pool,
            &matcher,
            &batch,
            ds_store::incidents::RowHeartbeat::On,
        )
        .await
        .expect("apply snapshot");
        sqlx::query(
            "UPDATE incidents SET affected_lines = '{test-line}' \
              WHERE incident_id = 'route-test-cleared-1'",
        )
        .execute(&pool)
        .await
        .expect("place one on the line");

        let ids = |body: &str| -> Vec<String> {
            let json: Value = serde_json::from_str(body).unwrap();
            json["results"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|item| item["incidentId"].as_str())
                .filter(|id| id.starts_with("route-test-"))
                .map(str::to_string)
                .collect()
        };
        for (uri, expected) in [
            (
                "/incidents/recently-cleared?line=test-line",
                vec!["route-test-cleared-1"],
            ),
            (
                "/incidents/recently-cleared?station=WOK",
                vec!["route-test-cleared-1"],
            ),
            ("/incidents/recently-cleared?station=PAD", vec![]),
        ] {
            let (status, body) = get(&pool, vec![fixture_line()], uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}: {body}");
            assert_eq!(ids(&body), expected, "{uri}");
        }
        let (status, body) = get(&pool, vec![fixture_line()], "/incidents/recently-cleared").await;
        assert_eq!(status, StatusCode::OK);
        let mut all = ids(&body);
        all.sort();
        assert_eq!(all, vec!["route-test-cleared-1", "route-test-cleared-2"]);
        let json: Value = serde_json::from_str(&body).unwrap();
        let item = json["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["incidentId"] == "route-test-cleared-1")
            .unwrap();
        assert_eq!(item["affectedLines"][0]["name"], "Test Line");

        let (status, _) = get(
            &pool,
            vec![fixture_line()],
            "/incidents/recently-cleared?line=nope",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        sqlx::query(delete_history)
            .execute(&pool)
            .await
            .expect("cleanup history");
        delete_fixtures(&pool).await;
    }

    fn fixture_line() -> common::LineDefinition {
        common::LineDefinition {
            id: "test-line".to_string(),
            name: "Test Line".to_string(),
            mode: "train".to_string(),
            category: "main".to_string(),
            operators: vec!["VT".to_string()],
            stations: vec![
                common::Station {
                    crs: "WAT".to_string(),
                    tiploc: None,
                    role: "principal".to_string(),
                    segment: None,
                },
                common::Station {
                    crs: "WOK".to_string(),
                    tiploc: None,
                    role: "principal".to_string(),
                    segment: None,
                },
            ],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: std::collections::HashMap::new(),
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
            pass_through: Vec::new(),
            crs_aliases: std::collections::BTreeMap::new(),
            trunk_for: Vec::new(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_with_no_filters_returns_every_seeded_row_as_200() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-1", &["VT"], &["WAT"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!results(&body).is_empty());
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_operator_param_is_comma_parsed_and_matches_on_overlap() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(
            &pool,
            "route-test-2",
            &["VT", "SW"],
            &["WAT"],
            1,
            false,
            false,
        )
        .await;
        seed_incident(&pool, "route-test-3", &["GW"], &["PAD"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents?operator=SW,XX").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let ids: Vec<&str> = rows
            .iter()
            .map(|r| r["incidentId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["route-test-2"]);
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_unknown_line_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?line=does-not-exist").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("line"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_known_line_returns_rows_the_matcher_attributed_to_it() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        // Attributed to test-line by the matcher at ingest.
        seed_incident_with_lines(&pool, "route-test-4", &["VT"], &["test-line"]).await;
        // Same operator, attributed to a different line. Must be excluded:
        // the Line filter is a line filter, not an operator filter in
        // disguise.
        seed_incident_with_lines(&pool, "route-test-5", &["VT"], &["other-line"]).await;
        // The shape every pre-existing production row has until the
        // backfill runs. Also must be excluded -- an unattributed row is
        // not silently attributed to whatever line was asked for.
        seed_incident_with_lines(&pool, "route-test-6", &["VT"], &[]).await;

        let (status, body) = get(&pool, vec![fixture_line()], "/incidents?line=test-line").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let ids: Vec<&str> = rows
            .iter()
            .map(|r| r["incidentId"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["route-test-4"],
            "only the incident attributed to this line must be returned: {ids:?}"
        );
        assert_eq!(
            rows[0]["affectedLines"][0], "test-line",
            "the row carries its line attribution onto the wire: {body}"
        );
        delete_fixtures(&pool).await;
    }

    /// 2026-10-06: `state=active|cleared|ended`, and `cleared=false` (the
    /// legacy spelling of active) no longer returning ended rows.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_state_filter_and_legacy_cleared() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-st-active", &["QZ"], &[], 1, false, false).await;
        seed_incident(&pool, "route-test-st-cleared", &["QZ"], &[], 1, false, true).await;
        seed_incident(&pool, "route-test-st-ended", &["QZ"], &[], 1, true, false).await;
        sqlx::query(
            "UPDATE incidents SET source_removed_at = fetched_at, source_missing_polls = 2 \
             WHERE incident_id = 'route-test-st-ended'",
        )
        .execute(&pool)
        .await
        .expect("mark ended");

        let ids = |body: &str| -> Vec<String> {
            results(body)
                .iter()
                .map(|r| r["incidentId"].as_str().unwrap().to_string())
                .collect()
        };
        for (query, expected) in [
            ("state=active", "route-test-st-active"),
            ("cleared=false", "route-test-st-active"),
            ("state=cleared", "route-test-st-cleared"),
            ("cleared=true", "route-test-st-cleared"),
            ("state=ended", "route-test-st-ended"),
        ] {
            let (status, body) =
                get(&pool, vec![], &format!("/incidents?operator=QZ&{query}")).await;
            assert_eq!(status, StatusCode::OK, "{query}: {body}");
            assert_eq!(ids(&body), vec![expected.to_string()], "{query}");
        }

        let (_, body) = get(&pool, vec![], "/incidents?operator=QZ&state=ended").await;
        let row = &results(&body)[0];
        assert!(row["sourceRemovedAt"].is_string(), "{row}");
        assert_eq!(row["isCleared"], false);
        let (_, body) = get(&pool, vec![], "/incidents?operator=QZ&state=active").await;
        assert!(results(&body)[0]["sourceRemovedAt"].is_null());

        let (status, _) = get(&pool, vec![], "/incidents?state=gone").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = get(&pool, vec![], "/incidents?state=active&cleared=false").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        delete_fixtures(&pool).await;
    }

    /// 2026-10-20: planned work whose only validity window is next week is
    /// `upcoming` on the list, in its count and on its detail page, not
    /// `active`; one whose window has finished is `ended`, with the end.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_state_follows_the_validity_windows() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        let now = chrono::Utc::now();
        let future_from = now + chrono::Duration::days(5);
        let past_to = now - chrono::Duration::days(1);
        for (id, periods) in [
            (
                "route-test-vw-future",
                serde_json::json!([{
                    "from_date": future_from.to_rfc3339(),
                    "to_date": (future_from + chrono::Duration::days(1)).to_rfc3339(),
                    "is_now": false,
                }]),
            ),
            (
                "route-test-vw-past",
                serde_json::json!([{
                    "from_date": (past_to - chrono::Duration::days(1)).to_rfc3339(),
                    "to_date": past_to.to_rfc3339(),
                    "is_now": false,
                }]),
            ),
            (
                "route-test-vw-now",
                serde_json::json!([{
                    "from_date": (now - chrono::Duration::hours(1)).to_rfc3339(),
                    "to_date": null,
                    "is_now": true,
                }]),
            ),
        ] {
            seed_incident(&pool, id, &["QW"], &[], 1, true, false).await;
            sqlx::query("UPDATE incidents SET validity_periods = $2 WHERE incident_id = $1")
                .bind(id)
                .bind(periods)
                .execute(&pool)
                .await
                .expect("set validity");
        }

        let rows_for = |body: &str| -> Vec<(String, Value)> {
            results(body)
                .iter()
                .map(|r| (r["incidentId"].as_str().unwrap().to_string(), r.clone()))
                .collect()
        };
        for (state, expected) in [
            ("active", "route-test-vw-now"),
            ("upcoming", "route-test-vw-future"),
            ("ended", "route-test-vw-past"),
        ] {
            let (status, body) = get(
                &pool,
                vec![],
                &format!("/incidents?operator=QW&state={state}"),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{state}: {body}");
            let rows = rows_for(&body);
            assert_eq!(rows.len(), 1, "{state}: {body}");
            assert_eq!(rows[0].0, expected, "{state}");
            assert_eq!(rows[0].1["state"], state, "{state}");
            let parsed: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(parsed["total"], 1, "{state} count");
        }

        let (_, body) = get(&pool, vec![], "/incidents?operator=QW&state=upcoming").await;
        let row = &results(&body)[0];
        let starts_at = chrono::DateTime::parse_from_rfc3339(row["startsAt"].as_str().unwrap())
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(starts_at.timestamp(), future_from.timestamp());
        assert!(row["validityEndedAt"].is_null());

        let (status, body) = get(&pool, vec![], "/incidents/route-test-vw-future").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let detail: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(detail["state"], "upcoming");
        assert!(detail["startsAt"].is_string());

        let (_, body) = get(&pool, vec![], "/incidents/route-test-vw-past").await;
        let detail: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(detail["state"], "ended");
        assert!(detail["sourceRemovedAt"].is_null());
        let ended =
            chrono::DateTime::parse_from_rfc3339(detail["validityEndedAt"].as_str().unwrap())
                .unwrap()
                .with_timezone(&chrono::Utc);
        assert_eq!(ended.timestamp(), past_to.timestamp());

        let (_, body) = get(&pool, vec![], "/incidents/route-test-vw-now").await;
        let detail: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(detail["state"], "active");
        assert!(detail["startsAt"].is_null());
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_priority_min_greater_than_max_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?priority_min=5&priority_max=1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("priority"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_from_after_to_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(
            &pool,
            vec![],
            "/incidents?from=2026-01-02T00:00:00Z&to=2026-01-01T00:00:00Z",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("from"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_malformed_after_cursor_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?after=!!!not-base64!!!").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("after"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_rejects_an_unrecognized_query_parameter_instead_of_silently_ignoring_it()
     {
        let pool = connect().await;
        let (status, _) = get(&pool, vec![], "/incidents?operater=SW").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-6", &["VT"], &["WAT"], 1, false, false).await;

        let (status, _) = get(&pool, vec![], "/incidents?limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, vec![], "/incidents?limit=lots").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("limit"),
            "400 body should name the field: {body}"
        );

        let (status, _) = get(&pool, vec![], "/incidents?limit=99999").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an over-large limit is clamped to MAX_INCIDENT_SEARCH_LIMIT, never rejected"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_q_matches_text_and_the_first_page_carries_the_total() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        for id in ["route-test-q1", "route-test-q2", "route-test-q3"] {
            seed_incident(&pool, id, &["QZ"], &[], 1, false, false).await;
        }
        sqlx::query(
            "UPDATE incidents SET summary = 'Signal failure at Woking' \
             WHERE incident_id IN ('route-test-q1', 'route-test-q2')",
        )
        .execute(&pool)
        .await
        .expect("set fixture summaries");

        let (status, body) = get(
            &pool,
            vec![],
            "/incidents?operator=QZ&q=signal%20wok&limit=1",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(results(&body).len(), 1);
        assert_eq!(json["total"], 2, "every match, not just this page: {body}");
        let cursor = next_cursor(&body).expect("a second page");

        let (status, body) = get(
            &pool,
            vec![],
            &format!("/incidents?operator=QZ&q=signal%20wok&limit=1&after={cursor}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(results(&body).len(), 1);
        assert_eq!(json["total"], Value::Null, "only the first page counts");

        // Blank or punctuation-only is no filter at all.
        let (status, body) = get(&pool, vec![], "/incidents?operator=QZ&q=%20%26%21").await;
        assert_eq!(status, StatusCode::OK);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["total"], 3);

        let long = "a".repeat(201);
        let (status, body) = get(&pool, vec![], &format!("/incidents?q={long}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains('q'), "400 body should name the field: {body}");
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_returns_a_null_next_cursor_when_the_page_is_the_last_one() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-7", &["VT"], &["WAT"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents?limit=100").await;
        assert_eq!(status, StatusCode::OK);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["nextCursor"],
            Value::Null,
            "nextCursor is explicit JSON null on the last page, never omitted"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_paginates_with_a_cursor_and_after_continues_from_it() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-8", &["VT"], &["WAT"], 1, false, false).await;
        seed_incident(&pool, "route-test-9", &["VT"], &["WAT"], 1, false, false).await;

        let (status, first) = get(&pool, vec![], "/incidents?limit=1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results(&first).len(), 1);
        let cursor = next_cursor(&first).expect("a second page exists");

        let (status, second) =
            get(&pool, vec![], &format!("/incidents?limit=1&after={cursor}")).await;
        assert_eq!(status, StatusCode::OK);
        let second_rows = results(&second);
        assert_eq!(second_rows.len(), 1);
        assert_ne!(
            second_rows[0]["incidentId"],
            results(&first)[0]["incidentId"],
            "`after` must continue from the cursor, not restart at page 1"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_renders_camel_case_rows_with_no_leaked_snake_case_fields() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-10", &["VT"], &["WAT"], 3, true, false).await;

        let (status, body) = get(&pool, vec![], "/incidents").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let row = rows
            .iter()
            .find(|r| r["incidentId"] == "route-test-10")
            .expect("fixture row present");
        assert_eq!(row["summary"], "Fixture incident route-test-10");
        assert_eq!(row["priority"], 3);
        assert_eq!(row["isPlanned"], true);
        assert_eq!(row["isCleared"], false);
        assert!(
            row.get("first_seen_at").is_none(),
            "no stray snake_case field"
        );
        assert!(row.get("is_planned").is_none(), "no stray snake_case field");
        assert!(
            row.get("description").is_none(),
            "list rows never include description"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_published_with_no_matches_is_200_with_an_empty_results_array() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?operator=ZZ_NO_SUCH_OPERATOR").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an unmatched filter is a 200 with an empty results array, never a 404 -- there is \
             no 'unpublished' concept for this table"
        );
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
    }

    // --- `currentlyAffectsLines` privacy gate ---------------------------
    //
    // `lines_currently_reporting_incident` is an ungated read of
    // `line_status`, which holds private custom-line rows next to the
    // public catalogue ones. Until this gate existed, this route handed
    // any anonymous caller another user's custom-line id AND name (see the
    // 2026-09-16 custom-line archive research, §5c). These are the
    // HTTP-level proofs that it no longer does -- a data-layer test can
    // show what the query returns, never what the handler emits.

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_incident_hides_another_users_custom_line -- --ignored --test-threads=1`"]
    async fn get_incident_hides_another_users_custom_line_from_anonymous_and_non_owner_callers() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        let incident_id = "route-test-privacy-1";
        seed_incident(&pool, incident_id, &["SW"], &["WOK"], 1, false, false).await;

        let owner_token = seed_session(&pool, "TEST-INCIDENT-PRIVACY-OWNER").await;
        let stranger_token = seed_session(&pool, "TEST-INCIDENT-PRIVACY-STRANGER").await;

        // A public catalogue row and a private custom row, both reporting
        // the same incident -- the catalogue one is the control: it must
        // stay visible to everyone, proving the gate drops only what it
        // should.
        seed_line_reporting(&pool, "route-test-privacy-catalogue", incident_id).await;
        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Mums House To Work".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-INCIDENT-PRIVACY-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_reporting(&pool, &custom.id, incident_id).await;

        // Anonymous: catalogue row only. Asserted twice over -- by exact
        // id, and by the blanket "no `custom-` prefix anywhere in the
        // response," so a future custom line seeded by some other fixture
        // can't slip through this assertion either.
        let (status, headers, body) = get_detail(&pool, incident_id, None).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "anonymous callers still get a 200, just fewer lines"
        );
        let anon_ids = affected_line_ids(&body);
        assert!(anon_ids.contains(&"route-test-privacy-catalogue".to_string()));
        assert!(
            !anon_ids.iter().any(|id| id.starts_with("custom-")),
            "an anonymous caller must never see a custom line: {anon_ids:?}"
        );
        assert!(
            !body.to_string().contains("Mums House To Work"),
            "the custom line's NAME must not leak either, not just its id"
        );
        assert_eq!(
            headers
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("private, no-store"),
            "a session-dependent body must not be cacheable at the edge"
        );

        // A logged-in caller who neither owns the line nor shares a group
        // with its owner sees exactly what the anonymous one does.
        let (status, _, body) = get_detail(&pool, incident_id, Some(&stranger_token)).await;
        assert_eq!(status, StatusCode::OK);
        let stranger_ids = affected_line_ids(&body);
        assert!(stranger_ids.contains(&"route-test-privacy-catalogue".to_string()));
        assert!(
            !stranger_ids.contains(&custom.id),
            "being logged in is not itself permission to read someone else's custom line: \
             {stranger_ids:?}"
        );

        // The owner, and only the owner, gets their own line back.
        let (status, _, body) = get_detail(&pool, incident_id, Some(&owner_token)).await;
        assert_eq!(status, StatusCode::OK);
        let owner_ids = affected_line_ids(&body);
        assert!(owner_ids.contains(&"route-test-privacy-catalogue".to_string()));
        assert!(
            owner_ids.contains(&custom.id),
            "the owner must still see their own custom line -- the gate filters, it does not \
             blanket-strip: {owner_ids:?}"
        );

        cleanup_line_status(&pool, "route-test-privacy-catalogue").await;
        cleanup_line_status(&pool, &custom.id).await;
        cleanup_user(&pool, "TEST-INCIDENT-PRIVACY-OWNER").await;
        cleanup_user(&pool, "TEST-INCIDENT-PRIVACY-STRANGER").await;
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_incident_shows_a_group_granted_custom_line -- --ignored --test-threads=1`"]
    async fn get_incident_shows_a_group_granted_custom_line_to_a_member_but_not_a_stranger() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        let incident_id = "route-test-privacy-2";
        seed_incident(&pool, incident_id, &["SW"], &["WOK"], 1, false, false).await;

        seed_session(&pool, "TEST-INCIDENT-GRANT-OWNER").await;
        let member_token = seed_session(&pool, "TEST-INCIDENT-GRANT-MEMBER").await;
        let stranger_token = seed_session(&pool, "TEST-INCIDENT-GRANT-STRANGER").await;

        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Incident Grant Shared Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-INCIDENT-GRANT-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_reporting(&pool, &custom.id, incident_id).await;

        let group_id = crate::data::groups::create_group(
            &pool,
            "Incident Grant Group",
            "TEST-INCIDENT-GRANT-OWNER",
        )
        .await
        .expect("create fixture group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-INCIDENT-GRANT-MEMBER")
        .execute(&pool)
        .await
        .expect("seed fixture membership");
        crate::data::groups::grant_custom_line(
            &pool,
            &group_id,
            &custom.id,
            "TEST-INCIDENT-GRANT-OWNER",
        )
        .await
        .expect("seed fixture grant");

        let (_, _, body) = get_detail(&pool, incident_id, Some(&member_token)).await;
        assert!(
            affected_line_ids(&body).contains(&custom.id),
            "a current member of a group the line is shared into may read it -- the gate is the \
             same `readable_custom_line_ids` every other custom-line read uses, not a narrower \
             owner-only check"
        );

        let (_, _, body) = get_detail(&pool, incident_id, Some(&stranger_token)).await;
        assert!(
            !affected_line_ids(&body).contains(&custom.id),
            "a logged-in non-member still sees nothing"
        );
        let (_, _, body) = get_detail(&pool, incident_id, None).await;
        assert!(
            !affected_line_ids(&body).contains(&custom.id),
            "sharing a line into a group never makes it public"
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_line_status(&pool, &custom.id).await;
        for id in [
            "TEST-INCIDENT-GRANT-OWNER",
            "TEST-INCIDENT-GRANT-MEMBER",
            "TEST-INCIDENT-GRANT-STRANGER",
        ] {
            cleanup_user(&pool, id).await;
        }
        delete_fixtures(&pool).await;
    }
}
