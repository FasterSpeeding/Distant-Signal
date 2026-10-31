//! Who runs each connection and fixed link of a trip-planner graph, for
//! operator filters (docs/superpowers/specs/2026-10-31-planner-operator-
//! filtering-design.md, "the spec" below, §5.1 and §6.1-6.2).
//!
//! - **CIF connections** take their schedule's ATOC code from
//!   `schedule_services.operator_atoc` ([`OperatorTable`]). That table
//!   covers every UID the graph holds, buses and ferries included; the old
//!   source, `schedule_destination_departures`, missed about 850 buses and
//!   ferries a day (spec §2.1).
//! - **Transit trips** take their network's DS operator
//!   ([`transit_operator`]).
//! - **Fixed links** ([`attribute_fixed_links`]): `TUBE` links are London
//!   Underground, and so is a `TRANSFER` between a pair of stations that
//!   also has a `TUBE` link (spec §3.2: those are Tube-speed transfer
//!   allowances); `METRO` links follow `reference-data/fixed-link-operators.csv`.
//!   Anything else stays [`OperatorId::UNKNOWN`] and passes every filter.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::Result;
use chrono::NaiveDate;
use schedule_query::{Connection, DsOperator, InterchangeData, OperatorId};
use sqlx::PgPool;

/// `reference-data/fixed-link-operators.csv`, compiled in.
const FIXED_LINK_OPERATORS_CSV: &str =
    include_str!("../../../../reference-data/fixed-link-operators.csv");

/// [`FIXED_LINK_OPERATORS_CSV`] parsed: station CRS -> operator.
static METRO_STATIONS: LazyLock<HashMap<String, OperatorId>> =
    LazyLock::new(|| parse_metro_stations(FIXED_LINK_OPERATORS_CSV));

fn parse_metro_stations(csv: &str) -> HashMap<String, OperatorId> {
    csv.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && *line != "crs,operator")
        .filter_map(|line| {
            let (crs, operator) = line.split_once(',')?;
            Some((crs.trim().to_owned(), OperatorId::from_code(operator)?))
        })
        .collect()
}

/// A date's UID -> operator, from `schedule_services` (spec §6.1).
#[derive(Debug, Clone, Default)]
pub struct OperatorTable {
    by_uid: HashMap<String, OperatorId>,
}

impl OperatorTable {
    #[must_use]
    pub fn new(by_uid: HashMap<String, OperatorId>) -> Self {
        Self { by_uid }
    }

    /// Reads `service_date`'s published operators: one row per schedule
    /// (about 26k), `NULL` codes left out.
    pub async fn fetch(pool: &PgPool, service_date: NaiveDate) -> Result<Self> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT uid, operator_atoc FROM schedule_services \
             WHERE service_date = $1 AND operator_atoc IS NOT NULL",
        )
        .bind(service_date)
        .fetch_all(pool)
        .await?;
        Ok(Self::new(
            rows.into_iter()
                .filter_map(|(uid, code)| Some((uid, OperatorId::from_atoc(&code)?)))
                .collect(),
        ))
    }

    /// `uid`'s operator: a CIF schedule's from the table, a transit trip's
    /// network ([`transit_operator`]), else [`OperatorId::UNKNOWN`].
    #[must_use]
    pub fn operator_of(&self, uid: &str) -> OperatorId {
        if crate::data::transit_planning::is_transit_uid(uid) {
            return transit_operator(uid);
        }
        self.by_uid.get(uid).copied().unwrap_or(OperatorId::UNKNOWN)
    }

    /// Sets every connection's `operator`, and returns how many CIF
    /// connections have none (should be 0: spec §6.7).
    pub fn attribute_connections(&self, connections: &mut [Connection]) -> usize {
        let mut unknown = 0;
        for connection in connections {
            connection.operator = self.operator_of(&connection.uid);
            if connection.operator.is_unknown()
                && !crate::data::transit_planning::is_transit_uid(&connection.uid)
            {
                unknown += 1;
            }
        }
        unknown
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_uid.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_uid.is_empty()
    }
}

/// A transit trip's operator, by its network (`metrolink:12`). The `tfl`
/// network (`TfL` P4) has no rule yet: its trips will be attributed by line
/// mode (Tube to `LT`) when they join the graph.
#[must_use]
pub fn transit_operator(uid: &str) -> OperatorId {
    match uid.split_once(':').map(|(network, _)| network) {
        Some("metrolink") => OperatorId::ds(DsOperator::Metrolink),
        Some("net") => OperatorId::ds(DsOperator::Net),
        _ => OperatorId::UNKNOWN,
    }
}

/// Sets every fixed link's `operator` (see the module doc), and returns
/// how many non-walk links stay unattributed, by mode (spec §6.7).
pub fn attribute_fixed_links(data: &mut InterchangeData) -> HashMap<String, usize> {
    let pair = |a: &str, b: &str| {
        if a <= b {
            (a.to_owned(), b.to_owned())
        } else {
            (b.to_owned(), a.to_owned())
        }
    };
    let tube_pairs: HashSet<(String, String)> = data
        .fixed_links_from_crs
        .iter()
        .flat_map(|(from, links)| {
            links
                .iter()
                .filter(|link| link.mode == "TUBE")
                .map(move |link| pair(from, &link.to_crs))
        })
        .collect();
    let metro = &*METRO_STATIONS;
    let mut unattributed: HashMap<String, usize> = HashMap::new();
    for (from, links) in &mut data.fixed_links_from_crs {
        for link in links.iter_mut() {
            link.operator = match link.mode.as_str() {
                "TUBE" => OperatorId::LONDON_UNDERGROUND,
                "TRANSFER" if tube_pairs.contains(&pair(from, &link.to_crs)) => {
                    OperatorId::LONDON_UNDERGROUND
                }
                "METRO" => metro_operator(metro, from, &link.to_crs),
                _ => OperatorId::UNKNOWN,
            };
            if link.operator.is_unknown() && !link.is_walk() {
                *unattributed.entry(link.mode.clone()).or_default() += 1;
            }
        }
    }
    unattributed
}

/// The graph build's attribution gauges (spec §6.7):
/// `api_trip_plan_graph_unknown_operator_connections` (CIF connections with
/// no `schedule_services` operator: should be 0, else that table lags the
/// calling points) and `api_trip_plan_graph_unattributed_links{mode}`
/// (non-walk fixed links no operator is known for: a new `METRO` link
/// belongs in `reference-data/fixed-link-operators.csv`).
#[expect(clippy::cast_precision_loss, reason = "gauges of a few thousand")]
#[expect(
    clippy::implicit_hasher,
    reason = "the caller passes attribute_fixed_links' own map"
)]
pub fn record_attribution(
    date: NaiveDate,
    unknown_connections: usize,
    unattributed_links: &HashMap<String, usize>,
) {
    metrics::gauge!(common::metrics::metric_name(
        "api_trip_plan_graph_unknown_operator_connections"
    ))
    .set(unknown_connections as f64);
    if unknown_connections > 0 {
        tracing::warn!(
            %date,
            unknown_connections,
            "trip-planner connections with no schedule_services operator"
        );
    }
    for (mode, count) in unattributed_links {
        metrics::gauge!(
            common::metrics::metric_name("api_trip_plan_graph_unattributed_links"),
            "mode" => mode.clone()
        )
        .set(*count as f64);
    }
}

/// A `METRO` link's operator: the listed operator of either end, unless
/// the two ends are listed under different operators.
fn metro_operator(metro: &HashMap<String, OperatorId>, a: &str, b: &str) -> OperatorId {
    match (metro.get(a), metro.get(b)) {
        (Some(x), Some(y)) if x != y => OperatorId::UNKNOWN,
        (Some(x), _) | (None, Some(x)) => *x,
        (None, None) => OperatorId::UNKNOWN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schedule_query::FixedLink;

    fn link(mode: &str, to: &str) -> FixedLink {
        FixedLink {
            mode: mode.to_owned(),
            to_crs: to.to_owned(),
            minutes: 10,
            valid_from: "0000".to_owned(),
            valid_to: "2359".to_owned(),
            days_mask: "1111111".to_owned(),
            operator: OperatorId::UNKNOWN,
        }
    }

    fn interchange(rows: &[(&str, FixedLink)]) -> InterchangeData {
        let mut data = InterchangeData {
            modal_change: schedule_query::ModalChangeBuffer::default(),
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        for (from, link) in rows {
            data.fixed_links_from_crs
                .entry((*from).to_owned())
                .or_default()
                .push(link.clone());
        }
        data
    }

    fn operator(data: &InterchangeData, from: &str, mode: &str, to: &str) -> OperatorId {
        data.fixed_links_from_crs[from]
            .iter()
            .find(|l| l.mode == mode && l.to_crs == to)
            .unwrap()
            .operator
    }

    #[test]
    fn the_csv_parses_every_row() {
        let metro = &*METRO_STATIONS;
        assert_eq!(metro.get("CWX"), Some(&OperatorId::ds(DsOperator::Dlr)));
        assert_eq!(
            metro.get("ELE"),
            Some(&OperatorId::ds(DsOperator::LondonTrams))
        );
        assert_eq!(
            metro.get("ALT"),
            Some(&OperatorId::ds(DsOperator::Metrolink))
        );
        assert_eq!(metro.get("NOP"), OperatorId::from_atoc("TW").as_ref());
        assert_eq!(metro.get("LTN"), None);
        let rows = FIXED_LINK_OPERATORS_CSV
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty() && *l != "crs,operator")
            .count();
        assert_eq!(metro.len(), rows, "every row parses");
    }

    #[test]
    fn tube_links_and_their_transfer_twins_are_london_underground() {
        let mut data = interchange(&[
            ("EUS", link("TUBE", "VIC")),
            // The reverse direction of the same pair.
            ("VIC", link("TRANSFER", "EUS")),
            ("BFR", link("TRANSFER", "CHX")),
            ("KGX", link("WALK", "STP")),
            ("FEL", link("BUS", "HTR")),
        ]);
        let unattributed = attribute_fixed_links(&mut data);
        let lu = OperatorId::LONDON_UNDERGROUND;
        assert_eq!(operator(&data, "EUS", "TUBE", "VIC"), lu);
        assert_eq!(operator(&data, "VIC", "TRANSFER", "EUS"), lu);
        assert!(operator(&data, "BFR", "TRANSFER", "CHX").is_unknown());
        assert!(operator(&data, "KGX", "WALK", "STP").is_unknown());
        assert_eq!(unattributed.get("TRANSFER"), Some(&1));
        assert_eq!(unattributed.get("BUS"), Some(&1));
        assert_eq!(unattributed.get("WALK"), None, "walks aren't counted");
    }

    #[test]
    fn metro_links_follow_the_csv() {
        let mut data = interchange(&[
            ("CWX", link("METRO", "GNW")),
            ("ELE", link("METRO", "BKJ")),
            // One listed end is enough (Metrolink's own stop codes).
            ("AMM", link("METRO", "ALT")),
            ("LTN", link("METRO", "LUA")),
            // Ends listed under two operators: no attribution.
            ("CWX", link("METRO", "ALT")),
        ]);
        attribute_fixed_links(&mut data);
        assert_eq!(
            operator(&data, "CWX", "METRO", "GNW"),
            OperatorId::ds(DsOperator::Dlr)
        );
        assert_eq!(
            operator(&data, "ELE", "METRO", "BKJ"),
            OperatorId::ds(DsOperator::LondonTrams)
        );
        assert_eq!(
            operator(&data, "AMM", "METRO", "ALT"),
            OperatorId::ds(DsOperator::Metrolink)
        );
        assert!(operator(&data, "LTN", "METRO", "LUA").is_unknown());
        assert!(operator(&data, "CWX", "METRO", "ALT").is_unknown());
    }

    #[test]
    fn connections_take_the_table_or_their_network() {
        let table = OperatorTable::new(HashMap::from([(
            "C1".to_owned(),
            OperatorId::from_atoc("VT").unwrap(),
        )]));
        let conn = |uid: &str| Connection {
            uid: uid.to_owned(),
            from_tiploc: "A".to_owned(),
            to_tiploc: "B".to_owned(),
            departure_min: 0,
            arrival_min: 1,
            working_departure_min: 0,
            working_arrival_min: 1,
            can_board: true,
            can_alight: true,
            operator: OperatorId::UNKNOWN,
        };
        let mut connections = vec![conn("C1"), conn("metrolink:4"), conn("C2"), conn("tfl:1")];
        assert_eq!(
            table.attribute_connections(&mut connections),
            1,
            "C2 has no row"
        );
        assert_eq!(
            connections[0].operator,
            OperatorId::from_atoc("VT").unwrap()
        );
        assert_eq!(
            connections[1].operator,
            OperatorId::ds(DsOperator::Metrolink)
        );
        assert!(connections[2].operator.is_unknown());
        assert!(connections[3].operator.is_unknown());
    }
}
