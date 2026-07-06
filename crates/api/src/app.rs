use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::data::config::ServiceArguments;


#[derive(Debug)]
pub struct AppState {
    pub config: ServiceArguments,
    pub database: PgPool,
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

        Ok(Arc::new(Self {
            config,
            database: db,
        }))
    }
}
