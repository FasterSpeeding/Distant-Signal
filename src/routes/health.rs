use axum::{Json, routing::get};

use crate::{app::Router, dataclasses::HealthStatus};

pub fn router() -> Router {
    Router::new().route("/health", get(get_health))
}

async fn get_health() -> Json<HealthStatus> {
    Json(HealthStatus {
        message: "Alive".to_string(),
    })
}
