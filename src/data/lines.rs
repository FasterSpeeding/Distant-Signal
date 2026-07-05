use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}};

use anyhow::Result;
use glob::glob;
use serde::{Deserialize, Serialize};

use crate::types::Severity;

/// A station as it appears on one specific line.
///
/// `segment` groups consecutive stations into a named section of track.
/// Segments shared between lines represent shared trunks; segments unique to a
/// line are that line's exclusive sections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Station {
    pub crs: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiploc: Option<String>,
    #[serde(default = "Station::default_role")]
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<String>,
}

impl Station {
    fn default_role() -> String {
        "minor".to_string()
    }
}

/// A user-facing "line" the aggregator reports status for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineDefinition {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub category: String,
    pub operators: Vec<String>,
    pub stations: Vec<Station>,
    #[serde(default)]
    pub sample_stations: Vec<String>,
    #[serde(default)]
    pub match_keywords: Vec<String>,
    #[serde(default)]
    pub excluded_keywords: Vec<String>,
    #[serde(default)]
    pub severity_overrides: HashMap<String, Severity>,
    /// Segments this line considers exclusive (not shared with other lines).
    /// If empty, the matcher derives exclusivity by comparing segment usage
    /// across all loaded lines.
    #[serde(default)]
    pub exclusive_segments: Vec<String>,
    /// Destination CRS filters used during LDBWS inference.
    #[serde(default)]
    pub destination_crs_filter: Vec<String>,
    /// Headcode prefix filters used during LDBWS inference.
    #[serde(default)]
    pub headcode_prefixes: Vec<String>,
}

impl LineDefinition {
    pub fn from_file(path: &Path) -> Result<Self> {
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn from_dir(dir_path: &Path) -> Result<Vec<Self>> {
        let paths = glob(&format!("{}/*.toml", dir_path.display()))?;
        paths.map(|path| { Self::from_file(&path?) }).collect()
    }

    pub fn has_station(&self, crs: &str) -> bool {
        self.stations.iter().any(|s| s.crs == crs)
    }

    pub fn segment_for(&self, crs: &str) -> Option<&str> {
        self.stations
            .iter()
            .find(|s| s.crs == crs)
            .and_then(|s| s.segment.as_deref())
    }

    pub fn segments(&self) -> HashSet<&str> {
        self.stations
            .iter()
            .filter_map(|s| s.segment.as_deref())
            .collect()
    }

    /// Returns CRS codes between two stations inclusive, in order.
    pub fn stations_between(&self, from_crs: &str, to_crs: &str) -> Vec<&str> {
        let crs_list: Vec<&str> = self.stations.iter().map(|s| s.crs.as_str()).collect();
        let Some(i) = crs_list.iter().position(|&c| c == from_crs) else {
            return vec![];
        };
        let Some(j) = crs_list.iter().position(|&c| c == to_crs) else {
            return vec![];
        };
        let (lo, hi) = if i <= j { (i, j) } else { (j, i) };
        crs_list[lo..=hi].to_vec()
    }
}
