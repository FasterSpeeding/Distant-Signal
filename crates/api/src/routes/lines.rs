//! `/public/lines`: enumerate official + custom lines. Reads (`GET /lines`,
//! `GET /lines/{id}`, `GET /lines/{id}/definition`) are unauthenticated —
//! see `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
//! Non-goals for the original reasoning. Custom-line *writes*
//! (`create_line`/`update_line`/`delete_line`) are no longer part of that
//! "yet" — they require `AuthenticatedUser` and are ownership-scoped (see
//! `crate::data::custom_lines::update_custom_line`/`delete_custom_line`),
//! as of the commit that closed that doc's "yet". `GET /lines/{id}` reports
//! an `isOwner` flag (see `CustomLineDetail`) so the frontend can hide
//! Edit/Delete for a non-owner viewer instead of rendering controls that
//! only fail once clicked.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::{AuthenticatedUser, OptionalAuthenticatedUser};
use crate::data::{custom_lines::{self, NewCustomLine}, queries};

pub fn router() -> Router {
    Router::new()
        .route("/lines", axum::routing::get(list_lines).post(create_line))
        .route(
            "/lines/{id}",
            axum::routing::get(get_line).put(update_line).delete(delete_line),
        )
        .route("/lines/{id}/definition", axum::routing::get(get_line_definition))
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
/// `is_owner` is computed via `OptionalAuthenticatedUser` (never rejects,
/// so this read stays unauthenticated-friendly) comparing the requester's
/// id against the stored `user_id` — `false` for an anonymous visitor, a
/// logged-in non-owner, AND a legacy line with no owner at all (see
/// `crate::data::custom_lines::get_custom_line`'s doc comment); `true`
/// only for the real owner. This is the one ownership signal the frontend
/// needs to hide Edit/Delete for everyone else instead of rendering
/// controls that only fail once clicked — see
/// `docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md`'s
/// Policy, Tier 3.
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
}

/// Pure comparison behind `CustomLineDetail.is_owner` — see that field's
/// doc comment for the three ways this can be `false`. Kept separate from
/// `get_line` so it's unit-testable without a database.
fn is_owner(user: &Option<AuthenticatedUser>, owner_user_id: &Option<String>) -> bool {
    match (user, owner_user_id) {
        (Some(user), Some(owner_user_id)) => &user.id == owner_user_id,
        _ => false,
    }
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

async fn get_line_definition(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<LineDefinitionSummary>, (StatusCode, String)> {
    if let Some(catalogue_line) = app.config.lines.iter().find(|l| l.id == id) {
        return Ok(Json(LineDefinitionSummary {
            stations: catalogue_line.stations.iter().map(|s| s.crs.clone()).collect(),
            operators: catalogue_line.operators.clone(),
        }));
    }

    let custom = custom_lines::get_custom_line(&app.database, &id)
        .await
        .map_err(internal_error)?;
    let Some((custom, _owner)) = custom else {
        return Err((StatusCode::NOT_FOUND, "line not found".to_string()));
    };

    Ok(Json(LineDefinitionSummary {
        stations: custom.stations,
        operators: custom.operators,
    }))
}

async fn list_lines(
    State(app): State<App>,
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

    let custom = custom_lines::list_custom_lines(&app.database)
        .await
        .map_err(internal_error)?;
    out.extend(custom.into_iter().map(|c| LineSummary {
        id: c.id,
        name: c.name,
        category: "custom".to_string(),
        operators: c.operators,
        source: "custom",
    }));

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
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<CustomLineDetail>, (StatusCode, String)> {
    let line = custom_lines::get_custom_line(&app.database, &id)
        .await
        .map_err(internal_error)?;

    // No separate catalogue-id check needed here (unlike `update_line`/
    // `delete_line`): `get_custom_line` only ever queries the
    // `custom_lines` table, so a catalogue id naturally comes back `None`
    // and 404s the same way an unknown id would — there's no distinct
    // error message worth giving for "that's a catalogue line" on a
    // read-only lookup the way there is for a rejected write.
    let Some((line, owner)) = line else {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    };

    Ok(Json(CustomLineDetail {
        id: line.id,
        name: line.name,
        operators: line.operators,
        stations: line.stations,
        headcode_prefixes: line.headcode_prefixes,
        destination_crs_filter: line.destination_crs_filter,
        is_owner: is_owner(&user, &owner),
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

    fn user(id: &str) -> AuthenticatedUser {
        AuthenticatedUser { id: id.to_string(), email: None, name: None }
    }

    #[test]
    fn the_real_owner_is_reported_as_owner() {
        assert!(is_owner(&Some(user("rider-1")), &Some("rider-1".to_string())));
    }

    #[test]
    fn a_logged_in_non_owner_is_not_reported_as_owner() {
        assert!(!is_owner(&Some(user("rider-2")), &Some("rider-1".to_string())));
    }

    #[test]
    fn an_anonymous_visitor_is_never_reported_as_owner() {
        assert!(!is_owner(&None, &Some("rider-1".to_string())));
    }

    #[test]
    fn a_legacy_ownerless_line_is_never_reported_as_owner_even_when_logged_in() {
        // Pre-ownership-retrofit rows have `user_id = NULL` -- see
        // `crates/api/migrations/20260828100000_add_ownership.sql`. No
        // logged-in visitor should be treated as owning one of these.
        assert!(!is_owner(&Some(user("rider-1")), &None));
    }

    #[test]
    fn an_anonymous_visitor_against_a_legacy_ownerless_line_is_not_owner() {
        assert!(!is_owner(&None, &None));
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
}
