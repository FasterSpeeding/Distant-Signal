//! `/public/lines`: enumerate official + custom lines, and create/delete
//! custom ones. Unauthenticated — see
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
//! Non-goals for why.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::data::custom_lines::{self, NewCustomLine};

pub fn router() -> Router {
    Router::new()
        .route("/lines", axum::routing::get(list_lines).post(create_line))
        .route("/lines/{id}", axum::routing::delete(delete_line))
}

#[derive(Debug, Serialize)]
struct LineSummary {
    id: String,
    name: String,
    category: String,
    operators: Vec<String>,
    source: &'static str,
}

async fn list_lines(State(app): State<App>) -> Result<Json<Vec<LineSummary>>, (StatusCode, String)> {
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

    let custom = custom_lines::list_custom_lines(&app.database).await.map_err(internal_error)?;
    out.extend(custom.into_iter().map(|c| LineSummary {
        id: c.id,
        name: c.name,
        category: "custom".to_string(),
        operators: c.operators,
        source: "custom",
    }));

    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
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
    Json(req): Json<CreateLineRequest>,
) -> Result<Json<LineSummary>, (StatusCode, String)> {
    if req.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name must not be empty".to_string()));
    }
    if req.stations.len() < 2 {
        return Err((StatusCode::BAD_REQUEST, "a line needs at least 2 stations".to_string()));
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

async fn delete_line(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    if app.config.lines.iter().any(|l| l.id == id) {
        return Err((StatusCode::BAD_REQUEST, "cannot delete a catalogue line".to_string()));
    }

    let deleted = custom_lines::delete_custom_line(&app.database, &id).await.map_err(internal_error)?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "custom line operation failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "operation failed".to_string())
}
