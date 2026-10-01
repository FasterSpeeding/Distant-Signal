//! Read-only endpoint exposing which stations `poller-ldbws` should
//! sample, computed from the line catalogue loaded into `AppState` at
//! startup plus any custom lines stored in the database. Custom lines can
//! be created/deleted at any time, so they're queried fresh on every
//! request rather than cached like the static catalogue.
//!
//! LEG-18 operator knobs: two optional query parameters narrow the list,
//! `pinned_lines_only=true` (only lines at least one user has pinned) and
//! `max_stations=N` (a line-fair cap; see
//! `data::samples::select_sample_stations`). With neither, the response is
//! the full deduplicated list, exactly as before, and no pin data is read.
//!
//! They are request parameters rather than `api` config because every
//! LDBWS volume setting then lives in one place, `poller-ldbws`'s own
//! config and its `pollers.ldbws` chart block, next to the poll interval
//! and hourly request budget it has to be weighed against. The selection
//! itself is done here, not in the poller, because only `api` knows which
//! line each station serves and which lines are pinned: the poller sees a
//! flat CRS list, so a poller-side cap could only cut it alphabetically
//! and would drop whole lines at the end of the alphabet first.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use common::LineDefinition;
use serde::Deserialize;

use crate::app::{App, Router};
use crate::data::samples::{SampleSelection, select_sample_stations};
use crate::data::{custom_lines, preferences};

pub fn router() -> Router {
    Router::new().route("/sample-stations", axum::routing::get(get_sample_stations))
}

/// `GET /private/sample-stations` query parameters. Both optional; see
/// the module docs.
#[derive(Debug, Default, Deserialize)]
struct SampleStationsQuery {
    #[serde(default)]
    pinned_lines_only: bool,
    #[serde(default)]
    max_stations: Option<usize>,
}

impl From<SampleStationsQuery> for SampleSelection {
    fn from(query: SampleStationsQuery) -> Self {
        SampleSelection {
            pinned_lines_only: query.pinned_lines_only,
            max_stations: query.max_stations,
        }
    }
}

async fn get_sample_stations(
    State(app): State<App>,
    Query(query): Query<SampleStationsQuery>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    let selection = SampleSelection::from(query);
    let custom = custom_lines::list_custom_lines(&app.database)
        .await
        .map_err(internal_error)?;
    let mut lines: Vec<LineDefinition> = app.config.lines.to_vec();
    lines.extend(custom.into_iter().map(LineDefinition::from));

    let pin_counts = if selection.is_unrestricted() {
        HashMap::new()
    } else {
        preferences::count_pins_per_line(&app.database)
            .await
            .map_err(internal_error)?
    };
    Ok(Json(select_sample_stations(&lines, &pin_counts, selection)))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used as a map_err callback, which passes the error by value"
)]
fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "sample-stations query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use axum::extract::Query;
    use axum::http::Uri;

    use super::*;

    fn parse(uri: &str) -> SampleSelection {
        let uri: Uri = uri.parse().expect("valid uri");
        let Query(query) =
            Query::<SampleStationsQuery>::try_from_uri(&uri).expect("query should parse");
        SampleSelection::from(query)
    }

    #[test]
    fn no_query_string_is_the_unrestricted_default() {
        let selection = parse("/private/sample-stations");
        assert_eq!(selection, SampleSelection::default());
        assert!(selection.is_unrestricted());
    }

    #[test]
    fn both_knobs_parse_from_the_query_string() {
        assert_eq!(
            parse("/private/sample-stations?pinned_lines_only=true&max_stations=120"),
            SampleSelection {
                pinned_lines_only: true,
                max_stations: Some(120),
            }
        );
    }
}
