use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::app::{AppState, Router};

pub mod app;
pub mod auth;
pub mod data;
pub mod render;
pub mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let app = AppState::init().await?;

    // Permissive by design: none of the routes behind this layer rely on
    // browser-enforced CORS for protection. The four line-status endpoints
    // and /public/health are intentionally public. /private/* requires a
    // shared-secret X-Internal-Token header (crates/api/src/auth.rs) — a
    // header check CORS doesn't bypass — not cookie/credential-based auth,
    // so a permissive origin policy doesn't weaken it. A configurable
    // origin allowlist would add config surface for no real benefit here.
    let cors = CorsLayer::new()
        .allow_methods([axum::http::Method::GET])
        .allow_origin(Any);

    let router = Router::new()
        .merge(routes::line_status::router())
        .nest("/public", routes::public_router())
        .nest("/private", routes::private_router(app.clone()))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(app.clone());

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    sqlx::migrate!().run(&app.database).await?;

    let listener = tokio::net::TcpListener::bind(&app.config.bind_url).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
