//! Transit networks (Manchester Metrolink first) in `/Trips/plan`'s day
//! graph: docs/superpowers/specs/2026-10-24-transit-trip-planning-design.md
//! ("the spec" below), phases M1 and M2.
//!
//! **Trams are ordinary connections.** Each trip of a network's active
//! `transit_*` feed version that runs on the date becomes connections in
//! the same array as the trains (spec §6), so Connection Scan, RAPTOR,
//! arrive-by, waypoints and vias need no change. A trip's UID is
//! `<network>:<n>` ([`transit_uid`]): it can never match a CIF UID, a TRUST
//! id or a Darwin RID (none contains `:`), so the live overlay never
//! attaches a train's report to it.
//!
//! **Nodes** (spec §5). A tram station is a planner node keyed by its
//! `NaPTAN` code (`9400ZZMAPIC`); its platforms are its members, the way a
//! station's TIPLOCs are its CRS's, so the planner's same-CRS sibling rule
//! charges a platform change at the network's `change_minutes` (§8.2).
//!
//! **Rail links** (spec §8.1). Each `transit_rail_links` row becomes a
//! `WALK` fixed link both ways between the rail station and the tram node.
//! Its minutes are the estimated walk ([`rail_link_walk_minutes`]) plus the
//! tram-side change buffer (`TRIP_PLAN_TRANSIT_CHANGE_MINUTES`, spec §8.3).
//! The buffer rides on the link because every rail-to-tram or tram-to-rail
//! change crosses exactly one such walk (a tram node and a rail station are
//! never one node), and a change between two trams crosses none, which is
//! the spec's rule ("a change between two trams owes none") with no change
//! to any search: arrive-by mirrors it for free. Like the bus buffer, a walk
//! from a rail origin to the tram (or from the tram to a rail destination)
//! also carries it.
//!
//! **Off by default** (spec §14). `API_TRIP_PLAN_TRANSIT_NETWORKS` (chart
//! `api.tripPlanTransitNetworks`) lists the networks; empty, nothing here
//! runs and the graph, its cache key and every response are as before.
//! With it set, the tram connections are in every cached graph, but a
//! request plans with them only when it opts in (`transit=true`, or a tram
//! stop as an end): otherwise it searches the graph's rail-only interchange
//! data, where no walk reaches a tram platform, so no tram can be boarded
//! and the response is byte-identical to the switch being off.
//!
//! **Scheduled, never live** (spec §7). A transit leg carries
//! `realtime: false` and `liveTracking: false`, no `live` block, and an
//! "about every N min" hint ([`headway_minutes`]).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use schedule_query::{CallingPointForConnections, Connection, FixedLink, InterchangeData};
use serde::Serialize;
use sqlx::PgPool;

/// The networks whose trams enter the planner's graph
/// (`API_TRIP_PLAN_TRANSIT_NETWORKS`, comma-separated; chart
/// `api.tripPlanTransitNetworks`). Empty by default.
pub const SWITCH_ENV: &str = "API_TRIP_PLAN_TRANSIT_NETWORKS";

/// `TRIP_PLAN_TRANSIT_CHANGE_MINUTES` (chart
/// `api.tripPlanTransitChangeMinutes`): the extra change time on the tram
/// side of a rail-to-tram or tram-to-rail change (spec §8.3, Q6), clamped to
/// `0..=15`, default 2.
pub const CHANGE_MINUTES_ENV: &str = "TRIP_PLAN_TRANSIT_CHANGE_MINUTES";
pub const DEFAULT_CHANGE_MINUTES: u32 = 2;
const MAX_CHANGE_MINUTES: u32 = 15;

/// How a network's stations are spelt on the wire and in URLs (spec §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicStopId {
    /// The `NaPTAN` access-node code, `9400ZZMAPIC` (Metrolink's pages).
    AccessNode,
    /// The stop-area code, `940GZZCRWMB` (`TfL`'s pages; London Trams, later).
    StopArea,
}

/// A network the planner can take (spec §13.4). The producer's network file
/// (`crates/poller-transit-schedule/networks/*.toml`) is not read by the
/// api, so its planner settings live here, compiled in, next to the code
/// that uses them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannerNetwork {
    pub id: &'static str,
    /// The minimum change between two platforms of one station, and for a
    /// fresh boarding on the same platform (spec §8.2).
    pub change_minutes: i32,
    pub public_stop_id: PublicStopId,
    /// Its stations are `TfL`'s stops (the `TfL` transit design §11): their
    /// rail links are `tfl_stop_crs`'s (no rail-links file), the stations
    /// of one `TfL` hub (Bank's Underground and DLR) are joined by walks,
    /// and a `TfL` stop end becomes a node only for a request that opted in
    /// on a date the timetable covers; otherwise it stays a `TfL` stop and
    /// `tfl_access`'s model plans it, as before.
    pub tfl_stops: bool,
}

/// Every network the planner can take. A network listed in
/// [`SWITCH_ENV`] must be here (and in `common::transit::NETWORKS`).
pub const PLANNER_NETWORKS: &[PlannerNetwork] = &[
    PlannerNetwork {
        id: "metrolink",
        change_minutes: 3,
        public_stop_id: PublicStopId::AccessNode,
        tfl_stops: false,
    },
    // TfL's Underground, DLR and trams (the TfL transit design §11).
    PlannerNetwork {
        id: "tfl",
        change_minutes: 3,
        public_stop_id: PublicStopId::StopArea,
        tfl_stops: true,
    },
];

pub fn planner_network(id: &str) -> Option<&'static PlannerNetwork> {
    // The DB tests seed a `test-*` network (`ds_store::test_support::transit`)
    // rather than touch a real network's rows.
    #[cfg(test)]
    if id.starts_with("test-") {
        return Some(&TEST_PLANNER_NETWORK);
    }
    PLANNER_NETWORKS.iter().find(|network| network.id == id)
}

#[cfg(test)]
static TEST_PLANNER_NETWORK: PlannerNetwork = PlannerNetwork {
    id: "test",
    change_minutes: 3,
    public_stop_id: PublicStopId::AccessNode,
    tfl_stops: false,
};

/// Parses [`SWITCH_ENV`]: comma-separated ids, blanks dropped, duplicates
/// kept once, in order. An id the planner does not know is an error.
pub fn parse_switch(raw: Option<&str>) -> Result<Vec<String>, String> {
    let mut networks: Vec<String> = Vec::new();
    for id in raw.unwrap_or_default().split(',').map(str::trim) {
        if id.is_empty() || networks.iter().any(|known| known == id) {
            continue;
        }
        if planner_network(id).is_none() || !common::transit::NETWORKS.contains(&id) {
            return Err(format!(
                "{SWITCH_ENV}: '{id}' is not a network the trip planner can take (known: {})",
                PLANNER_NETWORKS
                    .iter()
                    .map(|network| network.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        networks.push(id.to_owned());
    }
    Ok(networks)
}

static SWITCH: LazyLock<Result<Vec<String>, String>> =
    LazyLock::new(|| parse_switch(std::env::var(SWITCH_ENV).ok().as_deref()));

/// Fails startup on an unknown id in [`SWITCH_ENV`].
pub fn check_switch_from_env() -> Result<()> {
    match &*SWITCH {
        Ok(networks) => {
            if !networks.is_empty() {
                tracing::info!(?networks, "transit networks in the trip planner");
            }
            Ok(())
        }
        Err(message) => Err(anyhow::anyhow!("{message}")),
    }
}

#[cfg(test)]
thread_local! {
    static SWITCH_OVERRIDE: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// The networks in the planner switch. Empty when it is off (or invalid,
/// which [`check_switch_from_env`] refuses at startup).
pub fn enabled_networks() -> Vec<String> {
    #[cfg(test)]
    if let Some(networks) = SWITCH_OVERRIDE.with(|cell| cell.borrow().clone()) {
        return networks;
    }
    SWITCH.as_ref().cloned().unwrap_or_default()
}

/// Sets the switch on the current thread until the guard is dropped (the
/// DB tests run their requests on the test's own thread).
#[cfg(test)]
#[must_use = "the override is lifted as soon as the guard is dropped"]
pub fn set_switch_for_tests(networks: &[&str]) -> SwitchOverride {
    SWITCH_OVERRIDE.with(|cell| {
        *cell.borrow_mut() = Some(networks.iter().map(|n| (*n).to_owned()).collect());
    });
    SwitchOverride(())
}

#[cfg(test)]
pub struct SwitchOverride(());

#[cfg(test)]
impl Drop for SwitchOverride {
    fn drop(&mut self) {
        SWITCH_OVERRIDE.with(|cell| *cell.borrow_mut() = None);
    }
}

/// The configured tram-side change buffer, read once.
pub fn change_minutes() -> u32 {
    static MINUTES: LazyLock<u32> =
        LazyLock::new(|| parse_change_minutes(std::env::var(CHANGE_MINUTES_ENV).ok().as_deref()));
    *MINUTES
}

fn parse_change_minutes(raw: Option<&str>) -> u32 {
    match raw.map(str::trim) {
        None | Some("") => DEFAULT_CHANGE_MINUTES,
        Some(raw) => raw.parse::<u32>().map_or_else(
            |_| {
                tracing::warn!(raw, "invalid {CHANGE_MINUTES_ENV}; using the default");
                DEFAULT_CHANGE_MINUTES
            },
            |minutes| minutes.min(MAX_CHANGE_MINUTES),
        ),
    }
}

/// A transit trip's UID in the graph: `<network>:<n>`, `n` its index in
/// the day's trips ordered by start and trip id (spec §6.2).
pub fn transit_uid(network: &str, index: usize) -> String {
    format!("{network}:{index}")
}

/// Whether `uid` is a transit trip's ([`transit_uid`]): no CIF UID, TRUST
/// id or Darwin RID contains `:`.
pub fn is_transit_uid(uid: &str) -> bool {
    uid.contains(':')
}

fn split_uid(uid: &str) -> Option<(&str, usize)> {
    let (network, index) = uid.split_once(':')?;
    Some((network, index.parse().ok()?))
}

/// Whether `code` could be a `NaPTAN` tram stop code (`9400ZZMAPIC`,
/// `940GZZMAPIC`, a platform `9400ZZMAPIC1`): `940` and at least eight
/// upper-case letters and digits, as `common::transit::is_stop_id` allows.
pub fn looks_like_naptan(code: &str) -> bool {
    code.len() >= 8 && code.starts_with("940") && common::transit::is_stop_id(code)
}

/// `940GZZMAPIC` -> `9400ZZMAPIC` (the stop-area to access-node rule,
/// transit networks spec §6.3); any other code unchanged.
pub fn access_node_spelling(code: &str) -> String {
    match code.strip_prefix("940G") {
        Some(rest) => format!("9400{rest}"),
        None => code.to_owned(),
    }
}

// --- Reading a network's day ---------------------------------------------

/// Why a network in the switch was left out of a date's graph (spec §6.4).
/// Counted by `api_trip_plan_transit_networks_skipped_total`; every reason
/// is `noTimetable` on the wire ("Tram times aren't available for this
/// date"), never "no trams".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The network has no active feed version.
    NoFeed,
    /// The date is outside the active version's horizon (60 days, Q10).
    OutsideHorizon,
    /// The horizon covers the date but no service runs on it (a gap).
    NoServiceDay,
    /// Reading the feed failed; the plan is made without it.
    Error,
}

impl SkipReason {
    pub fn metric_label(self) -> &'static str {
        match self {
            Self::NoFeed => "no_feed",
            Self::OutsideHorizon => "outside_horizon",
            Self::NoServiceDay => "no_service_day",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StopRow {
    pub stop_id: String,
    /// `station` or `platform`.
    pub kind: String,
    pub parent_stop_id: Option<String>,
    pub name: String,
    pub indicator: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternStopRow {
    pub stop_id: String,
    pub arr_offset_s: i32,
    pub dep_offset_s: i32,
    pub pickup: i16,
    pub drop_off: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternRow {
    pub line_id: Option<String>,
    pub direction: i16,
    pub headsign: Option<String>,
    /// In `seq` order.
    pub stops: Vec<PatternStopRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripRow {
    pub trip_id: String,
    pub line_id: Option<String>,
    pub pattern_id: String,
    pub start_s: i32,
    pub headsign: Option<String>,
    pub replacement_bus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRow {
    pub name: String,
    pub colour: Option<String>,
    /// The line's own mode (`tube`, `dlr`, `tram`), when its network mixes
    /// them: a leg's `serviceMode`.
    pub mode: Option<String>,
}

/// A walk between two stations of one network that are one `TfL` hub
/// (Bank's Underground and DLR stations), both ways.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubWalkRow {
    pub from: String,
    pub to: String,
    pub minutes: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RailLinkRow {
    /// The tram station.
    pub stop_id: String,
    pub crs: String,
    /// `interchange` or `nearby`.
    pub kind: String,
    /// The rail station's `(latitude, longitude)`, when `stations` has it.
    pub station_position: Option<(f64, f64)>,
}

/// One network's timetable for one date: what [`TransitGraph`] is built
/// from. Read by [`read_network_day`]; hand-built in tests.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkDay {
    pub network: String,
    pub settings: PlannerNetwork,
    pub stops: Vec<StopRow>,
    pub patterns: HashMap<String, PatternRow>,
    /// The date's trips, ordered by `(start_s, trip_id)`: a trip's index is
    /// its UID's `n`.
    pub trips: Vec<TripRow>,
    pub lines: HashMap<String, LineRow>,
    pub rail_links: Vec<RailLinkRow>,
    /// Walks between stations of one `TfL` hub (a `tfl_stops` network).
    pub hub_walks: Vec<HubWalkRow>,
    /// A `tfl_stops` network's stations' `TfL` modes (`tube`, `dlr`), by
    /// node: what a hub walk's ends say they are.
    pub stop_modes: HashMap<String, Vec<String>>,
}

type ActiveRow = (Option<String>, Option<NaiveDate>, Option<NaiveDate>);

/// `network`'s day on `date` from its active feed version, or why it is
/// left out. Seven small indexed reads (the trips are about 1,500 rows a
/// weekday for Metrolink, the pattern stops under 2,000).
#[expect(
    clippy::too_many_lines,
    clippy::type_complexity,
    reason = "seven linear reads, each into its row tuple"
)]
pub async fn read_network_day(
    pool: &PgPool,
    network: &str,
    date: NaiveDate,
) -> Result<Result<NetworkDay, SkipReason>> {
    let Some(settings) = planner_network(network).copied() else {
        return Ok(Err(SkipReason::NoFeed));
    };
    let active: Option<ActiveRow> = sqlx::query_as(
        "SELECT n.active_feed_version, v.horizon_from, v.horizon_to \
         FROM transit_networks n LEFT JOIN transit_feed_versions v \
           ON v.network = n.network AND v.feed_version = n.active_feed_version \
         WHERE n.network = $1",
    )
    .bind(network)
    .fetch_optional(pool)
    .await
    .context("transit active feed version")?;
    let Some((Some(feed_version), from, to)) = active else {
        return Ok(Err(SkipReason::NoFeed));
    };
    if from.is_none_or(|from| date < from) || to.is_none_or(|to| date > to) {
        return Ok(Err(SkipReason::OutsideHorizon));
    }
    let trips: Vec<(String, Option<String>, String, i32, Option<String>, bool)> = sqlx::query_as(
        "SELECT t.trip_id, t.line_id, t.pattern_id, t.start_s, t.headsign, t.replacement_bus \
             FROM transit_trips t \
             WHERE t.network = $1 AND t.feed_version = $2 AND EXISTS ( \
               SELECT 1 FROM transit_service_days d \
               WHERE d.network = t.network AND d.feed_version = t.feed_version \
                 AND d.service_id = t.service_id AND d.service_date = $3) \
             ORDER BY t.start_s, t.trip_id",
    )
    .bind(network)
    .bind(&feed_version)
    .bind(date)
    .fetch_all(pool)
    .await
    .context("transit trips")?;
    if trips.is_empty() {
        return Ok(Err(SkipReason::NoServiceDay));
    }
    let pattern_rows: Vec<(String, Option<String>, i16, Option<String>)> = sqlx::query_as(
        "SELECT pattern_id, line_id, direction, headsign FROM transit_patterns \
         WHERE network = $1 AND feed_version = $2",
    )
    .bind(network)
    .bind(&feed_version)
    .fetch_all(pool)
    .await
    .context("transit patterns")?;
    let mut patterns: HashMap<String, PatternRow> = pattern_rows
        .into_iter()
        .map(|(id, line_id, direction, headsign)| {
            (
                id,
                PatternRow {
                    line_id,
                    direction,
                    headsign,
                    stops: Vec::new(),
                },
            )
        })
        .collect();
    let pattern_stops: Vec<(String, String, i32, i32, i16, i16)> = sqlx::query_as(
        "SELECT pattern_id, stop_id, arr_offset_s, dep_offset_s, pickup, drop_off \
         FROM transit_pattern_stops WHERE network = $1 AND feed_version = $2 \
         ORDER BY pattern_id, seq",
    )
    .bind(network)
    .bind(&feed_version)
    .fetch_all(pool)
    .await
    .context("transit pattern stops")?;
    for (pattern_id, stop_id, arr_offset_s, dep_offset_s, pickup, drop_off) in pattern_stops {
        if let Some(pattern) = patterns.get_mut(&pattern_id) {
            pattern.stops.push(PatternStopRow {
                stop_id,
                arr_offset_s,
                dep_offset_s,
                pickup,
                drop_off,
            });
        }
    }
    let stops: Vec<StopRow> = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            Option<f64>,
            Option<f64>,
        ),
    >(
        "SELECT stop_id, kind, parent_stop_id, name, indicator, lat, lon FROM transit_stops \
         WHERE network = $1 AND feed_version = $2 ORDER BY stop_id",
    )
    .bind(network)
    .bind(&feed_version)
    .fetch_all(pool)
    .await
    .context("transit stops")?
    .into_iter()
    .map(
        |(stop_id, kind, parent_stop_id, name, indicator, lat, lon)| StopRow {
            stop_id,
            kind,
            parent_stop_id,
            name,
            indicator,
            lat,
            lon,
        },
    )
    .collect();
    let lines: HashMap<String, LineRow> =
        sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
            "SELECT line_id, name, colour, mode FROM transit_lines \
             WHERE network = $1 AND feed_version = $2",
        )
        .bind(network)
        .bind(&feed_version)
        .fetch_all(pool)
        .await
        .context("transit lines")?
        .into_iter()
        .map(|(id, name, colour, mode)| (id, LineRow { name, colour, mode }))
        .collect();
    let (rail_links, hub_walks, stop_modes) = if settings.tfl_stops {
        tfl_stop_links(pool, &stops).await?
    } else {
        (
            read_rail_links(pool, network).await?,
            Vec::new(),
            HashMap::new(),
        )
    };
    Ok(Ok(NetworkDay {
        network: network.to_owned(),
        settings,
        stops,
        patterns,
        trips: trips
            .into_iter()
            .map(
                |(trip_id, line_id, pattern_id, start_s, headsign, replacement_bus)| TripRow {
                    trip_id,
                    line_id,
                    pattern_id,
                    start_s,
                    headsign,
                    replacement_bus,
                },
            )
            .collect(),
        lines,
        rail_links,
        hub_walks,
        stop_modes,
    }))
}

/// A network's rail links from `transit_rail_links` (its checked-in file).
async fn read_rail_links(pool: &PgPool, network: &str) -> Result<Vec<RailLinkRow>> {
    let rail_links: Vec<RailLinkRow> =
        sqlx::query_as::<_, (String, String, String, Option<f64>, Option<f64>)>(
            "SELECT l.stop_id, l.crs, l.kind, s.latitude, s.longitude \
             FROM transit_rail_links l LEFT JOIN stations s ON s.crs = l.crs \
             WHERE l.network = $1 ORDER BY l.stop_id, l.crs",
        )
        .bind(network)
        .fetch_all(pool)
        .await
        .context("transit rail links")?
        .into_iter()
        .map(|(stop_id, crs, kind, lat, lon)| RailLinkRow {
            stop_id,
            crs: crs.trim().to_ascii_uppercase(),
            kind,
            station_position: lat.zip(lon),
        })
        .collect();
    Ok(rail_links)
}

/// A `tfl_stops` network's links, from `TfL`'s stop tables: each station's
/// National Rail stations (`tfl_stop_crs`, an `interchange` each), the
/// walks between the network's stations of one hub (`tfl_stops.hub_code`,
/// timed as an interchange walk from their coordinates), and each
/// station's `TfL` modes. A station is its stop's access-node spelling
/// (`940GZZLUKSX` is `9400ZZLUKSX`).
async fn tfl_stop_links(
    pool: &PgPool,
    stops: &[StopRow],
) -> Result<(
    Vec<RailLinkRow>,
    Vec<HubWalkRow>,
    HashMap<String, Vec<String>>,
)> {
    let stations: HashMap<&str, &StopRow> = stops
        .iter()
        .filter(|s| s.kind == "station")
        .map(|s| (s.stop_id.as_str(), s))
        .collect();
    let rows: Vec<(String, Vec<String>, Option<String>)> =
        sqlx::query_as("SELECT stop_id, modes, hub_code FROM tfl_stops")
            .fetch_all(pool)
            .await
            .context("tfl stops")?;
    let mut stop_modes = HashMap::new();
    let mut hubs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (stop_id, modes, hub) in rows {
        let node = access_node_spelling(&stop_id);
        if !stations.contains_key(node.as_str()) {
            continue;
        }
        if let Some(hub) = hub.filter(|h| !h.is_empty()) {
            hubs.entry(hub).or_default().push(node.clone());
        }
        stop_modes.insert(node, modes);
    }
    let position = |node: &str| stations.get(node).and_then(|s| s.lat.zip(s.lon));
    let mut hub_walks = Vec::new();
    for members in hubs.values() {
        for (i, from) in members.iter().enumerate() {
            for to in &members[i + 1..] {
                let distance = position(from)
                    .zip(position(to))
                    .map(|(a, b)| metres_between(a, b));
                let minutes = rail_link_walk_minutes(distance, "interchange");
                hub_walks.push(HubWalkRow {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    minutes,
                });
            }
        }
    }
    let links: Vec<(String, String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT c.stop_id, c.crs, s.latitude, s.longitude \
         FROM tfl_stop_crs c LEFT JOIN stations s ON s.crs = c.crs \
         ORDER BY c.stop_id, c.crs",
    )
    .fetch_all(pool)
    .await
    .context("tfl stop stations")?;
    let rail_links = links
        .into_iter()
        .filter_map(|(stop_id, crs, lat, lon)| {
            let node = access_node_spelling(&stop_id);
            stations.contains_key(node.as_str()).then(|| RailLinkRow {
                stop_id: node,
                crs: crs.trim().to_ascii_uppercase(),
                kind: "interchange".to_owned(),
                station_position: lat.zip(lon),
            })
        })
        .collect();
    Ok((rail_links, hub_walks, stop_modes))
}

/// The station `code` names in a switched-on network's active feed (`code`
/// already upper-case; either `NaPTAN` spelling, or a platform, which folds
/// to its station), as `(network, station node)`. One indexed read; the
/// node is in the network's stored (access-node) spelling.
pub async fn find_station(
    pool: &PgPool,
    networks: &[String],
    code: &str,
) -> Result<Option<(String, String)>> {
    let code = access_node_spelling(code);
    sqlx::query_as(
        "SELECT s.network, \
                CASE WHEN s.kind = 'platform' AND s.parent_stop_id IS NOT NULL \
                     THEN s.parent_stop_id ELSE s.stop_id END \
         FROM transit_stops s JOIN transit_networks n \
           ON n.network = s.network AND n.active_feed_version = s.feed_version \
         WHERE s.network = ANY($1) AND s.stop_id = $2 \
         ORDER BY s.network LIMIT 1",
    )
    .bind(networks)
    .bind(&code)
    .fetch_optional(pool)
    .await
    .context("transit stop lookup")
}

/// Whether `network`'s active timetable covers `date` (its horizon, and a
/// service that day): a `tfl_stops` network's stop end becomes a node only
/// then (the `TfL` transit design §11.1); otherwise `tfl_access` plans it.
pub async fn network_covers(pool: &PgPool, network: &str, date: NaiveDate) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM transit_networks n \
           JOIN transit_feed_versions v ON v.network = n.network \
                AND v.feed_version = n.active_feed_version \
           JOIN transit_service_days d ON d.network = n.network \
                AND d.feed_version = n.active_feed_version AND d.service_date = $2 \
         WHERE n.network = $1 AND v.horizon_from <= $2 AND v.horizon_to >= $2)",
    )
    .bind(network)
    .bind(date)
    .fetch_one(pool)
    .await
    .context("transit coverage")
}

/// The latest `activated_at` among `networks`: the graph cache's key is the
/// later of this and the schedule publish marker (spec §6.4), so a new tram
/// feed invalidates the cached graphs at once. `None` for an empty list
/// (no read).
pub async fn latest_activation(
    pool: &PgPool,
    networks: &[String],
) -> Result<Option<DateTime<Utc>>> {
    if networks.is_empty() {
        return Ok(None);
    }
    sqlx::query_scalar("SELECT max(activated_at) FROM transit_networks WHERE network = ANY($1)")
        .bind(networks)
        .fetch_one(pool)
        .await
        .context("transit activation marker")
}

/// The graph cache key: the later of the two markers.
pub fn cache_marker(
    schedule: Option<DateTime<Utc>>,
    transit: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    schedule.max(transit)
}

// --- Expansion -------------------------------------------------------------

/// Seconds after midnight to the nearest minute, half up (spec §6.1): both
/// ends of a connection use it, so a no-dwell stop arrives and departs in
/// the same minute and a trip stays non-decreasing.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to >= 0; start_s is below 172,800 and offsets a few hours"
)]
pub fn round_to_minute(seconds: i64) -> u32 {
    ((seconds.max(0) + 30) / 60) as u32
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::expect_used,
    reason = "minutes are below two days; a time modulo 1440 minutes is always valid"
)]
fn clock(minutes: u32) -> (NaiveTime, u8) {
    (
        NaiveTime::from_num_seconds_from_midnight_opt((minutes % 1440) * 60, 0)
            .expect("minutes modulo 1440 is a valid time"),
        (minutes / 1440) as u8,
    )
}

/// One trip as calling points for `schedule_query::build_connections`:
/// each pattern stop at `start_s` plus its offsets, rounded
/// ([`round_to_minute`]), boarding where GTFS `pickup` is not 1 (none) and
/// alighting where `drop_off` is not 1 (2 and 3, phone or ask the driver,
/// are allowed). No public times: the timetable is the only one.
pub fn trip_calling_points(
    trip: &TripRow,
    pattern: &PatternRow,
) -> Vec<CallingPointForConnections> {
    let last = pattern.stops.len().saturating_sub(1);
    pattern
        .stops
        .iter()
        .enumerate()
        .map(|(seq, stop)| {
            let arrival = round_to_minute(i64::from(trip.start_s) + i64::from(stop.arr_offset_s));
            let departure = round_to_minute(i64::from(trip.start_s) + i64::from(stop.dep_offset_s));
            let (arrival_time, arrival_day) = clock(arrival);
            let (departure_time, departure_day) = clock(departure);
            CallingPointForConnections {
                tiploc: stop.stop_id.clone(),
                booked_arrival: (seq > 0).then_some(arrival_time),
                booked_departure: (seq < last).then_some(departure_time),
                day_offset: if seq > 0 { arrival_day } else { departure_day },
                can_board: stop.pickup != 1,
                can_alight: stop.drop_off != 1,
                public_arrival: None,
                public_departure: None,
            }
        })
        .collect()
}

/// A day's trips of every network as calling points keyed by their UIDs
/// ([`transit_uid`]), to join the trains' before the one sort. A trip whose
/// pattern is missing is skipped.
pub fn calling_points(days: &[NetworkDay]) -> HashMap<String, Vec<CallingPointForConnections>> {
    let mut by_uid = HashMap::new();
    for day in days {
        for (index, trip) in day.trips.iter().enumerate() {
            let Some(pattern) = day.patterns.get(&trip.pattern_id) else {
                continue;
            };
            by_uid.insert(
                transit_uid(&day.network, index),
                trip_calling_points(trip, pattern),
            );
        }
    }
    by_uid
}

// --- Nodes and walks -------------------------------------------------------

/// The `mode` of a rail-link walk: the ALF's own walking mode.
pub const RAIL_LINK_MODE: &str = "WALK";
/// A rail-link walk's least minutes: `interchange` (one complex) and
/// `nearby` (a street or two), and the most for either (spec §8.1).
const INTERCHANGE_MIN_MINUTES: i32 = 2;
const NEARBY_MIN_MINUTES: i32 = 4;
const RAIL_LINK_MAX_MINUTES: i32 = 15;
/// The walk assumed when either end has no coordinates.
const INTERCHANGE_FALLBACK_MINUTES: i32 = 5;
const NEARBY_FALLBACK_MINUTES: i32 = 10;
const EARTH_RADIUS_METRES: f64 = 6_371_000.0;

/// The great-circle distance in metres. (`tfl_access` has the same private
/// helper; that module is left alone while the Tube branch and Old Street
/// work changes it, so this is not yet shared.)
pub fn metres_between(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1, lat2, lon2) = (
        a.0.to_radians(),
        a.1.to_radians(),
        b.0.to_radians(),
        b.1.to_radians(),
    );
    let h = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    EARTH_RADIUS_METRES * 2.0 * h.sqrt().asin()
}

/// The estimated walk between a tram station and a rail station (spec
/// §8.1, Q2): the straight line times `tfl_access::WALK_DETOUR` at
/// `tfl_access::WALK_METRES_PER_MINUTE` (the `TfL` access model's walk),
/// rounded up, clamped to at least 2 minutes (`interchange`) or 4
/// (`nearby`) and at most 15. With no distance, 5 or 10.
#[expect(clippy::cast_possible_truncation, reason = "clamped to a few minutes")]
pub fn rail_link_walk_minutes(distance_metres: Option<f64>, kind: &str) -> i32 {
    let nearby = kind == "nearby";
    let Some(metres) = distance_metres else {
        return if nearby {
            NEARBY_FALLBACK_MINUTES
        } else {
            INTERCHANGE_FALLBACK_MINUTES
        };
    };
    let minutes = (metres * crate::data::tfl_access::WALK_DETOUR
        / crate::data::tfl_access::WALK_METRES_PER_MINUTE)
        .ceil()
        .min(f64::from(RAIL_LINK_MAX_MINUTES)) as i32;
    let floor = if nearby {
        NEARBY_MIN_MINUTES
    } else {
        INTERCHANGE_MIN_MINUTES
    };
    minutes.clamp(floor, RAIL_LINK_MAX_MINUTES)
}

/// A station's planner node and the stop a leg names: what a platform
/// belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopInfo {
    pub network: String,
    /// The node: the station's code, or the stop's own when it has no
    /// station row.
    pub station_id: String,
    pub station_name: String,
    /// The platform's `NaPTAN` indicator (`To Bury`), when it is one.
    pub platform: Option<String>,
}

/// One trip of [`TransitGraph`], by its UID's index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripInfo {
    pub line_id: Option<String>,
    pub direction: i16,
    pub headsign: Option<String>,
    pub replacement_bus: bool,
}

/// One network's part of a date's graph.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkGraph {
    pub network: String,
    pub public_stop_id: PublicStopId,
    pub trips: Vec<TripInfo>,
    pub lines: HashMap<String, LineRow>,
    /// How many connections its trips added (the
    /// `api_trip_plan_transit_connections` gauge).
    pub connections: usize,
}

/// A rail-link walk as built: its walk minutes alone, for the leg's
/// estimate (the link's own minutes add the change buffer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RailLinkWalk {
    pub walk_minutes: i32,
}

/// Everything a date's graph holds for its transit networks: the
/// interchange data with the tram nodes and rail-link walks (a copy of the
/// rail data, used only by requests that opt in), the trips' details for
/// the legs, and the networks left out.
#[derive(Debug, Clone)]
pub struct TransitGraph {
    pub interchange: Arc<InterchangeData>,
    pub networks: Vec<NetworkGraph>,
    /// Every stop a pattern calls at, and every station, by code.
    pub stops: HashMap<String, StopInfo>,
    /// `(tram node, CRS)` -> the walk, both ways.
    pub walks: HashMap<(String, String), RailLinkWalk>,
    /// `(node, node)` -> the walk between two stations of one `TfL` hub,
    /// both ways (a `tfl_stops` network).
    pub hub_walks: HashMap<(String, String), RailLinkWalk>,
    /// A `tfl_stops` network's stations' `TfL` modes, by node.
    pub stop_modes: HashMap<String, Vec<String>>,
    pub skipped: Vec<(String, SkipReason)>,
}

impl TransitGraph {
    fn network(&self, id: &str) -> Option<&NetworkGraph> {
        self.networks.iter().find(|network| network.network == id)
    }

    /// The trip behind a transit UID.
    pub fn trip(&self, uid: &str) -> Option<(&NetworkGraph, &TripInfo)> {
        let (network, index) = split_uid(uid)?;
        let network = self.network(network)?;
        Some((network, network.trips.get(index)?))
    }

    /// The mode of the line a transit trip rides (`tube`, `dlr`, `tram`; a
    /// line's own, from `transit_lines.mode`), `None` for a replacement bus
    /// or a single-mode network's line: what a transit connection's
    /// operator follows (`LT` for the Underground, `ds:DLR`, ...; the
    /// planner operator filtering spec §6.2).
    pub fn trip_mode(&self, uid: &str) -> Option<&str> {
        let (network, trip) = self.trip(uid)?;
        network.lines.get(trip.line_id.as_ref()?)?.mode.as_deref()
    }

    /// Whether `code` is a tram station node of this graph.
    pub fn is_node(&self, code: &str) -> bool {
        self.stops
            .get(code)
            .is_some_and(|stop| stop.station_id == code)
    }

    /// `code` (either spelling, a platform, any case) as its station's node,
    /// when it is a stop of this graph.
    pub fn resolve_end(&self, code: &str) -> Option<&StopInfo> {
        let code = access_node_spelling(&code.trim().to_ascii_uppercase());
        let stop = self.stops.get(&code)?;
        self.stops.get(&stop.station_id)
    }
}

/// The interchange data `rail` plus each network's stations as nodes, their
/// platforms as members with the network's change time, and the rail-link
/// walks with `buffer` added; a replacement bus's UID gets the bus change
/// buffer (spec §6.3). Returns the stop table and walks as well.
#[expect(
    clippy::cast_possible_wrap,
    clippy::implicit_hasher,
    clippy::too_many_lines,
    reason = "the buffer is clamped to 15 minutes; callers use the default hasher; one linear pass per network"
)]
pub fn build_transit_graph(
    rail: &InterchangeData,
    days: &[NetworkDay],
    connections_by_network: &HashMap<String, usize>,
    skipped: Vec<(String, SkipReason)>,
    buffer: u32,
    road_water_minutes: u32,
) -> TransitGraph {
    let mut data = rail.clone();
    let mut stops: HashMap<String, StopInfo> = HashMap::new();
    let mut walks = HashMap::new();
    let mut hub_walks = HashMap::new();
    let mut stop_modes = HashMap::new();
    let mut networks = Vec::new();
    for day in days {
        let by_id: HashMap<&str, &StopRow> =
            day.stops.iter().map(|s| (s.stop_id.as_str(), s)).collect();
        // Every called stop, and every station, gets its node.
        let called: BTreeSet<&str> = day
            .patterns
            .values()
            .flat_map(|pattern| pattern.stops.iter().map(|s| s.stop_id.as_str()))
            .collect();
        // Every platform folds to its station as an end, called or not.
        for stop in &day.stops {
            if let (true, Some(parent)) = (
                stop.kind == "platform",
                stop.parent_stop_id
                    .as_deref()
                    .and_then(|parent| by_id.get(parent)),
            ) {
                stops.insert(
                    stop.stop_id.clone(),
                    StopInfo {
                        network: day.network.clone(),
                        station_id: parent.stop_id.clone(),
                        station_name: parent.name.clone(),
                        platform: stop.indicator.clone(),
                    },
                );
            }
        }
        for stop in &day.stops {
            if stop.kind == "station" {
                stops.insert(
                    stop.stop_id.clone(),
                    StopInfo {
                        network: day.network.clone(),
                        station_id: stop.stop_id.clone(),
                        station_name: stop.name.clone(),
                        platform: None,
                    },
                );
            }
        }
        for code in called {
            let row = by_id.get(code);
            let station = row
                .filter(|row| row.kind == "platform")
                .and_then(|row| row.parent_stop_id.as_deref())
                .and_then(|parent| by_id.get(parent))
                .copied()
                .or_else(|| row.copied());
            let (station_id, station_name) = station.map_or_else(
                || (code.to_owned(), code.to_owned()),
                |s| (s.stop_id.clone(), s.name.clone()),
            );
            let platform = row
                .filter(|row| row.kind == "platform")
                .and_then(|row| row.indicator.clone());
            stops.entry(station_id.clone()).or_insert_with(|| StopInfo {
                network: day.network.clone(),
                station_id: station_id.clone(),
                station_name: station_name.clone(),
                platform: None,
            });
            if code != station_id {
                stops.insert(
                    code.to_owned(),
                    StopInfo {
                        network: day.network.clone(),
                        station_id: station_id.clone(),
                        station_name,
                        platform,
                    },
                );
            }
            data.tiploc_to_crs
                .insert(code.to_owned(), station_id.clone());
            let members = data.crs_to_tiplocs.entry(station_id).or_default();
            if !members.iter().any(|member| member == code) {
                members.push(code.to_owned());
            }
            data.change_time_by_tiploc
                .insert(code.to_owned(), day.settings.change_minutes);
        }
        // Rail-link walks, both ways, all day.
        for link in &day.rail_links {
            if !data.crs_to_tiplocs.contains_key(&link.stop_id)
                || !rail.crs_to_tiplocs.contains_key(&link.crs)
            {
                continue;
            }
            let tram_position = by_id
                .get(link.stop_id.as_str())
                .and_then(|stop| stop.lat.zip(stop.lon));
            let distance = tram_position
                .zip(link.station_position)
                .map(|(a, b)| metres_between(a, b));
            let walk = rail_link_walk_minutes(distance, &link.kind);
            let minutes = walk + buffer as i32;
            let fixed = |to: &str| FixedLink {
                operator: schedule_query::OperatorId::UNKNOWN,
                mode: RAIL_LINK_MODE.to_owned(),
                to_crs: to.to_owned(),
                minutes,
                valid_from: "0000".to_owned(),
                valid_to: "2359".to_owned(),
                days_mask: "1111111".to_owned(),
            };
            data.fixed_links_from_crs
                .entry(link.crs.clone())
                .or_default()
                .push(fixed(&link.stop_id));
            data.fixed_links_from_crs
                .entry(link.stop_id.clone())
                .or_default()
                .push(fixed(&link.crs));
            walks.insert(
                (link.stop_id.clone(), link.crs.clone()),
                RailLinkWalk { walk_minutes: walk },
            );
        }
        // Walks between stations of one TfL hub, both ways, all day: a
        // change of network station, so the walk alone (no rail-side
        // buffer; each station's own change time applies on boarding).
        for walk in &day.hub_walks {
            if !data.crs_to_tiplocs.contains_key(&walk.from)
                || !data.crs_to_tiplocs.contains_key(&walk.to)
            {
                continue;
            }
            for (from, to) in [(&walk.from, &walk.to), (&walk.to, &walk.from)] {
                data.fixed_links_from_crs
                    .entry(from.clone())
                    .or_default()
                    .push(FixedLink {
                        // A walk: no operator, passes every filter.
                        operator: schedule_query::OperatorId::UNKNOWN,
                        mode: RAIL_LINK_MODE.to_owned(),
                        to_crs: to.clone(),
                        minutes: walk.minutes,
                        valid_from: "0000".to_owned(),
                        valid_to: "2359".to_owned(),
                        days_mask: "1111111".to_owned(),
                    });
                hub_walks.insert(
                    (from.clone(), to.clone()),
                    RailLinkWalk {
                        walk_minutes: walk.minutes,
                    },
                );
            }
        }
        stop_modes.extend(day.stop_modes.clone());
        let trips: Vec<TripInfo> = day
            .trips
            .iter()
            .map(|trip| {
                let pattern = day.patterns.get(&trip.pattern_id);
                TripInfo {
                    line_id: trip.line_id.clone(),
                    direction: pattern.map_or(0, |p| p.direction),
                    headsign: trip
                        .headsign
                        .clone()
                        .or_else(|| pattern.and_then(|p| p.headsign.clone())),
                    replacement_bus: trip.replacement_bus,
                }
            })
            .collect();
        if road_water_minutes > 0 {
            for (index, trip) in day.trips.iter().enumerate() {
                if trip.replacement_bus {
                    data.modal_change
                        .road_or_water_uids
                        .insert(transit_uid(&day.network, index));
                }
            }
            data.modal_change.minutes = road_water_minutes;
        }
        networks.push(NetworkGraph {
            network: day.network.clone(),
            public_stop_id: day.settings.public_stop_id,
            trips,
            lines: day.lines.clone(),
            connections: connections_by_network
                .get(&day.network)
                .copied()
                .unwrap_or_default(),
        });
    }
    data.sort_fixed_links();
    TransitGraph {
        interchange: Arc::new(data),
        networks,
        stops,
        walks,
        hub_walks,
        stop_modes,
        skipped,
    }
}

/// How many of `connections` each network's trips added.
pub fn count_connections(connections: &[Connection]) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for connection in connections {
        if let Some((network, _)) = connection.uid.split_once(':') {
            *counts.entry(network.to_owned()).or_default() += 1;
        }
    }
    counts
}

/// Reads every switched-on network's day for `date`. A network that is
/// missing, out of its horizon, has no service that day or fails to read
/// is left out with its reason (spec §6.4: a rail plan never fails because
/// of tram data); each is counted.
pub async fn read_days(
    pool: &PgPool,
    networks: &[String],
    date: NaiveDate,
) -> (Vec<NetworkDay>, Vec<(String, SkipReason)>) {
    let mut days = Vec::new();
    let mut skipped = Vec::new();
    for network in networks {
        let reason = match read_network_day(pool, network, date).await {
            Ok(Ok(day)) => {
                days.push(day);
                continue;
            }
            Ok(Err(reason)) => reason,
            Err(err) => {
                tracing::warn!(error = ?err, network, %date, "could not read a transit timetable; planning without it");
                SkipReason::Error
            }
        };
        metrics::counter!(
            common::metrics::metric_name("api_trip_plan_transit_networks_skipped_total"),
            "network" => network.clone(),
            "reason" => reason.metric_label()
        )
        .increment(1);
        skipped.push((network.clone(), reason));
    }
    (days, skipped)
}

/// Sets the `api_trip_plan_transit_connections{network}` gauge.
#[expect(clippy::cast_precision_loss, reason = "a gauge of tens of thousands")]
pub fn record_connections(graph: &TransitGraph) {
    for network in &graph.networks {
        metrics::gauge!(
            common::metrics::metric_name("api_trip_plan_transit_connections"),
            "network" => network.network.clone()
        )
        .set(network.connections as f64);
    }
}

// --- Legs ------------------------------------------------------------------

/// A transit leg's `serviceMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransitServiceMode {
    Tram,
    ReplacementBus,
    /// The Underground (the `TfL` network's `tube` lines).
    Tube,
    /// The DLR.
    Dlr,
}

impl TransitServiceMode {
    /// A line's mode (`transit_lines.mode`) as a leg's: `tube` and `dlr`,
    /// else a tram.
    fn of_line(mode: Option<&str>) -> Self {
        match mode {
            Some("tube") => Self::Tube,
            Some("dlr") => Self::Dlr,
            _ => Self::Tram,
        }
    }
}

/// A transit leg's `line`: `null` for a replacement bus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransitLegLine {
    /// The public id, `metrolink:blue`.
    pub id: String,
    pub name: String,
    pub colour: Option<String>,
}

/// A transit leg's `originStop` or `destinationStop`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransitLegStop {
    /// The station, in the network's public spelling.
    pub id: String,
    pub name: String,
    /// The platform's indicator (`To Bury`), when known.
    pub platform: Option<String>,
}

/// A notice on a transit leg (spec §7.3): a current disruption on its line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitLegNotice {
    /// `disruption`.
    pub kind: &'static str,
    pub line_id: String,
    /// A `common::Severity` number.
    pub severity: u8,
    /// The severity's name (`Minor Delays`).
    pub description: String,
    /// "Disruption reported on the Blue Line".
    pub message: String,
    /// The situation's text.
    pub reason: String,
}

/// `kind: "transit"` (spec §11.1): a tram or replacement-bus ride on a
/// transit network's timetable. Scheduled only: `realtime` and
/// `liveTracking` are always `false`, and it never has `live`, `expected`,
/// `delay`, `cancelled`, `status`, `public*` or `working*` fields.
///
/// Carried on a `PlannedLeg::Train` (`transit`), so every consumer that
/// walks legs treats it as a ride (it counts as a change, Q5), and
/// serialized in its place by `PlannedLeg`'s `Serialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename = "transit", rename_all = "camelCase")]
pub struct TransitLeg {
    pub network: String,
    pub service_mode: TransitServiceMode,
    pub live_tracking: bool,
    pub realtime: bool,
    pub service_date: NaiveDate,
    pub scheduled_departure: NaiveTime,
    pub scheduled_arrival: NaiveTime,
    pub departure_day_offset: u8,
    pub arrival_day_offset: u8,
    pub line: Option<TransitLegLine>,
    pub headsign: Option<String>,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub origin_stop: TransitLegStop,
    pub destination_stop: TransitLegStop,
    /// The median gap between departures of the same line and direction
    /// from the boarding platform in the hour around this one; absent with
    /// fewer than three ([`headway_minutes`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headway_minutes: Option<u32>,
    pub notices: Vec<TransitLegNotice>,
}

/// On a rail-link walk (`kind: "transfer"`, `mode: "WALK"`): the tram stop
/// at one end and the estimated walk alone (Q2). The transfer's own
/// `minutes` also hold the tram-side change buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitWalk {
    pub network: String,
    pub stop: TransitWalkStop,
    /// Walking from the station to the tram stop.
    pub towards_stop: bool,
    pub walk_minutes: i32,
    /// Always `true`: worked out from coordinates.
    pub estimated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransitWalkStop {
    pub id: String,
    pub name: String,
}

/// The station's code in the network's public spelling.
pub fn public_id(code: &str, spelling: PublicStopId) -> String {
    match (spelling, code.strip_prefix("9400")) {
        (PublicStopId::StopArea, Some(rest)) => format!("940G{rest}"),
        _ => code.to_owned(),
    }
}

/// The transit leg for a ride on `uid` from platform `from` to platform
/// `to`, on `date`, at the searched minutes. `None` when `uid` is not a
/// trip of `graph`.
pub fn transit_leg(
    graph: &TransitGraph,
    uid: &str,
    from: &str,
    to: &str,
    date: NaiveDate,
    departure_min: u32,
    arrival_min: u32,
) -> Option<TransitLeg> {
    let (network, trip) = graph.trip(uid)?;
    let stop = |code: &str| {
        let info = graph.stops.get(schedule_query::normalize_tiploc(code));
        TransitLegStop {
            id: public_id(
                info.map_or(code, |info| info.station_id.as_str()),
                network.public_stop_id,
            ),
            name: info.map_or_else(|| code.to_owned(), |info| info.station_name.clone()),
            platform: info.and_then(|info| info.platform.clone()),
        }
    };
    let line = if trip.replacement_bus {
        None
    } else {
        trip.line_id.as_ref().map(|line_id| {
            let row = network.lines.get(line_id);
            TransitLegLine {
                id: format!("{}:{line_id}", network.network),
                name: row.map_or_else(|| line_id.clone(), |row| row.name.clone()),
                colour: row.and_then(|row| row.colour.clone()),
            }
        })
    };
    let (scheduled_departure, departure_day_offset) = clock(departure_min);
    let (scheduled_arrival, arrival_day_offset) = clock(arrival_min);
    Some(TransitLeg {
        network: network.network.clone(),
        service_mode: if trip.replacement_bus {
            TransitServiceMode::ReplacementBus
        } else {
            TransitServiceMode::of_line(
                trip.line_id
                    .as_ref()
                    .and_then(|line| network.lines.get(line))
                    .and_then(|row| row.mode.as_deref()),
            )
        },
        live_tracking: false,
        realtime: false,
        service_date: date,
        scheduled_departure,
        scheduled_arrival,
        departure_day_offset,
        arrival_day_offset,
        line,
        headsign: trip.headsign.clone(),
        origin_crs: None,
        destination_crs: None,
        origin_stop: stop(from),
        destination_stop: stop(to),
        headway_minutes: None,
        notices: Vec::new(),
    })
}

/// How often `uid`'s line runs (spec §7.2): the median gap between
/// departures of trips of the same line and direction (and, for a
/// replacement bus, other replacement buses) from platform `from` in the 60
/// minutes around `departure_min`, from the graph's own connections. `None`
/// with fewer than three such departures.
pub fn headway_minutes(
    graph: &TransitGraph,
    connections: &[Connection],
    uid: &str,
    from: &str,
    departure_min: u32,
) -> Option<u32> {
    let (network, trip) = graph.trip(uid)?;
    let from = schedule_query::normalize_tiploc(from);
    let start = departure_min.saturating_sub(30);
    let end = departure_min + 30;
    let first = connections.partition_point(|c| c.departure_min < start);
    let mut departures: Vec<u32> = connections[first..]
        .iter()
        .take_while(|c| c.departure_min <= end)
        .filter(|c| c.can_board && schedule_query::normalize_tiploc(&c.from_tiploc) == from)
        .filter(|c| {
            graph.trip(&c.uid).is_some_and(|(other_network, other)| {
                other_network.network == network.network
                    && other.replacement_bus == trip.replacement_bus
                    && other.line_id == trip.line_id
                    && other.direction == trip.direction
            })
        })
        .map(|c| c.departure_min)
        .collect();
    departures.sort_unstable();
    departures.dedup();
    if departures.len() < 3 {
        return None;
    }
    let mut gaps: Vec<u32> = departures.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_unstable();
    Some(gaps[(gaps.len() - 1) / 2])
}

/// The walk details of a rail-link transfer between `from_crs` and
/// `to_crs` (one a tram node), and which end is the tram stop.
pub fn transit_walk(graph: &TransitGraph, from_crs: &str, to_crs: &str) -> Option<TransitWalk> {
    let (node, crs, towards_stop) = if graph.is_node(to_crs) {
        (to_crs, from_crs, true)
    } else if graph.is_node(from_crs) {
        (from_crs, to_crs, false)
    } else {
        return None;
    };
    let walk = graph.walks.get(&(node.to_owned(), crs.to_owned()))?;
    let info = graph.stops.get(node)?;
    let spelling = graph
        .network(&info.network)
        .map_or(PublicStopId::AccessNode, |n| n.public_stop_id);
    Some(TransitWalk {
        network: info.network.clone(),
        stop: TransitWalkStop {
            id: public_id(node, spelling),
            name: info.station_name.clone(),
        },
        towards_stop,
        walk_minutes: walk.walk_minutes,
        estimated: true,
    })
}

/// The two ends of a hub walk between `from` and `to` (both stations of a
/// `tfl_stops` network, one `TfL` hub) as `TfL` stops: the public id
/// (`940GZZLUBNK`), the station's name and its `TfL` modes.
fn hub_walk_stops(
    graph: &TransitGraph,
    from: &str,
    to: &str,
) -> Option<(
    crate::data::tfl_stops::TflStopSummary,
    crate::data::tfl_stops::TflStopSummary,
)> {
    graph.hub_walks.get(&(from.to_owned(), to.to_owned()))?;
    let stop = |node: &str| {
        let info = graph.stops.get(node)?;
        let spelling = graph
            .network(&info.network)
            .map_or(PublicStopId::StopArea, |n| n.public_stop_id);
        Some(crate::data::tfl_stops::TflStopSummary {
            id: public_id(node, spelling),
            name: info.station_name.clone(),
            modes: graph.stop_modes.get(node).cloned().unwrap_or_default(),
            lines: Vec::new(),
        })
    };
    Some((stop(from)?, stop(to)?))
}

/// `transitNotices` (spec §7.3): each network left out of the plan, as
/// `{network, reason: "noTimetable", message}`.
pub fn notices_json<'a>(
    skipped: impl IntoIterator<Item = &'a (String, SkipReason)>,
) -> serde_json::Value {
    let mut seen: HashSet<&str> = HashSet::new();
    serde_json::Value::Array(
        skipped
            .into_iter()
            .filter(|(network, _)| seen.insert(network.as_str()))
            .map(|(network, _)| {
                serde_json::json!({
                    "network": network,
                    "reason": "noTimetable",
                    "message": "Tram times aren't available for this date.",
                })
            })
            .collect(),
    )
}

/// A disruption notice per current non-good status of `line_id` (a local
/// id) in `stored` (`ds_store::transit::disruptions::stored_line_statuses`),
/// shown only while the network's situations are fresh.
pub fn line_notices(
    network: &str,
    line_id: &str,
    line_name: &str,
    view: &ds_store::transit::disruptions::LineStatusView,
) -> Vec<TransitLegNotice> {
    // A network whose status is TfL's has no situations: every status that
    // is not good is a disruption (the TfL transit design §7).
    let from_tfl = common::transit::status_from_tfl(network);
    let good = [
        common::Severity::GoodService as u8,
        common::Severity::NoIssues as u8,
        common::Severity::NoReportedDisruption as u8,
    ];
    view.statuses
        .iter()
        .filter(|status| {
            if from_tfl {
                !good.contains(&status.severity)
            } else {
                status.situation_id.is_some()
            }
        })
        .map(|status| TransitLegNotice {
            kind: "disruption",
            line_id: format!("{network}:{line_id}"),
            severity: status.severity,
            description: status.description.clone(),
            message: format!("Disruption reported on the {line_name}"),
            reason: status.reason.clone(),
        })
        .collect()
}

/// Dresses a planned request's legs with the transit networks (spec §11):
/// a train leg on a transit trip gets its `kind: "transit"` leg (with the
/// headway), a walk to or from a tram stop its `transitWalk` (the tram
/// end's CRS becomes `null`), and every itinerary `scheduledOnly`. Only for
/// a request that opted in.
pub fn dress_segments(
    graph: &TransitGraph,
    connections: &[Connection],
    date: NaiveDate,
    segments: &mut [crate::data::trip_planning_itinerary::SegmentResult],
) {
    use crate::data::trip_planning_itinerary::PlannedLeg;
    for segment in segments.iter_mut() {
        for itinerary in &mut segment.itineraries {
            let mut scheduled_only = false;
            for leg in &mut itinerary.legs {
                match leg {
                    PlannedLeg::Train {
                        train_uid,
                        from_tiploc,
                        to_tiploc,
                        departure_min,
                        arrival_min,
                        origin_crs,
                        destination_crs,
                        transit,
                        ..
                    } if is_transit_uid(train_uid) => {
                        let Some(mut dressed) = transit_leg(
                            graph,
                            train_uid,
                            from_tiploc,
                            to_tiploc,
                            date,
                            *departure_min,
                            *arrival_min,
                        ) else {
                            continue;
                        };
                        dressed.headway_minutes = headway_minutes(
                            graph,
                            connections,
                            train_uid,
                            from_tiploc,
                            *departure_min,
                        );
                        *origin_crs = None;
                        *destination_crs = None;
                        *transit = Some(Box::new(dressed));
                        scheduled_only = true;
                    }
                    PlannedLeg::Transfer {
                        origin_crs,
                        destination_crs,
                        transit_walk: walk_slot,
                        origin_stop,
                        destination_stop,
                        ..
                    } => {
                        let (Some(from), Some(to)) =
                            (origin_crs.as_deref(), destination_crs.as_deref())
                        else {
                            continue;
                        };
                        // A walk between two stations of one TfL hub: both
                        // ends are TfL stops, named as an access leg's are.
                        if let Some((from_stop, to_stop)) = hub_walk_stops(graph, from, to) {
                            *origin_stop = Some(Box::new(from_stop));
                            *destination_stop = Some(Box::new(to_stop));
                            *origin_crs = None;
                            *destination_crs = None;
                            scheduled_only = true;
                            continue;
                        }
                        let Some(walk) = transit_walk(graph, from, to) else {
                            continue;
                        };
                        if walk.towards_stop {
                            *destination_crs = None;
                        } else {
                            *origin_crs = None;
                        }
                        *walk_slot = Some(Box::new(walk));
                    }
                    PlannedLeg::Train { .. } => {}
                }
            }
            itinerary.scheduled_only = Some(scheduled_only);
        }
    }
}

/// Adds each transit leg's line disruptions (spec §7.3): one read of the
/// stored line statuses and the situations marker per network ridden. A
/// failed read leaves the notices empty.
pub async fn attach_line_notices(
    pool: &PgPool,
    segments: &mut [crate::data::trip_planning_itinerary::SegmentResult],
) {
    use crate::data::trip_planning_itinerary::PlannedLeg;
    use ds_store::transit::disruptions;
    let networks: BTreeSet<String> = segments
        .iter()
        .flat_map(|segment| &segment.itineraries)
        .flat_map(|itinerary| &itinerary.legs)
        .filter_map(|leg| match leg {
            PlannedLeg::Train {
                transit: Some(transit),
                ..
            } if transit.line.is_some() => Some(transit.network.clone()),
            _ => None,
        })
        .collect();
    let now = Utc::now();
    for network in networks {
        let read = async {
            let as_of = disruptions::situations_as_of(pool, &network).await?;
            let stored = disruptions::stored_line_statuses(pool, &network).await?;
            anyhow::Ok((disruptions::state_at(as_of, now), as_of, stored))
        };
        let (state, as_of, stored) = match read.await {
            Ok(read) => read,
            Err(err) => {
                tracing::warn!(error = ?err, network, "could not read transit line statuses for trip plan notices");
                continue;
            }
        };
        let prefix = format!("{network}:");
        for leg in segments
            .iter_mut()
            .flat_map(|segment| &mut segment.itineraries)
            .flat_map(|itinerary| &mut itinerary.legs)
        {
            let PlannedLeg::Train {
                transit: Some(transit),
                ..
            } = leg
            else {
                continue;
            };
            if transit.network != network {
                continue;
            }
            let Some(line) = transit.line.clone() else {
                continue;
            };
            let local = line.id.strip_prefix(&prefix).unwrap_or(&line.id);
            let view = disruptions::line_view(state, as_of, stored.get(local));
            transit.notices = line_notices(&network, local, &line.name, &view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform(id: &str, parent: &str, indicator: &str, position: (f64, f64)) -> StopRow {
        StopRow {
            stop_id: id.to_owned(),
            kind: "platform".to_owned(),
            parent_stop_id: Some(parent.to_owned()),
            name: format!("{parent} platform"),
            indicator: Some(indicator.to_owned()),
            lat: Some(position.0),
            lon: Some(position.1),
        }
    }

    fn station(id: &str, name: &str, position: (f64, f64)) -> StopRow {
        StopRow {
            stop_id: id.to_owned(),
            kind: "station".to_owned(),
            parent_stop_id: None,
            name: name.to_owned(),
            indicator: None,
            lat: Some(position.0),
            lon: Some(position.1),
        }
    }

    fn call(stop: &str, arr: i32, dep: i32, pickup: i16, drop_off: i16) -> PatternStopRow {
        PatternStopRow {
            stop_id: stop.to_owned(),
            arr_offset_s: arr,
            dep_offset_s: dep,
            pickup,
            drop_off,
        }
    }

    fn trip(id: &str, start_s: i32, replacement_bus: bool) -> TripRow {
        TripRow {
            trip_id: id.to_owned(),
            line_id: (!replacement_bus).then(|| "blue".to_owned()),
            pattern_id: "P1".to_owned(),
            start_s,
            headsign: None,
            replacement_bus,
        }
    }

    /// Piccadilly (two platforms) to Victoria, linked to MAN and MCV.
    fn day(trips: Vec<TripRow>) -> NetworkDay {
        let pic = (53.4774, -2.2309);
        let vic = (53.4875, -2.2425);
        NetworkDay {
            network: "metrolink".to_owned(),
            settings: PLANNER_NETWORKS[0],
            stops: vec![
                station("9400ZZMAPIC", "Piccadilly", pic),
                platform("9400ZZMAPIC1", "9400ZZMAPIC", "To Bury", pic),
                platform("9400ZZMAPIC2", "9400ZZMAPIC", "To Altrincham", pic),
                station("9400ZZMAVIC", "Victoria", vic),
                platform("9400ZZMAVIC1", "9400ZZMAVIC", "To Bury", vic),
            ],
            patterns: HashMap::from([(
                "P1".to_owned(),
                PatternRow {
                    line_id: Some("blue".to_owned()),
                    direction: 0,
                    headsign: Some("Bury".to_owned()),
                    stops: vec![
                        call("9400ZZMAPIC1", 0, 0, 0, 1),
                        call("9400ZZMAVIC1", 450, 450, 1, 0),
                    ],
                },
            )]),
            trips,
            lines: HashMap::from([(
                "blue".to_owned(),
                LineRow {
                    name: "Blue Line".to_owned(),
                    colour: Some("#1f6fb5".to_owned()),
                    mode: None,
                },
            )]),
            rail_links: vec![
                RailLinkRow {
                    stop_id: "9400ZZMAPIC".to_owned(),
                    crs: "MAN".to_owned(),
                    kind: "interchange".to_owned(),
                    station_position: Some((53.4773, -2.2309)),
                },
                RailLinkRow {
                    stop_id: "9400ZZMAVIC".to_owned(),
                    crs: "MCV".to_owned(),
                    kind: "nearby".to_owned(),
                    station_position: Some((53.4873, -2.2425)),
                },
                RailLinkRow {
                    stop_id: "9400ZZMAVIC".to_owned(),
                    crs: "ZZZ".to_owned(),
                    kind: "nearby".to_owned(),
                    station_position: None,
                },
            ],
            hub_walks: Vec::new(),
            stop_modes: HashMap::new(),
        }
    }

    fn rail() -> InterchangeData {
        InterchangeData {
            tiploc_to_crs: HashMap::from([
                ("MNCRPIC".to_owned(), "MAN".to_owned()),
                ("MNCRVIC".to_owned(), "MCV".to_owned()),
            ]),
            crs_to_tiplocs: HashMap::from([
                ("MAN".to_owned(), vec!["MNCRPIC".to_owned()]),
                ("MCV".to_owned(), vec!["MNCRVIC".to_owned()]),
            ]),
            ..InterchangeData::default()
        }
    }

    #[test]
    fn the_switch_takes_known_planner_networks_only() {
        assert_eq!(parse_switch(None).unwrap(), Vec::<String>::new());
        assert_eq!(parse_switch(Some("")).unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_switch(Some(" metrolink, ,metrolink")).unwrap(),
            ["metrolink"]
        );
        let err = parse_switch(Some("metrolink,net")).unwrap_err();
        assert!(err.contains("'net'"), "{err}");
    }

    #[test]
    fn change_minutes_default_to_two_and_clamp_to_fifteen() {
        assert_eq!(parse_change_minutes(None), 2);
        assert_eq!(parse_change_minutes(Some(" ")), 2);
        assert_eq!(parse_change_minutes(Some("0")), 0);
        assert_eq!(parse_change_minutes(Some("40")), 15);
        assert_eq!(parse_change_minutes(Some("x")), 2);
    }

    #[test]
    fn rounding_is_to_the_nearest_minute_half_up() {
        assert_eq!(round_to_minute(0), 0);
        assert_eq!(round_to_minute(29), 0);
        assert_eq!(round_to_minute(30), 1);
        assert_eq!(round_to_minute(89), 1);
        assert_eq!(round_to_minute(90), 2);
        assert_eq!(round_to_minute(-5), 0);
    }

    /// No-dwell stops arrive and depart in the same minute, so the trip
    /// stays non-decreasing even where the offsets are not whole minutes.
    #[test]
    fn a_trip_expands_with_the_same_rounding_at_both_ends() {
        let pattern = PatternRow {
            line_id: Some("blue".to_owned()),
            direction: 0,
            headsign: None,
            stops: vec![
                call("A1", 0, 0, 0, 1),
                call("B1", 95, 95, 0, 0),
                call("C1", 150, 150, 2, 3),
                call("D1", 215, 215, 1, 0),
            ],
        };
        // 08:00:40 start: 08:00:40, 08:02:15, 08:03:10, 08:04:15.
        let points = trip_calling_points(&trip("T1", 8 * 3600 + 40, false), &pattern);
        let connections = schedule_query::build_connections([("metrolink:0", points.as_slice())]);
        let minutes: Vec<(u32, u32, bool, bool)> = connections
            .iter()
            .map(|c| (c.departure_min, c.arrival_min, c.can_board, c.can_alight))
            .collect();
        assert_eq!(
            minutes,
            [
                (481, 482, true, true),
                (482, 483, true, true),
                (483, 484, true, true),
            ]
        );
        // Pickup 1 at the last stop and drop-off 1 at the first.
        assert!(!points[0].can_alight && points[0].can_board);
        assert!(!points[3].can_board && points[3].can_alight);
        // 2 and 3 (phone, ask the driver) are allowed.
        assert!(points[2].can_board && points[2].can_alight);
    }

    #[test]
    fn a_trip_past_midnight_keeps_counting_minutes() {
        let pattern = PatternRow {
            line_id: None,
            direction: 0,
            headsign: None,
            stops: vec![call("A1", 0, 0, 0, 0), call("B1", 600, 600, 0, 0)],
        };
        let points = trip_calling_points(&trip("T1", 86_100, false), &pattern);
        let connections = schedule_query::build_connections([("metrolink:0", points.as_slice())]);
        assert_eq!(connections[0].departure_min, 1435);
        assert_eq!(connections[0].arrival_min, 1445);
    }

    #[test]
    fn uids_are_unique_and_never_cif_trust_or_darwin_ids() {
        let day = day(vec![
            trip("T2", 8 * 3600, false),
            trip("T1", 8 * 3600, false),
            trip("T3", 9 * 3600, true),
        ]);
        let by_uid = calling_points(std::slice::from_ref(&day));
        let mut uids: Vec<&String> = by_uid.keys().collect();
        uids.sort();
        assert_eq!(uids, ["metrolink:0", "metrolink:1", "metrolink:2"]);
        for uid in uids {
            assert!(is_transit_uid(uid));
            // CIF: a letter and five digits; TRUST: 10 characters with no
            // colon; Darwin RID: digits.
            let cif = uid.len() == 6
                && uid.as_bytes()[0].is_ascii_alphabetic()
                && uid[1..].bytes().all(|b| b.is_ascii_digit());
            assert!(!cif);
            assert!(!uid.bytes().all(|b| b.is_ascii_alphanumeric()));
        }
        assert!(!is_transit_uid("C01355"));
        assert!(!is_transit_uid("202611047654321"));
    }

    #[test]
    fn rail_link_walks_are_estimated_and_clamped() {
        assert_eq!(rail_link_walk_minutes(Some(0.0), "interchange"), 2);
        assert_eq!(rail_link_walk_minutes(Some(0.0), "nearby"), 4);
        // 300 m * 1.25 / 75 = 5 minutes.
        assert_eq!(rail_link_walk_minutes(Some(300.0), "interchange"), 5);
        assert_eq!(rail_link_walk_minutes(Some(301.0), "interchange"), 6);
        assert_eq!(rail_link_walk_minutes(Some(5_000.0), "nearby"), 15);
        assert_eq!(rail_link_walk_minutes(None, "interchange"), 5);
        assert_eq!(rail_link_walk_minutes(None, "nearby"), 10);
    }

    #[test]
    fn stations_become_nodes_with_platform_members_and_rail_walks() {
        let day = day(vec![
            trip("T1", 8 * 3600, false),
            trip("T2", 9 * 3600, true),
        ]);
        let rail = rail();
        let graph = build_transit_graph(
            &rail,
            std::slice::from_ref(&day),
            &HashMap::from([("metrolink".to_owned(), 2)]),
            Vec::new(),
            2,
            5,
        );
        let data = &graph.interchange;
        // Only the called platform is a member; the change time is the
        // network's.
        assert_eq!(data.crs_to_tiplocs["9400ZZMAPIC"], ["9400ZZMAPIC1"]);
        assert_eq!(data.tiploc_to_crs["9400ZZMAVIC1"], "9400ZZMAVIC");
        assert_eq!(data.change_time_by_tiploc["9400ZZMAPIC1"], 3);
        // MAN <-> Piccadilly: about 11 m apart, so the 2-minute floor, plus
        // the 2-minute buffer, both ways.
        let link = |from: &str, to: &str| {
            schedule_query::fixed_links_between(data, from, to)
                .next()
                .map(|l| (l.mode.clone(), l.minutes))
        };
        assert_eq!(link("MAN", "9400ZZMAPIC"), Some(("WALK".to_owned(), 4)));
        assert_eq!(link("9400ZZMAPIC", "MAN"), Some(("WALK".to_owned(), 4)));
        // Nearby: the 4-minute floor plus 2.
        assert_eq!(link("MCV", "9400ZZMAVIC"), Some(("WALK".to_owned(), 6)));
        // A rail station the planner does not know is skipped.
        assert_eq!(link("ZZZ", "9400ZZMAVIC"), None);
        // The replacement bus owes the bus buffer.
        assert_eq!(data.modal_change.extra_for("metrolink:1"), 5);
        assert_eq!(data.modal_change.extra_for("metrolink:0"), 0);
        // The rail data is untouched.
        assert!(!rail.crs_to_tiplocs.contains_key("9400ZZMAPIC"));
        assert!(rail.fixed_links_from_crs.is_empty());
        assert_eq!(
            graph.walks[&("9400ZZMAPIC".to_owned(), "MAN".to_owned())].walk_minutes,
            2
        );
    }

    #[test]
    fn ends_resolve_from_either_spelling_or_a_platform() {
        let day = day(vec![trip("T1", 8 * 3600, false)]);
        let graph = build_transit_graph(
            &rail(),
            std::slice::from_ref(&day),
            &HashMap::new(),
            Vec::new(),
            2,
            5,
        );
        for code in [
            "9400ZZMAPIC",
            "940GZZMAPIC",
            "9400zzmapic1",
            " 9400ZZMAPIC2 ",
        ] {
            assert_eq!(
                graph.resolve_end(code).map(|s| s.station_id.as_str()),
                Some("9400ZZMAPIC"),
                "{code}"
            );
        }
        assert!(graph.resolve_end("9400ZZXXXX").is_none());
        assert!(graph.is_node("9400ZZMAPIC"));
        assert!(!graph.is_node("9400ZZMAPIC1"));
        assert!(looks_like_naptan("9400ZZMAPIC"));
        assert!(looks_like_naptan("940GZZLUUXB"));
        assert!(!looks_like_naptan("MAN"));
        assert!(!looks_like_naptan("tiploc:SANWBUS"));
    }

    fn connection(uid: &str, from: &str, departure_min: u32) -> Connection {
        Connection {
            operator: schedule_query::OperatorId::UNKNOWN,
            uid: uid.to_owned().into(),
            from_tiploc: from.to_owned().into(),
            to_tiploc: "X".to_owned().into(),
            departure_min,
            arrival_min: departure_min + 5,
            working_departure_min: departure_min,
            working_arrival_min: departure_min + 5,
            can_board: true,
            can_alight: true,
        }
    }

    #[test]
    fn headway_is_the_median_gap_with_a_three_departure_floor() {
        let trips: Vec<TripRow> = (0..6).map(|n| trip(&format!("T{n}"), 0, false)).collect();
        let day = day(trips);
        let graph = build_transit_graph(
            &rail(),
            std::slice::from_ref(&day),
            &HashMap::new(),
            Vec::new(),
            2,
            5,
        );
        let mut connections = vec![
            connection("metrolink:0", "9400ZZMAPIC1", 470),
            connection("metrolink:1", "9400ZZMAPIC1", 476),
            connection("metrolink:2", "9400ZZMAPIC1", 482),
            connection("metrolink:3", "9400ZZMAPIC1", 490),
            // Another platform, and a train, do not count.
            connection("metrolink:4", "9400ZZMAPIC2", 484),
            connection("C01355", "9400ZZMAPIC1", 485),
            // Outside the hour.
            connection("metrolink:5", "9400ZZMAPIC1", 520),
        ];
        connections.sort_by_key(|c| c.departure_min);
        // Gaps 6, 6, 8: median 6.
        assert_eq!(
            headway_minutes(&graph, &connections, "metrolink:1", "9400ZZMAPIC1", 476),
            Some(6)
        );
        // Two departures only: no hint.
        assert_eq!(
            headway_minutes(
                &graph,
                &connections[..2],
                "metrolink:1",
                "9400ZZMAPIC1",
                476
            ),
            None
        );
    }

    #[test]
    fn a_leg_is_dressed_from_the_trip_and_never_live() {
        let day = day(vec![
            trip("T1", 8 * 3600, false),
            trip("T2", 9 * 3600, true),
        ]);
        let graph = build_transit_graph(
            &rail(),
            std::slice::from_ref(&day),
            &HashMap::new(),
            Vec::new(),
            2,
            5,
        );
        let date = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        let leg = transit_leg(
            &graph,
            "metrolink:0",
            "9400ZZMAPIC1",
            "9400ZZMAVIC1",
            date,
            480,
            488,
        )
        .unwrap();
        let json = serde_json::to_value(&leg).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "transit",
                "network": "metrolink",
                "serviceMode": "tram",
                "liveTracking": false,
                "realtime": false,
                "serviceDate": "2026-11-04",
                "scheduledDeparture": "08:00:00",
                "scheduledArrival": "08:08:00",
                "departureDayOffset": 0,
                "arrivalDayOffset": 0,
                "line": {"id": "metrolink:blue", "name": "Blue Line", "colour": "#1f6fb5"},
                "headsign": "Bury",
                "originCrs": null,
                "destinationCrs": null,
                "originStop": {"id": "9400ZZMAPIC", "name": "Piccadilly", "platform": "To Bury"},
                "destinationStop": {"id": "9400ZZMAVIC", "name": "Victoria", "platform": "To Bury"},
                "notices": [],
            })
        );
        let bus = transit_leg(
            &graph,
            "metrolink:1",
            "9400ZZMAPIC1",
            "9400ZZMAVIC1",
            date,
            540,
            1450,
        )
        .unwrap();
        assert_eq!(bus.service_mode, TransitServiceMode::ReplacementBus);
        assert!(bus.line.is_none());
        assert_eq!(bus.arrival_day_offset, 1);
        assert!(transit_leg(&graph, "C01355", "A", "B", date, 1, 2).is_none());
    }

    #[test]
    fn a_stop_area_network_echoes_940g_ids() {
        assert_eq!(
            public_id("9400ZZCRWMB", PublicStopId::StopArea),
            "940GZZCRWMB"
        );
        assert_eq!(
            public_id("9400ZZMAPIC", PublicStopId::AccessNode),
            "9400ZZMAPIC"
        );
        assert_eq!(access_node_spelling("940GZZCRWMB"), "9400ZZCRWMB");
    }

    #[test]
    fn a_rail_link_walk_names_its_tram_stop_and_walk_alone() {
        let day = day(vec![trip("T1", 8 * 3600, false)]);
        let graph = build_transit_graph(
            &rail(),
            std::slice::from_ref(&day),
            &HashMap::new(),
            Vec::new(),
            2,
            5,
        );
        let walk = transit_walk(&graph, "MAN", "9400ZZMAPIC").unwrap();
        assert!(walk.towards_stop);
        assert_eq!(walk.walk_minutes, 2);
        assert_eq!(walk.stop.name, "Piccadilly");
        let back = transit_walk(&graph, "9400ZZMAVIC", "MCV").unwrap();
        assert!(!back.towards_stop);
        assert_eq!(back.walk_minutes, 4);
        assert!(transit_walk(&graph, "MAN", "MCV").is_none());
    }

    #[test]
    fn notices_name_each_left_out_network_once() {
        let notices = notices_json(&[
            ("metrolink".to_owned(), SkipReason::OutsideHorizon),
            ("metrolink".to_owned(), SkipReason::NoServiceDay),
        ]);
        assert_eq!(
            notices,
            serde_json::json!([{
                "network": "metrolink",
                "reason": "noTimetable",
                "message": "Tram times aren't available for this date.",
            }])
        );
    }

    #[test]
    fn the_cache_marker_moves_on_a_transit_activation() {
        let at = |hour: u32| {
            Some(
                NaiveDate::from_ymd_opt(2026, 11, 4)
                    .unwrap()
                    .and_hms_opt(hour, 0, 0)
                    .unwrap()
                    .and_utc(),
            )
        };
        assert_eq!(cache_marker(at(3), None), at(3));
        assert_eq!(cache_marker(at(3), at(5)), at(5));
        assert_eq!(cache_marker(at(6), at(5)), at(6));
        assert_eq!(cache_marker(None, at(5)), at(5));
    }
}
