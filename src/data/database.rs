
/// A parsed Knowledgebase incident.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMessage {
    pub incident_id: String,
    pub summary: String,
    pub description: String,
    /// ATOC codes mentioned.
    pub operators: Vec<String>,
    /// CRS codes parsed from message.
    pub affected_stations: Vec<String>,
    /// `"major"` | `"minor"` if NR tagged it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub is_planned: bool,
}

/// An LDBWS poll result for one station along a line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationSample {
    pub crs: String,
    pub polled_at: DateTime<Utc>,
    pub departures: Vec<StationDeparture>,
}
