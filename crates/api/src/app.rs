use std::sync::Arc;

use anyhow::{Context, Result};
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
