use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use redis::aio::ConnectionManager;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::data::config::ServiceArguments;


pub struct AppState {
    pub config: ServiceArguments,
    pub database: PgPool,
    pub redis: ConnectionManager,
}

// Manual `Debug` rather than `#[derive(Debug)]`: `redis::aio::ConnectionManager`
// doesn't implement `Debug`, so the derive doesn't compile once it's a field.
impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("config", &self.config)
            .field("database", &self.database)
            .field("redis", &"ConnectionManager { .. }")
            .finish()
    }
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

        let redis_client = redis::Client::open(config.redis_url.clone())
            .context("Could not parse REDIS_URL")?;
        let redis = redis_client
            .get_connection_manager()
            .await
            .context("Could not connect to redis")?;

        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
        }))
    }
}
