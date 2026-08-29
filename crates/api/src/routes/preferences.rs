//! `/public/preferences`: which lines/stations are pinned to the home
//! page. Fully session-gated, both read and write -- unlike `/public/lines`,
//! whose *reads* stay unauthenticated (see
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
//! Non-goals), pinned lines/stations are per-user state with no useful
//! anonymous reading, so every handler here requires a resolved session.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::{custom_lines, preferences, queries};

pub fn router() -> Router {
    Router::new()
        .route("/preferences", axum::routing::get(get_preferences))
        .route("/preferences/pinned-lines", axum::routing::put(put_pinned_lines))
        .route("/preferences/pinned-stations", axum::routing::put(put_pinned_stations))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreferencesResponse {
    pinned_lines: Vec<String>,
    pinned_stations: Vec<String>,
}

async fn get_preferences(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<PreferencesResponse>, (StatusCode, String)> {
    let pinned_line_ids = preferences::list_pinned_line_ids(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    let custom = custom_lines::list_custom_lines(&app.database)
        .await
        .map_err(internal_error)?;
    // TfL lines live in neither `app.config.lines` (the static catalogue)
    // nor `custom_lines` -- they're ingested straight into `line_status`
    // with `source = 'tfl'` (see `queries::upsert_tfl_line_status`).
    // Without this, a pin on a TfL line (e.g. `tfl-victoria`) is written
    // fine by `replace_pinned_lines` -- which validates nothing -- but
    // silently dropped here on every read, so it renders unstarred again
    // after the next fetch/reload.
    let tfl = queries::tfl_line_summaries(&app.database)
        .await
        .map_err(internal_error)?;
    let pinned_lines = filter_known_pinned_lines(
        pinned_line_ids,
        app.config.lines.iter().map(|l| l.id.clone()),
        custom.into_iter().map(|c| c.id),
        tfl.into_iter().map(|l| l.id),
    );

    let pinned_station_candidates = preferences::list_pinned_station_crs(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    let pinned_stations = preferences::filter_existing_station_crs(&app.database, &pinned_station_candidates)
        .await
        .map_err(internal_error)?;

    Ok(Json(PreferencesResponse { pinned_lines, pinned_stations }))
}

async fn put_pinned_lines(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(ids): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    preferences::replace_pinned_lines(&app.database, &user.id, &ids)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn put_pinned_stations(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(crs_codes): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    if crs_codes.iter().any(|crs| crs.len() != 3) {
        return Err((
            StatusCode::BAD_REQUEST,
            "station codes must be exactly 3 characters".to_string(),
        ));
    }

    preferences::replace_pinned_stations(&app.database, &user.id, &crs_codes)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "preferences operation failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "operation failed".to_string())
}

/// Filters `pinned_line_ids` down to ones that still resolve to a real
/// line, dropping stale ids for lines that have since been removed/renamed.
/// A line is "real" if it appears in the static catalogue, in
/// `custom_lines`, or among the TfL lines `crates/poller-tfl` has ingested
/// -- all three are valid targets of `PUT /preferences/pinned-lines`, which
/// itself validates nothing (see `preferences::replace_pinned_lines`), so
/// this is the only place a stale or foreign id gets caught.
///
/// Factored out of `get_preferences` so the "TfL ids count as known" rule
/// is unit-testable without a database, unlike the three id sources
/// themselves, which each need one to produce for real.
fn filter_known_pinned_lines(
    pinned_line_ids: Vec<String>,
    catalogue_ids: impl IntoIterator<Item = String>,
    custom_ids: impl IntoIterator<Item = String>,
    tfl_ids: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let known_line_ids: HashSet<String> =
        catalogue_ids.into_iter().chain(custom_ids).chain(tfl_ids).collect();
    pinned_line_ids.into_iter().filter(|id| known_line_ids.contains(id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinned_tfl_line_survives_the_known_ids_filter() {
        // This is the regression case: before TfL ids were folded into
        // `known_line_ids`, a pin on a TfL line -- written fine by
        // `replace_pinned_lines`, which validates nothing -- was silently
        // dropped on every read, so a starred TfL line looked unstarred
        // again after the next fetch/reload.
        let pinned = vec!["tfl-victoria".to_string(), "northern".to_string()];
        let result = filter_known_pinned_lines(
            pinned,
            vec!["northern".to_string()],
            vec![],
            vec!["tfl-victoria".to_string()],
        );
        assert_eq!(result, vec!["tfl-victoria".to_string(), "northern".to_string()]);
    }

    #[test]
    fn a_pinned_custom_line_survives_the_known_ids_filter() {
        let pinned = vec!["custom-my-commute".to_string()];
        let result = filter_known_pinned_lines(
            pinned,
            vec![],
            vec!["custom-my-commute".to_string()],
            vec![],
        );
        assert_eq!(result, vec!["custom-my-commute".to_string()]);
    }

    #[test]
    fn a_pinned_id_unknown_to_every_source_is_dropped() {
        // e.g. a line withdrawn from the catalogue, or a TfL line that
        // left the feed and was pruned from `line_status`
        // (`queries::upsert_tfl_line_status`).
        let pinned = vec!["long-gone-line".to_string()];
        let result = filter_known_pinned_lines(pinned, vec![], vec![], vec![]);
        assert!(result.is_empty());
    }
}

/// End-to-end version of the `tests` module's regression case, exercising
/// the real `preferences`/`queries` DB round trip that `get_preferences`
/// itself makes, rather than hand-built inputs: writes a TfL line status
/// row (as `crates/poller-tfl` -> `queries::upsert_tfl_line_status` would),
/// pins it via `preferences::replace_pinned_lines` (the real write path,
/// same as `PUT /preferences/pinned-lines`), then reads it back through
/// `preferences::list_pinned_line_ids` + `queries::tfl_line_summaries` +
/// `filter_known_pinned_lines` -- the same three calls `get_preferences`
/// makes, minus the axum plumbing (constructing a full `App` needs OIDC/
/// Redis config this test has no need of).
#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::data::{preferences, queries};

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_pinned_tfl_line_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip \
                -- --ignored`"]
    async fn a_pinned_tfl_line_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip() {
        use sqlx::postgres::PgPoolOptions;

        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");

        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-PREFS-USER', 'test@example.com', 'Test Rider') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES ('TEST-TFL-PIN', 'test tfl pin line', 'tube', '{TfL}', '[]', 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture tfl line");

        // The real write path: identical to what `PUT /preferences/pinned-lines`
        // does, and validates nothing -- see `preferences::replace_pinned_lines`.
        preferences::replace_pinned_lines(
            &pool,
            "TEST-PREFS-USER",
            &["TEST-TFL-PIN".to_string(), "TEST-UNKNOWN-LINE".to_string()],
        )
        .await
        .expect("pin lines");

        // The real read path: identical to what `get_preferences` does.
        let pinned_line_ids = preferences::list_pinned_line_ids(&pool, "TEST-PREFS-USER")
            .await
            .expect("list pinned line ids");
        let tfl = queries::tfl_line_summaries(&pool).await.expect("tfl_line_summaries");
        let pinned_lines =
            filter_known_pinned_lines(pinned_line_ids, vec![], vec![], tfl.into_iter().map(|l| l.id));

        sqlx::query("DELETE FROM pinned_lines WHERE user_id = 'TEST-PREFS-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM line_status WHERE line_id = 'TEST-TFL-PIN'")
            .execute(&pool)
            .await
            .expect("cleanup fixture tfl line");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-PREFS-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");

        assert!(
            pinned_lines.contains(&"TEST-TFL-PIN".to_string()),
            "a pinned TfL line should survive the read path, not be silently dropped"
        );
        assert!(
            !pinned_lines.contains(&"TEST-UNKNOWN-LINE".to_string()),
            "a pinned id with no matching line anywhere should still be dropped"
        );
    }
}
