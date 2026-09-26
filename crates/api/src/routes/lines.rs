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

use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::{App, Router};
use crate::auth::{AuthenticatedUser, OptionalAuthenticatedUser};
use crate::data::{
    custom_lines::{self, NewCustomLine},
    queries,
    trains::{self, PublicTrainState},
};
use crate::render::{ScheduleRouteEndpoints, line_train_json};

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
        .route("/lines/{id}/trains", axum::routing::get(get_line_trains))
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
/// `isOwner` exists again, and this is load-bearing. The 2026-08-31
/// privacy hardening removed it on the (then true) grounds that "a `200`
/// from this endpoint is by construction always the real owner's own
/// line." Custom-line group sharing
/// (docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md)
/// made that false: a granted group member now gets a `200` here too, with
/// the same full detail (§3.5 -- full detail or nothing; a custom line has
/// no private per-viewer overlay to withhold). `frontend/app/lines/[id]/page.tsx`
/// had come to use "did `getCustomLine` succeed" as its ownership gate for
/// the Edit/Delete controls, so without this flag a granted member would
/// be shown mutation controls for someone else's line whose only possible
/// outcome is a `404`. The backend is still the authority
/// (`update_line`/`delete_line` are completely grant-blind and unchanged);
/// this is the "never render a control that can only fail" half.
///
/// `sharedWithGroups` is populated ONLY when `isOwner` is true, and is
/// `[]` for every other caller -- that, not a query-level scope, is what
/// keeps a fellow group member from learning which OTHER groups the owner
/// shared this line into (design §3.5). For the owner it lists every group
/// the line is currently granted into, including one they have since left
/// (which keeps its grant, per §2.7) -- hiding a live grant from the
/// person entitled to revoke it would be the real privacy failure. See
/// `groups::groups_shared_with_line`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomLineDetail {
    id: String,
    name: String,
    operators: Vec<String>,
    stations: Vec<String>,
    headcode_prefixes: Vec<String>,
    destination_crs_filter: Vec<String>,
    is_owner: bool,
    shared_with_groups: Vec<crate::data::groups::LineGroupRef>,
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
/// Thin `routes::lines` adapter over
/// [`custom_lines::caller_may_read_line_id`], mapping its error the way
/// this module's handlers do.
async fn readable_line_id(
    app: &App,
    id: &str,
    user: &Option<AuthenticatedUser>,
) -> Result<bool, (StatusCode, String)> {
    custom_lines::caller_may_read_line_id(&app.database, id, user.as_ref().map(|u| u.id.as_str()))
        .await
        .map_err(internal_error)
}

async fn get_line_schedule(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(query): Query<ScheduleQuery>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // London-local "today", not UTC -- see `routes::trains`'s `london_now`
    // split (baa4e75) for the original incident: during the 00:00-01:00 BST
    // window a UTC "today" is still yesterday in London, so this route
    // would 404 or serve yesterday's CIF-derived line schedule for the
    // first hour of every service day. `schedule_line_population` is keyed
    // by London rail-day date, never UTC.
    let london_today = chrono::Utc::now()
        .with_timezone(&chrono_tz::Europe::London)
        .date_naive();
    let service_date = resolve_schedule_date(query.date, london_today);
    // No `custom-` id can reach `schedule_line_population` today: its only
    // producer iterates the static catalogue (`crates/schedule-reference`'s
    // `lines_to_publish`). That is a property of the producer, not of this
    // route -- and "an ungated reader that happens to be safe because of
    // what the writer currently writes" is exactly how
    // `lines_currently_reporting_incident` came to disclose other users'
    // private lines (2026-09-16 custom-line archive research §5c). Gate it
    // here so the invariant lives where the data is read, and refuse with
    // the same 404 an unpublished `(id, date)` already gets.
    if !readable_line_id(&app, &id, &user).await? {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule population for line {id} on {service_date}"),
        ));
    }
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

/// `GET /public/lines/{id}/trains?date=`: every scheduled UID on line `id`
/// for one rail day (from `schedule_line_population`, the same source
/// `get_line_schedule` reads), each paired with its live status from the
/// shared `trains`/`train_current_state` tables where one already exists.
/// See docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md
/// §5.3 for the full design.
///
/// Closes the gap between `get_line_schedule` (schedule only, one line at
/// a time) and `routes::train::get_by_uid_and_date` (schedule + live
/// status, but one train at a time): without this route, "what's running
/// on this line right now" costs one `GET /Train/by-uid` call per
/// scheduled service. This route costs exactly two queries regardless of
/// how many trains the line has: one for the population, one batched
/// `trains::get_public_train_states_for_line` covering every UID in it.
///
/// Same 404 semantics as `get_line_schedule` -- an unknown/not-yet-
/// published `(id, date)` 404s naming both. Unlike `get_by_uid_and_date`,
/// this handler never writes: a UID with no existing `trains` row simply
/// renders `liveStatus: null` (an honest, expected gap -- see the spec's
/// Open question 2), never triggering a `find_or_create_train` upsert.
/// The first and last TIPLOCs of one population entry's `calling_points`
/// array (raw, unnormalized) -- `None` for a missing/empty/malformed array,
/// or for a calling point whose own `tiploc` key is absent. Used by
/// `get_line_trains` to know which TIPLOCs need resolving to a schedule-side
/// origin/destination (see `ScheduleRouteEndpoints`'s own doc comment for
/// why).
fn first_and_last_tiploc(entry: &Value) -> (Option<String>, Option<String>) {
    let Some(points) = entry.get("calling_points").and_then(Value::as_array) else {
        return (None, None);
    };
    let tiploc_of = |p: &Value| p.get("tiploc").and_then(Value::as_str).map(str::to_string);
    (
        points.first().and_then(tiploc_of),
        points.last().and_then(tiploc_of),
    )
}

async fn get_line_trains(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(query): Query<ScheduleQuery>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    // Same London-local "today" as `get_line_schedule` above, same reason.
    let london_today = chrono::Utc::now()
        .with_timezone(&chrono_tz::Europe::London)
        .date_naive();
    let service_date = resolve_schedule_date(query.date, london_today);
    // Same gate, same rationale, same 404 as `get_line_schedule` above --
    // these two routes read the same table off the same caller-supplied id.
    if !readable_line_id(&app, &id, &user).await? {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule population for line {id} on {service_date}"),
        ));
    }
    let Some(population) = queries::get_schedule_line_population(&app.database, &id, service_date)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule population for line {id} on {service_date}"),
        ));
    };

    // `population` is already owned here -- destructure it directly rather
    // than `.as_array().cloned()`, which would clone the whole array (a
    // line's full daily service list; size unmeasured, see the spec's Open
    // question 1) just to unwrap it.
    let entries = match population {
        Value::Array(entries) => entries,
        _ => Vec::new(),
    };
    let uids: Vec<String> = entries
        .iter()
        .filter_map(|e| e.get("uid").and_then(Value::as_str).map(str::to_string))
        .collect();

    let live_states = trains::get_public_train_states_for_line(&app.database, &uids, service_date)
        .await
        .map_err(internal_error)?;
    let live_by_uid: HashMap<&str, &PublicTrainState> = live_states
        .iter()
        .map(|s| (s.train_uid.as_str(), s))
        .collect();

    // Resolves each entry's schedule-side origin/destination (first/last
    // calling point, TIPLOC -> CRS -> name) so a row can still name its
    // route when `liveStatus` is null or has no schedule match of its own
    // -- see `ScheduleRouteEndpoints`'s own doc comment (2026-09-22 UX
    // review §4.1, "Unknown station" rows). Two batched queries cover every
    // entry on the line regardless of population size, mirroring
    // `live_states`'s own one-query-per-line shape above rather than one
    // per entry.
    let endpoint_tiplocs: Vec<(Option<String>, Option<String>)> =
        entries.iter().map(first_and_last_tiploc).collect();
    let all_tiplocs: Vec<String> = endpoint_tiplocs
        .iter()
        .flat_map(|(first, last)| [first.clone(), last.clone()])
        .flatten()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let tiploc_to_crs = queries::crs_for_tiplocs_batch(&app.database, &all_tiplocs)
        .await
        .map_err(internal_error)?;
    // `.filter(queries::is_bookable_crs)` -- see that function's own doc
    // comment: `tiploc_to_crs` (built above from `crs_for_tiplocs_batch`)
    // resolves an X-prefixed Network Rail pseudo-CRS just like a real one,
    // so without this an origin/destination that's really a junction or
    // depot (e.g. `VICTRCR` -> `XVR`) would render as if it were a genuine
    // station on this line's "Trains running today" panel
    // (`ScheduleRouteEndpoints`, below). Blanked to `None` here, the same
    // "treat like unresolved" degrade `journey::stops_from_calling_points`
    // already applies for the single-train journey timeline.
    let crs_of = |tiploc: &Option<String>| -> Option<String> {
        tiploc
            .as_deref()
            .and_then(|t| tiploc_to_crs.get(&t.trim().to_uppercase()).cloned())
            .filter(|crs| queries::is_bookable_crs(crs))
    };
    let endpoint_crs: Vec<(Option<String>, Option<String>)> = endpoint_tiplocs
        .iter()
        .map(|(first, last)| (crs_of(first), crs_of(last)))
        .collect();
    let all_crs: Vec<String> = endpoint_crs
        .iter()
        .flat_map(|(origin, destination)| [origin.clone(), destination.clone()])
        .flatten()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let crs_to_name = queries::station_names_for_crs_batch(&app.database, &all_crs)
        .await
        .map_err(internal_error)?;
    let name_of = |crs: &Option<String>| -> Option<String> {
        crs.as_deref()
            .and_then(|c| crs_to_name.get(&c.to_uppercase()).cloned())
    };

    let result: Vec<Value> = entries
        .iter()
        .zip(endpoint_crs.iter())
        .map(|(entry, (origin_crs, destination_crs))| {
            let live = entry
                .get("uid")
                .and_then(Value::as_str)
                .and_then(|uid| live_by_uid.get(uid).copied());
            let schedule_route = ScheduleRouteEndpoints {
                origin_name: name_of(origin_crs),
                origin_crs: origin_crs.clone(),
                destination_name: name_of(destination_crs),
                destination_crs: destination_crs.clone(),
            };
            line_train_json(entry, live, &schedule_route)
        })
        .collect();

    Ok(Json(result))
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
    // ...or the caller is a member of a group the owner granted this line
    // into (custom-line group sharing, design §3.2): one more disjunct on
    // the ownership gate, same 404 and same message for everyone outside
    // both sets. An anonymous caller has no `user_id` to bind and so falls
    // straight through to the 404 with no extra query, exactly as before.
    let caller_may_read = caller_owns_it
        || match &user {
            Some(u) => custom_lines::readable_custom_line_ids(
                &app.database,
                std::slice::from_ref(&id),
                &u.id,
            )
            .await
            .map_err(internal_error)?
            .contains(&id),
            None => false,
        };
    if !caller_may_read {
        return Err((StatusCode::NOT_FOUND, "line not found".to_string()));
    }

    Ok(Json(LineDefinitionSummary {
        stations: custom.stations,
        operators: custom.operators,
    }))
}

/// `Cache-Control` for `GET /lines` when the caller is anonymous --
/// same rationale/value as `routes::stanox_crs::STANOX_CRS_CACHE_CONTROL`
/// (see that constant's doc comment): the catalogue+TfL portion of this
/// route's output is a slow-changing whole-table dump served with no cache
/// header at all (finding "Whole-table dumps served uncached to anonymous
/// callers"). Only used for the anonymous branch -- see
/// [`LINES_PRIVATE_CACHE_CONTROL`] for why an authenticated response must
/// never share this value.
const LINES_PUBLIC_CACHE_CONTROL: &str = "public, max-age=3600";

/// `Cache-Control` for `GET /lines` when the caller IS authenticated. Unlike
/// `stanox_crs`/`island_of_ireland`'s catalogue dumps, this route's body
/// varies per caller once logged in -- it splices in the caller's own
/// private custom lines (see the `if let Some(user)` block below) -- so a
/// shared/public cache header here would risk one user's browser or an
/// intermediary cache serving another user's private custom-line list back
/// to them. `private, no-store` + `Vary: Cookie` mirrors
/// `routes::incidents::get_incident`'s existing precedent for the same
/// "session-dependent body on an otherwise-public-looking path" shape (see
/// that route's own doc comment).
const LINES_PRIVATE_CACHE_CONTROL: &str = "private, no-store";

async fn list_lines(
    State(app): State<App>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<
    (
        [(axum::http::header::HeaderName, &'static str); 2],
        Json<Vec<LineSummary>>,
    ),
    (StatusCode, String),
> {
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
    //
    // Deliberately NOT widened by custom-line group sharing (design §3.2),
    // unlike the four read gates that were: this list is "what can I
    // create/edit" -- it backs the All Lines table's own-lines rows, the
    // edit-picker flows, and the group share picker -- not "what am I
    // allowed to view". A line granted to the caller through a group
    // appears on that group's page, on the home page's "Lines shared with
    // you" section, and at /lines/{id}; listing it HERE would blur "mine
    // to edit" with "visible to me via a group" and would also offer the
    // caller a line they don't own in the share picker, which
    // `groups::grant_custom_line` would then refuse.
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

    let cache_control = if user.is_some() {
        LINES_PRIVATE_CACHE_CONTROL
    } else {
        LINES_PUBLIC_CACHE_CONTROL
    };
    Ok((
        [
            (axum::http::header::CACHE_CONTROL, cache_control),
            (axum::http::header::VARY, "Cookie"),
        ],
        Json(out),
    ))
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

/// A real 3-letter CRS code, ASCII-alphabetic only, case-insensitive --
/// same check as `routes::trains::normalize_crs`/`journeys::is_three_letter_crs`/
/// `journey_templates::is_three_letter_crs` (each module keeps its own
/// private copy rather than reaching across the module boundary, matching
/// this codebase's existing convention for this exact check).
///
/// This is the gate for `CreateLineRequest::stations`: a custom line's
/// stations flow, completely unvalidated before this, straight into
/// `LineDefinition::sample_stations` (`From<CustomLine> for LineDefinition`)
/// and from there into `poller-ldbws`'s `fetch_departures_once`, which
/// splices each one directly into a `GetDepBoardWithDetails/{crs}` URL path
/// alongside the org's own RDM API key. Without this check, any
/// authenticated user could submit a station value containing `/`, `?`, or
/// other URL-structuring characters and redirect that request to an
/// arbitrary path/query on the RDM host, or simply submit a large number of
/// distinct bogus values to inflate per-cycle request volume against quota
/// (M3, 2026-09-26 review).
fn is_three_letter_crs(crs: &str) -> bool {
    let trimmed = crs.trim();
    trimmed.chars().count() == 3 && trimmed.chars().all(|c| c.is_ascii_alphabetic())
}

/// Rejects `stations` if any entry isn't a plausible CRS code (see
/// [`is_three_letter_crs`]), naming the first offender in the error message.
fn validate_station_crs_codes(stations: &[String]) -> Result<(), (StatusCode, String)> {
    if let Some(bad) = stations.iter().find(|s| !is_three_letter_crs(s)) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("'{bad}' is not a valid 3-letter CRS code"),
        ));
    }
    Ok(())
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
    validate_station_crs_codes(&req.stations)?;
    if custom_lines::slugify(&req.name) == "custom-" {
        return Err((
            StatusCode::BAD_REQUEST,
            "name must contain at least one letter or digit".to_string(),
        ));
    }

    // Per-user cap. Every custom line is reloaded and fully re-evaluated by
    // `aggregator`'s cycle (matcher, segment rebuild, status/stats writes)
    // every 60 seconds for as long as it exists, so unbounded creation is
    // unbounded recurring work for the whole system, not just this user's own
    // storage -- see `custom_lines::MAX_CUSTOM_LINES_PER_USER`.
    //
    // A count-then-insert can in principle be raced by a user firing
    // concurrent creates, letting them land a handful over the cap. That is a
    // deliberate tradeoff: the cap exists to bound an order of magnitude
    // (tens, not thousands), which this achieves, and enforcing it inside
    // `insert_custom_line`'s transaction instead would mean plumbing a
    // distinguishable "limit reached" error out through its `anyhow::Result`
    // just to turn a 500 back into this 400.
    let owned = custom_lines::count_custom_lines_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    if owned >= custom_lines::MAX_CUSTOM_LINES_PER_USER {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "You already have {} custom lines, which is the maximum. Delete one to make \
                 room for a new one.",
                custom_lines::MAX_CUSTOM_LINES_PER_USER
            ),
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
    let is_owner = owner.as_deref() == Some(user.id.as_str());
    if !is_owner {
        // Not the owner -- but they may still be a member of a group the
        // owner granted this line into (custom-line group sharing, design
        // §3.2). One more disjunct on the existing ownership gate; anyone
        // outside BOTH sets still gets the identical 404 and the identical
        // message a total stranger gets, so "doesn't exist" and "exists,
        // not shared with you" stay indistinguishable. Never 403.
        let readable = custom_lines::readable_custom_line_ids(
            &app.database,
            std::slice::from_ref(&id),
            &user.id,
        )
        .await
        .map_err(internal_error)?;
        if !readable.contains(&id) {
            return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
        }
    }

    // Owner-only. Guarded twice on purpose: this branch, and
    // `groups_shared_with_line`'s own `EXISTS (... cl.user_id = $2)` --
    // see that function's doc comment and `CustomLineDetail`'s.
    let shared_with_groups = if is_owner {
        crate::data::groups::groups_shared_with_line(&app.database, &id, &user.id)
            .await
            .map_err(internal_error)?
    } else {
        Vec::new()
    };

    Ok(Json(CustomLineDetail {
        id: line.id,
        name: line.name,
        operators: line.operators,
        stations: line.stations,
        headcode_prefixes: line.headcode_prefixes,
        destination_crs_filter: line.destination_crs_filter,
        is_owner,
        shared_with_groups,
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
    validate_station_crs_codes(&req.stations)?;
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
mod custom_line_detail_wire_shape_tests {
    use super::*;

    /// Pins `CustomLineDetail`'s exact JSON key set, the same way
    /// `data::groups`'s `group_train_wire_shape_tests` pins `GroupTrain`'s.
    /// This is the most privacy-sensitive shape this file serves: since
    /// custom-line group sharing it reaches a NON-owner (a fellow group
    /// member) in full, so a field added here by accident is disclosed to
    /// everyone in every group the line is shared into. `isOwner` and
    /// `sharedWithGroups` are load-bearing and must not be dropped either
    /// -- the frontend's Edit/Delete gate reads the first.
    #[test]
    fn custom_line_detail_json_keys_are_exactly_the_definition_plus_the_two_sharing_fields() {
        let value = serde_json::to_value(CustomLineDetail {
            id: "custom-my-commute".to_string(),
            name: "My Commute".to_string(),
            operators: vec!["SW".to_string()],
            stations: vec!["WOK".to_string(), "CLJ".to_string()],
            headcode_prefixes: vec![],
            destination_crs_filter: vec![],
            is_owner: true,
            shared_with_groups: vec![],
        })
        .expect("serialize");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "destinationCrsFilter",
                "headcodePrefixes",
                "id",
                "isOwner",
                "name",
                "operators",
                "sharedWithGroups",
                "stations",
            ]
        );
    }

    /// `LineGroupRef` carries a group's id and display name and nothing
    /// else -- never a member list, a role, a member count, or the
    /// grant's own timestamp.
    #[test]
    fn line_group_ref_json_carries_only_an_id_and_a_name() {
        let value = serde_json::to_value(crate::data::groups::LineGroupRef {
            id: "grp-1".to_string(),
            name: "Family".to_string(),
        })
        .expect("serialize");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["id", "name"]);
    }
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
    fn first_and_last_tiploc_reads_both_ends_of_a_multi_stop_entry() {
        let entry = serde_json::json!({
            "uid": "C1",
            "calling_points": [
                {"tiploc": "KNGX", "kind": "Origin"},
                {"tiploc": "PBRO", "kind": "Intermediate"},
                {"tiploc": "YORK", "kind": "Terminate"},
            ],
        });
        assert_eq!(
            first_and_last_tiploc(&entry),
            (Some("KNGX".to_string()), Some("YORK".to_string()))
        );
    }

    #[test]
    fn first_and_last_tiploc_a_single_stop_entry_returns_the_same_tiploc_twice() {
        let entry = serde_json::json!({
            "uid": "C1",
            "calling_points": [{"tiploc": "KNGX", "kind": "Origin"}],
        });
        assert_eq!(
            first_and_last_tiploc(&entry),
            (Some("KNGX".to_string()), Some("KNGX".to_string()))
        );
    }

    #[test]
    fn first_and_last_tiploc_missing_or_empty_calling_points_is_none_none() {
        assert_eq!(
            first_and_last_tiploc(&serde_json::json!({"uid": "C1"})),
            (None, None)
        );
        assert_eq!(
            first_and_last_tiploc(&serde_json::json!({"uid": "C1", "calling_points": []})),
            (None, None)
        );
        assert_eq!(
            first_and_last_tiploc(&serde_json::json!({"uid": "C1", "calling_points": null})),
            (None, None)
        );
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

    #[test]
    fn is_three_letter_crs_accepts_a_real_code_in_either_case() {
        assert!(is_three_letter_crs("WOK"));
        assert!(is_three_letter_crs("wok"));
        assert!(is_three_letter_crs(" WOK "));
    }

    #[test]
    fn is_three_letter_crs_rejects_wrong_length() {
        assert!(!is_three_letter_crs("WO"));
        assert!(!is_three_letter_crs("WOKE"));
        assert!(!is_three_letter_crs(""));
    }

    #[test]
    fn is_three_letter_crs_rejects_non_alphabetic_characters() {
        // The exact shape M3 (2026-09-26 review) is guarding against: a
        // value with URL-structuring characters that would otherwise flow
        // straight into `poller-ldbws`'s `GetDepBoardWithDetails/{crs}` path.
        assert!(!is_three_letter_crs("W1K"));
        assert!(!is_three_letter_crs("W/K"));
        assert!(!is_three_letter_crs("W?K"));
    }

    #[test]
    fn validate_station_crs_codes_accepts_genuine_codes() {
        assert!(validate_station_crs_codes(&["WOK".to_string(), "CLJ".to_string()]).is_ok());
    }

    #[test]
    fn validate_station_crs_codes_rejects_a_malformed_entry_naming_it_in_the_message() {
        let err = validate_station_crs_codes(&["WOK".to_string(), "../evil".to_string()])
            .expect_err("malformed station should be rejected");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("../evil"), "message was: {}", err.1);
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

    use super::{LINES_PRIVATE_CACHE_CONTROL, LINES_PUBLIC_CACHE_CONTROL};
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
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
        };

        std::sync::Arc::new(AppState {
            // Built from the same catalogue the real `AppState::init`
            // builds it from, so a test never gets a matcher that
            // disagrees with its own `config.lines`.
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
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
            schedule_crs_line_index: std::collections::HashMap::new(),
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

    /// Issues a `PUT` or `DELETE` against `/public/lines/{id}` -- the two
    /// owner-only mutation routes, which custom-line group sharing
    /// deliberately did NOT widen. Same `(status, JSON-or-plain-text body)`
    /// return shape as `get_line` below.
    async fn mutate(
        router: axum::Router,
        method: &str,
        id: &str,
        raw_token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(format!("/public/lines/{id}"));
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&value).expect("serialize request body"),
                ))
                .expect("build request"),
            None => builder.body(Body::empty()).expect("build request"),
        };
        let response = router.oneshot(request).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
            })
        };
        (status, value)
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

    /// Issues `POST /public/lines` with a minimally-valid create body (a
    /// name and the 2 stations `create_line` requires).
    async fn create_line_request(
        router: axum::Router,
        raw_token: &str,
        name: &str,
    ) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("POST")
            .uri("/public/lines")
            .header(
                header::COOKIE,
                format!("distant_signal_session={raw_token}"),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "name": name,
                    "operators": ["SW"],
                    "stations": ["WAT", "SUR"],
                }))
                .expect("serialize request body"),
            ))
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
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                creating_more_custom_lines_than_the_per_user_cap_is_rejected_with_400 \
                -- --ignored`"]
    async fn creating_more_custom_lines_than_the_per_user_cap_is_rejected_with_400() {
        // The real cost this cap exists for is in another service entirely:
        // `aggregator`'s `run_cycle` reloads every custom line and
        // re-evaluates it (matcher, segment rebuild, status/stats writes)
        // every 60 seconds, forever. Before this cap, `create_line` enforced
        // only a non-empty name and >= 2 stations, so one user could script
        // thousands of creates and permanently degrade the cycle for
        // everyone.
        const USER: &str = "test-user-custom-line-cap";
        let pool = connect().await;
        let raw_token = seed_session(&pool, USER).await;

        // Seed the user right up to one BELOW the cap in a single statement,
        // so the two requests below exercise the exact boundary.
        sqlx::query(
            "INSERT INTO custom_lines \
                (id, name, operators, stations, headcode_prefixes, destination_crs_filter, \
                 user_id, created_at) \
             SELECT 'custom-cap-fixture-' || i, 'Cap Fixture ' || i, ARRAY['SW']::text[], \
                    ARRAY['WAT','SUR']::text[], ARRAY[]::text[], ARRAY[]::text[], $1, NOW() \
             FROM generate_series(1, $2::int) AS i",
        )
        .bind(USER)
        .bind(custom_lines::MAX_CUSTOM_LINES_PER_USER - 1)
        .execute(&pool)
        .await
        .expect("seed fixture custom lines");

        // The one that lands exactly ON the cap still succeeds.
        let (at_cap_status, _) = create_line_request(
            test_router(test_app(pool.clone(), vec![])),
            &raw_token,
            "Cap Boundary Line",
        )
        .await;

        // The next one is refused.
        let (over_cap_status, over_cap_body) = create_line_request(
            test_router(test_app(pool.clone(), vec![])),
            &raw_token,
            "One Too Many",
        )
        .await;

        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM custom_lines WHERE user_id = $1")
                .bind(USER)
                .fetch_one(&pool)
                .await
                .expect("count fixture custom lines");
        cleanup_user(&pool, USER).await;

        assert_eq!(
            at_cap_status,
            StatusCode::OK,
            "a create that lands exactly on the cap must still succeed"
        );
        assert_eq!(
            over_cap_status,
            StatusCode::BAD_REQUEST,
            "exceeding the cap must be a client error, not a 500 or a silent success"
        );
        assert!(
            over_cap_body
                .as_str()
                .unwrap_or_default()
                .contains("maximum"),
            "the 400 body should tell the user what happened, got {over_cap_body:?}"
        );
        assert_eq!(
            remaining,
            custom_lines::MAX_CUSTOM_LINES_PER_USER,
            "the refused create must not have inserted a row"
        );
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
                the_real_owner_gets_200_with_full_detail_and_is_owner_true -- --ignored`"]
    async fn the_real_owner_gets_200_with_full_detail_and_is_owner_true() {
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
        // `isOwner` was removed by the 2026-08-31 privacy hardening (a
        // `200` proved ownership back then) and REINSTATED by custom-line
        // group sharing, which made a `200` reachable for a granted
        // non-owner too. It is what the frontend's Edit/Delete gate reads,
        // so "true for the real owner" is the load-bearing half of that
        // pair -- see
        // `a_granted_group_member_gets_200_with_is_owner_false_and_no_shared_groups`
        // for the other.
        assert_eq!(object.get("isOwner").and_then(Value::as_bool), Some(true));
        // No grants exist for this line, so the owner's own "Shared with"
        // list is empty rather than absent.
        assert_eq!(
            object
                .get("sharedWithGroups")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(0)
        );

        cleanup_user(&pool, "TEST-GET-LINE-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_granted_group_member_gets_200_with_is_owner_false_and_no_shared_groups \
                -- --ignored --test-threads=1`"]
    async fn a_granted_group_member_gets_200_with_is_owner_false_and_no_shared_groups() {
        // The HTTP-level version of the read-path widening: a fellow group
        // member gets the SAME full detail the owner does (design §3.5 --
        // full detail or nothing), but `isOwner: false` so no mutation
        // control is ever rendered for them, and an EMPTY
        // `sharedWithGroups` so they never learn which other groups the
        // owner shared this line into.
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-GET-LINE-GRANT-OWNER").await;
        let member_token = seed_session(&pool, "TEST-GET-LINE-GRANT-MEMBER").await;
        let stranger_token = seed_session(&pool, "TEST-GET-LINE-GRANT-STRANGER").await;

        let line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Granted Detail Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-GET-LINE-GRANT-OWNER",
        )
        .await
        .expect("insert fixture line");
        let group_id = crate::data::groups::create_group(
            &pool,
            "Get Line Grant Group",
            "TEST-GET-LINE-GRANT-OWNER",
        )
        .await
        .expect("create fixture group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GET-LINE-GRANT-MEMBER")
        .execute(&pool)
        .await
        .expect("seed fixture membership");
        crate::data::groups::grant_custom_line(
            &pool,
            &group_id,
            &line.id,
            "TEST-GET-LINE-GRANT-OWNER",
        )
        .await
        .expect("seed fixture grant");

        let router = test_router(test_app(pool.clone(), vec![]));

        let (status, body) = get_line(router.clone(), &line.id, Some(&member_token)).await;
        assert_eq!(status, StatusCode::OK);
        let object = body.as_object().expect("200 body should be a JSON object");
        assert_eq!(
            object.get("name").and_then(Value::as_str),
            Some("Test Granted Detail Line")
        );
        assert_eq!(object.get("isOwner").and_then(Value::as_bool), Some(false));
        assert_eq!(
            object
                .get("sharedWithGroups")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(0),
            "a granted member must never learn which groups this line is shared into"
        );

        // The owner sees the grant reflected back on their own copy.
        let (status, body) = get_line(router.clone(), &line.id, Some(&owner_token)).await;
        assert_eq!(status, StatusCode::OK);
        let groups = body
            .get("sharedWithGroups")
            .and_then(Value::as_array)
            .expect("owner's sharedWithGroups");
        assert_eq!(groups.len(), 1, "got {groups:?}");
        assert_eq!(
            groups[0].get("name").and_then(Value::as_str),
            Some("Get Line Grant Group")
        );

        // Someone in no group at all is still completely shut out, with
        // the same 404 and the same message a nonexistent id gets.
        let (status, body) = get_line(router.clone(), &line.id, Some(&stranger_token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("custom line not found".to_string()));

        // A grant conveys READ access and nothing else (design §5's first
        // non-goal). Pinned at the HTTP level, not left implicit in
        // "`update_custom_line`/`delete_custom_line` were not modified":
        // this is precisely the invariant a future refactor that "made the
        // write gate match the read gate" would break, and the existing
        // non-owner tests in this file use a plain stranger, so they would
        // keep passing through exactly that mistake.
        let (status, body) = mutate(
            router.clone(),
            "PUT",
            &line.id,
            Some(&member_token),
            Some(serde_json::json!({
                "name": "Hijacked",
                "operators": ["SW"],
                "stations": ["WOK", "CLJ"],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("custom line not found".to_string()));

        let (status, body) = mutate(router, "DELETE", &line.id, Some(&member_token), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("custom line not found".to_string()));

        let (still_named,): (String,) =
            sqlx::query_as("SELECT name FROM custom_lines WHERE id = $1")
                .bind(&line.id)
                .fetch_one(&pool)
                .await
                .expect("the line must still exist, unchanged");
        assert_eq!(still_named, "Test Granted Detail Line");

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        for id in [
            "TEST-GET-LINE-GRANT-OWNER",
            "TEST-GET-LINE-GRANT-MEMBER",
            "TEST-GET-LINE-GRANT-STRANGER",
        ] {
            cleanup_user(&pool, id).await;
        }
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
                get_line_definition_a_granted_group_member_gets_200_but_a_non_member_404s \
                -- --ignored --test-threads=1`"]
    async fn get_line_definition_a_granted_group_member_gets_200_but_a_non_member_404s() {
        // `get_line_definition` is the fourth of the read gates custom-line
        // group sharing widened, and the easiest one to forget: unlike
        // `get_line` it takes `OptionalAuthenticatedUser` and serves
        // catalogue ids to anyone, so its custom branch is a narrow strip
        // of code with two very different callers passing through it.
        let pool = connect().await;
        seed_session(&pool, "TEST-GET-DEF-GRANT-OWNER").await;
        let member_token = seed_session(&pool, "TEST-GET-DEF-GRANT-MEMBER").await;
        let stranger_token = seed_session(&pool, "TEST-GET-DEF-GRANT-STRANGER").await;

        let line = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Granted Definition Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-GET-DEF-GRANT-OWNER",
        )
        .await
        .expect("insert fixture line");
        let group_id = crate::data::groups::create_group(
            &pool,
            "Get Definition Grant Group",
            "TEST-GET-DEF-GRANT-OWNER",
        )
        .await
        .expect("create fixture group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GET-DEF-GRANT-MEMBER")
        .execute(&pool)
        .await
        .expect("seed fixture membership");
        crate::data::groups::grant_custom_line(
            &pool,
            &group_id,
            &line.id,
            "TEST-GET-DEF-GRANT-OWNER",
        )
        .await
        .expect("seed fixture grant");

        let router = test_router(test_app(pool.clone(), vec![]));

        let (status, body) =
            get_line_definition(router.clone(), &line.id, Some(&member_token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body.get("stations").and_then(Value::as_array).map(Vec::len),
            Some(2)
        );

        // A logged-in caller in none of the owner's groups, and an
        // anonymous one, both get the identical 404 an unknown id gets.
        let (status, body) =
            get_line_definition(router.clone(), &line.id, Some(&stranger_token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("line not found".to_string()));
        let (status, _) = get_line_definition(router, &line.id, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        for id in [
            "TEST-GET-DEF-GRANT-OWNER",
            "TEST-GET-DEF-GRANT-MEMBER",
            "TEST-GET-DEF-GRANT-STRANGER",
        ] {
            cleanup_user(&pool, id).await;
        }
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
                list_lines_cache_control_is_public_for_anonymous_and_private_for_authenticated_callers \
                -- --ignored`"]
    async fn list_lines_cache_control_is_public_for_anonymous_and_private_for_authenticated_callers()
     {
        // Regression for "Whole-table dumps served uncached to anonymous
        // callers": the anonymous response (catalogue + TfL only) is a
        // slow-changing whole-table dump, safe to cache publicly. But this
        // route's body varies per caller once authenticated -- it splices
        // in the caller's own private custom lines (see `list_lines`'s own
        // doc comment) -- so an authenticated response must never carry
        // that same public, shared-cacheable header: doing so would risk
        // one user's private custom-line list being served back to a
        // different caller by an intermediary cache.
        let pool = connect().await;
        let raw_token = seed_session(&pool, "TEST-LIST-LINES-CACHE-CONTROL-USER").await;

        let router = test_router(test_app(pool.clone(), vec![]));

        let anon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/public/lines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anon_response.status(), StatusCode::OK);
        assert_eq!(
            anon_response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some(LINES_PUBLIC_CACHE_CONTROL),
            "an anonymous request must get a public, positive-max-age Cache-Control"
        );

        let auth_response = router
            .oneshot(
                Request::builder()
                    .uri("/public/lines")
                    .header(
                        header::COOKIE,
                        format!("distant_signal_session={raw_token}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(auth_response.status(), StatusCode::OK);
        assert_eq!(
            auth_response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some(LINES_PRIVATE_CACHE_CONTROL),
            "an authenticated request's session-dependent body must never be marked publicly \
             cacheable"
        );

        cleanup_user(&pool, "TEST-LIST-LINES-CACHE-CONTROL-USER").await;
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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                schedule_uses_london_local_today_not_bare_utc -- --ignored`"]
    async fn schedule_uses_london_local_today_not_bare_utc() {
        // Regression for the 19-pass review's Medium finding: this route
        // used to key its lookup off `chrono::Utc::now().date_naive()`,
        // exactly the bug `routes::trains`'s `london_now` split (baa4e75)
        // already fixed for `/trains/search` -- during the roughly-hour-
        // long window where UTC's calendar day still lags London's (every
        // night of British Summer Time, 23:00-00:00 UTC = 00:00-01:00
        // London), a bare-UTC "today" is one day behind, so this route
        // served yesterday's CIF-derived line schedule or 404'd for the
        // first hour of every service day.
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-utc-gap").await;

        let utc_today = chrono::Utc::now().date_naive();
        let london_today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();

        if utc_today == london_today {
            // Outside the UTC/London date-boundary gap right now -- see
            // `routes::departures::tests::schedule_departures_uses_london_local_today_not_bare_utc`
            // for the identical, established skip pattern. True no-op, not
            // lost coverage.
            return;
        }

        // Inside the gap: seed a row keyed ONLY by the wrong, bare-UTC
        // date. If the route regresses to `Utc::now().date_naive()` it
        // will find this row and return 200; the fix must 404 instead.
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, '[]')",
        )
        .bind("test-schedule-2a-utc-gap")
        .bind(utc_today)
        .execute(&pool)
        .await
        .expect("seed a bare-UTC-dated fixture row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, _) = get_line_schedule(router, "test-schedule-2a-utc-gap", None).await;

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a row keyed by bare UTC \"today\" must not satisfy a London-local \"today\" \
             lookup during the UTC/London date gap; if this fails, the route has regressed \
             to `Utc::now().date_naive()`"
        );

        delete_schedule_population_fixture(&pool, "test-schedule-2a-utc-gap").await;
    }

    /// Issues `GET /public/lines/{id}/trains`, with an optional `?date=`
    /// query string. Mirrors `get_line_schedule`'s own request-building.
    async fn get_line_trains(
        router: axum::Router,
        id: &str,
        date: Option<&str>,
    ) -> (StatusCode, Value) {
        let uri = match date {
            Some(date) => format!("/public/lines/{id}/trains?date={date}"),
            None => format!("/public/lines/{id}/trains"),
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
                trains_no_row_for_the_line_and_date_is_404_naming_both -- --ignored`"]
    async fn trains_no_row_for_the_line_and_date_is_404_naming_both() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-missing").await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_trains(router, "test-trains-3-missing", None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        let body = body
            .as_str()
            .expect("404 body is a plain string")
            .to_string();
        assert!(body.contains("test-trains-3-missing"), "body: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                trains_a_population_with_no_trains_rows_returns_every_entry_with_null_live_status \
                -- --ignored`"]
    async fn trains_a_population_with_no_trains_rows_returns_every_entry_with_null_live_status() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-no-live").await;
        sqlx::query("DELETE FROM trains WHERE train_uid IN ('TEST-TRAINS-3-A', 'TEST-TRAINS-3-B')")
            .execute(&pool)
            .await
            .ok();

        let today = chrono::Utc::now().date_naive();
        let population = serde_json::json!([
            {"uid": "TEST-TRAINS-3-A", "calling_points": [{"tiploc": "PADTON", "kind": "Origin", "booked_arrival": null, "booked_departure": "08:15:00", "is_half_minute_arrival": false, "is_half_minute_departure": false}]},
            {"uid": "TEST-TRAINS-3-B", "calling_points": []},
        ]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-trains-3-no-live")
        .bind(today)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_trains(router, "test-trains-3-no-live", None).await;

        assert_eq!(status, StatusCode::OK);
        let entries = body.as_array().expect("body is a JSON array");
        assert_eq!(entries.len(), 2);
        for entry in entries {
            assert!(entry["liveStatus"].is_null(), "entry: {entry:?}");
        }
        assert_eq!(entries[0]["uid"], "TEST-TRAINS-3-A");
        assert_eq!(
            entries[0]["callingPoints"], population[0]["calling_points"],
            "callingPoints must be the raw population entry, unchanged"
        );

        delete_schedule_population_fixture(&pool, "test-trains-3-no-live").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                trains_a_uid_with_an_existing_trains_row_gets_its_live_status_attached \
                -- --ignored`"]
    async fn trains_a_uid_with_an_existing_trains_row_gets_its_live_status_attached() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-live").await;
        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-TRAINS-3-LIVE'")
            .execute(&pool)
            .await
            .ok();

        let today = chrono::Utc::now().date_naive();
        let population = serde_json::json!([
            {"uid": "TEST-TRAINS-3-LIVE", "calling_points": []},
        ]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-trains-3-live")
        .bind(today)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let trains_id: (i64,) = sqlx::query_as(
            "INSERT INTO trains (train_uid, service_date, train_id) \
             VALUES ($1, $2, $3) RETURNING id",
        )
        .bind("TEST-TRAINS-3-LIVE")
        .bind(today)
        .bind("1B22")
        .fetch_one(&pool)
        .await
        .expect("seed fixture trains row");
        sqlx::query(
            "INSERT INTO train_current_state \
                (trains_id, status, last_reported_location, delay_minutes, updated_at) \
             VALUES ($1, 'en_route', 'Reading', 5, NOW())",
        )
        .bind(trains_id.0)
        .execute(&pool)
        .await
        .expect("seed fixture train_current_state row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_trains(router, "test-trains-3-live", None).await;

        assert_eq!(status, StatusCode::OK);
        let entries = body.as_array().expect("body is a JSON array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["uid"], "TEST-TRAINS-3-LIVE");
        assert_eq!(entries[0]["liveStatus"]["trainId"], "1B22");
        assert_eq!(entries[0]["liveStatus"]["status"], "en_route");
        assert_eq!(entries[0]["liveStatus"]["lastReportedLocation"], "Reading");
        assert_eq!(entries[0]["liveStatus"]["delayMinutes"], 5);

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id.0)
            .execute(&pool)
            .await
            .ok();
        delete_schedule_population_fixture(&pool, "test-trains-3-live").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                trains_uses_london_local_today_not_bare_utc -- --ignored`"]
    async fn trains_uses_london_local_today_not_bare_utc() {
        // Same regression as `schedule_uses_london_local_today_not_bare_utc`
        // above, for `get_line_trains`'s own independent
        // `chrono::Utc::now().date_naive()` call.
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-utc-gap").await;

        let utc_today = chrono::Utc::now().date_naive();
        let london_today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();

        if utc_today == london_today {
            // Outside the UTC/London date-boundary gap right now -- see
            // `routes::departures::tests::schedule_departures_uses_london_local_today_not_bare_utc`
            // for the identical, established skip pattern. True no-op, not
            // lost coverage.
            return;
        }

        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, '[]')",
        )
        .bind("test-trains-3-utc-gap")
        .bind(utc_today)
        .execute(&pool)
        .await
        .expect("seed a bare-UTC-dated fixture row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, _) = get_line_trains(router, "test-trains-3-utc-gap", None).await;

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a row keyed by bare UTC \"today\" must not satisfy a London-local \"today\" \
             lookup during the UTC/London date gap; if this fails, the route has regressed \
             to `Utc::now().date_naive()`"
        );

        delete_schedule_population_fixture(&pool, "test-trains-3-utc-gap").await;
    }

    /// Regression test for the `ScheduleRouteEndpoints` half of the shared
    /// `queries::is_bookable_crs` filter -- see that function's own doc
    /// comment. `VICTRCR` resolves to the X-prefixed pseudo-CRS `XVR` via
    /// the `tiploc_crs` crosswalk (the same real fixture
    /// `find_schedule_match_blanks_an_x_prefixed_pseudo_crs_destination`,
    /// in `data::schedule_matching`, uses). Before this fix,
    /// `get_line_trains`'s `crs_of` closure resolved `tiploc_to_crs`
    /// unfiltered, so a population entry terminating at `VICTRCR` would
    /// have rendered `scheduleDestinationCrs: "XVR"` on the line's "Trains
    /// running today" panel, as if it were a real station.
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                trains_blanks_an_x_prefixed_pseudo_crs_schedule_destination -- --ignored`"]
    async fn trains_blanks_an_x_prefixed_pseudo_crs_schedule_destination() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-xvr").await;
        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-TRAINS-3-XVR'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query(
            "INSERT INTO tiploc_crs (tiploc, crs, station_name, stanox, source_sequence) \
             VALUES ('VICTRCR', 'XVR', 'VICTORIA C.S.', 'TEST-VICTRCR2-STANOX', 1) \
             ON CONFLICT (tiploc) DO UPDATE SET crs = EXCLUDED.crs",
        )
        .execute(&pool)
        .await
        .expect("seed tiploc_crs");

        let today = chrono::Utc::now().date_naive();
        let population = serde_json::json!([{
            "uid": "TEST-TRAINS-3-XVR",
            "calling_points": [
                {"tiploc": "ECSORIG", "kind": "Origin", "booked_arrival": null, "booked_departure": "23:10:00", "is_half_minute_arrival": false, "is_half_minute_departure": false},
                {"tiploc": "VICTRCR", "kind": "Terminate", "booked_arrival": "23:40:00", "booked_departure": null, "is_half_minute_arrival": false, "is_half_minute_departure": false},
            ],
        }]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-trains-3-xvr")
        .bind(today)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_trains(router, "test-trains-3-xvr", None).await;

        assert_eq!(status, StatusCode::OK);
        let entries = body.as_array().expect("body is a JSON array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["uid"], "TEST-TRAINS-3-XVR");
        assert!(
            entries[0]["scheduleDestinationCrs"].is_null(),
            "VICTRCR resolves to the X-prefixed pseudo-CRS XVR, which must blank to null \
             rather than render as a real station's code: entry: {:?}",
            entries[0]
        );
        assert!(entries[0]["scheduleDestinationName"].is_null());

        sqlx::query("DELETE FROM tiploc_crs WHERE tiploc = 'VICTRCR'")
            .execute(&pool)
            .await
            .ok();
        delete_schedule_population_fixture(&pool, "test-trains-3-xvr").await;
    }

    /// The mirror of the test directly above: a genuine, non-X-prefixed
    /// schedule destination must still resolve normally through the same
    /// `get_line_trains` route -- `queries::is_bookable_crs` only excludes
    /// the `X`-prefixed convention, never a real station code.
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                trains_keeps_a_genuine_non_x_crs_schedule_destination -- --ignored`"]
    async fn trains_keeps_a_genuine_non_x_crs_schedule_destination() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-real-dest").await;
        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-TRAINS-3-REALDEST'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TEST-TRAINS3-BSK-STANOX', 'BSK', 'BSKDEST', 'BASINGSTOKE', 1) \
             ON CONFLICT (stanox) DO UPDATE SET crs = EXCLUDED.crs",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let today = chrono::Utc::now().date_naive();
        let population = serde_json::json!([{
            "uid": "TEST-TRAINS-3-REALDEST",
            "calling_points": [
                {"tiploc": "PADTON", "kind": "Origin", "booked_arrival": null, "booked_departure": "12:00:00", "is_half_minute_arrival": false, "is_half_minute_departure": false},
                {"tiploc": "BSKDEST", "kind": "Terminate", "booked_arrival": "12:30:00", "booked_departure": null, "is_half_minute_arrival": false, "is_half_minute_departure": false},
            ],
        }]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-trains-3-real-dest")
        .bind(today)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_trains(router, "test-trains-3-real-dest", None).await;

        assert_eq!(status, StatusCode::OK);
        let entries = body.as_array().expect("body is a JSON array");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]["scheduleDestinationCrs"], "BSK",
            "a genuine, non-X-prefixed destination CRS must still resolve: entry: {:?}",
            entries[0]
        );

        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-TRAINS3-BSK-STANOX'")
            .execute(&pool)
            .await
            .ok();
        delete_schedule_population_fixture(&pool, "test-trains-3-real-dest").await;
    }
}
