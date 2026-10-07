//! The `tiploc_locations` product: a name, a [`LocationType`] and, for a
//! bus stop or ferry terminal, a parent station for every CIF `TI` record.
//!
//! Pure, apart from [`ModeTally::from_mca`], which streams the delivery's
//! `MCA` file. See docs/superpowers/specs/2026-10-06-tiploc-locations-design.md
//! for the rules; each function below documents its own.

use std::collections::{BTreeMap, HashMap, HashSet};

use common::location_naming::{self, LocationType};
use common::{ParentSource, TiplocLocationRecord};

use crate::parser::TiRecord;

/// Shortest MSN `A` line [`parse_msn_records`] decodes: the northing field
/// ends at byte 63.
const MIN_MSN_RECORD_LEN: usize = 63;

/// How far (metres, between MSN grid references) a bus stop or ferry
/// terminal may be from a station and still be linked to it as its parent
/// by proximity alone. MSN references are to the nearest 100 m, so this
/// admits up to four grid squares' offset.
pub(crate) const NEAREST_PARENT_MAX_METRES: f64 = 400.0;

/// One MSN `A` (station) record, as far as `tiploc_locations` needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MsnRecord {
    pub tiploc: String,
    pub name: String,
    /// The CATE interchange-status digit: 0-3, or 9 for a subsidiary
    /// TIPLOC of the station named by `code`.
    pub interchange: Option<u8>,
    /// The 3-letter code at `49..52`: a CRS for a station, an MSN-only code
    /// (`SAO`, `KWK`) for most bus stops.
    pub code: Option<String>,
    /// OSGB36 metres. `None` when the field is blank or one of MSN's
    /// placeholders (`19500E69999`).
    pub easting: Option<i32>,
    pub northing: Option<i32>,
}

/// Every MSN `A` record in `text` (already filtered to `A` lines), keyed by
/// TIPLOC. Same skip-don't-fail posture as `parser::parse_msn_a_lines`:
/// a short, non-ASCII or header line is ignored.
///
/// Byte layout (the 30-character-name variant this app's deliveries use,
/// see `parser::parse_msn_change_time_by_tiploc`): `5..35` name, `35` CATE,
/// `36..43` TIPLOC, `49..52` code, `52..57` easting (`1EEEE`, 100 m units
/// with a leading 1), `57` `E` when estimated, `58..63` northing (`6NNNN`).
/// When the same TIPLOC appears twice, the first record wins.
pub(crate) fn parse_msn_records(text: &str) -> HashMap<String, MsnRecord> {
    let mut records = HashMap::new();
    for line in text.lines() {
        if line.len() < MIN_MSN_RECORD_LEN || !line.is_ascii() || !line.starts_with('A') {
            continue;
        }
        let tiploc = line[36..43].trim();
        if tiploc.is_empty() || !tiploc.chars().all(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        let name = line[5..35].trim().to_string();
        let interchange = line[35..36].parse::<u8>().ok();
        let code = Some(line[49..52].trim())
            .filter(|code| code.len() == 3 && code.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(str::to_string);
        let (easting, northing) = grid_reference(&line[52..57], &line[58..63]);
        records
            .entry(tiploc.to_string())
            .or_insert_with(|| MsnRecord {
                tiploc: tiploc.to_string(),
                name,
                interchange,
                code,
                easting,
                northing,
            });
    }
    records
}

/// MSN's `1EEEE`/`6NNNN` grid fields as OSGB36 metres, or `None` for a
/// blank, non-numeric or placeholder reference (`19500`/`69999`, used for
/// pseudo-locations such as Blackpool's tram interchanges). Great Britain
/// fits in eastings `10000..=17000` and northings `60000..=69998`.
fn grid_reference(easting: &str, northing: &str) -> (Option<i32>, Option<i32>) {
    let (Ok(easting), Ok(northing)) = (
        easting.trim().parse::<i32>(),
        northing.trim().parse::<i32>(),
    ) else {
        return (None, None);
    };
    if !(10_000..=17_000).contains(&easting) || !(60_000..=69_998).contains(&northing) {
        return (None, None);
    }
    (
        Some((easting - 10_000) * 100),
        Some((northing - 60_000) * 100),
    )
}

/// How often each kind of service calls at (or, for rail, passes) one
/// TIPLOC, over every schedule in a delivery, all STP variants included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    pub rail_calls: i32,
    pub rail_passes: i32,
    pub bus_calls: i32,
    pub ship_calls: i32,
}

/// Per-TIPLOC [`Tally`] of a whole delivery.
#[derive(Debug, Default)]
pub(crate) struct ModeTally {
    by_tiploc: HashMap<String, Tally>,
}

impl ModeTally {
    /// Feeds one schedule: CIF train status `B`/`5` is a bus, `S`/`4` a
    /// ship (`schedule_query::records::is_bus_or_ship`), anything else rail.
    /// A rail call with no booked arrival or departure is a pass.
    pub(crate) fn add(&mut self, schedule: &schedule_query::records::RawSchedule) {
        let status = schedule.basic.train_status;
        for point in &schedule.calling_points {
            let tiploc = schedule_query::normalize_tiploc(point.tiploc.as_str());
            let tally = self.by_tiploc.entry(tiploc.to_string()).or_default();
            match status {
                Some('B' | '5') => tally.bus_calls += 1,
                Some('S' | '4') => tally.ship_calls += 1,
                _ if point.is_pass() => tally.rail_passes += 1,
                _ => tally.rail_calls += 1,
            }
        }
    }

    /// Tallies every schedule in CIF text, one line at a time.
    #[cfg(test)]
    pub(crate) fn from_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        let mut tally = Self::default();
        let mut parser = schedule_query::ScheduleRecordParser::default();
        for line in lines {
            parser.push_line(line, |schedule| tally.add(&schedule));
        }
        parser.finish(|schedule| tally.add(&schedule));
        tally
    }

    /// [`Self::from_lines`] over the `MCA` file at `path`, streamed: only
    /// one schedule is held at a time, never the file.
    pub(crate) fn from_mca(path: &std::path::Path) -> anyhow::Result<Self> {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
        let mut tally = Self::default();
        let mut parser = schedule_query::ScheduleRecordParser::default();
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let trimmed = line.strip_suffix('\n').unwrap_or(&line);
            let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
            parser.push_line(trimmed, |schedule| tally.add(&schedule));
        }
        parser.finish(|schedule| tally.add(&schedule));
        Ok(tally)
    }

    pub(crate) fn get(&self, tiploc: &str) -> Tally {
        self.by_tiploc.get(tiploc).copied().unwrap_or_default()
    }
}

/// TIPLOC -> parent CRS from a curated CSV
/// ([`common::tiploc_parents::parse_curated_parents`]: comments, the header
/// and malformed rows skipped). The CSV's `walk_minutes` is the planner's
/// (`api`), not this crate's.
pub(crate) fn curated_parents(csv: &str) -> BTreeMap<String, String> {
    common::tiploc_parents::parse_curated_parents(csv)
        .into_iter()
        .map(|(tiploc, parent)| (tiploc, parent.parent_crs))
        .collect()
}

/// The checked-in curated parents
/// (`reference-data/tiploc-parent-stations.csv`, compiled into `common`).
pub(crate) fn checked_in_curated_parents() -> BTreeMap<String, String> {
    curated_parents(common::tiploc_parents::CURATED_PARENTS_CSV)
}

/// An X-prefixed CRS is Network Rail's pseudo-code for a non-passenger
/// location (`XVR` Victoria carriage road), never a station
/// (`api::data::queries::is_bookable_crs` applies the same rule).
fn is_bookable_crs(crs: &str) -> bool {
    !crs.starts_with('X')
}

/// A TIPLOC's own CRS: its `TI` CRS, else its MSN code (the same
/// completion `parser::resolve_tiploc_crs` does).
fn own_code<'a>(ti: &'a TiRecord, msn: Option<&'a MsnRecord>) -> Option<&'a str> {
    ti.crs
        .as_deref()
        .or_else(|| msn.and_then(|m| m.code.as_deref()))
}

/// A station: a STANOX and a bookable CRS of its own -- exactly the
/// TIPLOCs `parser::resolve_tiploc_crs` publishes to `tiploc_crs`, less the
/// X-prefixed pseudo-codes.
fn is_station(ti: &TiRecord, msn: Option<&MsnRecord>) -> bool {
    ti.stanox.is_some() && own_code(ti, msn).is_some_and(is_bookable_crs)
}

/// The [`LocationType`] of one non-station-or-station TIPLOC, first rule
/// that applies:
///
/// 1. a station ([`is_station`]) is [`LocationType::Station`], whatever
///    calls there -- a station whose trains are replaced by buses for a
///    while is still a station;
/// 2. no rail service calls or passes, but bus or ship services call: a
///    ferry terminal when ships call at least as often as buses (or the
///    name says ferry), else a bus stop;
/// 3. no rail service calls or passes: whatever the `TI` or MSN name says
///    ([`location_naming::classify_name`], bus and ferry words included);
/// 4. rail services call or pass: the name's junction, siding or
///    passing-point words ([`location_naming::classify_rail_name`]);
/// 5. rail services only ever pass it: a passing point;
/// 6. otherwise [`LocationType::Other`].
pub(crate) fn classify(ti: &TiRecord, msn: Option<&MsnRecord>, tally: Tally) -> LocationType {
    if is_station(ti, msn) {
        return LocationType::Station;
    }
    let names = [Some(ti.station_name.as_str()), msn.map(|m| m.name.as_str())];
    let by_name = |classify: fn(&str) -> Option<LocationType>| {
        names.iter().flatten().find_map(|name| classify(name))
    };
    let rail = tally.rail_calls + tally.rail_passes;
    if rail == 0 && (tally.bus_calls > 0 || tally.ship_calls > 0) {
        let ferry_named =
            by_name(location_naming::classify_name) == Some(LocationType::FerryTerminal);
        return if tally.ship_calls >= tally.bus_calls || ferry_named {
            LocationType::FerryTerminal
        } else {
            LocationType::BusStop
        };
    }
    if rail == 0
        && let Some(kind) = by_name(location_naming::classify_name)
    {
        return kind;
    }
    if let Some(kind) = by_name(location_naming::classify_rail_name) {
        return kind;
    }
    if tally.rail_passes > 0 && tally.rail_calls == 0 {
        return LocationType::PassingPoint;
    }
    LocationType::Other
}

/// A station's MSN grid reference, by CRS: the principal (CATE 0-3) MSN
/// record whose code is that CRS, the first by TIPLOC when there are
/// several.
fn station_grid_references(
    msn: &HashMap<String, MsnRecord>,
    station_crs: &HashSet<String>,
) -> BTreeMap<String, (i32, i32)> {
    let mut sorted: Vec<&MsnRecord> = msn.values().collect();
    sorted.sort_by(|a, b| a.tiploc.cmp(&b.tiploc));
    let mut out = BTreeMap::new();
    for record in sorted {
        let (Some(code), Some(easting), Some(northing)) =
            (&record.code, record.easting, record.northing)
        else {
            continue;
        };
        if record.interchange.is_some_and(|cate| cate <= 3) && station_crs.contains(code) {
            out.entry(code.clone()).or_insert((easting, northing));
        }
    }
    out
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "distances within Great Britain fit an i32 many times over"
)]
fn distance_metres(a: (i32, i32), b: (i32, i32)) -> i32 {
    f64::from(a.0 - b.0).hypot(f64::from(a.1 - b.1)).round() as i32
}

/// A bus stop's or ferry terminal's parent station, first rule that finds
/// one:
///
/// 1. same TIPLOC -- its own `TI`/MSN code is a station's CRS (MSN files a
///    station's subsidiary bus stops under the station's CRS, e.g.
///    `BANSBUS` under `BAD`);
/// 2. nearest -- the station whose MSN grid reference is closest, if within
///    [`NEAREST_PARENT_MAX_METRES`] (ties go to the alphabetically first
///    CRS);
/// 3. curated -- `reference-data/tiploc-parent-stations.csv`.
///
/// Returns the CRS, the rule and the grid distance when both references
/// are known.
fn parent_for(
    tiploc: &str,
    code: Option<&str>,
    grid: Option<(i32, i32)>,
    station_grid: &BTreeMap<String, (i32, i32)>,
    station_crs: &HashSet<String>,
    curated: &BTreeMap<String, String>,
) -> Option<(String, ParentSource, Option<i32>)> {
    let distance_to = |crs: &str| {
        grid.zip(station_grid.get(crs).copied())
            .map(|(a, b)| distance_metres(a, b))
    };
    if let Some(code) = code
        && station_crs.contains(code)
    {
        return Some((
            code.to_string(),
            ParentSource::SameTiploc,
            distance_to(code),
        ));
    }
    if let Some(grid) = grid {
        let nearest = station_grid
            .iter()
            .map(|(crs, &reference)| (distance_metres(grid, reference), crs))
            .min();
        if let Some((distance, crs)) = nearest
            && f64::from(distance) <= NEAREST_PARENT_MAX_METRES
        {
            return Some((crs.clone(), ParentSource::Nearest, Some(distance)));
        }
    }
    curated
        .get(tiploc)
        .map(|crs| (crs.clone(), ParentSource::Curated, distance_to(crs)))
}

/// Builds one [`TiplocLocationRecord`] per `TI` record (the last one wins
/// for a TIPLOC listed twice), sorted by TIPLOC.
///
/// The display name comes from the `TI` description, else the MSN name
/// ([`location_naming::location_name`]). Only bus stops and ferry terminals
/// get a parent (see [`parent_for`]); a station is its own CRS.
pub(crate) fn build_location_records(
    ti_records: &[TiRecord],
    msn: &HashMap<String, MsnRecord>,
    tally: &ModeTally,
    curated: &BTreeMap<String, String>,
    source_sequence: i32,
) -> Vec<TiplocLocationRecord> {
    let mut by_tiploc: BTreeMap<&str, &TiRecord> = BTreeMap::new();
    for record in ti_records {
        if !record.tiploc.is_empty() {
            by_tiploc.insert(record.tiploc.as_str(), record);
        }
    }
    let station_crs: HashSet<String> = by_tiploc
        .values()
        .filter(|ti| is_station(ti, msn.get(&ti.tiploc)))
        .filter_map(|ti| own_code(ti, msn.get(&ti.tiploc)).map(str::to_string))
        .collect();
    let station_grid = station_grid_references(msn, &station_crs);

    by_tiploc
        .into_values()
        .map(|ti| {
            let msn_record = msn.get(&ti.tiploc);
            let counts = tally.get(&ti.tiploc);
            let location_type = classify(ti, msn_record, counts);
            let raw_name = if ti.station_name.trim().is_empty() {
                msn_record.map_or(ti.tiploc.as_str(), |m| m.name.as_str())
            } else {
                ti.station_name.as_str()
            };
            let names = location_naming::location_name(raw_name, location_type);
            let grid = msn_record.and_then(|m| m.easting.zip(m.northing));
            let parent = if location_type.is_road_or_water() {
                parent_for(
                    &ti.tiploc,
                    own_code(ti, msn_record),
                    grid,
                    &station_grid,
                    &station_crs,
                    curated,
                )
            } else {
                None
            };
            let (parent_crs, parent_source, parent_distance_m) = match parent {
                Some((crs, source, distance)) => (Some(crs), Some(source), distance),
                None => (None, None, None),
            };
            TiplocLocationRecord {
                tiploc: ti.tiploc.clone(),
                location_type,
                name: names.name,
                display_name: names.display_name,
                ti_name: Some(ti.station_name.clone()).filter(|name| !name.is_empty()),
                ti_crs: ti.crs.clone(),
                stanox: ti.stanox.clone(),
                msn_name: msn_record.map(|m| m.name.clone()),
                msn_code: msn_record.and_then(|m| m.code.clone()),
                msn_easting: grid.map(|(easting, _)| easting),
                msn_northing: grid.map(|(_, northing)| northing),
                msn_interchange: msn_record.and_then(|m| m.interchange).map(i32::from),
                parent_crs,
                parent_source,
                parent_distance_m,
                rail_calls: counts.rail_calls,
                rail_passes: counts.rail_passes,
                bus_calls: counts.bus_calls,
                ship_calls: counts.ship_calls,
                source_sequence,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_ti_lines;

    // Real `TI` and MSN `A` lines, byte-verbatim (carriage returns
    // stripped) from the 2026-10-05 delivery's RJTTF980MCA.txt and
    // RJTTF980MSN.txt.
    const TI_LINES: &str = "\
TIBANSBUS08534850TBANSTEAD (FIRTREE ROAD)   00000   0
TIBANSTED00534800FBANSTEAD                  87677   0BADBANSTEAD
TIBDICK  00904600XBRODICK                   00000   0BDC
TIEDINAIR00631700PEDINBURGH AIRPORT         00000   0EDAEdinburgh Airpt
TIEDINBUR00932800TEDINBURGH                 043032851EDBEDINBURGH
TIHTRBUS228317901HHEATHROW TERMINAL 2 BUS   00000   0
TIHTRBUS328318001PHEATHROW TERMINAL 3 BUS   00000   0
TIHTRWAPT08709000QHEATHROW TERMINALS 2 & 3  73778   0HXXHEX T1
TIKESWICK00205500RKESWICK (BUS STATION)     00000   0KWKKESWICK BUS
TILEUCHRS00918800RLEUCHARS                  03203   0LEULEUCHARS
TIMARY10 00147508BMARYLEBONE 10 SIGNAL      63027   0
TIRDNGBUS00639200AREADING BUS               00000   0RBUREADING BUS
TIRDNGSTN00314900WREADING                   742372957RDGREADING
TISANWBUS00399500ZST ANDREWS BUS STATION    00000   0SAOST ANDREWS BUS
";

    const MSN_LINES: &str = "\
A    BANSTEAD                      9BANSBUSBAD   BAD15245 61605 4
A    BANSTEAD                      0BANSTEDBAD   BAD15245 61605 4
A    BPOOL NTHBUSTRAM              0CATZQBNQBN   QBN19500E69999 5
A    EDINBURGH                     3EDINBUREDB   EDB13259 6673910
A    EDINBURGH AIRPORT BUS/TRAM    0EDINAIREDA   EDA13151 6673599
A    HEATHROW TERMINAL 2 BUS       9HTRBUS2HWA   HWA15077E6175899
A    HEATHROW TERMINAL 3 BUS       9HTRBUS3HWE   HWE15078E6175610
A    HEATHROW TERMINALS 2 & 3      1HTRWAPTHXX   HXX15077E61760 2
A    KESWICK BUS                   0KESWICKKWK   KWK13263 65235 5
A    LEUCHARS                      1LEUCHRSLEU   LEU13449 67207 5
A    READING                       2RDNGSTNRDG   RDG14714 61738 7
A    READING BUS                   1RDNGBUSRBU   RBU14715 61737 5
A    ST ANDREWS BUS                0SANWBUSSAO   SAO13504 67168 5
";

    // Real schedule records from RJTTF980MCA.txt: a St Andrews bus
    // (`B`), a Largs-Cumbrae ferry (`S`) and the tail of a Chiltern train
    // that passes MARY10. The rail BS line is the real one that follows the
    // ferry's block in the file, paired here with the real LI/LT lines of a
    // different Chiltern service so the fixture stays short. The second bus
    // block is SYNTHETIC: the next real St Andrews BS record with its body
    // swapped for calls at Banstead's bus stop and station, so the fixture
    // has a bus at a TIPLOC MSN files under a station's CRS.
    const SCHEDULES: &str = "\
BSNG182452605172612060000001 BBS0B00    123541003                              P
BX         SRYSR536000
LOSANWBUS 0700 0700   BUS    TB
LTLEUCHRS 0711 0711      TF
BSNG182482605172612060000001 BBS0B00    123541003                              P
LOBANSBUS 0700 0700   BUS    TB
LTBANSTED 0711 0711      TF
BSNC048562605172610180000001 S  0S00    113571015                              P
BX         QCYQC000100
LOLARGS   2005 2005   SHP    TB
LICUMBRAE 2015 2025      20152025         T
LTLARGS  22035 2035      TF
BSNC296792605172612060000001 POO2H54    125211004 DMUN   075      S            P
BX         CHYCH003100
LONTHOLTP 1944 1945      19441945         TB
LIMARY10            2001 00000000
LTMARYLBN 2003 20036     TF
";

    /// The lines above, re-padded to the real records' 80 bytes (the
    /// source file keeps no trailing spaces).
    fn ti() -> Vec<TiRecord> {
        use std::fmt::Write as _;
        let mut padded = String::new();
        for line in TI_LINES.lines() {
            writeln!(padded, "{line:<80}").unwrap();
        }
        parse_ti_lines(&padded)
    }

    fn by_tiploc(records: &[TiplocLocationRecord]) -> HashMap<&str, &TiplocLocationRecord> {
        records.iter().map(|r| (r.tiploc.as_str(), r)).collect()
    }

    fn build(curated: &BTreeMap<String, String>) -> Vec<TiplocLocationRecord> {
        build_location_records(
            &ti(),
            &parse_msn_records(MSN_LINES),
            &ModeTally::from_lines(SCHEDULES.lines()),
            curated,
            980,
        )
    }

    #[test]
    fn msn_records_carry_name_interchange_code_and_grid_reference() {
        let msn = parse_msn_records(MSN_LINES);
        assert_eq!(
            msn["HTRBUS3"],
            MsnRecord {
                tiploc: "HTRBUS3".to_string(),
                name: "HEATHROW TERMINAL 3 BUS".to_string(),
                interchange: Some(9),
                code: Some("HWE".to_string()),
                easting: Some(507_800),
                northing: Some(175_600),
            }
        );
        assert_eq!(msn["SANWBUS"].easting, Some(350_400));
        assert_eq!(msn["SANWBUS"].northing, Some(716_800));
        // The placeholder reference is no reference at all.
        assert_eq!(msn["CATZQBN"].easting, None);
        assert_eq!(msn["CATZQBN"].northing, None);
        // The FILE-SPEC header and short lines are skipped.
        assert!(
            parse_msn_records(
                "A                             FILE-SPEC=05 1.00 28/08/26 18.08.01   944\nA short"
            )
            .is_empty()
        );
    }

    #[test]
    fn the_mode_tally_counts_bus_ship_and_rail_calls_and_passes() {
        let tally = ModeTally::from_lines(SCHEDULES.lines());
        assert_eq!(
            tally.get("SANWBUS"),
            Tally {
                bus_calls: 1,
                ..Tally::default()
            }
        );
        assert_eq!(tally.get("LEUCHRS").bus_calls, 1);
        assert_eq!(tally.get("BANSBUS").bus_calls, 1);
        assert_eq!(tally.get("LARGS").ship_calls, 2);
        assert_eq!(tally.get("CUMBRAE").ship_calls, 1);
        assert_eq!(
            tally.get("MARY10"),
            Tally {
                rail_passes: 1,
                ..Tally::default()
            }
        );
        assert_eq!(tally.get("MARYLBN").rail_calls, 1);
        assert_eq!(tally.get("NOWHERE"), Tally::default());
    }

    #[test]
    fn the_mode_tally_streams_an_mca_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("RJTTF980MCA.txt");
        std::fs::write(&path, SCHEDULES.replace('\n', "\r\n")).unwrap();
        let tally = ModeTally::from_mca(&path).unwrap();
        assert_eq!(tally.get("SANWBUS").bus_calls, 1);
        assert_eq!(tally.get("MARY10").rail_passes, 1);
    }

    #[test]
    fn stations_bus_stops_ferries_and_passing_points_are_told_apart() {
        let records = build(&BTreeMap::new());
        let records = by_tiploc(&records);
        for (tiploc, kind) in [
            ("BANSTED", LocationType::Station),
            ("LEUCHRS", LocationType::Station),
            ("HTRWAPT", LocationType::Station),
            ("EDINBUR", LocationType::Station),
            // Served by the tallied bus.
            ("SANWBUS", LocationType::BusStop),
            // No services in the fixture: the name says bus.
            ("HTRBUS3", LocationType::BusStop),
            ("KESWICK", LocationType::BusStop),
            ("RDNGBUS", LocationType::BusStop),
            // Only passed by rail; the name says signal anyway.
            ("MARY10", LocationType::PassingPoint),
            // No services, no telling name, no STANOX.
            ("BDICK", LocationType::Other),
            // No services; the MSN name says `BUS/TRAM`.
            ("EDINAIR", LocationType::BusStop),
            ("BANSBUS", LocationType::BusStop),
        ] {
            assert_eq!(records[tiploc].location_type, kind, "{tiploc}");
        }
    }

    #[test]
    fn ships_make_a_ferry_terminal_and_buses_a_bus_stop() {
        let brodick = TiRecord {
            tiploc: "BDICK".to_string(),
            station_name: "BRODICK".to_string(),
            stanox: None,
            crs: Some("BDC".to_string()),
        };
        let ship = Tally {
            ship_calls: 10,
            bus_calls: 2,
            ..Tally::default()
        };
        assert_eq!(classify(&brodick, None, ship), LocationType::FerryTerminal);
        let bus = Tally {
            bus_calls: 3,
            ..Tally::default()
        };
        assert_eq!(classify(&brodick, None, bus), LocationType::BusStop);
        let pier = TiRecord {
            station_name: "GOUROCK PIER".to_string(),
            ..brodick.clone()
        };
        assert_eq!(classify(&pier, None, bus), LocationType::FerryTerminal);
        // A freight "port" rail calls at is not a ferry terminal.
        let port = TiRecord {
            station_name: "IMMINGHAM PORT".to_string(),
            ..brodick
        };
        let rail = Tally {
            rail_calls: 4,
            ..Tally::default()
        };
        assert_eq!(classify(&port, None, rail), LocationType::Other);
    }

    #[test]
    fn a_station_stays_a_station_even_when_only_buses_call() {
        let leuchars = &ti()[9];
        assert_eq!(leuchars.tiploc, "LEUCHRS");
        let buses_only = Tally {
            bus_calls: 50,
            ..Tally::default()
        };
        assert_eq!(classify(leuchars, None, buses_only), LocationType::Station);
    }

    #[test]
    fn display_names_come_from_the_ti_description() {
        let records = build(&BTreeMap::new());
        let records = by_tiploc(&records);
        assert_eq!(
            records["HTRBUS3"].display_name,
            "Heathrow Terminal 3 (bus stop)"
        );
        assert_eq!(records["HTRBUS3"].name, "Heathrow Terminal 3");
        assert_eq!(records["SANWBUS"].display_name, "St Andrews (bus station)");
        assert_eq!(records["KESWICK"].display_name, "Keswick (bus station)");
        assert_eq!(records["MARY10"].display_name, "Marylebone 10 Signal");
        assert_eq!(records["LEUCHRS"].display_name, "Leuchars");
        assert_eq!(records["SANWBUS"].msn_code.as_deref(), Some("SAO"));
        assert_eq!(records["SANWBUS"].ti_crs.as_deref(), Some("SAO"));
        assert_eq!(records["SANWBUS"].stanox, None);
        assert_eq!(records["SANWBUS"].msn_interchange, Some(0));
        assert_eq!(records["SANWBUS"].source_sequence, 980);
    }

    #[test]
    fn parents_link_by_same_tiploc_then_nearest_then_curated() {
        let curated = BTreeMap::from([("HTRBUS3".to_string(), "HXX".to_string())]);
        let records = build(&curated);
        let records = by_tiploc(&records);
        let parent = |tiploc: &str| {
            let r = records[tiploc];
            (
                r.parent_crs.as_deref(),
                r.parent_source,
                r.parent_distance_m,
            )
        };
        // MSN files BANSBUS under Banstead's own CRS.
        assert_eq!(
            parent("BANSBUS"),
            (Some("BAD"), Some(ParentSource::SameTiploc), Some(0))
        );
        // Reading's bus stop is one grid square diagonally off the station.
        assert_eq!(
            parent("RDNGBUS"),
            (Some("RDG"), Some(ParentSource::Nearest), Some(141))
        );
        // 412 m from Heathrow Terminals 2 & 3: too far for proximity alone,
        // so only the curated row links it.
        assert_eq!(
            parent("HTRBUS3"),
            (Some("HXX"), Some(ParentSource::Curated), Some(412))
        );
        assert_eq!(
            parent("HTRBUS2"),
            (Some("HXX"), Some(ParentSource::Nearest), Some(200))
        );
        // St Andrews and Keswick have no station nearby.
        assert_eq!(parent("SANWBUS"), (None, None, None));
        assert_eq!(parent("KESWICK"), (None, None, None));
        // Stations and timing points never get a parent.
        assert_eq!(parent("LEUCHRS"), (None, None, None));
        assert_eq!(parent("MARY10"), (None, None, None));
    }

    #[test]
    fn without_the_curated_row_the_412_m_stop_is_unlinked() {
        let records = build(&BTreeMap::new());
        assert_eq!(by_tiploc(&records)["HTRBUS3"].parent_crs, None);
    }

    #[test]
    fn the_checked_in_curated_csv_parses_completely() {
        let parents = checked_in_curated_parents();
        let rows = common::tiploc_parents::CURATED_PARENTS_CSV
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .count();
        // Every data row (all but the header) parsed.
        assert_eq!(parents.len(), rows - 1);
        assert_eq!(parents.get("HTRBUS3").map(String::as_str), Some("HXX"));
    }

    #[test]
    fn curated_rows_are_validated() {
        let parents = curated_parents(
            "# comment\ntiploc,parent_crs,walk_minutes,note\nGOOD,ABC,,fine, with a comma\nBAD-ONE,ABC,,x\nOK2,abcd,,x\nlow,xyz,4,lower case is upper-cased\n",
        );
        assert_eq!(
            parents.into_iter().collect::<Vec<_>>(),
            vec![
                ("GOOD".to_string(), "ABC".to_string()),
                ("LOW".to_string(), "XYZ".to_string()),
            ]
        );
    }

    #[test]
    fn the_last_ti_record_wins_and_the_output_is_sorted() {
        let mut ti_records = ti();
        ti_records.push(TiRecord {
            tiploc: "BANSBUS".to_string(),
            station_name: "BANSTEAD FIRTREE RD".to_string(),
            stanox: None,
            crs: None,
        });
        let records = build_location_records(
            &ti_records,
            &HashMap::new(),
            &ModeTally::default(),
            &BTreeMap::new(),
            0,
        );
        assert_eq!(records.len(), 14);
        assert!(records.windows(2).all(|w| w[0].tiploc < w[1].tiploc));
        assert_eq!(by_tiploc(&records)["BANSBUS"].name, "Banstead Firtree Rd");
    }
}
