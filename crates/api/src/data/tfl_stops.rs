//! Reads of `TfL`'s Tube, DLR and tram stops (`tfl_stops`, `tfl_stop_crs`;
//! docs/superpowers/specs/2026-10-20-tfl-stop-lookup-design.md §3 and §5):
//! the stop search behind the untracked-leg form, the stop-id check of a
//! saved leg, and the mapping from a National Rail station (CRS) to its
//! Tube stop, which the planner's Tube transfers and
//! `GET /tfl/stops/by-crs` use, and the line a planner transfer rides
//! ([`attach_transfer_lines`]).
//!
//! The mapping is meant to be reused (a later card may derive Tube links
//! between National Rail stations from it): [`stops_for_crs`] takes any set
//! of CRS codes and modes and returns each code's stops, best first.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use anyhow::Result;
use common::supplementary_links::SupplementaryLink;
use common::tfl_stops::{TflStopLine, search_key};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::reference::escape_ilike_pattern;
use crate::data::trip_planning_itinerary::{PlannedLeg, SegmentResult};

/// One stop as the api serves it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TflStopSummary {
    /// The `NaPTAN` id (`940GZZLUEUS`): what a leg stores.
    pub id: String,
    /// `Euston`.
    pub name: String,
    /// `tube`, `dlr`, `tram`.
    pub modes: Vec<String>,
    pub lines: Vec<TflStopLine>,
}

#[derive(sqlx::FromRow)]
struct StopRow {
    stop_id: String,
    name: String,
    modes: Vec<String>,
    lines: sqlx::types::Json<Vec<TflStopLine>>,
}

impl From<StopRow> for TflStopSummary {
    fn from(row: StopRow) -> Self {
        Self {
            id: row.stop_id,
            name: row.name,
            modes: row.modes,
            lines: row.lines.0,
        }
    }
}

/// The modes of `raw` (`tube,dlr`), each one of
/// `common::tfl_stops::MODES`; every mode when `raw` is absent or blank.
/// An unknown mode is an `Err` naming it.
pub fn parse_modes(raw: Option<&str>) -> Result<Vec<String>, String> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(common::tfl_stops::MODES
            .iter()
            .map(|mode| (*mode).to_owned())
            .collect());
    };
    let mut modes = Vec::new();
    for mode in raw.split(',').map(|m| m.trim().to_ascii_lowercase()) {
        if mode.is_empty() {
            continue;
        }
        if !common::tfl_stops::MODES.contains(&mode.as_str()) {
            return Err(format!(
                "modes must be a comma-separated list of: {}.",
                common::tfl_stops::MODES.join(", ")
            ));
        }
        if !modes.contains(&mode) {
            modes.push(mode);
        }
    }
    if modes.is_empty() {
        return Err(format!(
            "modes must be a comma-separated list of: {}.",
            common::tfl_stops::MODES.join(", ")
        ));
    }
    Ok(modes)
}

/// The stops of `modes` whose name matches `q`, at most `limit`, ranked:
/// the whole name, then a name starting with `q`, then a word of the name
/// starting with `q` ("cross" finds "King's Cross St. Pancras"), then any
/// other substring; alphabetical within each tier. Matching is on
/// [`search_key`] forms, so punctuation and case do not matter. A `q` with
/// no letters or digits matches nothing.
pub async fn search(
    pool: &PgPool,
    q: &str,
    modes: &[String],
    limit: i64,
) -> Result<Vec<TflStopSummary>> {
    let key = search_key(q);
    if key.is_empty() {
        return Ok(Vec::new());
    }
    let escaped = escape_ilike_pattern(&key);
    let rows: Vec<StopRow> = sqlx::query_as(
        r"
        SELECT stop_id, name, modes, lines FROM tfl_stops
        WHERE modes && $1::text[]
          AND search_name LIKE '%' || $2 || '%' ESCAPE '\'
        ORDER BY
          CASE
            WHEN search_name = $3 THEN 0
            WHEN search_name LIKE $2 || '%' ESCAPE '\' THEN 1
            WHEN search_name LIKE '% ' || $2 || '%' ESCAPE '\' THEN 2
            ELSE 3
          END,
          name, stop_id
        LIMIT $4
        ",
    )
    .bind(modes)
    .bind(&escaped)
    .bind(&key)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(TflStopSummary::from).collect())
}

/// The stops among `ids` that exist, by id.
pub async fn stops_by_id(pool: &PgPool, ids: &[String]) -> Result<HashMap<String, TflStopSummary>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<StopRow> = sqlx::query_as(
        "SELECT stop_id, name, modes, lines FROM tfl_stops WHERE stop_id = ANY($1::text[])",
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.stop_id.clone(), TflStopSummary::from(row)))
        .collect())
}

/// National Rail stations whose Tube stop the crosswalk and the 300 m
/// fallback could get wrong or miss, pinned by hand: the London termini,
/// where a second Tube station is close by (Euston Square, Paddington's
/// Hammersmith & City platforms). An entry is used only while its stop is
/// in `tfl_stops` with a wanted mode, so a stale one is skipped, never
/// served.
pub const CRS_OVERRIDES: [(&str, &str); 10] = [
    ("CHX", "940GZZLUCHX"),
    ("EUS", "940GZZLUEUS"),
    ("KGX", "940GZZLUKSX"),
    ("LBG", "940GZZLULNB"),
    ("LST", "940GZZLULVT"),
    ("MYB", "940GZZLUMYB"),
    ("PAD", "940GZZLUPAC"),
    ("STP", "940GZZLUKSX"),
    ("VIC", "940GZZLUVIC"),
    ("WAT", "940GZZLUWLO"),
];

/// How far a Tube stop may be from a National Rail station's own
/// coordinates for [`stops_for_crs`]'s fallback.
pub const NEAREST_MAX_METRES: f64 = 300.0;

#[derive(sqlx::FromRow)]
struct CrsStopRow {
    crs: String,
    stop_id: String,
    name: String,
    modes: Vec<String>,
    lines: sqlx::types::Json<Vec<TflStopLine>>,
}

impl CrsStopRow {
    fn split(self) -> (String, TflStopSummary) {
        (
            self.crs,
            TflStopSummary {
                id: self.stop_id,
                name: self.name,
                modes: self.modes,
                lines: self.lines.0,
            },
        )
    }
}

/// Each of `crs_codes`' `TfL` stops with one of `modes`, best first: the
/// override ([`CRS_OVERRIDES`]), then the hub crosswalk (`tfl_stop_crs`;
/// a stop serving `modes[0]` before others, a hub link before a `TIPLOC`
/// one), and only for a code with neither, the nearest stop within
/// [`NEAREST_MAX_METRES`] of the station's `stations` coordinates. A code
/// with no stop is absent.
pub async fn stops_for_crs(
    pool: &PgPool,
    crs_codes: &[String],
    modes: &[String],
) -> Result<BTreeMap<String, Vec<TflStopSummary>>> {
    let mut codes: Vec<String> = crs_codes
        .iter()
        .map(|code| code.trim().to_ascii_uppercase())
        .filter(|code| code.len() == 3)
        .collect();
    codes.sort_unstable();
    codes.dedup();
    let mut out: BTreeMap<String, Vec<TflStopSummary>> = BTreeMap::new();
    if codes.is_empty() || modes.is_empty() {
        return Ok(out);
    }
    let first_mode = modes[0].clone();
    let push = |out: &mut BTreeMap<String, Vec<TflStopSummary>>,
                (crs, stop): (String, TflStopSummary)| {
        let stops = out.entry(crs).or_default();
        if !stops.iter().any(|known| known.id == stop.id) {
            stops.push(stop);
        }
    };

    let (override_crs, override_ids): (Vec<&str>, Vec<&str>) = CRS_OVERRIDES
        .iter()
        .filter(|(crs, _)| codes.iter().any(|code| code == crs))
        .copied()
        .unzip();
    if !override_crs.is_empty() {
        let rows: Vec<CrsStopRow> = sqlx::query_as(
            r"
            SELECT o.crs, t.stop_id, t.name, t.modes, t.lines
            FROM UNNEST($1::text[], $2::text[]) AS o(crs, stop_id)
            JOIN tfl_stops t ON t.stop_id = o.stop_id
            WHERE t.modes && $3::text[]
            ",
        )
        .bind(&override_crs)
        .bind(&override_ids)
        .bind(modes)
        .fetch_all(pool)
        .await?;
        for row in rows {
            push(&mut out, row.split());
        }
    }

    let rows: Vec<CrsStopRow> = sqlx::query_as(
        r"
        SELECT c.crs::text AS crs, t.stop_id, t.name, t.modes, t.lines
        FROM tfl_stop_crs c
        JOIN tfl_stops t ON t.stop_id = c.stop_id
        WHERE c.crs::text = ANY($1::text[]) AND t.modes && $2::text[]
        ORDER BY c.crs, NOT ($3 = ANY(t.modes)), c.source <> 'hub', t.name, t.stop_id
        ",
    )
    .bind(&codes)
    .bind(modes)
    .bind(&first_mode)
    .fetch_all(pool)
    .await?;
    for row in rows {
        push(&mut out, row.split());
    }

    let missing: Vec<String> = codes
        .iter()
        .filter(|code| !out.contains_key(*code))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let rows: Vec<CrsStopRow> = sqlx::query_as(
            r"
            SELECT DISTINCT ON (s.crs) s.crs::text AS crs, t.stop_id, t.name, t.modes, t.lines
            FROM stations s
            JOIN tfl_stops t
              ON t.modes && $2::text[] AND t.latitude IS NOT NULL AND t.longitude IS NOT NULL
            CROSS JOIN LATERAL (
              SELECT 6371000.0 * 2 * asin(sqrt(
                power(sin(radians(t.latitude - s.latitude) / 2), 2)
                + cos(radians(s.latitude)) * cos(radians(t.latitude))
                  * power(sin(radians(t.longitude - s.longitude) / 2), 2)
              )) AS metres
            ) d
            WHERE s.crs::text = ANY($1::text[])
              AND s.latitude IS NOT NULL AND s.longitude IS NOT NULL
              AND d.metres <= $4
            ORDER BY s.crs, NOT ($3 = ANY(t.modes)), d.metres, t.stop_id
            ",
        )
        .bind(&missing)
        .bind(modes)
        .bind(&first_mode)
        .bind(NEAREST_MAX_METRES)
        .fetch_all(pool)
        .await?;
        for row in rows {
            push(&mut out, row.split());
        }
    }
    Ok(out)
}

/// Each of `stop_ids`' National Rail stations (CRS codes), best first: the
/// reverse of [`CRS_OVERRIDES`] (a stop pinned for a station is that
/// station's), then the hub crosswalk (`tfl_stop_crs`, a hub link before a
/// `TIPLOC` one), and only for a stop with neither, the nearest station
/// within [`NEAREST_MAX_METRES`]. The reverse of [`stops_for_crs`], for a
/// journey leg that ends at a Tube stop and is followed by a train
/// (`Euston` `940GZZLUEUS` -> `EUS`). A stop that is not in `tfl_stops`,
/// or has no station, is absent.
pub async fn rail_crs_for_stops(
    pool: &PgPool,
    stop_ids: &[String],
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut ids: Vec<String> = stop_ids.iter().map(|id| id.trim().to_owned()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let push = |out: &mut BTreeMap<String, Vec<String>>, (stop_id, crs): (String, String)| {
        let codes = out.entry(stop_id).or_default();
        if !codes.contains(&crs) {
            codes.push(crs);
        }
    };

    let (override_ids, override_crs): (Vec<&str>, Vec<&str>) = CRS_OVERRIDES
        .iter()
        .filter(|(_, stop)| ids.iter().any(|id| id == stop))
        .map(|(crs, stop)| (*stop, *crs))
        .unzip();
    if !override_ids.is_empty() {
        let rows: Vec<(String, String)> = sqlx::query_as(
            r"
            SELECT o.stop_id, o.crs
            FROM UNNEST($1::text[], $2::text[]) WITH ORDINALITY AS o(stop_id, crs, n)
            JOIN tfl_stops t ON t.stop_id = o.stop_id
            ORDER BY o.n
            ",
        )
        .bind(&override_ids)
        .bind(&override_crs)
        .fetch_all(pool)
        .await?;
        for row in rows {
            push(&mut out, row);
        }
    }

    let rows: Vec<(String, String)> = sqlx::query_as(
        r"
        SELECT c.stop_id, c.crs::text
        FROM tfl_stop_crs c
        JOIN tfl_stops t ON t.stop_id = c.stop_id
        WHERE c.stop_id = ANY($1::text[])
        ORDER BY c.stop_id, c.source <> 'hub', c.crs
        ",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    for row in rows {
        push(&mut out, row);
    }

    let missing: Vec<String> = ids
        .iter()
        .filter(|id| !out.contains_key(*id))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let rows: Vec<(String, String)> = sqlx::query_as(
            r"
            SELECT DISTINCT ON (t.stop_id) t.stop_id, s.crs::text
            FROM tfl_stops t
            JOIN stations s ON s.latitude IS NOT NULL AND s.longitude IS NOT NULL
            CROSS JOIN LATERAL (
              SELECT 6371000.0 * 2 * asin(sqrt(
                power(sin(radians(t.latitude - s.latitude) / 2), 2)
                + cos(radians(s.latitude)) * cos(radians(t.latitude))
                  * power(sin(radians(t.longitude - s.longitude) / 2), 2)
              )) AS metres
            ) d
            WHERE t.stop_id = ANY($1::text[])
              AND t.latitude IS NOT NULL AND t.longitude IS NOT NULL
              AND d.metres <= $2
            ORDER BY t.stop_id, d.metres, s.crs
            ",
        )
        .bind(&missing)
        .bind(NEAREST_MAX_METRES)
        .fetch_all(pool)
        .await?;
        for row in rows {
            push(&mut out, row);
        }
    }
    Ok(out)
}

/// The stop modes a planner transfer's `mode` (the CIF fixed link's) maps
/// to, the preferred first: `TUBE` -> Tube; `METRO` -> DLR, tram or Tube.
/// `None` for a walk, bus, ferry or anything else.
pub fn transfer_modes(mode: &str) -> Option<Vec<String>> {
    let modes: &[&str] = match mode.trim().to_ascii_uppercase().as_str() {
        "TUBE" => &["tube"],
        "METRO" => &["dlr", "tram", "tube"],
        _ => return None,
    };
    Some(modes.iter().map(|m| (*m).to_owned()).collect())
}

/// Fills `originStop`/`destinationStop` on every `TUBE` and `METRO`
/// transfer leg of `segments` (spec §5): each end's CRS through
/// [`stops_for_crs`], the best stop. Best effort: on a database error the
/// legs keep no stops and the plan is served as before.
pub async fn attach_transfer_stops(pool: &PgPool, segments: &mut [SegmentResult]) {
    let mut wanted: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for leg in segments
        .iter()
        .flat_map(|segment| &segment.itineraries)
        .flat_map(|itinerary| &itinerary.legs)
    {
        if let PlannedLeg::Transfer {
            mode,
            origin_crs,
            destination_crs,
            ..
        } = leg
            && let Some(modes) = transfer_modes(mode)
        {
            let codes = wanted.entry(modes).or_default();
            codes.extend(origin_crs.iter().cloned());
            codes.extend(destination_crs.iter().cloned());
        }
    }
    let mut found: BTreeMap<(Vec<String>, String), TflStopSummary> = BTreeMap::new();
    for (modes, codes) in wanted {
        match stops_for_crs(pool, &codes, &modes).await {
            Ok(by_crs) => {
                for (crs, stops) in by_crs {
                    if let Some(best) = stops.into_iter().next() {
                        found.insert((modes.clone(), crs), best);
                    }
                }
            }
            Err(err) => {
                tracing::warn!(error = ?err, "TfL stops for planner transfers failed; serving them without stops");
                return;
            }
        }
    }
    if found.is_empty() {
        return;
    }
    for leg in segments
        .iter_mut()
        .flat_map(|segment| &mut segment.itineraries)
        .flat_map(|itinerary| &mut itinerary.legs)
    {
        if let PlannedLeg::Transfer {
            mode,
            origin_crs,
            destination_crs,
            origin_stop,
            destination_stop,
            ..
        } = leg
            && let Some(modes) = transfer_modes(mode)
        {
            let lookup = |crs: &Option<String>| {
                crs.as_ref()
                    .and_then(|crs| found.get(&(modes.clone(), crs.to_ascii_uppercase())))
                    .cloned()
                    .map(Box::new)
            };
            *origin_stop = lookup(origin_crs);
            *destination_stop = lookup(destination_crs);
        }
    }
}

/// The curated links' lines (`reference-data/supplementary-fixed-links.csv`)
/// by (mode, the two CRS codes in order): a link runs both ways.
type CuratedLines = HashMap<(String, String, String), Vec<String>>;

fn curated_lines(links: &[SupplementaryLink]) -> CuratedLines {
    let mut out = CuratedLines::new();
    for link in links {
        let names = out
            .entry(curated_key(&link.mode, &link.from_crs, &link.to_crs))
            .or_default();
        if !names.contains(&link.line) {
            names.push(link.line.clone());
        }
    }
    out
}

fn curated_key(mode: &str, a: &str, b: &str) -> (String, String, String) {
    let (a, b) = (a.trim().to_ascii_uppercase(), b.trim().to_ascii_uppercase());
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    (mode.trim().to_ascii_uppercase(), a, b)
}

static CURATED_LINES: LazyLock<CuratedLines> =
    LazyLock::new(|| curated_lines(&common::supplementary_links::supplementary_links()));

/// The line a transfer rides, where it is known: the curated link's line
/// for this mode and pair (when the CSV gives exactly one), else the one
/// line the `TfL` stops at its two ends share. `None` when they share none
/// or several (Euston and King's Cross St. Pancras: Northern and Victoria),
/// and for a transfer with neither.
fn transfer_line(
    curated: &CuratedLines,
    mode: &str,
    ends: (Option<&str>, Option<&str>),
    stops: (Option<&TflStopSummary>, Option<&TflStopSummary>),
) -> Option<String> {
    if let (Some(origin), Some(destination)) = ends
        && let Some([line]) = curated
            .get(&curated_key(mode, origin, destination))
            .map(Vec::as_slice)
    {
        return Some(line.clone());
    }
    let (origin, destination) = (stops.0?, stops.1?);
    let mut shared = origin
        .lines
        .iter()
        .filter(|line| destination.lines.iter().any(|other| other.id == line.id));
    match (shared.next(), shared.next()) {
        (Some(line), None) => Some(line.name.clone()),
        _ => None,
    }
}

/// Fills `line` on every transfer leg of `segments` where
/// [`transfer_line`] knows it. Run after [`attach_transfer_stops`], whose
/// stops it reads.
pub fn attach_transfer_lines(segments: &mut [SegmentResult]) {
    for leg in segments
        .iter_mut()
        .flat_map(|segment| &mut segment.itineraries)
        .flat_map(|itinerary| &mut itinerary.legs)
    {
        if let PlannedLeg::Transfer {
            mode,
            origin_crs,
            destination_crs,
            line,
            origin_stop,
            destination_stop,
            ..
        } = leg
        {
            *line = transfer_line(
                &CURATED_LINES,
                mode,
                (origin_crs.as_deref(), destination_crs.as_deref()),
                (origin_stop.as_deref(), destination_stop.as_deref()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(id: &str, lines: &[&str]) -> TflStopSummary {
        TflStopSummary {
            id: id.to_owned(),
            name: id.to_owned(),
            modes: vec!["tube".to_owned()],
            lines: lines
                .iter()
                .map(|name| TflStopLine {
                    id: name.to_ascii_lowercase(),
                    name: (*name).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_checked_in_csv_names_vauxhall_to_victoria_both_ways() {
        for (from, to) in [("VXH", "VIC"), ("vic", "vxh")] {
            assert_eq!(
                transfer_line(&CURATED_LINES, "TUBE", (Some(from), Some(to)), (None, None)),
                Some("Victoria".to_owned())
            );
        }
        // Another mode for the same pair is not that link.
        assert_eq!(
            transfer_line(
                &CURATED_LINES,
                "WALK",
                (Some("VXH"), Some("VIC")),
                (None, None)
            ),
            None
        );
    }

    #[test]
    fn a_transfer_line_is_the_curated_one_else_the_one_line_both_stops_share() {
        let parse = |rows: &str| {
            curated_lines(&common::supplementary_links::parse_supplementary_links(
                &format!(
                    "from_crs,to_crs,mode,line,minutes,valid_from,valid_to,days_mask,source\n{rows}"
                ),
            ))
        };
        let curated = parse(
            "AAA,BBB,TUBE,Curated,5,0000,2359,1111111,test\n\
             CCC,DDD,TUBE,One,5,0000,2359,1111111,test\n\
             CCC,DDD,TUBE,Two,5,0000,2359,1111111,test",
        );
        let euston = stop("EUS", &["Northern", "Victoria"]);
        let vauxhall = stop("VXH", &["Victoria"]);
        let kings_cross = stop("KGX", &["Circle", "Northern", "Victoria"]);
        let bank = stop("BNK", &["Central"]);
        let line = |ends: (&str, &str),
                    stops: (Option<&TflStopSummary>, Option<&TflStopSummary>)| {
            transfer_line(&curated, "TUBE", (Some(ends.0), Some(ends.1)), stops)
        };
        // The curated row wins over the stops.
        assert_eq!(
            line(("BBB", "AAA"), (Some(&euston), Some(&kings_cross))),
            Some("Curated".to_owned())
        );
        // Two curated lines for the pair: the stops decide.
        assert_eq!(
            line(("CCC", "DDD"), (Some(&euston), Some(&vauxhall))),
            Some("Victoria".to_owned())
        );
        assert_eq!(line(("CCC", "DDD"), (None, None)), None);
        // Several shared lines, none, or a stop unknown: no line.
        assert_eq!(
            line(("EUS", "KGX"), (Some(&euston), Some(&kings_cross))),
            None
        );
        assert_eq!(line(("EUS", "BNK"), (Some(&euston), Some(&bank))), None);
        assert_eq!(line(("EUS", "VXH"), (Some(&euston), None)), None);
    }

    #[test]
    fn modes_default_to_all_and_refuse_unknown_ones() {
        assert_eq!(parse_modes(None).unwrap(), ["tube", "dlr", "tram"]);
        assert_eq!(parse_modes(Some("  ")).unwrap(), ["tube", "dlr", "tram"]);
        assert_eq!(parse_modes(Some("DLR, tube,dlr")).unwrap(), ["dlr", "tube"]);
        assert!(parse_modes(Some("bus")).is_err());
        assert!(parse_modes(Some(",")).is_err());
    }

    #[test]
    fn only_tube_and_metro_transfers_map_to_stops() {
        assert_eq!(transfer_modes("TUBE").unwrap(), ["tube"]);
        assert_eq!(transfer_modes("metro").unwrap(), ["dlr", "tram", "tube"]);
        assert_eq!(transfer_modes("WALK"), None);
        assert_eq!(transfer_modes("BUS"), None);
    }

    #[test]
    fn overrides_are_crs_codes_and_naptan_ids() {
        for (crs, stop) in CRS_OVERRIDES {
            assert!(
                crs.len() == 3 && crs.bytes().all(|b| b.is_ascii_uppercase()),
                "{crs}"
            );
            assert!(common::tfl_stops::is_stop_id(stop), "{stop}");
        }
    }
}

#[cfg(test)]
mod db_tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    const STOPS: &str = "'940GZZTESTA', '940GZZTESTB', '940GZZTESTC', '940GZZTESTD', '940GZZTESTE'";

    async fn cleanup(pool: &PgPool) {
        for sql in [
            format!("DELETE FROM tfl_stops WHERE stop_id IN ({STOPS})"),
            "DELETE FROM stations WHERE crs IN ('Z8A', 'Z8B', 'Z8C')".to_owned(),
        ] {
            sqlx::query(&sql).execute(pool).await.unwrap();
        }
    }

    /// Five invented stops: a hub-linked Tube stop for Z8A, a DLR stop at
    /// the same hub, a Tube stop 150 m from station Z8B (no crosswalk), one
    /// 600 m from station Z8C (too far), and a tram stop.
    async fn seed(pool: &PgPool) {
        cleanup(pool).await;
        let stop = |id: &str, name: &str, modes: &str, lat: f64, lon: f64| {
            format!(
                "INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, latitude, longitude, fetched_at) \
                 VALUES ('{id}', '{}', '{}', '{modes}', '[{{\"id\": \"testline\", \"name\": \"Testline\"}}]', {lat}, {lon}, NOW())",
                name.replace('\'', "''"),
                search_key(name),
            )
        };
        for sql in [
            stop("940GZZTESTA", "King's Testbury St. Zed", "{tube}", 51.9, 0.9),
            stop("940GZZTESTB", "King's Testbury", "{dlr}", 51.9, 0.9),
            stop("940GZZTESTC", "Testbury Low Level", "{tube}", 51.8, 0.8),
            stop("940GZZTESTD", "Far Testbury", "{tube}", 51.7, 0.7),
            stop("940GZZTESTE", "Elephant & Testbury", "{tram}", 51.6, 0.6),
            "INSERT INTO tfl_stop_crs (stop_id, crs, source) VALUES ('940GZZTESTA', 'Z8A', 'hub'), ('940GZZTESTB', 'Z8A', 'tiploc')".to_owned(),
            // About 150 m north of TESTC.
            "INSERT INTO stations (crs, name, latitude, longitude) VALUES ('Z8B', 'Testbury', 51.80135, 0.8), ('Z8C', 'Far Away', 51.70540, 0.7), ('Z8A', 'Testbury Hub', 51.9, 0.9)".to_owned(),
        ] {
            sqlx::query(&sql).execute(pool).await.unwrap_or_else(|err| panic!("{sql}: {err}"));
        }
    }

    fn ids(stops: &[TflStopSummary]) -> Vec<&str> {
        stops.iter().map(|stop| stop.id.as_str()).collect()
    }

    #[tokio::test]
    #[ignore = "requires a live database (DATABASE_URL)"]
    async fn search_ranks_and_filters_by_mode() {
        let pool = connect().await;
        seed(&pool).await;
        let all = parse_modes(None).unwrap();

        // Name prefix ("king's testbury" before "king's testbury st zed"
        // alphabetically), across modes, punctuation ignored.
        let found = search(&pool, "kings testbury", &all, 20).await.unwrap();
        assert_eq!(ids(&found), ["940GZZTESTB", "940GZZTESTA"]);
        // A word prefix ranks above a plain substring.
        let found = search(&pool, "testbury", &all, 20).await.unwrap();
        assert_eq!(found[0].id, "940GZZTESTC", "{found:?}");
        // "&" is "and"; tram only.
        let found = search(&pool, "elephant and", &["tram".to_owned()], 20)
            .await
            .unwrap();
        assert_eq!(ids(&found), ["940GZZTESTE"]);
        assert_eq!(found[0].lines[0].name, "Testline");
        // Mode filter.
        let found = search(&pool, "kings testbury", &["dlr".to_owned()], 20)
            .await
            .unwrap();
        assert_eq!(ids(&found), ["940GZZTESTB"]);
        // LIKE wildcards are literal.
        assert!(search(&pool, "%", &all, 20).await.unwrap().is_empty());
        assert!(
            search(&pool, "zzz_testbury", &all, 20)
                .await
                .unwrap()
                .is_empty()
        );

        let by_id = stops_by_id(&pool, &["940GZZTESTA".to_owned(), "940GZZNOPE".to_owned()])
            .await
            .unwrap();
        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id["940GZZTESTA"].name, "King's Testbury St. Zed");
        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database (DATABASE_URL)"]
    async fn crs_maps_through_the_crosswalk_then_the_nearest_stop() {
        let pool = connect().await;
        seed(&pool).await;
        let codes: Vec<String> = ["Z8A", "z8b", "Z8C", "ZZZ"].map(str::to_owned).to_vec();

        let tube = stops_for_crs(&pool, &codes, &["tube".to_owned()])
            .await
            .unwrap();
        assert_eq!(ids(&tube["Z8A"]), ["940GZZTESTA"]);
        // No crosswalk: the Tube stop 150 m away.
        assert_eq!(ids(&tube["Z8B"]), ["940GZZTESTC"]);
        // 600 m is too far; an unknown code has nothing.
        assert!(!tube.contains_key("Z8C"));
        assert!(!tube.contains_key("ZZZ"));

        // Metro: DLR first, both crosswalk stops listed.
        let metro = stops_for_crs(&pool, &codes, &transfer_modes("METRO").unwrap())
            .await
            .unwrap();
        assert_eq!(ids(&metro["Z8A"]), ["940GZZTESTB", "940GZZTESTA"]);
        cleanup(&pool).await;
    }

    /// The reverse lookup: a stop's override station, then its crosswalk
    /// stations (hub first), then the nearest station within 300 m.
    #[tokio::test]
    #[ignore = "requires a live database (DATABASE_URL)"]
    async fn stops_map_back_to_their_rail_stations() {
        let pool = connect().await;
        seed(&pool).await;
        // Euston's override stop, inserted only when this database has no
        // real one (a fresh test database never does), and removed after.
        let euston_inserted = sqlx::query(
            "INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, fetched_at) \
             VALUES ('940GZZLUEUS', 'Euston', 'euston', '{tube}', '[]', NOW()) \
             ON CONFLICT (stop_id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .unwrap()
        .rows_affected()
            == 1;
        // A crosswalk link on the override stop must not outrank the
        // override.
        sqlx::query(
            "INSERT INTO tfl_stop_crs (stop_id, crs, source) VALUES ('940GZZLUEUS', 'Z8C', 'tiploc') \
             ON CONFLICT DO NOTHING",
        )
        .execute(&pool)
        .await
        .unwrap();

        let ids: Vec<String> = [
            "940GZZLUEUS",
            "940GZZTESTB",
            "940GZZTESTA",
            "940GZZTESTC",
            "940GZZTESTD",
            "940GZZNOPE",
        ]
        .map(str::to_owned)
        .to_vec();
        let found = rail_crs_for_stops(&pool, &ids).await.unwrap();
        assert_eq!(found["940GZZLUEUS"][0], "EUS", "{found:?}");
        assert!(
            found["940GZZLUEUS"].contains(&"Z8C".to_owned()),
            "{found:?}"
        );
        // Hub and TIPLOC links alike.
        assert_eq!(found["940GZZTESTA"], ["Z8A"]);
        assert_eq!(found["940GZZTESTB"], ["Z8A"]);
        // No crosswalk: the station 150 m away.
        assert_eq!(found["940GZZTESTC"], ["Z8B"]);
        // 600 m is too far; an unknown stop has nothing.
        assert!(!found.contains_key("940GZZTESTD"), "{found:?}");
        assert!(!found.contains_key("940GZZNOPE"), "{found:?}");
        assert!(rail_crs_for_stops(&pool, &[]).await.unwrap().is_empty());

        sqlx::query("DELETE FROM tfl_stop_crs WHERE stop_id = '940GZZLUEUS' AND crs = 'Z8C'")
            .execute(&pool)
            .await
            .unwrap();
        if euston_inserted {
            sqlx::query("DELETE FROM tfl_stops WHERE stop_id = '940GZZLUEUS'")
                .execute(&pool)
                .await
                .unwrap();
        }
        cleanup(&pool).await;
    }

    fn transfer(mode: &str, from: &str, to: &str) -> PlannedLeg {
        PlannedLeg::Transfer {
            mode: mode.to_owned(),
            origin_crs: Some(from.to_owned()),
            destination_crs: Some(to.to_owned()),
            minutes: 12,
            line: None,
            origin_stop: None,
            destination_stop: None,
        }
    }

    /// Spec §5: a `TUBE` transfer between two National Rail stations names
    /// the Tube stop at each end; a walk names none, and its JSON is as
    /// before (no `originStop` key).
    #[tokio::test]
    #[ignore = "requires a live database (DATABASE_URL)"]
    async fn tube_transfers_get_their_stops() {
        let pool = connect().await;
        seed(&pool).await;
        let mut segments = vec![SegmentResult {
            origin_crs: "Z8A".to_owned(),
            destination_crs: "Z8B".to_owned(),
            itineraries: vec![crate::data::trip_planning_itinerary::PlannedItinerary {
                legs: vec![
                    transfer("TUBE", "Z8A", "Z8B"),
                    transfer("WALK", "Z8A", "Z8B"),
                    transfer("TUBE", "Z8A", "Z8C"),
                ],
                change_count: 0,
                total_duration_minutes: 36,
                exceeds_recommended_changes: None,
                departure_min: 0,
                arrival_min: 36,
                arrival_tiploc: None,
                departure_tiploc: None,
                live_feasible: None,
                continues_previous_train: false,
                via_satisfied_by: Vec::new(),
            }],
            capped_by_max_changes: false,
            depart_after_min: None,
            arrive_by_min: None,
            no_result_reason: None,
        }];
        attach_transfer_stops(&pool, &mut segments).await;
        let legs = serde_json::to_value(&segments[0].itineraries[0].legs).unwrap();
        assert_eq!(legs[0]["originStop"]["id"], "940GZZTESTA");
        assert_eq!(legs[0]["originStop"]["name"], "King's Testbury St. Zed");
        assert_eq!(legs[0]["destinationStop"]["id"], "940GZZTESTC");
        assert!(legs[1].get("originStop").is_none(), "{legs}");
        assert!(legs[1].get("destinationStop").is_none(), "{legs}");
        assert_eq!(legs[2]["originStop"]["id"], "940GZZTESTA");
        assert!(
            legs[2].get("destinationStop").is_none(),
            "Z8C has none: {legs}"
        );
        // Both stops serve only Testline: the leg names it. A walk, and a
        // leg with one stop unknown, name none.
        attach_transfer_lines(&mut segments);
        let legs = serde_json::to_value(&segments[0].itineraries[0].legs).unwrap();
        assert_eq!(legs[0]["line"], "Testline");
        assert!(legs[1].get("line").is_none(), "{legs}");
        assert!(legs[2].get("line").is_none(), "{legs}");
        cleanup(&pool).await;
    }
}
