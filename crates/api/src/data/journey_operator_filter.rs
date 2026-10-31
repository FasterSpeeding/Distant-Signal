//! A journey's operator filter (planner operator filtering spec 2026-10-31,
//! §5.6 and §6.9): the `/plan` filter its route was planned with, kept on
//! `journeys` so its mid-journey replans and break suggestions honour it.
//!
//! Stored as the mode (`only`, `avoid`, `prefer`), the operators as applied
//! (groups expanded, what replans use) and the entries as chosen (what the
//! journey page shows). Written by `POST /Journeys/itinerary` and
//! `PUT /Journeys/{id}/operator-filter`; read by the replans
//! (`routes::journey_replan`) and served on `GET /Journeys/{id}`.

use anyhow::Result;
use schedule_query::{OperatorId, OperatorMask};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::data::operator_groups;

/// The request shape (`POST /Journeys/itinerary`'s `operatorFilter`, and
/// the `PUT` body's): a mode and the entries as `/Trips/plan` takes them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperatorFilterRequest {
    pub mode: String,
    pub entries: Vec<String>,
}

/// A stored filter, as `GET /Journeys/{id}` serves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyOperatorFilter {
    /// `only`, `avoid` or `prefer`.
    pub mode: String,
    /// The operators as applied, groups expanded.
    pub codes: Vec<String>,
    /// As chosen (`group:GTR`).
    pub entries: Vec<String>,
}

/// A filter request parsed and checked, ready to store: `Err` is a 400's
/// sentence (an unknown mode, no entry, or what
/// [`operator_groups::parse_operator_list`] refuses). The caller still
/// checks the named ATOC codes against `tocs` and the timetable
/// ([`OperatorFilterRequest`] carries no dates).
pub fn parse(request: &OperatorFilterRequest) -> Result<JourneyOperatorFilter, String> {
    if !matches!(request.mode.as_str(), "only" | "avoid" | "prefer") {
        return Err("operatorFilter.mode must be only, avoid or prefer".to_string());
    }
    let list =
        operator_groups::parse_operator_list("operatorFilter", Some(&request.entries.join(",")))?
            .ok_or_else(|| "operatorFilter needs at least one operator".to_string())?;
    Ok(JourneyOperatorFilter {
        mode: request.mode.clone(),
        codes: list.codes(),
        entries: list.entries,
    })
}

impl JourneyOperatorFilter {
    /// The ATOC codes named directly (not through a group), for the
    /// caller's `known_operator_codes` check.
    pub fn named_atoc_codes(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| {
                !entry.starts_with(operator_groups::GROUP_PREFIX) && !entry.starts_with("ds:")
            })
            .cloned()
            .collect()
    }

    /// The operators as a mask (codes the build no longer knows dropped).
    pub fn mask(&self) -> OperatorMask {
        OperatorMask::of(
            self.codes
                .iter()
                .filter_map(|code| OperatorId::from_code(code)),
        )
    }

    /// The replans' search rule for `only`/`avoid` (every way of riding an
    /// operator, as on `/Trips/plan`); `None` for `prefer`, which filters
    /// nothing.
    pub fn rule(&self) -> Option<trip_planner::OperatorRule> {
        match self.mode.as_str() {
            "only" => Some(trip_planner::OperatorRule::only(self.mask())),
            "avoid" => Some(trip_planner::OperatorRule::avoid(self.mask())),
            _ => None,
        }
    }

    /// The ticket's "same operator" search's operators under this filter:
    /// `only` keeps the ticket's operators this filter allows, `avoid`
    /// drops the ones it avoids, `prefer` changes nothing.
    pub fn narrow_ticket_operators(&self, ticket: &[String]) -> Vec<String> {
        let mask = self.mask();
        let listed =
            |code: &String| OperatorId::from_atoc(code).is_some_and(|id| mask.contains(id));
        match self.mode.as_str() {
            "only" => ticket.iter().filter(|code| listed(code)).cloned().collect(),
            "avoid" => ticket
                .iter()
                .filter(|code| !listed(code))
                .cloned()
                .collect(),
            _ => ticket.to_vec(),
        }
    }
}

/// `journey_id`'s filter, `None` when it has none.
pub async fn load(pool: &PgPool, journey_id: i64) -> Result<Option<JourneyOperatorFilter>> {
    let row: Option<(Option<String>, Vec<String>, Vec<String>)> = sqlx::query_as(
        "SELECT operator_filter_mode, operator_filter_codes, operator_filter_entries \
         FROM journeys WHERE id = $1",
    )
    .bind(journey_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(mode, codes, entries)| {
        mode.map(|mode| JourneyOperatorFilter {
            mode,
            codes,
            entries,
        })
    }))
}

/// Sets (or, with `None`, clears) `journey_id`'s filter. `user_id` scopes
/// it to the owner: `false` when no journey of theirs has that id.
pub async fn store<'e, E: sqlx::PgExecutor<'e>>(
    executor: E,
    journey_id: i64,
    user_id: &str,
    filter: Option<&JourneyOperatorFilter>,
) -> Result<bool> {
    let empty: Vec<String> = Vec::new();
    let result = sqlx::query(
        "UPDATE journeys SET operator_filter_mode = $3, operator_filter_codes = $4, \
             operator_filter_entries = $5, updated_at = NOW() \
         WHERE id = $1 AND user_id = $2",
    )
    .bind(journey_id)
    .bind(user_id)
    .bind(filter.map(|f| f.mode.as_str()))
    .bind(filter.map_or(&empty, |f| &f.codes))
    .bind(filter.map_or(&empty, |f| &f.entries))
    .execute(executor)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(mode: &str, entries: &[&str]) -> OperatorFilterRequest {
        OperatorFilterRequest {
            mode: mode.to_string(),
            entries: entries.iter().map(|e| (*e).to_string()).collect(),
        }
    }

    #[test]
    fn parses_and_expands_groups_keeping_the_entries() {
        let filter = parse(&request("only", &["vt", "group:gtr"])).unwrap();
        assert_eq!(filter.mode, "only");
        assert_eq!(filter.entries, ["VT", "group:GTR"]);
        assert_eq!(filter.codes, ["VT", "TL", "GN", "SN", "GX"]);
        assert_eq!(filter.named_atoc_codes(), ["VT"]);
    }

    #[test]
    fn refuses_bad_modes_and_lists() {
        assert!(parse(&request("sometimes", &["VT"])).is_err());
        assert!(parse(&request("only", &[])).is_err());
        assert!(parse(&request("avoid", &["VTX"])).is_err());
    }

    #[test]
    fn rules_follow_the_mode() {
        let lu = OperatorId::LONDON_UNDERGROUND;
        let only = parse(&request("only", &["VT"])).unwrap().rule().unwrap();
        assert!(!only.allows(lu));
        let avoid = parse(&request("avoid", &["LT"])).unwrap().rule().unwrap();
        assert!(!avoid.allows(lu));
        assert!(parse(&request("prefer", &["GR"])).unwrap().rule().is_none());
    }

    #[test]
    fn narrows_the_tickets_operators() {
        let ticket = vec!["GR".to_string(), "VT".to_string()];
        let only = parse(&request("only", &["VT"])).unwrap();
        assert_eq!(only.narrow_ticket_operators(&ticket), ["VT"]);
        let avoid = parse(&request("avoid", &["VT"])).unwrap();
        assert_eq!(avoid.narrow_ticket_operators(&ticket), ["GR"]);
        let prefer = parse(&request("prefer", &["VT"])).unwrap();
        assert_eq!(prefer.narrow_ticket_operators(&ticket), ticket);
    }
}
