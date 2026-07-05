use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::Severity;



/// How confident are we in this status?
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataQuality {
    #[default]
    Knowledgebase,
    LdbwsInferred,
    TrustInferred,
    Planned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidityPeriod {
    pub from_date: DateTime<Utc>,
    pub to_date: Option<DateTime<Utc>>,
    pub is_now: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedRoute {
    pub from_crs: String,
    pub to_crs: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disruption {
    /// `"RealTime"` | `"PlannedWork"` | `"Information"`
    pub category: String,
    pub description: String,
    #[serde(default)]
    pub affected_stops: Vec<String>,
    #[serde(default)]
    pub affected_routes: Vec<AffectedRoute>,
    /// e.g. `"knowledgebase-incident-12345"`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// One status entry on a line. A line may have several simultaneously.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineStatus {
    pub severity: Severity,
    pub reason: String,
    pub validity: ValidityPeriod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disruption: Option<Disruption>,
    #[serde(default)]
    pub data_quality: DataQuality,
}

/// Top-level object returned by the API for one line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineStatusReport {
    pub id: String,
    pub name: String,
    pub mode_name: String,
    pub operators: Vec<String>,
    pub statuses: Vec<LineStatus>,
}

impl LineStatusReport {
    /// Lowest numeric severity is the most disruptive.
    pub fn worst_severity(&self) -> Severity {
        self.statuses
            .iter()
            .map(|s| s.severity)
            .min()
            .unwrap_or(Severity::GoodService)
    }
}

// --- Inputs the aggregator consumes ---

/// One service from an LDBWS departure board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationDeparture {
    pub service_id: String,
    pub operator: String,
    pub destination_crs: String,
    /// `std` field.
    pub scheduled: String,
    /// `etd` — may be `"On time"`, `"Cancelled"`, or `"HH:MM"`.
    pub estimated: String,
    pub is_cancelled: bool,
    pub delay_minutes: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_reason: Option<String>,
    /// e.g. `"1P23"`, from Darwin's `trainid`/`rid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headcode: Option<String>,
}


#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    pub message: String,
}
