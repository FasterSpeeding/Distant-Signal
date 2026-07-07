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

    let router = Router::new()
        // Merged at the top level, unprefixed — see the comment on
        // `routes::public_router()` for why these four TfL-shaped
        // endpoints can't live under `/public` like the rest of that
        // function's routes without breaking TfL-client URL compatibility.
        // Still fully unauthenticated: no auth middleware layer applies
        // to this merge.
        .merge(routes::line_status::router())
        .nest("/public", routes::public_router())
        .nest("/private", routes::private_router(app.clone()))
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
