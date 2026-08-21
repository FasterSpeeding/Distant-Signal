use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::data::config::ServiceArguments;


#[derive(Debug)]
pub struct AppState {
    pub config: ServiceArguments,
    pub database: PgPool,
    /// Deliberately a lazy `redis::Client`, NOT a live
    /// `redis::aio::ConnectionManager`. `Client::open` only parses the URL --
    /// it never opens a socket -- so an unreachable Redis cannot fail
    /// `AppState::init` and crash-loop the whole public status API. The one
    /// consumer (`data::queries::upsert_incidents`) connects at publish time
    /// and already logs-and-continues on failure, and the enricher's hourly
    /// sweep is the backstop for anything that misses the stream. A broken
    /// enrichment path must never be able to take displayed status down.
    pub redis: redis::Client,
}

pub type App = Arc<AppState>;
pub type Router = axum::Router<App>;

impl AppState {
    pub async fn init() -> Result<App> {
        let config = ServiceArguments::parse();

        // An empty token would make `auth::constant_time_eq` compare two
        // empty byte slices and accept any request with no
        // `X-Internal-Token` header at all — reject that at startup rather
        // than silently running an unauthenticated `private_router()`.
        ensure!(
            !config.internal_token.is_empty(),
            "internal_token (--internal-token / INTERNAL_TOKEN) must not be empty"
        );

        let db = PgPoolOptions::new()
            .max_connections(50)
            .connect(&config.database_url)
            .await
            .context("Could not connect to database")?;

        // No eager connect: only the URL is validated here. See the `redis`
        // field's doc comment on `AppState`.
        let redis = redis::Client::open(config.redis_url.clone())
            .context("Could not parse REDIS_URL")?;

        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
        }))
    }
}
