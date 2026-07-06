
use std::path::PathBuf;

use anyhow::Result;
use clap::ValueHint;
use serde::{Deserialize, de::DeserializeOwned};
use serde_inline_default::serde_inline_default;

use crate::data::LineDefinition;

#[serde_inline_default]
#[derive(Clone, Deserialize, Debug)]
pub struct Defaults {
    // service is "delayed" above this
    #[serde_inline_default(5)]
    delay_threshold_minutes: i64,
    // >25% of services delayed -> Minor Delays
    #[serde_inline_default(0.25)]
    minor_delays_pct: f64,
    // >50% of services delayed -> Severe Delays
    #[serde_inline_default(0.50)]
    severe_delays_pct: f64,
    // >25% cancelled -> Reduced Service
    #[serde_inline_default(0.25)]
    reduced_service_pct: f64,
    // >60% cancelled -> Part Suspended
    #[serde_inline_default(0.60)]
    part_suspended_pct: f64,
    // Knowledgebase incident handling.
    // aA active KB incident is at least Minor Delays
    #[serde_inline_default(0)]
    knowledgebase_severity_floor: i8,
    // Sample sizing
    // below this many services, don't infer alone
    #[serde_inline_default(3)]
    min_sample_size: i64,
}

fn parse_toml_path<T: DeserializeOwned>(path: &'_ str) -> Result<T> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

fn parse_lines(path: &str) -> Result<LineCatalogue> {
    LineDefinition::from_dir(&PathBuf::from(path)).map(LineCatalogue)
}

/// Newtype around the parsed line catalogue.
///
/// `clap_derive` infers the type it downcasts an `ArgMatches` entry to from
/// the field's *syntactic* shape, not from the `value_parser`'s `Value`
/// type: a bare `Vec<LineDefinition>` field is always treated as "one
/// `LineDefinition` per CLI occurrence, collected via `ArgAction::Append`" —
/// confirmed by a runtime panic ("Mismatch between definition and access of
/// `lines`") the moment `--lines-dir`/`LINES_DIR`/`default_value` actually
/// supplied a value, which nothing did before this field had a default.
/// `parse_lines` instead produces the *entire* vec from a single
/// `--lines-dir` occurrence, so the field type must not look like `Vec<T>`
/// to the derive macro. This newtype (plus `Deref`) sidesteps that:
/// `app.config.lines` still coerces to `&[LineDefinition]` at every existing
/// call site (`crate::routes::samples`, `data::samples::dedup_sample_stations`)
/// with no changes needed there.
#[derive(Debug, Clone, Default)]
pub struct LineCatalogue(pub Vec<LineDefinition>);

impl std::ops::Deref for LineCatalogue {
    type Target = Vec<LineDefinition>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, clap::Parser)]
pub struct ServiceArguments {
    #[arg(short, long, env, default_value = "0.0.0.0:8080")]
    pub bind_url: String,
    #[arg(short, long, env)]
    pub database_url: String,
    /// Shared secret pollers must present via `X-Internal-Token` to reach
    /// `private_router()` endpoints.
    #[arg(long, env)]
    pub internal_token: String,
    #[arg(long, value_parser = parse_toml_path::<Defaults>, value_hint = ValueHint::FilePath, value_name = "FILE")]
    pub defaults_file: Option<Defaults>,
    /// Directory of line-catalogue TOML files, loaded once at startup.
    /// Defaults to `/app/lines` (baked into the Docker image — see
    /// `docker/api.Dockerfile`), overridable via `LINES_DIR` for local
    /// (non-Docker) runs.
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines, value_hint = ValueHint::FilePath, value_name = "DIR")]
    pub lines: LineCatalogue,
}
