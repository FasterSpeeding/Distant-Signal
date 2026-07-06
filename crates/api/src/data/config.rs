
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

fn parse_lines(path: &str) -> Result<Vec<LineDefinition>> {
    LineDefinition::from_dir(&PathBuf::from(path))
}

#[derive(Debug, clap::Parser)]
pub struct ServiceArguments {
    #[arg(short, long, env, default_value = "0.0.0.0:8080")]
    pub bind_url: String,
    #[arg(short, long, env)]
    pub database_url: String,
    #[arg(long, value_parser = parse_toml_path::<Defaults>, value_hint = ValueHint::FilePath, value_name = "FILE")]
    pub defaults_file: Option<Defaults>,
    #[arg(long = "lines-dir", value_parser = parse_lines, value_hint = ValueHint::FilePath, value_name = "DIR")]
    pub lines: Vec<LineDefinition>,
}
