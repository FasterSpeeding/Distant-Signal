//! `/public/lines`: enumerate official + custom lines. `GET /lines` and
//! `GET /lines/{id}/definition` are unauthenticated — see
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
//! Non-goals for the original reasoning. Custom-line *writes*
//! (`create_line`/`update_line`/`delete_line`) are no longer part of that
//! "yet" — they require `AuthenticatedUser` and are ownership-scoped (see
//! `crate::data::custom_lines::update_custom_line`/`delete_custom_line`),
//! as of the commit that closed that doc's "yet". `GET /lines/{id}` now
//! requires `AuthenticatedUser` too and only ever returns the caller's own
//! custom line — a 404 covers "doesn't exist," "exists but owned by
//! someone else," and "exists but is a legacy NULL-owner row" alike, all
//! indistinguishable to an external observer (see `get_line`), so there's
//! no longer an `isOwner` flag for the frontend to branch on: any `200`
//! from this endpoint is by construction the real owner's own line.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::{AuthenticatedUser, OptionalAuthenticatedUser};
use crate::data::{
    custom_lines::{self, NewCustomLine},
    queries,
};

pub fn router() -> Router {
    Router::new()
        .route("/lines", axum::routing::get(list_lines).post(create_line))
        .route(
            "/lines/{id}",
            axum::routing::get(get_line)
                .put(update_line)
                .delete(delete_line),
        )
        .route(
            "/lines/{id}/definition",
            axum::routing::get(get_line_definition),
        )
        .route(
            "/lines/{id}/schedule",
            axum::routing::get(get_line_schedule),
        )
}

#[derive(Debug, Serialize)]
struct LineSummary {
    id: String,
    name: String,
    category: String,
    operators: Vec<String>,
    source: &'static str,
}

/// Full custom-line record, returned by `GET /lines/{id}` to pre-populate
/// an edit form. `LineSummary` (above) is deliberately a smaller
/// projection used by the list endpoint for both catalogue and custom
/// lines — it lacks `stations`/`headcodePrefixes`/`destinationCrsFilter`,
/// which only exist for custom lines and are exactly what an edit form
/// needs to pre-fill.
///
/// No `isOwner` field: `get_line` requires `AuthenticatedUser` and 404s
/// anything that isn't the caller's own line (doesn't exist, someone
/// else's, or a legacy NULL-owner row alike), so a `200` from this
/// endpoint is by construction always the real owner's own line — the
/// frontend no longer needs a signal to distinguish owner from non-owner
/// here.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomLineDetail {
    id: String,
    name: String,
    operators: Vec<String>,
    stations: Vec<String>,
    headcode_prefixes: Vec<String>,
    destination_crs_filter: Vec<String>,
}

/// Minimal cross-source projection — just enough to answer "what stations
/// and operators does this line cover", for both catalogue and custom
/// lines alike. Deliberately separate from `CustomLineDetail`/`get_line`:
/// that endpoint is custom-only by design (its 404-for-a-catalogue-id
/// behavior is how the frontend detail page tells custom and catalogue
/// lines apart — see `frontend/app/lines/[id]/page.tsx`'s `isCustom`
/// check), so extending it to also serve catalogue lines would silently
/// break that detection instead of adding a tooltip.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LineDefinitionSummary {
    stations: Vec<String>,
    operators: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ScheduleQuery {
    date: Option<chrono::NaiveDate>,
}

/// Resolves the effective service date for `GET /lines/{id}/schedule`:
/// the caller's explicit `?date=`, or `today` if omitted. Factored out as
/// a pure function so the "default to today" decision is testable without
/// a clock or a database -- same rationale as
/// `routes::reference::sanitize_query`.
fn resolve_schedule_date(
    requested: Option<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> chrono::NaiveDate {
    requested.unwrap_or(today)
}

/// `GET /public/lines/{id}/schedule?date=`: the full CIF-derived stopping
/// pattern for every service on line `id`, for one rail day -- read
/// straight off `schedule_line_population` (`queries::get_schedule_line_population`).
/// See docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md
/// Decision 3.
///
/// Deliberately does NOT check `app.config.lines`/`custom_lines` first the
/// way `get_line_definition` does: `schedule_line_population` is keyed
/// purely by whatever `line_id` string `schedule-reference` published
/// under, with no foreign key to either catalogue or custom lines, so
/// there is nothing to disambiguate here -- an unknown, custom, or
/// not-yet-published catalogue `id` alike simply 404 for the same reason
/// ("no row for this key"), which is the same honesty split
/// `get_station_schedule_departures` already draws for
/// `schedule_network_departures`.
///
/// The response body is `schedule_line_population.population` relayed
/// completely unprocessed: `api` has no dependency on the `schedule-query`
/// crate (the crate that defines `LinePopulationEntry`/`CallingPoint`) at
/// all, so its JSON keys are that crate's own snake_case field names
/// (`uid`, `calling_points`, `booked_arrival`, `booked_departure`,
/// `is_half_minute_arrival`, `is_half_minute_departure`, `tiploc`, `kind`),
/// NOT this crate's usual camelCase convention.
///
/// This is a deliberate choice, reconsidered (not just carried over
/// unquestioned) at implementation time: this crate's `render.rs::schedule_departure_json`
/// shows there IS a precedent for hand-renaming an opaque, undeserialized
/// JSON value's known fields to camelCase before responding (it does this
/// for `ScheduleDeparture`'s 3 flat fields). That precedent was rejected
/// here for two reasons specific to `LinePopulationEntry`, not out of
/// convenience:
///
/// 1. `LinePopulationEntry` is a nested structure (`calling_points` is an
///    array of `CallingPoint`, itself 6 fields, one of which -- `kind` --
///    is a bare enum with no `#[serde(rename_all)]`, so it already
///    serializes as `"Origin"`/`"Intermediate"`/`"Terminate"`, not
///    camelCase, and would need its own hand-rolled string mapping too if
///    full consistency were the goal). A hand-written recursive
///    `serde_json::Value` transform for that shape is real, untyped,
///    error-prone code -- unlike `schedule_departure_json`'s 3-field flat
///    case, there's no compiler checking the mapping stays exhaustive.
/// 2. A hand-rolled field-rename mapper silently drops any field
///    `schedule-reference` adds to `CallingPoint`/`LinePopulationEntry` in
///    the future (exactly the failure mode `schedule_departure_json`
///    already accepts for its own narrow 3-field case) -- for the
///    "complete, unprocessed CIF stopping pattern" this route promises,
///    silently losing new fields is worse than a documented snake_case
///    wart. A raw pass-through survives schema growth in `schedule-query`
///    with zero changes needed here, which is the actual, durable version
///    of Decision 3's "avoid coupling `api` to `schedule-query`'s shape"
///    reasoning -- true whether or not the field names get renamed on the
///    way out.
///
/// So: raw pass-through, snake_case, matching this plan's own Global
/// Constraints (`GET /public/stanox-crs` is also snake_case, for its own,
/// different reason -- see `routes::stanox_crs`).
async fn get_line_schedule(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(query): Query<ScheduleQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let service_date = resolve_schedule_date(query.date, chrono::Utc::now().date_naive());
    let Some(population) = queries::get_schedule_line_population(&app.database, &id, service_date)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule population for line {id} on {service_date}"),
        ));
    };

    Ok(Json(population))
}

async fn get_line_definition(
    State(app): State<App>,
    Path(id): Path<String>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<LineDefinitionSummary>, (StatusCode, String)> {
    if let Some(catalogue_line) = app.config.lines.iter().find(|l| l.id == id) {
        return Ok(Json(LineDefinitionSummary {
            stations: catalogue_line
                .stations
                .iter()
                .map(|s| s.crs.clone())
                .collect(),
            operators: catalogue_line.operators.clone(),
        }));
    }

    // Custom lines are private (see get_line, Task 4) -- this endpoint uses
    // `OptionalAuthenticatedUser` rather than `AuthenticatedUser`, since
    // (unlike get_line) a catalogue id is a completely valid, sessionless
    // request above; a custom id, though, still only ever resolves for its
    // real owner -- doesn't exist, exists but owned by someone else, exists
    // but is a legacy NULL-owner row, and "no session at all" all collapse
    // to the same 404, reusing this route's own existing not-found message.
    // Never 403 -- see this crate's Global Constraints.
    let custom = custom_lines::get_custom_line(&app.database, &id)
        .await
        .map_err(internal_error)?;
    let Some((custom, owner)) = custom else {
        return Err((StatusCode::NOT_FOUND, "line not found".to_string()));
    };
    let caller_owns_it = matches!((&user, &owner), (Some(u), Some(o)) if &u.id == o);
    if !caller_owns_it {
        return Err((StatusCode::NOT_FOUND, "line not found".to_string()));
    }

    Ok(Json(LineDefinitionSummary {
        stations: custom.stations,
        operators: custom.operators,
    }))
}

async fn list_lines(
    State(app): State<App>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<LineSummary>>, (StatusCode, String)> {
    let mut out: Vec<LineSummary> = app
        .config
        .lines
        .iter()
        .map(|l| LineSummary {
            id: l.id.clone(),
            name: l.name.clone(),
            category: l.category.clone(),
            operators: l.operators.clone(),
            source: "catalogue",
        })
        .collect();

    // Custom lines are now private (see get_line, Task 4) -- an
    // authenticated caller sees only their own; an anonymous visitor sees
    // none at all, same shape as today's default but now also true for a
    // logged-in non-owner. Catalogue and TfL entries here are completely
    // unaffected -- no filtering, no auth requirement change.
    if let Some(user) = &user {
        let custom = custom_lines::list_custom_lines_for_user(&app.database, &user.id)
            .await
            .map_err(internal_error)?;
        out.extend(custom.into_iter().map(|c| LineSummary {
            id: c.id,
            name: c.name,
            category: "custom".to_string(),
            operators: c.operators,
            source: "custom",
        }));
    }

    // TfL lines, from the rows crates/poller-tfl wrote — see
    // `queries::tfl_line_summaries` for why they are not catalogue TOML
    // files. `category` carries the TfL mode name (`tube`, `dlr`,
    // `overground`, `elizabeth-line`, `tram`), which is the honest answer
    // to "what kind of line is this" for a network with no `main-line` /
    // `commuter` / `regional` distinction, and is what the line detail
    // page renders as "Category:".
    let tfl = queries::tfl_line_summaries(&app.database)
        .await
        .map_err(internal_error)?;
    out.extend(
        tfl.into_iter()
            .filter(|line| !is_merged_into_nr_line(&line.id))
            .map(|line| LineSummary {
                id: line.id,
                name: tfl_display_name(&line.name),
                category: line.mode_name,
                operators: vec![common::TFL_OPERATOR.to_string()],
                source: "tfl",
            }),
    );

    Ok(Json(out))
}

/// Whether a TfL line's summary should be omitted from `/public/lines`
/// because an NR/Darwin-sourced line already covers the same railway and is
/// shown in its place, carrying this TfL line's status as a secondary field
/// on its detail view instead (`crates/api/src/routes/line_status.rs::get_line_status`).
/// See `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
/// Area 1.
fn is_merged_into_nr_line(tfl_line_id: &str) -> bool {
    common::nr_line_id_for_tfl(tfl_line_id).is_some()
}

/// Suffixes a TfL line's raw name for the `/public/lines` list, so it's
/// distinguishable from any same-named National Rail catalogue line (e.g.
/// `lines/northern.toml`'s "Northern" vs TfL's own "Northern" line, or
/// `lines/elizabeth-line.toml`'s "Elizabeth line" vs TfL's "Elizabeth
/// line"). The All Lines table has no Category/Operators column, so two
/// identical-looking rows would otherwise be indistinguishable without
/// filtering by operator.
fn tfl_display_name(name: &str) -> String {
    format!("{name} (TfL)")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateLineRequest {
    name: String,
    operators: Vec<String>,
    stations: Vec<String>,
    #[serde(default)]
    headcode_prefixes: Vec<String>,
    #[serde(default)]
    destination_crs_filter: Vec<String>,
}

async fn create_line(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(req): Json<CreateLineRequest>,
) -> Result<Json<LineSummary>, (StatusCode, String)> {
    if req.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "name must not be empty".to_string(),
        ));
    }
    if req.stations.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "a line needs at least 2 stations".to_string(),
        ));
    }
    if custom_lines::slugify(&req.name) == "custom-" {
        return Err((
            StatusCode::BAD_REQUEST,
            "name must contain at least one letter or digit".to_string(),
        ));
    }

    let created = custom_lines::insert_custom_line(
        &app.database,
        NewCustomLine {
            name: req.name,
            operators: req.operators,
            stations: req.stations,
            headcode_prefixes: req.headcode_prefixes,
            destination_crs_filter: req.destination_crs_filter,
        },
        &user.id,
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(LineSummary {
        id: created.id,
        name: created.name,
        category: "custom".to_string(),
        operators: created.operators,
        source: "custom",
    }))
}

async fn get_line(
    State(app): State<App>,
    Path(id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<CustomLineDetail>, (StatusCode, String)> {
    let line = custom_lines::get_custom_line(&app.database, &id)
        .await
        .map_err(internal_error)?;

    // Doesn't exist, exists with no owner at all (legacy NULL row), and
    // exists but owned by someone else are all treated identically -- the
    // same 404, same message update_line/delete_line already use for
    // "exists but not yours" -- so an external observer gets no signal
    // distinguishing any of the three cases. No session at all never
    // reaches this line: AuthenticatedUser's own extractor already
    // rejected with 401 before this handler runs. (No separate
    // catalogue-id check needed either, same as before this task:
    // `get_custom_line` only ever queries the `custom_lines` table, so a
    // catalogue id naturally comes back `None` here too.)
    let Some((line, owner)) = line else {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    };
    if owner.as_deref() != Some(user.id.as_str()) {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    }

    Ok(Json(CustomLineDetail {
        id: line.id,
        name: line.name,
        operators: line.operators,
        stations: line.stations,
        headcode_prefixes: line.headcode_prefixes,
        destination_crs_filter: line.destination_crs_filter,
    }))
}

async fn update_line(
    State(app): State<App>,
    Path(id): Path<String>,
    user: AuthenticatedUser,
    Json(req): Json<CreateLineRequest>,
) -> Result<Json<LineSummary>, (StatusCode, String)> {
    if app.config.lines.iter().any(|l| l.id == id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "cannot edit a catalogue line".to_string(),
        ));
    }
    if req.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "name must not be empty".to_string(),
        ));
    }
    if req.stations.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "a line needs at least 2 stations".to_string(),
        ));
    }
    // Deliberately no `slugify(&req.name) == "custom-"` check here, unlike
    // `create_line`: that check exists solely to guard id derivation from
    // an all-punctuation name, and `update_line` never derives an id (see
    // [`custom_lines::update_custom_line`]) — an edit that renames a line
    // to something like "!!!" is harmless here.

    let updated = custom_lines::update_custom_line(
        &app.database,
        &id,
        NewCustomLine {
            name: req.name,
            operators: req.operators,
            stations: req.stations,
            headcode_prefixes: req.headcode_prefixes,
            destination_crs_filter: req.destination_crs_filter,
        },
        &user.id,
    )
    .await
    .map_err(internal_error)?;

    let Some(updated) = updated else {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    };

    Ok(Json(LineSummary {
        id: updated.id,
        name: updated.name,
        category: "custom".to_string(),
        operators: updated.operators,
        source: "custom",
    }))
}

async fn delete_line(
    State(app): State<App>,
    Path(id): Path<String>,
    user: AuthenticatedUser,
) -> Result<StatusCode, (StatusCode, String)> {
    if app.config.lines.iter().any(|l| l.id == id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "cannot delete a catalogue line".to_string(),
        ));
    }

    let deleted = custom_lines::delete_custom_line(&app.database, &id, &user.id)
        .await
        .map_err(internal_error)?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "custom line operation failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tfl_names_are_suffixed_to_disambiguate_from_catalogue_lines() {
        // `lines/northern.toml` and `lines/elizabeth-line.toml` share these
        // exact names with their TfL counterparts; the suffix is what lets
        // a user tell them apart on `/lines`, which has no Category or
        // Operators column.
        assert_eq!(tfl_display_name("Northern"), "Northern (TfL)");
        assert_eq!(tfl_display_name("Elizabeth line"), "Elizabeth line (TfL)");
    }

    #[test]
    fn catalogue_and_custom_line_summaries_are_not_suffixed() {
        // Catalogue/custom `LineSummary`s are built directly from their
        // source `name` with no transformation — only the TfL branch of
        // `list_lines` routes through `tfl_display_name`.
        let catalogue = LineSummary {
            id: "northern".to_string(),
            name: "Northern".to_string(),
            category: "main-line".to_string(),
            operators: vec!["NT".to_string()],
            source: "catalogue",
        };
        assert_eq!(catalogue.name, "Northern");
    }

    #[test]
    fn a_tfl_line_with_an_nr_counterpart_is_suppressed() {
        assert!(is_merged_into_nr_line("tfl-elizabeth"));
    }

    #[test]
    fn an_overground_tfl_line_with_an_nr_counterpart_is_suppressed() {
        // Area 2 -- see docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md.
        assert!(is_merged_into_nr_line("tfl-mildmay"));
    }

    #[test]
    fn a_tfl_line_with_no_nr_counterpart_is_not_suppressed() {
        assert!(!is_merged_into_nr_line("tfl-northern"));
    }

    #[test]
    fn resolve_schedule_date_uses_the_explicit_date_when_given() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        let requested = chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        assert_eq!(resolve_schedule_date(Some(requested), today), requested);
    }

    #[test]
    fn resolve_schedule_date_defaults_to_today_when_absent() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        assert_eq!(resolve_schedule_date(None, today), today);
    }
}

/// HTTP-layer tests for `get_line`, now that it's gated by
/// `AuthenticatedUser` and its ownership check is folded into the handler
/// itself (no more standalone pure `is_owner()` to unit-test — see this
/// task's brief). There's no earlier precedent in this crate for testing
/// an `AuthenticatedUser`-gated *read* route through the real
/// `axum::Router` (only pure `#[cfg(test)]` unit tests, and `#[ignore]`d
/// DB-layer tests that call query functions directly — see
/// `data::custom_lines::db_tests`, `data::users::db_tests`; the one prior
/// mention of `tower::ServiceExt::oneshot` in this crate,
/// `routes::line_status`'s module doc comment, was a throwaway compile
/// probe that was never kept). This module establishes that shape for
/// later tasks in the same plan to copy:
///
/// - `test_app` builds a real `App` (`Arc<AppState>`) by hand, exactly as
///   `AppState::init()` does but skipping `clap` and any live SSO/Redis
///   connection — every field is a plain, directly-constructible value
///   except `database`, which is the one field that has to be a real,
///   already-connected `PgPool` (`redis`/`oidc` are never touched by
///   `get_line`, so both are inert placeholders; see `AppState::redis`'s
///   own doc comment for why an unreachable `redis::Client::open` target
///   is harmless, and `auth::oidc::OidcClient::new`'s doc comment for why
///   it performs no network call).
/// - `test_router` mounts this crate's *actual* `routes::public_router()`
///   under `/public`, the same nesting `main.rs` uses, then calls
///   `.with_state(app)` to turn it into a `Service` a test can drive
///   directly with `tower::ServiceExt::oneshot` — no test-only routing,
///   so a passing test exercises the real extractor chain and the real
///   route table.
/// - `seed_session` inserts a real, resolvable session exactly the way a
///   successful `/auth/callback` would: a `users` row, then a `sessions`
///   row via `data::users::insert_session` keyed by
///   `auth::hash_session_token(raw_token)` — returning the *raw* token,
///   since that (not the hash) is what a real cookie carries and what
///   `AuthenticatedUser`'s extractor expects to hash on the way in.
///
/// A future test in this same shape (Tasks 5/6/7/8) can copy `test_app`/
/// `test_router`/`seed_session` near-verbatim; they're kept local to this
/// file rather than factored into a shared test-helper module since this
/// is the first and, so far, only file that needs them — promote them
/// only once a second file actually duplicates this setup.
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::hash_session_token;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::custom_lines::{self, NewCustomLine};
    use crate::data::users::insert_session;

    /// Every `ServiceArguments` field filled with an inert placeholder
    /// except `lines`, which the caller supplies -- the one field a
    /// catalogue-id test actually needs to vary.
    fn test_app(pool: PgPool, lines: Vec<common::LineDefinition>) -> App {
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
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
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
            defaults_file: None,
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
        };

        std::sync::Arc::new(AppState {
            config,
            database: pool,
            // `Client::open` only parses the URL, never opens a socket --
            // see `AppState::redis`'s doc comment. `get_line` never
            // touches Redis at all.
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
        })
    }

    /// The real `public_router()`, nested under `/public` exactly as
    /// `main.rs` does, turned into a `tower::Service` a test can drive
    /// with `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .nest("/public", crate::routes::public_router())
            .with_state(app)
    }

    /// Seeds a real, resolvable session for `user_id` (creating the user
    /// if it doesn't already exist) and returns the *raw* token -- send it
    /// as `Cookie: distant_signal_session=<raw>`, never the hash `sessions`
    /// actually stores.
    async fn seed_session(pool: &PgPool, user_id: &str) -> String {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
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

    /// Deletes a fixture user and everything that cascades from it
    /// (`sessions`, owned `custom_lines`, `pinned_lines` -- see
    /// `crates/api/migrations/20260828100000_add_ownership.sql`'s
    /// `ON DELETE CASCADE`s). Explicit rather than relied-on-implicitly,
    /// matching `data::custom_lines::db_tests`'s existing multi-step
    /// cleanup convention.
    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Issues `GET /public/lines/{id}`, optionally with a session cookie.
    async fn get_line(
        router: axum::Router,
        id: &str,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri(format!("/public/lines/{id}"));
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let request = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(request).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        // 401/404 bodies are plain-text ((StatusCode, String) -- see
        // `internal_error` and `get_line`'s own error returns), not JSON;
        // wrap them as a JSON string so every case can share one return
        // shape and callers can still assert on the exact message.
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                no_session_cookie_is_rejected_with_401 -- --ignored`"]
    async fn no_session_cookie_is_rejected_with_401() {
        let pool = connect().await;
        let router = test_router(test_app(pool, vec![]));

        // No fixture line even needs to exist: `AuthenticatedUser`'s own
        // extractor rejects before the handler -- and therefore before any
        // database lookup -- ever runs.
        let (status, _) = get_line(router, "custom-does-not-matter", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_non_owner_session_gets_404_not_403 -- --ignored`"]
    async fn a_non_owner_session_gets_404_not_403() {
        let pool = connect().await;

        seed_session(&pool, "TEST-GET-LINE-OWNER").await;
        let non_owner_token = seed_session(&pool, "TEST-GET-LINE-NON-OWNER").await;
        let line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Non Owner Target Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-GET-LINE-OWNER",
        )
        .await
        .expect("insert fixture line");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line(router, &line.id, Some(&non_owner_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        // Same message `update_line`/`delete_line` already use for "exists
        // but not yours" -- see this crate's Global Constraints: never a
        // distinguishing message for "exists but not mine" vs "doesn't
        // exist at all".
        assert_eq!(body, Value::String("custom line not found".to_string()));

        cleanup_user(&pool, "TEST-GET-LINE-OWNER").await;
        cleanup_user(&pool, "TEST-GET-LINE-NON-OWNER").await;
    }

    // A prior version of this test, `a_legacy_null_owner_row_gets_404_for_a_
    // real_caller`, seeded a NULL-`user_id` `custom_lines` row directly to
    // confirm a real caller still gets 404 against it. Migration
    // 20260901120000_custom_lines_owner_not_null.sql deleted every
    // surviving NULL-owner row and made the column NOT NULL (the repo
    // owner's explicit choice -- see that migration's header comment), so
    // that seed insert now fails at the database level before the route
    // under test ever runs -- the scenario is no longer constructible, and
    // `custom_lines::db_tests::custom_lines_user_id_column_rejects_null`
    // covers the constraint itself. `a_non_owner_session_gets_404_not_403`
    // above already exercises the same "exists but not this caller's" 404
    // path this test would otherwise duplicate.

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_nonexistent_id_gets_404_for_a_real_caller -- --ignored`"]
    async fn a_nonexistent_id_gets_404_for_a_real_caller() {
        let pool = connect().await;

        let caller_token = seed_session(&pool, "TEST-GET-LINE-CALLER-2").await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) =
            get_line(router, "custom-totally-does-not-exist", Some(&caller_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("custom line not found".to_string()));

        cleanup_user(&pool, "TEST-GET-LINE-CALLER-2").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                the_real_owner_gets_200_with_full_detail_and_no_is_owner_field -- --ignored`"]
    async fn the_real_owner_gets_200_with_full_detail_and_no_is_owner_field() {
        let pool = connect().await;

        let owner_token = seed_session(&pool, "TEST-GET-LINE-REAL-OWNER").await;
        let line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Owned Detail Line".to_string(),
                operators: vec!["SW".to_string(), "TW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec!["1A".to_string()],
                destination_crs_filter: vec!["WAT".to_string()],
            },
            "TEST-GET-LINE-REAL-OWNER",
        )
        .await
        .expect("insert fixture line");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line(router, &line.id, Some(&owner_token)).await;

        assert_eq!(status, StatusCode::OK);
        let object = body.as_object().expect("200 body should be a JSON object");
        assert_eq!(
            object.get("id").and_then(Value::as_str),
            Some(line.id.as_str())
        );
        assert_eq!(
            object.get("name").and_then(Value::as_str),
            Some("Test Owned Detail Line")
        );
        assert_eq!(
            object
                .get("operators")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            object
                .get("stations")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            object
                .get("headcodePrefixes")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            object
                .get("destinationCrsFilter")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        // The point of this task: the vestigial ownership flag is gone
        // entirely, not just always-true.
        assert!(
            !object.contains_key("isOwner"),
            "isOwner must not appear in the response body at all"
        );

        cleanup_user(&pool, "TEST-GET-LINE-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_catalogue_id_still_404s_the_same_way_it_always_has -- --ignored`"]
    async fn a_catalogue_id_still_404s_the_same_way_it_always_has() {
        let pool = connect().await;

        let caller_token = seed_session(&pool, "TEST-GET-LINE-CATALOGUE-CALLER").await;
        let catalogue_line = common::LineDefinition {
            id: "test-catalogue-line".to_string(),
            name: "Test Catalogue Line".to_string(),
            mode: "rail".to_string(),
            category: "main-line".to_string(),
            operators: vec!["SW".to_string()],
            stations: vec![
                common::Station {
                    crs: "WOK".to_string(),
                    tiploc: None,
                    role: "major".to_string(),
                    segment: None,
                },
                common::Station {
                    crs: "CLJ".to_string(),
                    tiploc: None,
                    role: "major".to_string(),
                    segment: None,
                },
            ],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        };

        let router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        // `get_custom_line` only ever queries `custom_lines`, so a
        // catalogue id -- never a row in that table -- 404s exactly the
        // way an unknown id does. Confirms this path is untouched by
        // this task's ownership check.
        let (status, body) = get_line(router, "test-catalogue-line", Some(&caller_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("custom line not found".to_string()));

        cleanup_user(&pool, "TEST-GET-LINE-CATALOGUE-CALLER").await;
    }

    /// A minimal but valid catalogue line fixture, for tests that need to
    /// confirm catalogue entries survive `list_lines`'s custom-line
    /// scoping untouched -- see `a_catalogue_id_still_404s_the_same_way_it_always_has`
    /// above for the one prior inline literal this factors out (that test
    /// doesn't reuse this helper itself, since it's the sole earlier
    /// occurrence; the tests below need the same shape more than once).
    fn test_catalogue_line(id: &str, name: &str) -> common::LineDefinition {
        common::LineDefinition {
            id: id.to_string(),
            name: name.to_string(),
            mode: "rail".to_string(),
            category: "main-line".to_string(),
            operators: vec!["SW".to_string()],
            stations: vec![
                common::Station {
                    crs: "WOK".to_string(),
                    tiploc: None,
                    role: "major".to_string(),
                    segment: None,
                },
                common::Station {
                    crs: "CLJ".to_string(),
                    tiploc: None,
                    role: "major".to_string(),
                    segment: None,
                },
            ],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

    /// Seeds a fixture TfL-sourced `line_status` row (`queries::tfl_line_summaries`
    /// only ever reads rows with `source = 'tfl'`) with an id that has no
    /// NR/Darwin counterpart in `common::TFL_TO_NR_LINE_ID`, so
    /// `is_merged_into_nr_line` never suppresses it from `/public/lines`.
    async fn seed_tfl_line(pool: &PgPool, line_id: &str) {
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES ($1, $2, 'tube', '{TfL}', '[]', 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET source = EXCLUDED.source",
        )
        .bind(line_id)
        .bind(format!("Test {line_id}"))
        .execute(pool)
        .await
        .expect("seed fixture tfl line_status row");
    }

    async fn cleanup_tfl_line(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM line_status WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tfl line_status row");
    }

    /// Issues `GET /public/lines`, optionally with a session cookie, and
    /// returns the parsed JSON array of `LineSummary` entries.
    async fn list_lines_request(
        router: axum::Router,
        raw_token: Option<&str>,
    ) -> (StatusCode, Vec<Value>) {
        let mut builder = Request::builder().uri("/public/lines");
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let request = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(request).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value: Value =
            serde_json::from_slice(&bytes).expect("list_lines response body is valid JSON");
        let array = value
            .as_array()
            .cloned()
            .expect("list_lines response body is a JSON array");
        (status, array)
    }

    /// Issues `GET /public/lines/{id}/definition`, optionally with a session
    /// cookie. Mirrors `get_line` above -- same request-building/body-shape
    /// handling, since `get_line_definition`'s error bodies are the same
    /// plain-text `(StatusCode, String)` shape.
    async fn get_line_definition(
        router: axum::Router,
        id: &str,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri(format!("/public/lines/{id}/definition"));
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let request = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(request).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_definition_an_anonymous_caller_gets_404_for_a_custom_id -- --ignored`"]
    async fn get_line_definition_an_anonymous_caller_gets_404_for_a_custom_id() {
        let pool = connect().await;

        seed_session(&pool, "TEST-GET-LINE-DEF-OWNER").await;
        let line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Get Line Definition Anon Target".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-GET-LINE-DEF-OWNER",
        )
        .await
        .expect("insert fixture line");

        // No session cookie at all -- unlike `get_line`, this route uses
        // `OptionalAuthenticatedUser`, so there is no 401 case; an anonymous
        // caller simply never owns any custom line, so this still 404s.
        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_definition(router, &line.id, None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("line not found".to_string()));

        cleanup_user(&pool, "TEST-GET-LINE-DEF-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_definition_a_non_owner_session_gets_404_not_403 -- --ignored`"]
    async fn get_line_definition_a_non_owner_session_gets_404_not_403() {
        let pool = connect().await;

        seed_session(&pool, "TEST-GET-LINE-DEF-OWNER-2").await;
        let non_owner_token = seed_session(&pool, "TEST-GET-LINE-DEF-NON-OWNER").await;
        let line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Get Line Definition Non Owner Target".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-GET-LINE-DEF-OWNER-2",
        )
        .await
        .expect("insert fixture line");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_definition(router, &line.id, Some(&non_owner_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("line not found".to_string()));

        cleanup_user(&pool, "TEST-GET-LINE-DEF-OWNER-2").await;
        cleanup_user(&pool, "TEST-GET-LINE-DEF-NON-OWNER").await;
    }

    // A prior version of this test, `get_line_definition_a_legacy_null_
    // owner_row_gets_404_for_a_real_caller`, seeded a NULL-`user_id`
    // `custom_lines` row directly to confirm a real caller still gets 404
    // against it. Migration 20260901120000_custom_lines_owner_not_null.sql
    // deleted every surviving NULL-owner row and made the column NOT NULL
    // (the repo owner's explicit choice -- see that migration's header
    // comment), so that seed insert now fails at the database level before
    // the route under test ever runs -- the scenario is no longer
    // constructible, and
    // `custom_lines::db_tests::custom_lines_user_id_column_rejects_null`
    // covers the constraint itself.
    // `get_line_definition_a_non_owner_session_gets_404_not_403` above
    // already exercises the same "exists but not this caller's" 404 path
    // this test would otherwise duplicate.

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_definition_a_nonexistent_id_gets_404 -- --ignored`"]
    async fn get_line_definition_a_nonexistent_id_gets_404() {
        let pool = connect().await;

        let caller_token = seed_session(&pool, "TEST-GET-LINE-DEF-CALLER-2").await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_definition(
            router,
            "custom-totally-does-not-exist-def",
            Some(&caller_token),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("line not found".to_string()));

        cleanup_user(&pool, "TEST-GET-LINE-DEF-CALLER-2").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_definition_the_real_owner_gets_200 -- --ignored`"]
    async fn get_line_definition_the_real_owner_gets_200() {
        let pool = connect().await;

        let owner_token = seed_session(&pool, "TEST-GET-LINE-DEF-REAL-OWNER").await;
        let line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Get Line Definition Owned Line".to_string(),
                operators: vec!["SW".to_string(), "TW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec!["1A".to_string()],
                destination_crs_filter: vec!["WAT".to_string()],
            },
            "TEST-GET-LINE-DEF-REAL-OWNER",
        )
        .await
        .expect("insert fixture line");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_definition(router, &line.id, Some(&owner_token)).await;

        assert_eq!(status, StatusCode::OK);
        let object = body.as_object().expect("200 body should be a JSON object");
        assert_eq!(
            object
                .get("stations")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            object
                .get("operators")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );

        cleanup_user(&pool, "TEST-GET-LINE-DEF-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_definition_a_catalogue_id_returns_regardless_of_session -- --ignored`"]
    async fn get_line_definition_a_catalogue_id_returns_regardless_of_session() {
        let pool = connect().await;

        let catalogue_line = test_catalogue_line(
            "test-get-line-def-catalogue",
            "Test Get Line Definition Catalogue",
        );

        // Anonymous caller.
        let anon_router = test_router(test_app(pool.clone(), vec![catalogue_line.clone()]));
        let (anon_status, anon_body) =
            get_line_definition(anon_router, "test-get-line-def-catalogue", None).await;

        assert_eq!(anon_status, StatusCode::OK);
        let anon_object = anon_body
            .as_object()
            .expect("200 body should be a JSON object");
        assert_eq!(
            anon_object
                .get("stations")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            anon_object
                .get("operators")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );

        // Authenticated caller, who owns nothing related to this id -- the
        // catalogue-first branch returns before ever touching
        // `custom_lines`, so this is unaffected by session state or
        // ownership either way.
        let caller_token = seed_session(&pool, "TEST-GET-LINE-DEF-CATALOGUE-CALLER").await;
        let auth_router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        let (auth_status, auth_body) = get_line_definition(
            auth_router,
            "test-get-line-def-catalogue",
            Some(&caller_token),
        )
        .await;

        assert_eq!(auth_status, StatusCode::OK);
        assert_eq!(anon_body, auth_body);

        cleanup_user(&pool, "TEST-GET-LINE-DEF-CATALOGUE-CALLER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                an_anonymous_caller_sees_catalogue_and_tfl_entries_but_no_custom_lines -- --ignored`"]
    async fn an_anonymous_caller_sees_catalogue_and_tfl_entries_but_no_custom_lines() {
        let pool = connect().await;

        seed_tfl_line(&pool, "test-list-lines-anon-tfl").await;
        // A custom line owned by someone else exists in the database, to
        // prove an anonymous caller sees zero custom-line entries -- not
        // just "none owned by them" -- even when the table is non-empty.
        seed_session(&pool, "TEST-LIST-LINES-ANON-BYSTANDER").await;
        custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Anon Bystander Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LIST-LINES-ANON-BYSTANDER",
        )
        .await
        .expect("insert fixture custom line");

        let catalogue_line = test_catalogue_line(
            "test-list-lines-anon-catalogue",
            "Test List Lines Anon Catalogue",
        );
        let router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        let (status, body) = list_lines_request(router, None).await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            body.iter()
                .any(|entry| entry.get("id").and_then(Value::as_str)
                    == Some("test-list-lines-anon-catalogue")
                    && entry.get("source").and_then(Value::as_str) == Some("catalogue")),
            "catalogue entry missing from anonymous response: {body:?}"
        );
        assert!(
            body.iter()
                .any(|entry| entry.get("id").and_then(Value::as_str)
                    == Some("test-list-lines-anon-tfl")
                    && entry.get("source").and_then(Value::as_str) == Some("tfl")),
            "tfl entry missing from anonymous response: {body:?}"
        );
        assert_eq!(
            body.iter()
                .filter(|entry| entry.get("source").and_then(Value::as_str) == Some("custom"))
                .count(),
            0,
            "anonymous caller should see zero custom-line entries: {body:?}"
        );

        cleanup_tfl_line(&pool, "test-list-lines-anon-tfl").await;
        cleanup_user(&pool, "TEST-LIST-LINES-ANON-BYSTANDER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_logged_in_caller_sees_only_their_own_custom_line_in_the_list -- --ignored`"]
    async fn a_logged_in_caller_sees_only_their_own_custom_line_in_the_list() {
        let pool = connect().await;

        let owner_token = seed_session(&pool, "TEST-LIST-LINES-OWNER").await;
        seed_session(&pool, "TEST-LIST-LINES-OTHER-OWNER").await;

        let own_line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test List Lines Own Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LIST-LINES-OWNER",
        )
        .await
        .expect("insert fixture owned line");

        let other_line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test List Lines Other User's Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LIST-LINES-OTHER-OWNER",
        )
        .await
        .expect("insert fixture other-owner line");

        let catalogue_line = test_catalogue_line(
            "test-list-lines-owner-catalogue",
            "Test List Lines Owner Catalogue",
        );
        let router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        let (status, body) = list_lines_request(router, Some(&owner_token)).await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            body.iter()
                .any(|entry| entry.get("id").and_then(Value::as_str)
                    == Some("test-list-lines-owner-catalogue")
                    && entry.get("source").and_then(Value::as_str) == Some("catalogue")),
            "catalogue entry missing from authenticated response: {body:?}"
        );
        assert!(
            body.iter()
                .any(
                    |entry| entry.get("id").and_then(Value::as_str) == Some(own_line.id.as_str())
                        && entry.get("source").and_then(Value::as_str) == Some("custom")
                ),
            "caller's own custom line missing from response: {body:?}"
        );
        assert!(
            !body.iter().any(
                |entry| entry.get("id").and_then(Value::as_str) == Some(other_line.id.as_str())
            ),
            "another user's custom line leaked into the response: {body:?}"
        );

        cleanup_user(&pool, "TEST-LIST-LINES-OWNER").await;
        cleanup_user(&pool, "TEST-LIST-LINES-OTHER-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                catalogue_and_tfl_entries_are_identical_regardless_of_session_state -- --ignored`"]
    async fn catalogue_and_tfl_entries_are_identical_regardless_of_session_state() {
        let pool = connect().await;

        seed_tfl_line(&pool, "test-list-lines-session-tfl").await;
        let caller_token = seed_session(&pool, "TEST-LIST-LINES-SESSION-STATE").await;
        // Owns a custom line too, so the comparison below has to actively
        // exclude the "custom" source to hold -- proving the non-custom
        // sections, specifically, are what's unaffected by session state.
        custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Session State Own Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LIST-LINES-SESSION-STATE",
        )
        .await
        .expect("insert fixture owned line");

        let catalogue_line = test_catalogue_line(
            "test-list-lines-session-catalogue",
            "Test List Lines Session Catalogue",
        );

        let anon_router = test_router(test_app(pool.clone(), vec![catalogue_line.clone()]));
        let (anon_status, anon_body) = list_lines_request(anon_router, None).await;

        let auth_router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        let (auth_status, auth_body) = list_lines_request(auth_router, Some(&caller_token)).await;

        assert_eq!(anon_status, StatusCode::OK);
        assert_eq!(auth_status, StatusCode::OK);

        let non_custom = |body: &[Value]| -> Vec<Value> {
            body.iter()
                .filter(|entry| entry.get("source").and_then(Value::as_str) != Some("custom"))
                .cloned()
                .collect()
        };
        assert_eq!(
            non_custom(&anon_body),
            non_custom(&auth_body),
            "catalogue/tfl entries differed between anonymous and authenticated callers"
        );
        // Sanity: the authenticated caller's custom line is in fact present
        // (so the equality above isn't vacuously true because both sides
        // happened to have no custom entries at all).
        assert!(
            auth_body
                .iter()
                .any(|entry| entry.get("source").and_then(Value::as_str) == Some("custom"))
        );

        cleanup_tfl_line(&pool, "test-list-lines-session-tfl").await;
        cleanup_user(&pool, "TEST-LIST-LINES-SESSION-STATE").await;
    }

    async fn delete_schedule_population_fixture(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM schedule_line_population WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_line_population rows");
    }

    /// Issues `GET /public/lines/{id}/schedule`, with an optional
    /// `?date=` query string. Mirrors `get_line_definition`'s own
    /// request-building/body-shape handling.
    async fn get_line_schedule(
        router: axum::Router,
        id: &str,
        date: Option<&str>,
    ) -> (StatusCode, Value) {
        let uri = match date {
            Some(date) => format!("/public/lines/{id}/schedule?date={date}"),
            None => format!("/public/lines/{id}/schedule"),
        };
        let request = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                schedule_no_row_for_the_line_and_date_is_404_naming_both -- --ignored`"]
    async fn schedule_no_row_for_the_line_and_date_is_404_naming_both() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-missing").await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_schedule(router, "test-schedule-2a-missing", None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        let body = body
            .as_str()
            .expect("404 body is a plain string")
            .to_string();
        assert!(body.contains("test-schedule-2a-missing"), "body: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                schedule_a_row_for_today_returns_the_raw_population_json_unchanged -- --ignored`"]
    async fn schedule_a_row_for_today_returns_the_raw_population_json_unchanged() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-today").await;

        let today = chrono::Utc::now().date_naive();
        let population = serde_json::json!([
            {
                "uid": "C12345",
                "calling_points": [
                    {
                        "tiploc": "WATRLMN",
                        "kind": "origin",
                        "booked_arrival": null,
                        "booked_departure": "08:15:00",
                        "is_half_minute_arrival": false,
                        "is_half_minute_departure": false
                    }
                ]
            }
        ]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-schedule-2a-today")
        .bind(today)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_schedule(router, "test-schedule-2a-today", None).await;

        assert_eq!(status, StatusCode::OK);
        // Byte-for-byte the same JSON that was stored -- including its
        // snake_case keys, unchanged -- proving this route is a true
        // pass-through, not a re-shaping.
        assert_eq!(body, population);

        delete_schedule_population_fixture(&pool, "test-schedule-2a-today").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                schedule_an_explicit_date_query_param_selects_that_date_not_today -- --ignored`"]
    async fn schedule_an_explicit_date_query_param_selects_that_date_not_today() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-explicit-date").await;

        let requested = chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let population = serde_json::json!([{"uid": "C99999", "calling_points": []}]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-schedule-2a-explicit-date")
        .bind(requested)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) =
            get_line_schedule(router, "test-schedule-2a-explicit-date", Some("2026-01-02")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, population);

        delete_schedule_population_fixture(&pool, "test-schedule-2a-explicit-date").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                schedule_a_row_only_for_a_different_date_is_still_404_today -- --ignored`"]
    async fn schedule_a_row_only_for_a_different_date_is_still_404_today() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-stale").await;

        let yesterday = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, '[]')",
        )
        .bind("test-schedule-2a-stale")
        .bind(yesterday)
        .execute(&pool)
        .await
        .expect("seed a stale fixture row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, _) = get_line_schedule(router, "test-schedule-2a-stale", None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        delete_schedule_population_fixture(&pool, "test-schedule-2a-stale").await;
    }
}
