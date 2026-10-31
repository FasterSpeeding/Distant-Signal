//! Who runs each connection and fixed link of a trip-planner graph, for
//! operator filters (docs/superpowers/specs/2026-10-31-planner-operator-
//! filtering-design.md, "the spec" below, §5.1 and §6.1-6.2).
//!
//! - **CIF connections** take their schedule's ATOC code from
//!   `schedule_services.operator_atoc` ([`OperatorTable`]). That table
//!   covers every UID the graph holds, buses and ferries included; the old
//!   source, `schedule_destination_departures`, missed about 850 buses and
//!   ferries a day (spec §2.1).
//! - **Transit trips** take their network's DS operator, or on the `tfl`
//!   network their line's mode's: the Underground is `LT` ([`transit_operator`]).
//! - **Fixed links** ([`attribute_fixed_links`]): `TUBE` links are London
//!   Underground, and so is a `TRANSFER` between a pair of stations that
//!   also has a `TUBE` link (spec §3.2: those are Tube-speed transfer
//!   allowances); `METRO` links follow `reference-data/fixed-link-operators.csv`.
//!   Anything else stays [`OperatorId::UNKNOWN`] and passes every filter.
//! - **Stand-in walks** ([`add_tube_stand_in_walks`]): between two stations
//!   linked by the Tube, an estimated walk of at most
//!   [`STAND_IN_WALK_MAX_MINUTES`], used only when an operator filter rules
//!   the Tube out (spec §5.1, approved 2026-10-31): the timetable has no
//!   walk rows between London's termini.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::Result;
use chrono::NaiveDate;
use schedule_query::{Connection, DsOperator, FixedLink, InterchangeData, OperatorId};
use sqlx::PgPool;

/// The longest estimated walk offered instead of the Tube (spec §5.1).
pub const STAND_IN_WALK_MAX_MINUTES: i32 = 30;

/// Every station pair a London Underground link joins (either direction),
/// after [`attribute_fixed_links`].
fn tube_pairs(data: &InterchangeData) -> HashSet<(String, String)> {
    data.fixed_links_from_crs
        .iter()
        .flat_map(|(from, links)| {
            links
                .iter()
                .filter(|link| {
                    link.operator == OperatorId::LONDON_UNDERGROUND && !link.is_stand_in()
                })
                .map(move |link| unordered(from, &link.to_crs))
        })
        .collect()
}

fn unordered(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_owned(), b.to_owned())
    } else {
        (b.to_owned(), a.to_owned())
    }
}

/// The positions (`stations.latitude`, `longitude`) of the stations a
/// London Underground link touches: what [`add_tube_stand_in_walks`]
/// measures. A station with no position gets no stand-in walk.
pub async fn fetch_tube_station_positions(
    pool: &PgPool,
    data: &InterchangeData,
) -> Result<HashMap<String, (f64, f64)>> {
    let codes: Vec<String> = tube_pairs(data)
        .into_iter()
        .flat_map(|(a, b)| [a, b])
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let rows: Vec<(String, f64, f64)> = sqlx::query_as(
        "SELECT crs::text, latitude, longitude FROM stations \
         WHERE crs = ANY($1) AND latitude IS NOT NULL AND longitude IS NOT NULL",
    )
    .bind(&codes)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(crs, lat, lon)| (crs.trim().to_ascii_uppercase(), (lat, lon)))
        .collect())
}

/// Adds the stand-in walks (see the module doc), both ways, all day, and
/// re-sorts the links. Returns how many station pairs got one.
#[expect(
    clippy::implicit_hasher,
    reason = "the caller passes fetch_tube_station_positions' own map"
)]
pub fn add_tube_stand_in_walks(
    data: &mut InterchangeData,
    positions: &HashMap<String, (f64, f64)>,
) -> usize {
    let mut added = 0;
    let mut pairs: Vec<(String, String)> = tube_pairs(data).into_iter().collect();
    pairs.sort();
    for (a, b) in pairs {
        let (Some(&pa), Some(&pb)) = (positions.get(&a), positions.get(&b)) else {
            continue;
        };
        let minutes = crate::data::tfl_access::walk_minutes(
            crate::data::transit_planning::metres_between(pa, pb),
        )
        .max(1);
        if minutes > STAND_IN_WALK_MAX_MINUTES {
            continue;
        }
        for (from, to) in [(&a, &b), (&b, &a)] {
            data.fixed_links_from_crs
                .entry(from.clone())
                .or_default()
                .push(FixedLink {
                    mode: "WALK".to_owned(),
                    to_crs: to.clone(),
                    minutes,
                    valid_from: "0000".to_owned(),
                    valid_to: "2359".to_owned(),
                    days_mask: "1111111".to_owned(),
                    operator: OperatorId::UNKNOWN,
                    only_without: OperatorId::LONDON_UNDERGROUND,
                });
        }
        added += 1;
    }
    data.sort_fixed_links();
    added
}

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
    /// by its network and line mode ([`transit_operator`]; `line_mode` is
    /// the trip's line's, `TransitGraph::trip_mode`), else
    /// [`OperatorId::UNKNOWN`].
    #[must_use]
    pub fn operator_of(&self, uid: &str, line_mode: Option<&str>) -> OperatorId {
        if crate::data::transit_planning::is_transit_uid(uid) {
            return transit_operator(uid, line_mode);
        }
        self.by_uid.get(uid).copied().unwrap_or(OperatorId::UNKNOWN)
    }

    /// Sets every connection's `operator` (`trip_mode` gives a transit
    /// trip's line mode), and returns how many CIF connections have none
    /// (should be 0: spec §6.7).
    pub fn attribute_connections<'g>(
        &self,
        connections: &mut [Connection],
        trip_mode: impl Fn(&str) -> Option<&'g str>,
    ) -> usize {
        let mut unknown = 0;
        for connection in connections {
            connection.operator = self.operator_of(&connection.uid, trip_mode(&connection.uid));
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

/// A transit trip's operator, by its network (`metrolink:12`). On the
/// `tfl` network it follows the trip's line's mode (`line_mode`, from
/// `transit_lines.mode`; spec §6.2): `tube` is London Underground (`LT`),
/// `dlr` `ds:DLR` and `tram` `ds:LONDON_TRAMS`. Any other `tfl` line (none
/// is planned today) stays [`OperatorId::UNKNOWN`].
#[must_use]
pub fn transit_operator(uid: &str, line_mode: Option<&str>) -> OperatorId {
    match uid.split_once(':').map(|(network, _)| network) {
        Some("metrolink") => OperatorId::ds(DsOperator::Metrolink),
        Some("net") => OperatorId::ds(DsOperator::Net),
        Some("tfl") => match line_mode {
            Some("tube") => OperatorId::LONDON_UNDERGROUND,
            Some("dlr") => OperatorId::ds(DsOperator::Dlr),
            Some("tram") => OperatorId::ds(DsOperator::LondonTrams),
            _ => OperatorId::UNKNOWN,
        },
        _ => OperatorId::UNKNOWN,
    }
}

/// Sets every fixed link's `operator` (see the module doc), and returns
/// how many non-walk links stay unattributed, by mode (spec §6.7).
pub fn attribute_fixed_links(data: &mut InterchangeData) -> HashMap<String, usize> {
    let tube_pairs: HashSet<(String, String)> = data
        .fixed_links_from_crs
        .iter()
        .flat_map(|(from, links)| {
            links
                .iter()
                .filter(|link| link.mode == "TUBE")
                .map(move |link| unordered(from, &link.to_crs))
        })
        .collect();
    let metro = &*METRO_STATIONS;
    let mut unattributed: HashMap<String, usize> = HashMap::new();
    for (from, links) in &mut data.fixed_links_from_crs {
        for link in links.iter_mut() {
            link.operator = match link.mode.as_str() {
                "TUBE" => OperatorId::LONDON_UNDERGROUND,
                "TRANSFER" if tube_pairs.contains(&unordered(from, &link.to_crs)) => {
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

/// The fixed link a planned transfer took from `from_crs` to `to_crs`: one
/// of that `mode` and `minutes`, an ordinary link before a stand-in (they
/// can't be told apart by the journey alone, and an ordinary one is the
/// likelier). For the transfer leg's `operator` and `estimatedWalk`.
#[must_use]
pub fn transfer_link<'a>(
    data: &'a InterchangeData,
    from_crs: &str,
    to_crs: &'a str,
    mode: &str,
    minutes: i32,
) -> Option<&'a FixedLink> {
    schedule_query::fixed_links_between(data, from_crs, to_crs)
        .filter(|link| link.mode == mode && link.minutes == minutes)
        .min_by_key(|link| link.is_stand_in())
}

/// Every operator with a connection or a (non-stand-in) link in a graph.
#[must_use]
pub fn running_operators(
    connections: &[Connection],
    data: &InterchangeData,
) -> schedule_query::OperatorMask {
    let mut running = schedule_query::OperatorMask::default();
    for connection in connections {
        running.insert(connection.operator);
    }
    for link in data.fixed_links_from_crs.values().flatten() {
        if !link.is_stand_in() {
            running.insert(link.operator);
        }
    }
    running
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
            only_without: OperatorId::UNKNOWN,
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
    fn stand_in_walks_join_walkable_tube_pairs_only() {
        // London Bridge - Liverpool Street is about 24 minutes on foot,
        // Euston - Victoria about an hour (spec §3.2).
        let mut data = interchange(&[
            ("LBG", link("TUBE", "LST")),
            ("EUS", link("TUBE", "VIC")),
            ("EUS", link("TRANSFER", "VIC")),
            ("KGX", link("WALK", "STP")),
            ("PAD", link("TUBE", "ZZZ")),
        ]);
        attribute_fixed_links(&mut data);
        let positions: HashMap<String, (f64, f64)> = [
            ("LBG", (51.5049, -0.0864)),
            ("LST", (51.5178, -0.0817)),
            ("EUS", (51.5282, -0.1337)),
            ("VIC", (51.4952, -0.1441)),
            ("PAD", (51.5154, -0.1755)),
        ]
        .into_iter()
        .map(|(crs, at)| (crs.to_owned(), at))
        .collect();
        assert_eq!(add_tube_stand_in_walks(&mut data, &positions), 1);
        let walk = |from: &str, to: &str| {
            data.fixed_links_from_crs
                .get(from)
                .into_iter()
                .flatten()
                .find(|l| l.to_crs == to && l.is_stand_in())
                .cloned()
        };
        let lbg_lst = walk("LBG", "LST").expect("LBG-LST is walkable");
        assert_eq!(lbg_lst.mode, "WALK");
        assert_eq!(lbg_lst.only_without, OperatorId::LONDON_UNDERGROUND);
        assert!(lbg_lst.operator.is_unknown());
        assert!((10..=STAND_IN_WALK_MAX_MINUTES).contains(&lbg_lst.minutes));
        assert!(walk("LST", "LBG").is_some(), "both ways");
        assert!(walk("EUS", "VIC").is_none(), "an hour on foot");
        assert!(walk("PAD", "ZZZ").is_none(), "no position for ZZZ");
        // A stand-in isn't a Tube link itself, and running_operators skips it.
        assert_eq!(tube_pairs(&data).len(), 3);
        // The transfer leg picks the ordinary link over a stand-in.
        let found = transfer_link(&data, "LBG", "LST", "WALK", lbg_lst.minutes).unwrap();
        assert!(found.is_stand_in());
        let tube = transfer_link(&data, "EUS", "VIC", "TUBE", 10).unwrap();
        assert_eq!(tube.operator, OperatorId::LONDON_UNDERGROUND);
    }

    #[test]
    fn running_operators_list_connections_and_links() {
        let mut data = interchange(&[("EUS", link("TUBE", "VIC"))]);
        attribute_fixed_links(&mut data);
        let connections = vec![Connection {
            uid: "C1".into(),
            from_tiploc: "A".into(),
            to_tiploc: "B".into(),
            departure_min: 0,
            arrival_min: 1,
            working_departure_min: 0,
            working_arrival_min: 1,
            can_board: true,
            can_alight: true,
            operator: OperatorId::from_atoc("VT").unwrap(),
        }];
        let running = running_operators(&connections, &data);
        assert!(running.contains(OperatorId::from_atoc("VT").unwrap()));
        assert!(running.contains(OperatorId::LONDON_UNDERGROUND));
        assert!(!running.contains(OperatorId::from_atoc("GR").unwrap()));
    }

    #[test]
    fn connections_take_the_table_or_their_network() {
        let table = OperatorTable::new(HashMap::from([(
            "C1".to_owned(),
            OperatorId::from_atoc("VT").unwrap(),
        )]));
        let conn = |uid: &str| Connection {
            uid: uid.into(),
            from_tiploc: "A".into(),
            to_tiploc: "B".into(),
            departure_min: 0,
            arrival_min: 1,
            working_departure_min: 0,
            working_arrival_min: 1,
            can_board: true,
            can_alight: true,
            operator: OperatorId::UNKNOWN,
        };
        let mut connections = vec![
            conn("C1"),
            conn("metrolink:4"),
            conn("C2"),
            conn("tfl:1"),
            conn("tfl:2"),
            conn("tfl:3"),
            conn("tfl:4"),
        ];
        let trip_mode = |uid: &str| match uid {
            "tfl:1" => Some("tube"),
            "tfl:2" => Some("dlr"),
            "tfl:3" => Some("tram"),
            _ => None,
        };
        assert_eq!(
            table.attribute_connections(&mut connections, trip_mode),
            1,
            "C2 has no row; a transit trip is never counted"
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
        assert_eq!(connections[3].operator, OperatorId::LONDON_UNDERGROUND);
        assert_eq!(connections[3].operator.code().as_deref(), Some("LT"));
        assert_eq!(connections[4].operator, OperatorId::ds(DsOperator::Dlr));
        assert_eq!(
            connections[5].operator,
            OperatorId::ds(DsOperator::LondonTrams)
        );
        assert!(
            connections[6].operator.is_unknown(),
            "a tfl line with no mode"
        );
    }
}
