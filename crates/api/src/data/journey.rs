//! Merges a train's scheduled timetable (from `trains.calling_points` when
//! schedule-matching has populated it, else reconstructed from
//! `schedule_destination_departures`) with the latest reported movement
//! event per location, into one ordered `JourneyStop[]` -- the primary
//! data source for the train detail page's timeline. See
//! docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::data::eta_blend::london_to_utc;
use crate::data::queries;

/// Mirrors `schedule_matching::ScheduleCallingPointDto`'s exact camelCase
/// wire shape (the format `trains.calling_points` is stored in) -- a
/// separate, `Deserialize`-only type rather than importing that module's
/// private struct, matching this codebase's "each layer owns its own wire
/// shape" posture (the same relationship `frontend/lib/types.ts`'s
/// `ScheduleCallingPoint` already has to it, just on the Rust side).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCallingPoint {
    tiploc: String,
    kind: schedule_query::CallingPointKind,
    booked_arrival: Option<chrono::NaiveTime>,
    booked_departure: Option<chrono::NaiveTime>,
    /// Mirrors `schedule_matching::ScheduleCallingPointDto::day_offset` /
    /// `schedule_query::CallingPoint::day_offset` -- how many calendar days
    /// past `service_date` this calling point's booked times actually fall
    /// on (a real overnight service crosses midnight mid-schedule; see that
    /// field's own doc comment). `#[serde(default)]` so a `trains.calling_points`
    /// row written before this field existed still deserializes, as `0`
    /// (the previous, buggy "always same day" behavior) rather than
    /// failing outright.
    #[serde(default)]
    day_offset: u8,
}

/// The single lookup key both sides of the TIPLOC->CRS join must agree on:
/// trimmed (via [`schedule_query::normalize_tiploc`]) and uppercased.
///
/// Both halves are load-bearing, and getting either wrong fails silently:
///
/// - **Trimming.** `trains.calling_points`'s `tiploc` is the CIF schedule
///   body's **fixed 7-character, space-padded** field, stored verbatim --
///   `schedule_query`'s `parse_calling_point` does `line[2..9].to_string()`
///   with no trim, `schedule_matching::ScheduleCallingPointDto` clones it
///   unchanged, and `schedule_query::CallingPoint::tiploc`'s own doc
///   comment says so explicitly ("Stored here exactly as decoded, still
///   padded; see `normalize_tiploc` for trimming it at query time, not
///   parse time"). `stanox_crs.tiploc`, by contrast, holds the **trimmed**
///   value -- `schedule-reference`'s `parse_ti_lines` does
///   `line[2..9].trim().to_string()`. So a TIPLOC shorter than 7
///   characters (`"PUTNEY "`, `"WDON   "`) never equalled its own
///   `stanox_crs` row, the stop came back with `crs: None` and therefore
///   `name: None`, and the train detail page rendered it as "Unknown
///   location". That is not a rare edge case: in a real CORPUS extract
///   roughly a third of CRS-bearing station TIPLOCs are shorter than 7
///   characters, so this silently hit about one calling point in three,
///   nationwide. Confirmed against live production data for train `L82877`
///   on 2026-09-14 (SWR's Kingston loop): of that journey's 30 stops, all
///   11 whose TIPLOC is genuinely 7 characters resolved (`WATRLMN`,
///   `RAYNSPK`, `NEWMLDN`, `NRBITON`, `HAMWICK`, `TEDNGTN`, `STRWBYH`,
///   `TWCKNHM`, `RICHMND`, `WDWTOWN`, `QTRDBAT`) and all 8 padded ones did
///   not (`"ERLFLD "` Earlsfield, `"WDON   "` Wimbledon, `"KGSTON "`
///   Kingston, `"STMGTS "` St Margarets, `"NSHEEN "` North Sheen,
///   `"MRTLKE "` Mortlake, `"BARNES "` Barnes, `"PUTNEY "` Putney) --
///   every one of them a large, obviously-real station, which is exactly
///   why the symptom read as missing reference data rather than as a
///   key-format bug.
/// - **Uppercasing.** [`queries::crs_for_tiplocs_batch`] keys its returned
///   map on the SQL-side `UPPER(TRIM(tiploc))`, so the Rust-side `get` has
///   to produce the identical string.
///
/// Every other TIPLOC comparison in this codebase already normalizes
/// (`schedule_matching`'s own `crs_for_tiploc` call site,
/// `schedule_query::resolve`'s call sites, `schedule-reference`'s two) --
/// this module was the sole outlier.
///
/// **What this does NOT fix, and deliberately so.** Two further classes of
/// unresolved calling point remain, both visible in the same live journey
/// and neither one a key-format problem:
///
/// 1. **A real station reached via a line-group TIPLOC the crosswalk does
///    not hold.** `stanox_crs`'s primary key is `stanox`
///    (`migrations/20260901150000_stanox_crs.sql`), so the table stores at
///    most ONE TIPLOC per STANOX -- whichever one `schedule-reference`'s
///    `resolve` happened to pick as the CRS-bearing candidate. A station
///    whose STANOX covers several TIPLOCs therefore resolves for one of
///    them and silently misses for the rest. On `L82877` that is Vauxhall
///    (`VAUXHLM`, "Vauxhall Main Lines", STANOX 87214, called at twice) and
///    Clapham Junction (`CLPHMJM` main lines and `CLPHMJW` Windsor lines,
///    STANOX 87219) -- all 7 characters, so trimming cannot help them.
///    Closing this needs a TIPLOC-keyed crosswalk, which is an ingestion
///    and reference-data-schema change, not a query fix, and it needs a
///    stated policy for when a co-located TIPLOC may inherit its STANOX's
///    station identity (a naive "inherit always" would wrongly hand
///    Waterloo's CRS to the junction TIPLOCs in case 2).
/// 2. **A genuine non-station timing point.** A CIF `LI` passing record
///    carries its time in the pass field (bytes `20..24`), which
///    `schedule_query::parse_calling_point` does not decode, so such a stop
///    arrives here with no booked arrival AND no booked departure, and its
///    TIPLOC has no CRS anywhere because the location is a junction, not a
///    station. On `L82877` these are `SHCKLGJ` (Shacklegate Junction),
///    `TWCKNMJ` (Twickenham Junction), `NINELMJ` (Nine Elms Junction),
///    `WLNDNJW` and `WATRLWC`. These are correctly unresolvable; how (or
///    whether) they should appear in a passenger-facing calling-point list
///    is a product question, not a data-quality one.
fn tiploc_key(raw_tiploc: &str) -> String {
    schedule_query::normalize_tiploc(raw_tiploc).to_uppercase()
}

/// Whether -- and to what degree of confidence -- a stop was actually
/// called at, independent of (but computed from) `actual_arrival`/
/// `actual_departure`/`last_event_type` on [`JourneyStop`]. Exists because
/// those fields alone can no longer safely distinguish, for a genuine
/// booked calling point, "not yet reached" from "the train ran through
/// without calling" -- both now leave `actual_arrival`/`actual_departure`
/// `None` (see `overlay_movement_events`'s own doc comment on its `"PASS"`
/// arm, the fix this type is the planned follow-up to). `apply_stop_status`
/// (below) is the only place this is computed.
///
/// `Unknown` covers every stop this whole distinction does not apply to at
/// all: an `Origin`/`Terminate` call (no two-sided booked stop to begin
/// with) or an `Intermediate` entry missing a scheduled arrival or
/// departure (a CIF timing point with a blank public time, never a real
/// calling point) -- the exact same `booked_calling_point` gate
/// `overlay_movement_events`'s `"PASS"` arm already uses, so the two can
/// never disagree about which stops this applies to. Whether such a stop
/// was "reached" is still answered by `actual_arrival`/`actual_departure`
/// alone, exactly as before this type existed.
///
/// Plain PascalCase variant names on the wire, no `rename_all` override --
/// matching `schedule_query::CallingPointKind`'s own convention, the field
/// this one sits right next to on every `JourneyStop`
/// (`frontend/lib/types.ts`'s `JourneyStopKind` mirrors it the same way).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StopStatus {
    /// Not a genuine booked calling point -- see this type's own doc
    /// comment.
    Unknown,
    /// A booked calling point, not yet reached, with no signal saying it
    /// will be skipped -- the ordinary state of every future booked stop.
    Scheduled,
    /// A booked calling point the train genuinely called at (a real
    /// ARRIVAL and/or DEPARTURE was reported here).
    Called,
    /// A booked calling point the train did NOT call at today. See
    /// `JourneyStop::skip_source` for which signal(s) say so.
    Skipped,
}

/// Which signal(s) support a [`StopStatus::Skipped`] verdict -- carried as
/// a SIBLING field on [`JourneyStop`] (`skip_source`), never nested inside
/// `StopStatus` itself. That mirrors this app's existing "surface
/// provenance as its own field, never collapse it into the primary value"
/// convention -- `EtaBadge.tsx`'s `etaSource` badge, shown alongside (never
/// instead of) the ETA it qualifies, is the precedent this follows -- so a
/// `Skipped` stop's `stop_status` always serializes as the same flat
/// string regardless of source, and a consumer that only cares "was this
/// stop skipped" never has to pattern-match a nested value to find out.
///
/// The two sources are not equally trustworthy, and this type exists so
/// that difference is never silently flattened away:
///
/// - `Darwin` is Darwin/LDBWS's own explicit per-calling-point
///   `isCancelled` flag for this specific service
///   (`common::StationDeparture.skipped_stations`) -- the operator's own
///   timetable system stating outright that this call will not happen
///   today. Treated as authoritative.
/// - `Trust` is inferred purely from a reported TRUST `PASS` event at a
///   booked stop -- real running data, but an INFERENCE about what a
///   `PASS` message means for a public calling point, not a first-party
///   "this call was withdrawn" signal (see
///   docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md's
///   still-open PASS-mapping caveat). A consumer must word this more
///   softly than the `Darwin` case.
/// - `Both` is the two signals independently agreeing -- as confident as
///   `Darwin` alone, just worth surfacing that TRUST's own running data
///   corroborates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SkipSource {
    Darwin,
    Trust,
    Both,
}

/// One calling point of a train's journey, booked schedule merged with the
/// latest reported live data for that location -- see this module's own
/// doc comment and the design doc §2/§3.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyStop {
    pub crs: Option<String>,
    pub name: Option<String>,
    pub tiploc: Option<String>,
    pub kind: Option<schedule_query::CallingPointKind>,
    pub scheduled_arrival: Option<DateTime<Utc>>,
    pub scheduled_departure: Option<DateTime<Utc>>,
    pub actual_arrival: Option<DateTime<Utc>>,
    pub actual_departure: Option<DateTime<Utc>>,
    /// Scheduled time + the train's current overall delay, filled in by
    /// `apply_delay_estimates` (below) ONLY while `actual_arrival` is still
    /// `None` -- i.e. only for a stop live movement data hasn't reported an
    /// actual arrival for yet. Always `None` on a freshly-built stop, same
    /// as `actual_arrival`/`actual_departure` above, until that pass runs.
    pub estimated_arrival: Option<DateTime<Utc>>,
    /// See `estimated_arrival`'s own doc comment -- same contract, gated on
    /// `actual_departure` instead.
    pub estimated_departure: Option<DateTime<Utc>>,
    pub last_event_type: Option<String>,
    pub variation_status: Option<String>,
    /// See `apply_stop_status`'s own doc comment for the one exception:
    /// cleared to `None` for a [`StopStatus::Skipped`] stop even when a
    /// TRUST `PASS` event supplied a value here first.
    pub delay_minutes: Option<i32>,
    /// See [`StopStatus`]'s own doc comment. Computed by `apply_stop_status`,
    /// after the live movement overlay -- always `StopStatus::Unknown` on a
    /// freshly-built stop, same "None/default until the relevant pass runs"
    /// contract every other computed field on this struct already has.
    pub stop_status: StopStatus,
    /// `Some` only when `stop_status` is [`StopStatus::Skipped`] -- see
    /// [`SkipSource`]'s own doc comment for why this lives here rather than
    /// nested inside `stop_status` itself.
    pub skip_source: Option<SkipSource>,
}

impl JourneyStop {
    fn from_calling_point(
        cp: &RawCallingPoint,
        crs: Option<String>,
        service_date: NaiveDate,
    ) -> Self {
        // `day_offset` calendar days past `service_date` -- see
        // `RawCallingPoint::day_offset`'s own doc comment for why this
        // can't just be `service_date` unconditionally: a real overnight
        // service's post-midnight calling points are really the NEXT
        // calendar day.
        let calling_point_date = service_date + Duration::days(cp.day_offset as i64);
        Self {
            crs,
            name: None, // filled in by a batch station-name pass in `build_journey_stops`
            // Normalized, not the raw padded field: `JourneyStop` is a wire
            // type (`frontend/lib/types.ts`'s `JourneyStop.tiploc`), and
            // emitting `"PUTNEY "` where every other TIPLOC-shaped value
            // this API serves is bare would just re-export the padding trap
            // `tiploc_key` exists to close. The raw, padded form stays
            // available verbatim on the separate `callingPoints` relay
            // (`render.rs`), which is documented as a pass-through of
            // exactly what was stored.
            tiploc: Some(tiploc_key(&cp.tiploc)),
            kind: Some(cp.kind),
            scheduled_arrival: cp
                .booked_arrival
                .and_then(|t| london_to_utc(calling_point_date.and_time(t))),
            scheduled_departure: cp
                .booked_departure
                .and_then(|t| london_to_utc(calling_point_date.and_time(t))),
            actual_arrival: None,
            actual_departure: None,
            estimated_arrival: None,
            estimated_departure: None,
            last_event_type: None,
            variation_status: None,
            delay_minutes: None,
            // Overwritten by `apply_stop_status`, later in
            // `build_journey_stops` -- see that field's own doc comment.
            stop_status: StopStatus::Unknown,
            skip_source: None,
        }
    }
}

/// Resolves a deserialized `trains.calling_points` blob into the base
/// `JourneyStop` list, given an already-fetched TIPLOC->CRS map.
///
/// Split out of [`build_journey_stops`] purely so the TIPLOC-key contract
/// (see [`tiploc_key`]) is exercisable by a plain `cargo test -p api --lib`
/// unit test rather than only by the `#[ignore]`d, live-database tests at
/// the bottom of this module -- the padding bug this guards against shipped
/// precisely because no DB-free test could see it.
fn stops_from_calling_points(
    raw: &[RawCallingPoint],
    tiploc_to_crs: &HashMap<String, String>,
    service_date: NaiveDate,
) -> Vec<JourneyStop> {
    raw.iter()
        .map(|cp| {
            let crs = tiploc_to_crs.get(&tiploc_key(&cp.tiploc)).cloned();
            JourneyStop::from_calling_point(cp, crs, service_date)
        })
        .collect()
}

/// Fills in each stop's `name` from an already-fetched CRS->name map
/// (`queries::station_names_for_crs_batch`) -- the second half of the
/// TIPLOC->CRS->name chain, split out of [`build_journey_stops`] for
/// exactly the same reason [`stops_from_calling_points`] is: chained
/// straight onto that function's own output, this is the DB-free unit
/// test's route to proving the WHOLE chain (a stop whose feed data
/// supplies only a TIPLOC ends up with a real display `name`), not just
/// the TIPLOC->CRS half `stops_from_calling_points` alone can exercise.
///
/// A CRS the map has no entry for (no `stations` row, or `stop.crs` itself
/// is `None` because the TIPLOC never resolved) leaves `name` as whatever
/// it already was -- `None` on a freshly-built stop -- never a fabricated
/// placeholder; the by-index/pin-endpoint fallback for that case is a
/// client-side, rendering-time decision (`frontend/components/
/// JourneyTimeline.tsx`'s `journeyStopLabel`), not something this
/// server-side pass second-guesses.
fn apply_station_names(stops: &mut [JourneyStop], names: &HashMap<String, String>) {
    for stop in stops {
        if let Some(crs) = &stop.crs {
            stop.name = names.get(&crs.to_uppercase()).cloned();
        }
    }
}

/// Builds the ordered stop list for `(train_uid, service_date)`, or `None`
/// if neither the primary (`calling_points_json`) nor fallback
/// (`schedule_destination_departures`) source has anything -- see the
/// design doc §1 for when this is/isn't called, and §0.2/§3.2 for the
/// fallback's synthetic-terminus construction.
///
/// `current_delay_minutes` is the train's current overall delay --
/// `train_current_state.delay_minutes`, the same corrected, TRUST-own-
/// timebase figure `routes::train`'s callers already read off
/// `TrackedTrainState`/`PublicTrainState` for `blend_darwin_eta` and the
/// top-level delay badge -- propagated onto every stop that has no
/// confirmed actual time yet via `apply_delay_estimates`, below.
///
/// `skipped_stations` is the shared `trains` row's own `skipped_stations`
/// column -- Darwin/LDBWS's explicit per-calling-point skip snapshot,
/// captured at pin time and merged in by
/// `data::trains::find_or_create_train_with_schedule_match` (see that
/// column's own migration comment) -- read by both callers
/// (`routes::train::attach_journey_stops`/`_public`) off
/// `TrackedTrainState`/`PublicTrainState` and passed straight through here
/// for `apply_stop_status` to combine with the TRUST-inferred `"PASS"`
/// signal. An empty slice (no `trains` row yet, or a row with no captured
/// snapshot) is a completely ordinary input -- every stop's `stop_status`
/// then depends on the TRUST signal alone, same as before this parameter
/// existed.
pub async fn build_journey_stops(
    pool: &PgPool,
    trains_id: i64,
    train_uid: &str,
    service_date: NaiveDate,
    calling_points_json: Option<&serde_json::Value>,
    current_delay_minutes: Option<i32>,
    skipped_stations: &[String],
) -> anyhow::Result<Option<Vec<JourneyStop>>> {
    let mut stops: Vec<JourneyStop> = match calling_points_json {
        Some(json) => {
            let raw: Vec<RawCallingPoint> = serde_json::from_value(json.clone())?;
            // `tiploc_key`, not the raw stored value, on BOTH sides -- see
            // that function's own doc comment for the padding bug this
            // closes.
            let tiplocs: Vec<String> = raw.iter().map(|cp| tiploc_key(&cp.tiploc)).collect();
            let tiploc_to_crs = queries::crs_for_tiplocs_batch(pool, &tiplocs).await?;
            stops_from_calling_points(&raw, &tiploc_to_crs, service_date)
        }
        None => {
            let rows =
                queries::list_calling_point_departures_for_train(pool, train_uid, service_date)
                    .await?;
            if rows.is_empty() {
                return Ok(None);
            }
            let mut built: Vec<JourneyStop> = rows
                .iter()
                .map(|row| JourneyStop {
                    crs: Some(row.origin_crs.clone()),
                    name: None,
                    tiploc: None,
                    kind: Some(
                        if row.true_origin_crs.as_deref().is_some_and(|true_origin| {
                            true_origin.eq_ignore_ascii_case(&row.origin_crs)
                        }) {
                            schedule_query::CallingPointKind::Origin
                        } else {
                            schedule_query::CallingPointKind::Intermediate
                        },
                    ),
                    scheduled_arrival: None,
                    // `row.day_offset` -- see `queries::CallingPointDepartureRow::day_offset`'s
                    // own doc comment -- shifts the base date forward for a
                    // calling point that falls on a calendar day AFTER
                    // `service_date` (a real overnight service). Same fix as
                    // the `calling_points_json` branch above, for this
                    // fallback source.
                    scheduled_departure: london_to_utc(
                        (service_date + Duration::days(row.day_offset as i64))
                            .and_time(row.scheduled),
                    ),
                    actual_arrival: None,
                    actual_departure: None,
                    estimated_arrival: None,
                    estimated_departure: None,
                    last_event_type: None,
                    variation_status: None,
                    delay_minutes: None,
                    stop_status: StopStatus::Unknown,
                    skip_source: None,
                })
                .collect();

            if let Some(destination_crs) = rows.last().and_then(|r| r.destination_crs.clone())
                && built
                    .last()
                    .and_then(|s| s.crs.as_deref())
                    .is_none_or(|last_crs| !last_crs.eq_ignore_ascii_case(&destination_crs))
            {
                built.push(JourneyStop {
                    crs: Some(destination_crs),
                    name: None,
                    tiploc: None,
                    kind: Some(schedule_query::CallingPointKind::Terminate),
                    scheduled_arrival: None,
                    scheduled_departure: None,
                    actual_arrival: None,
                    actual_departure: None,
                    estimated_arrival: None,
                    estimated_departure: None,
                    last_event_type: None,
                    variation_status: None,
                    delay_minutes: None,
                    stop_status: StopStatus::Unknown,
                    skip_source: None,
                });
            }
            built
        }
    };

    if stops.is_empty() {
        return Ok(None);
    }

    // Station names, batched over every distinct CRS this stop list has.
    let stop_crs: Vec<String> = stops.iter().filter_map(|s| s.crs.clone()).collect();
    let names = queries::station_names_for_crs_batch(pool, &stop_crs).await?;
    apply_station_names(&mut stops, &names);

    // Live overlay.
    let events = queries::movement_events_for_train(pool, trains_id).await?;
    overlay_movement_events(&mut stops, &events);

    apply_stop_status(&mut stops, skipped_stations);

    apply_delay_estimates(&mut stops, current_delay_minutes);

    Ok(Some(stops))
}

/// Merges the train's reported movement events onto its ordered stop list
/// (design doc §3.3) -- split out of [`build_journey_stops`] so the merge
/// itself is a pure, directly-testable function of its two arguments, with
/// no database in the way.
///
/// `assign_events_to_stops` decides WHICH stop each event belongs to, by
/// position in the journey rather than by CRS identity; this then applies
/// at most one event per stop, exactly as the CRS-keyed version did.
fn overlay_movement_events(stops: &mut [JourneyStop], events: &[queries::MovementEventRow]) {
    let assignment = assign_events_to_stops(stops, events);

    for (index, stop) in stops.iter_mut().enumerate() {
        let Some(event) = assignment[index].map(|event_index| &events[event_index]) else {
            continue;
        };
        stop.last_event_type = event.event_type.clone();
        stop.variation_status = event.variation_status.clone();

        match event.event_type.as_deref() {
            Some("ARRIVAL") => {
                stop.actual_arrival = event.actual_timestamp;
                stop.scheduled_arrival = stop.scheduled_arrival.or(event.planned_timestamp);
            }
            Some("DEPARTURE") => {
                stop.actual_departure = event.actual_timestamp;
                stop.scheduled_departure = stop.scheduled_departure.or(event.planned_timestamp);
            }
            Some("PASS") => {
                // A PASS at a genuine BOOKED calling point (`Intermediate`,
                // with both a scheduled arrival AND departure -- the CIF
                // `LI` shape a real public stop has, per
                // `schedule_query::records::CallingPoint`'s own doc comment)
                // means the train ran through WITHOUT calling: TRUST is
                // reporting that this booked stop did not happen, not that
                // it happened at this instant. Writing `event.actual_timestamp`
                // into `actual_arrival`/`actual_departure` here used to make
                // that render identically to a real completed stop
                // (`frontend/components/JourneyTimeline.tsx`'s
                // `JourneyStopRow`: `reached = actual !== null`) -- a false
                // "the train stopped and picked up/set down here" signal.
                // `last_event_type` below is still set to `"PASS"`
                // unconditionally, so that fact is never lost -- a follow-up
                // is expected to render a distinct "Skipped" state off it.
                //
                // Every other shape reaching this branch -- `Origin`/
                // `Terminate` (no two-sided booked stop to begin with; see
                // `CallingPointKind`'s own doc comment) or an `Intermediate`
                // entry missing one/both scheduled times (a CIF timing point
                // with blank public times, i.e. never actually a public
                // calling point per `schedule_query::parse::parse_calling_point`)
                // -- keeps the old behavior unchanged: there is no real
                // "booked but skipped" stop being misrepresented there.
                let booked_calling_point = stop.kind == Some(schedule_query::CallingPointKind::Intermediate)
                    && stop.scheduled_arrival.is_some()
                    && stop.scheduled_departure.is_some();
                if !booked_calling_point {
                    stop.actual_arrival = event.actual_timestamp;
                    stop.actual_departure = event.actual_timestamp;
                }
                stop.scheduled_arrival = stop.scheduled_arrival.or(event.planned_timestamp);
                stop.scheduled_departure = stop.scheduled_departure.or(event.planned_timestamp);
            }
            _ => {}
        }

        // Delay is diffed from THIS movement event's own two fields --
        // `actual_timestamp` and `planned_timestamp`, both off the SAME
        // `train_movement_events` row -- rather than against
        // `stop.scheduled_arrival`/`scheduled_departure`, which (once a CIF
        // schedule source has populated them, via `from_calling_point` or
        // the fallback branch above) come from a completely different
        // pipeline: the CIF timetable, correctly BST-converted via
        // `chrono_tz`/`london_to_utc`. The real TRUST `TRAIN_MVT_ALL_TOC`
        // feed has been observed delivering `planned_timestamp` AND
        // `actual_timestamp` both skewed by the same amount vs true UTC
        // (an upstream feed issue, outside this codebase -- this repo's own
        // epoch-millis parsing in `trust-consumer` is unaffected). Diffing
        // TRUST's own two fields against EACH OTHER cancels that skew out,
        // exactly as `trust-consumer`'s own top-level `delay_minutes`
        // already does (`crates/trust-consumer/src/process.rs:708`:
        // `derived.delay_minutes = Some((a - p).num_minutes() as i32)`) --
        // positive means late. Diffing TRUST's `actual` against the
        // CIF-derived scheduled time instead mixes two independent
        // timestamp bases and, under that skew, manufactures a bogus ~1
        // hour "late" even when the train is genuinely on time per TRUST's
        // own self-consistent numbers (see this fix's own regression
        // tests). Because both fields come off the one event row, there's
        // no ARRIVAL-vs-DEPARTURE pairing ambiguity to resolve here (unlike
        // the DISPLAYED `actual_arrival`/`actual_departure` /
        // `scheduled_arrival`/`scheduled_departure` above, which do need
        // that pairing).
        //
        // If this event has no `planned_timestamp` (some TRUST messages
        // omit it), `delay_minutes` is `None` -- "delay unknown" -- rather
        // than falling back to the CIF-derived scheduled time, which would
        // silently reintroduce the exact cross-basis bug this is fixing.
        // This matches this function's established "don't guess when data
        // is incomplete" convention (e.g. the event-type `_ => {}` arm
        // just above, and the no-match-found early `continue` at the top
        // of this loop).
        stop.delay_minutes = match (event.actual_timestamp, event.planned_timestamp) {
            (Some(a), Some(p)) => Some((a - p).num_minutes() as i32),
            _ => None,
        };
    }
}

/// Computes each stop's [`StopStatus`] (and, for a skipped one, its
/// [`SkipSource`]) -- the planned follow-up `overlay_movement_events`'s own
/// `"PASS"` doc comment points at. Run AFTER `overlay_movement_events`
/// (needs its `last_event_type`/`actual_arrival`/`actual_departure`), given
/// the train's own captured Darwin `skipped_stations` snapshot (see
/// `build_journey_stops`'s own doc comment for where that comes from).
///
/// Uses the EXACT SAME `booked_calling_point` gate as
/// `overlay_movement_events`'s own `"PASS"` arm -- an `Intermediate` stop
/// with both a scheduled arrival AND departure -- so the two functions can
/// never disagree about which stops this distinction even applies to.
/// Every other stop is left at `StopStatus::Unknown` (`skip_source: None`),
/// the value every freshly-built `JourneyStop` already carries.
///
/// `delay_minutes` DECISION: a skipped stop's `delay_minutes` -- when the
/// TRUST `PASS` event populated it (`overlay_movement_events`'s own comment
/// explains why it's diffed off that event's own two fields) -- is cleared
/// to `None` here. It measures how late that PASS instant was against ITS
/// OWN planned time, which is not a delay any passenger experienced AT this
/// stop -- no one boarded or alighted here at all -- so showing "+6m late"
/// next to a "did not stop here" badge would read as contradictory noise,
/// not useful information. The train's overall delay is still visible
/// everywhere else on the page (the top-level delay badge, and every OTHER
/// stop's own `delay_minutes`/`estimated_*`); this only suppresses the one
/// number that would be misleading in THIS stop's context. A Darwin-only
/// skip (no TRUST event at all yet) already has `delay_minutes: None` from
/// `overlay_movement_events` never having run for this stop, so this is a
/// no-op for that case -- stated explicitly here so it isn't mistaken for
/// an oversight.
fn apply_stop_status(stops: &mut [JourneyStop], skipped_stations: &[String]) {
    for stop in stops.iter_mut() {
        stop.skip_source = None;

        let booked_calling_point = stop.kind == Some(schedule_query::CallingPointKind::Intermediate)
            && stop.scheduled_arrival.is_some()
            && stop.scheduled_departure.is_some();
        if !booked_calling_point {
            stop.stop_status = StopStatus::Unknown;
            continue;
        }

        let trust_pass = stop.last_event_type.as_deref() == Some("PASS");
        let darwin_skip = stop
            .crs
            .as_deref()
            .is_some_and(|crs| skipped_stations.iter().any(|s| s.eq_ignore_ascii_case(crs)));

        stop.stop_status = match (trust_pass, darwin_skip) {
            (true, true) => {
                stop.skip_source = Some(SkipSource::Both);
                StopStatus::Skipped
            }
            (true, false) => {
                stop.skip_source = Some(SkipSource::Trust);
                StopStatus::Skipped
            }
            (false, true) => {
                stop.skip_source = Some(SkipSource::Darwin);
                StopStatus::Skipped
            }
            (false, false) if stop.actual_arrival.is_some() || stop.actual_departure.is_some() => {
                StopStatus::Called
            }
            (false, false) => StopStatus::Scheduled,
        };

        if stop.stop_status == StopStatus::Skipped {
            stop.delay_minutes = None;
        }
    }
}

/// The instant a movement event actually describes -- its reported
/// `actual_timestamp`, falling back to the booked `planned_timestamp` when
/// TRUST sent no actual one. Used only to group and order a train's own
/// events among themselves (see `assign_events_to_stops`), never compared
/// against a CIF-derived scheduled time: both fields come off the same
/// TRUST row, so the uniform feed-wide clock skew the per-stop
/// `delay_minutes` overlay documents at length cancels out of any
/// event-to-event comparison and cannot mis-order two events of one train.
fn event_instant(event: &queries::MovementEventRow) -> Option<DateTime<Utc>> {
    event.actual_timestamp.or(event.planned_timestamp)
}

/// Floor for the schedule-derived [`revisit_gap`] -- longer than any
/// station dwell, so an arrival and its own departure always read as one
/// visit even where the schedule puts two calls implausibly close together.
const MIN_REVISIT_GAP: Duration = Duration::minutes(5);

/// [`revisit_gap`] for a stop list whose scheduled times can't supply one.
const DEFAULT_REVISIT_GAP: Duration = Duration::minutes(30);

/// Decides WHICH stop each reported movement event belongs to, returning
/// one `Option<event index>` per stop, positionally (`assignment[i]` is the
/// event `stops[i]` should display, if any).
///
/// THE BUG THIS EXISTS FOR. The original overlay keyed events by CRS alone
/// -- `HashMap<String /* UPPER(crs) */, MovementEventRow>` fed by a
/// `DISTINCT ON (UPPER(loc_crs))` query -- so a station a journey calls at
/// more than once had its single latest-reported event copied onto EVERY
/// one of those calls. The design doc named that and deferred it (§5
/// Decision 2, "a real but rare CIF anomaly"); it is in fact the normal
/// shape of every circular service. On South Western Railway's Kingston
/// Loop (train L82877, 2026-09-14: London Waterloo 07:27 round via
/// Kingston and Richmond, terminating back at London Waterloo 08:46) the
/// terminus ARRIVAL was smeared back onto the ORIGIN row, which then
/// claimed the train had "arrived" at its own starting point at 08:49 and
/// left no departure recorded there at all. In the other direction it is
/// worse: while such a train is still out on the loop, the origin's
/// DEPARTURE lands on the TERMINUS row too, so `JourneyProgress`'s
/// `lastReachedIndex` (which scans from the end for any confirmed time)
/// jumps the "you are here" marker straight to the final stop the instant
/// the train leaves, and any "has it finished?" check reading the final
/// stop would agree with it.
///
/// THE RULE, in two phases.
///
/// *Phase one -- group each CRS's events into VISITS.* All the reports for
/// one call at one station cluster tightly in time (an arrival and the
/// departure a minute or two later, plus any redelivery or corrected copy
/// of either); two separate calls at the same station are a whole leg of
/// the journey apart. So a station's events are ordered by their own TRUST
/// timestamps and cut into visits wherever the gap between consecutive
/// reports reaches `revisit_gap` -- half the shortest interval the SCHEDULE
/// itself puts between two calls there, which is derived per CRS rather
/// than guessed, and only falls back to `DEFAULT_REVISIT_GAP` when the
/// schedule has no times to derive it from. A CRS called at once is one
/// visit by construction, no gap test applied at all.
/// [`rejoin_split_dwells`] then puts back together any single call an
/// unusually long dwell cut in two, which no time threshold can get right
/// on its own.
///
/// *Phase two -- map visits onto calls, in order.* The visits of the whole
/// journey are ordered by when they started and walked behind a
/// monotonically advancing `cursor` over the stop list; each is assigned to
/// the first not-yet-assigned call at or after the cursor with that CRS. A
/// repeated CRS therefore resolves by POSITION: the origin's own departure
/// lands on the origin, and the terminus arrival -- a visit that started
/// after every intermediate stop pushed the cursor down the list -- lands
/// on the terminus, even though both say only "WAT".
///
/// WHY THE TIME GAP, AND NOT THE EVENT TYPES. An earlier version of this
/// used the event types instead ("a DEPARTURE means the train has left, so
/// the next report at this CRS is the next visit"). That is sound for a
/// clean stream and wrong for a duplicated one -- and this data model
/// duplicates routinely. `trust_schema::dedup::dedup_key` hashes
/// `loc_stanox`, which the live consumer supplies and
/// `trust_event_backlog_match::replay_backlog_history` explicitly does not
/// (a named, accepted limitation in its own doc comment), so one real-world
/// event written down both paths lands as two `train_movement_events` rows
/// under two different dedup keys. A second `WAT DEPARTURE` for the origin
/// then read as "the train has departed a second time", walked forward and
/// landed on the TERMINUS -- re-creating, from a duplicate, the exact bug
/// this function exists to fix. Clustering by time is immune to that: a
/// duplicate sits at (or within seconds of) its original's timestamp, so it
/// always falls inside the same visit.
///
/// Four further deliberate details:
/// * WITHIN a visit the winner is the LAST-RECEIVED event -- the largest
///   index into `events`, which the query returns in `received_at` order.
///   That is precisely the old `DISTINCT ON ... ORDER BY received_at DESC`
///   rule, preserved exactly, and it means a corrected report still
///   supersedes the original even when the correction revises the timestamp
///   backwards. Only the GROUPING uses timestamps; the choice of winner
///   never does.
/// * Visits are ordered by their earliest event's own TRUST timestamp, not
///   by delivery order, so a late-written row can't drag the cursor past
///   stops the train hasn't reached. Both fields of `event_instant` come
///   off the same TRUST row, so the uniform feed-wide clock skew the
///   per-stop `delay_minutes` overlay documents at length cancels out of
///   any event-to-event comparison here.
/// * When no unassigned call at or after the cursor matches, the visit
///   falls back to that CRS's LAST call -- a straggling report for a stop
///   the train has already left, or more observed visits than the schedule
///   knows about (a diversion, an unscheduled reversal), folded into the
///   final scheduled call rather than dropped or allowed to overwrite an
///   earlier one. An event whose CRS appears in no stop at all still IS
///   dropped, exactly as before (design doc §3.3's "silently not merged
///   into any stop").
/// * The CRS comparison is case-insensitive on both sides, same posture as
///   every other CRS comparison in this codebase.
///
/// KNOWN RESIDUAL. If a repeated CRS's EARLIER call was never reported at
/// all and no other stop has reported either, its single visit is assigned
/// to the earlier call rather than the later one -- there is nothing in the
/// data to say the train skipped ahead. Any other reported stop resolves
/// it, because the cursor has already moved past the earlier call by then.
fn assign_events_to_stops(
    stops: &[JourneyStop],
    events: &[queries::MovementEventRow],
) -> Vec<Option<usize>> {
    let mut assignment: Vec<Option<usize>> = vec![None; stops.len()];
    if stops.is_empty() {
        return assignment;
    }

    // Every call at each CRS, in journey order.
    let mut calls: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, stop) in stops.iter().enumerate() {
        if let Some(crs) = &stop.crs {
            calls.entry(crs.to_uppercase()).or_default().push(index);
        }
    }

    // Every event for each of those CRSes, in `received_at` order. An event
    // naming a CRS this journey never calls at is dropped here.
    let mut reports: BTreeMap<&String, Vec<usize>> = BTreeMap::new();
    for (index, event) in events.iter().enumerate() {
        if let Some((crs, _)) = calls.get_key_value(&event.loc_crs.to_uppercase()) {
            reports.entry(crs).or_default().push(index);
        }
    }

    // Phase one: `(start instant, crs, winning event)` per visit.
    let mut visits: Vec<(Option<DateTime<Utc>>, &String, usize)> = Vec::new();
    for (crs, report_indices) in reports {
        let groups = if calls[crs].len() < 2 {
            vec![report_indices]
        } else {
            split_into_visits(events, &report_indices, revisit_gap(stops, &calls[crs]))
        };
        for group in groups {
            let Some(&winner) = group.iter().max() else {
                continue;
            };
            let started = group
                .iter()
                .filter_map(|&i| event_instant(&events[i]))
                .min();
            visits.push((started, crs, winner));
        }
    }

    // Phase two, in journey order: a visit that could not be placed in time
    // at all sorts LAST, never first -- `Option`'s own ordering would put
    // `None` ahead of every real instant, letting one untimed report claim a
    // call before any of the timed visits had a chance to and dragging the
    // cursor with it. Ties then break on the winning event's own received
    // order, so the walk is a total order and therefore reproducible.
    visits.sort_by(|a, b| {
        a.0.is_none()
            .cmp(&b.0.is_none())
            .then(a.0.cmp(&b.0))
            .then(a.2.cmp(&b.2))
    });
    let mut cursor = 0usize;
    for (_, crs, winner) in visits {
        let crs_calls = &calls[crs];
        let last_call = *crs_calls
            .last()
            .expect("a CRS key exists because a stop has it");
        let Some(target) = crs_calls
            .iter()
            .copied()
            .find(|&index| index >= cursor && assignment[index].is_none())
            // Nothing left ahead. A station called at ONCE takes the
            // straggler anyway -- overwriting its one call is exactly the
            // old "latest reported event wins" rule. A station called at
            // several times does NOT: this is a visit the schedule has no
            // call left for, and letting it overwrite the final call could
            // put another call's ARRIVAL on the terminus and have
            // `confirmed_final_arrival` read a still-running train as
            // finished. Dropping an unexplained extra report is the safer
            // of the two wrong answers.
            .or_else(|| {
                (crs_calls.len() == 1 || assignment[last_call].is_none()).then_some(last_call)
            })
        else {
            continue;
        };
        assignment[target] = Some(winner);
        cursor = cursor.max(target);
    }

    assignment
}

/// Half the shortest interval the SCHEDULE puts between two consecutive
/// calls at one station -- the widest gap between two reports that can
/// still safely be read as one visit (see `assign_events_to_stops`).
/// Derived from the CIF times on the stops themselves, which are internally
/// consistent, so this adapts to the route rather than assuming one number
/// fits a 20-minute city loop and a 6-hour cross-country diagram alike.
///
/// `MIN_REVISIT_GAP` floors it so a degenerate schedule (two calls a couple
/// of minutes apart) can't make the gap so small that an ordinary dwell
/// reads as two visits. `DEFAULT_REVISIT_GAP` covers a stop list with no
/// usable scheduled times at all -- comfortably longer than any station
/// dwell, comfortably shorter than any real interval between two calls at
/// one station.
fn revisit_gap(stops: &[JourneyStop], crs_calls: &[usize]) -> Duration {
    let scheduled = |index: usize| -> Option<DateTime<Utc>> {
        stops[index]
            .scheduled_arrival
            .or(stops[index].scheduled_departure)
    };
    let shortest = crs_calls
        .windows(2)
        .filter_map(|pair| match (scheduled(pair[0]), scheduled(pair[1])) {
            (Some(earlier), Some(later)) if later > earlier => Some(later - earlier),
            _ => None,
        })
        .min();
    match shortest {
        Some(gap) => (gap / 2).max(MIN_REVISIT_GAP),
        None => DEFAULT_REVISIT_GAP,
    }
}

/// Cuts one station's reports into visits wherever consecutive reports are
/// `gap` or more apart -- see `assign_events_to_stops`'s phase one.
/// `report_indices` is in `received_at` order and stays that way inside each
/// returned group, so a group's largest element is still its
/// latest-received event.
///
/// A report with no timestamp of its own can't be placed by time, so it
/// joins the LAST visit -- keeping the old "latest reported event wins"
/// behaviour for it rather than inventing a position for it. Reports that
/// are all untimed therefore form a single visit.
fn split_into_visits(
    events: &[queries::MovementEventRow],
    report_indices: &[usize],
    gap: Duration,
) -> Vec<Vec<usize>> {
    let mut timed: Vec<(DateTime<Utc>, usize)> = report_indices
        .iter()
        .filter_map(|&index| event_instant(&events[index]).map(|at| (at, index)))
        .collect();
    timed.sort_by_key(|&(at, index)| (at, index));

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut previous: Option<DateTime<Utc>> = None;
    for (at, index) in timed {
        if previous.is_none_or(|earlier| at - earlier >= gap) {
            groups.push(Vec::new());
        }
        groups
            .last_mut()
            .expect("the first iteration always pushes a group")
            .push(index);
        previous = Some(at);
    }

    let untimed = report_indices
        .iter()
        .copied()
        .filter(|&index| event_instant(&events[index]).is_none());
    if groups.is_empty() {
        groups.push(Vec::new());
    }
    groups
        .last_mut()
        .expect("non-empty by the guard above")
        .extend(untimed);

    // `received_at` order within each visit -- `timed` was sorted by
    // instant, and the untimed tail was appended after it.
    for group in &mut groups {
        group.sort_unstable();
    }
    groups.retain(|group| !group.is_empty());
    rejoin_split_dwells(events, groups)
}

/// Every event in `group` is a report of type `event_type`, and there is at
/// least one. Used only by [`rejoin_split_dwells`].
fn every_report_is(
    events: &[queries::MovementEventRow],
    group: &[usize],
    event_type: &str,
) -> bool {
    !group.is_empty()
        && group
            .iter()
            .all(|&index| events[index].event_type.as_deref() == Some(event_type))
}

/// Puts back together a single call that [`split_into_visits`]'s time gap
/// cut in two, using the one thing the event types CAN say safely.
///
/// A dwell longer than `revisit_gap` is rare but real -- a train held at a
/// platform during disruption, at a station the same journey calls at again
/// later -- and splitting one call into two costs more than a cosmetic
/// error: the surplus visit consumes the next call's slot, every later
/// visit at that CRS cascades one position along, and an intermediate
/// call's ARRIVAL can end up on the terminus, where
/// [`confirmed_final_arrival`] would read it as a still-running train
/// having finished.
///
/// The asymmetry this leans on: within ONE call the order is always ARRIVAL
/// then DEPARTURE, so a group of arrivals immediately followed by a group of
/// departures is overwhelmingly likely to be one call, and is rejoined. The
/// reverse order -- departures then arrivals -- is precisely the shape of a
/// genuine revisit (a loop's origin DEPARTURE, then its terminus ARRIVAL an
/// hour later), and is never merged. Anything else (a `PASS`, which is a
/// whole call by itself, a group that already holds both types, an untyped
/// report) is left alone too.
///
/// HOW FAR THAT HOLDS. "Arrivals then departures is one call" is an
/// inference, not a certainty: it assumes each call reports both halves. Two
/// genuinely separate calls, the first having lost its DEPARTURE report and
/// the second its ARRIVAL, present the same shape and are wrongly rejoined
/// -- the earlier call then shows nothing. That needs two complementary
/// report losses at one station in one journey, and its cost is confined to
/// display: the merged visit's winner is the DEPARTURE, and
/// `overlay_movement_events` only ever sets `actual_arrival` from a winning
/// event whose own type is `ARRIVAL`, so [`confirmed_final_arrival`] cannot
/// be made to read a still-running train as finished by it. The same bound
/// applies to the mirror case, where an untimed report of a different type
/// lands in the departures group (`split_into_visits` appends untimed
/// reports to the last group) and suppresses a rejoin that should have
/// happened. Both are regression-tested below, and both fail in the safe
/// direction -- a missing arrival, never an invented one.
fn rejoin_split_dwells(
    events: &[queries::MovementEventRow],
    groups: Vec<Vec<usize>>,
) -> Vec<Vec<usize>> {
    let mut rejoined: Vec<Vec<usize>> = Vec::with_capacity(groups.len());
    for group in groups {
        if let Some(previous) = rejoined.last_mut()
            && every_report_is(events, previous, "ARRIVAL")
            && every_report_is(events, &group, "DEPARTURE")
        {
            previous.extend(group);
            previous.sort_unstable();
            continue;
        }
        rejoined.push(group);
    }
    rejoined
}

/// How many minutes past a train's estimated final-stop arrival counts as
/// "may have arrived" (`may_have_arrived`, below) -- generous enough to
/// absorb the estimate's own imprecision (a delay observed at wherever the
/// train was last actually reported is propagated forward UNIFORMLY, not
/// re-measured at each stop), while still catching a train that's
/// genuinely stopped reporting within the same rail-service-length
/// timescale the rest of this module already reasons in.
const MAY_HAVE_ARRIVED_THRESHOLD: Duration = Duration::minutes(15);

/// Fills in `estimated_arrival`/`estimated_departure` for every stop that
/// has no confirmed actual time yet, by propagating the train's current
/// overall delay onto that stop's CIF-derived scheduled time -- the same
/// "extrapolate forward from the currently-known delay" idea
/// `trust-consumer`'s own (currently unused) `eta::propagate_eta` encodes,
/// applied here per-stop instead of to a single next-calling-point value.
///
/// A stop that already has a confirmed `actual_arrival`/`actual_departure`
/// (the live overlay loop above already populated it from a real reported
/// movement event) is left completely alone -- this never overwrites real
/// data with a guess, matching this whole function's established "don't
/// guess when data is incomplete" convention (see the per-stop
/// `delay_minutes` overlay's own doc comment above).
///
/// A `None` `current_delay_minutes` (the train's overall delay is itself
/// unknown) leaves every stop's estimate `None` too, rather than silently
/// assuming a delay of zero -- guessing "on time" would be worse than
/// showing nothing.
pub fn apply_delay_estimates(stops: &mut [JourneyStop], current_delay_minutes: Option<i32>) {
    let Some(delay_minutes) = current_delay_minutes else {
        return;
    };
    let delay = Duration::minutes(delay_minutes as i64);
    for stop in stops.iter_mut() {
        if stop.actual_arrival.is_none() {
            stop.estimated_arrival = stop.scheduled_arrival.map(|t| t + delay);
        }
        if stop.actual_departure.is_none() {
            stop.estimated_departure = stop.scheduled_departure.map(|t| t + delay);
        }
    }
}

/// The server-side replacement for the frontend's old client-only "may
/// have finished" heuristic (`state.status === 'en_route' &&
/// state.nextCallingPoint === null`, which fired almost always since
/// `nextCallingPoint` is essentially never populated in practice). `true`
/// once `now` is more than `MAY_HAVE_ARRIVED_THRESHOLD` past the journey's
/// FINAL calling point's ESTIMATED arrival (`apply_delay_estimates`,
/// above, must already have been run on `stops`) -- an inference, never
/// asserted as fact (see `TrainJourney.tsx`'s copy for this field).
///
/// `false`, not an inference, whenever the final stop's `estimated_arrival`
/// is `None` -- which covers BOTH "the train has genuinely, for real,
/// already arrived" (a confirmed `actual_arrival` means
/// `apply_delay_estimates` never set an estimate for that stop at all) AND
/// "there's no schedule/delay data to estimate from" (no scheduled time,
/// or the overall delay is unknown). Neither case has anything for this
/// heuristic to safely infer from, so it stays silent rather than
/// guessing -- same posture as `apply_delay_estimates` itself.
pub fn may_have_arrived(stops: &[JourneyStop], now: DateTime<Utc>) -> bool {
    stops
        .last()
        .and_then(|stop| stop.estimated_arrival)
        .is_some_and(|eta| now - eta > MAY_HAVE_ARRIVED_THRESHOLD)
}

/// Real, reported evidence that this journey has finished, anchored to the
/// train's FINAL scheduled calling point BY POSITION -- `stops.last()`, the
/// end of the ordered list `build_journey_stops` produced -- and never to a
/// CRS code, which a circular or reversing service can repeat earlier in
/// the same journey.
///
/// The condition mirrors `trust_schema::journey::apply_movement`'s own
/// confirmed-terminus rule exactly, so the two can never disagree about
/// what counts: a reported `ARRIVAL` there, not a `DEPARTURE` or a `PASS`
/// (empty stock running through the terminus's own location, a diversion),
/// and only with a real `actual_arrival` behind it. `false`, not an
/// inference, for everything else -- an unreported terminus, a journey with
/// no stops at all, or a train that has only reached an earlier stop that
/// happens to share the terminus's CRS.
pub fn confirmed_final_arrival(stops: &[JourneyStop]) -> bool {
    stops.last().is_some_and(|stop| {
        stop.actual_arrival.is_some() && stop.last_event_type.as_deref() == Some("ARRIVAL")
    })
}

/// Read-time reconciliation of the stored `train_current_state.status`
/// against what the journey timeline itself can prove -- the same "read the
/// row, then overlay a computed field" shape `blend_darwin_eta` and
/// `may_have_arrived` already have on these structs, and written back to
/// the database by neither.
///
/// WHY THIS IS NEEDED. `status` is derived ONCE, as each TRUST event is
/// ingested, by `trust_schema::journey::apply_movement` -- which can only
/// recognise a finished journey if `trains.destination_crs` is already
/// known at that moment. It very often isn't: the shared ingest path writes
/// events for every train in the feed, schedule-matched or not, and
/// `routes::train::enrich_shared_train` replays a train's retained history
/// BEFORE running the schedule match that fills `destination_crs` in. Once
/// that match lands, nothing ever re-derives the status, so a train that
/// demonstrably reached its terminus hours ago stays `'en_route'` forever
/// (observed in production on L82877/2026-09-14 and on every other
/// completed service checked alongside it). Reading completion back off the
/// timeline closes that gap without a migration, a backfill, or a second
/// writer racing the live consumer for the same column.
///
/// WHERE THIS IS (AND ISN'T) APPLIED. Both single-train read routes --
/// `routes::train::attach_journey_stops` and its `_public` sibling -- call
/// this, so `GET /Train/{trackingId}` and `GET /Train/by-uid/{uid}/{date}`
/// agree. The two LIST routes do not, and deliberately: `GET /Train/mine`
/// (`train_tracking::list_tracked_trains_for_user`) and `GET
/// /public/lines/{id}/trains` (`render::line_train_json`) both read
/// `train_current_state.status` straight out of one batched SELECT, and
/// applying this would mean building a full journey timeline per row. They
/// keep showing the stored status, exactly as they did before this
/// function existed -- a known gap, not a regression, and the right fix for
/// it is to re-derive the stored status when a schedule match first
/// supplies `trains.destination_crs`, not to fan this overlay out across
/// list endpoints.
///
/// UPGRADE ONLY, and only from `'en_route'`. `'cancelled'`,
/// `'awaiting_activation'`, an already-`'completed'` row and an absent
/// status are all returned untouched: a timeline can positively prove a
/// train DID arrive, but a missing final-stop arrival proves nothing (the
/// stop's CRS may simply never have resolved -- L82877 has eight such stops)
/// and must never be read as proof a train did NOT. So this can only ever
/// move a train from "we have not noticed it finish" to "it finished",
/// never the other way.
pub fn apply_confirmed_arrival(
    status: Option<String>,
    stops: Option<&[JourneyStop]>,
) -> Option<String> {
    if status.as_deref() != Some("en_route") {
        return status;
    }
    match stops {
        Some(stops) if confirmed_final_arrival(stops) => Some("completed".to_string()),
        _ => status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_stop() -> JourneyStop {
        JourneyStop {
            crs: None,
            name: None,
            tiploc: None,
            kind: None,
            scheduled_arrival: None,
            scheduled_departure: None,
            actual_arrival: None,
            actual_departure: None,
            estimated_arrival: None,
            estimated_departure: None,
            last_event_type: None,
            variation_status: None,
            delay_minutes: None,
            stop_status: StopStatus::Unknown,
            skip_source: None,
        }
    }

    /// Builds one `RawCallingPoint` the same way a real
    /// `trains.calling_points` row does: `tiploc` exactly as the CIF
    /// schedule body carried it, i.e. space-padded to 7 characters.
    fn raw_cp(tiploc: &str) -> RawCallingPoint {
        RawCallingPoint {
            tiploc: tiploc.to_string(),
            kind: schedule_query::CallingPointKind::Intermediate,
            booked_arrival: "08:00:00".parse().ok(),
            booked_departure: "08:01:00".parse().ok(),
            day_offset: 0,
        }
    }

    #[test]
    fn tiploc_key_trims_the_fixed_seven_char_schedule_body_padding() {
        // The real shape `schedule_query::parse_calling_point` produces and
        // `ScheduleCallingPointDto` stores verbatim, for a TIPLOC shorter
        // than the fixed 7-character field.
        assert_eq!(tiploc_key("PUTNEY "), "PUTNEY");
        assert_eq!(tiploc_key("WDON   "), "WDON");
        assert_eq!(tiploc_key("BARNES "), "BARNES");
    }

    #[test]
    fn tiploc_key_is_unchanged_for_an_exactly_seven_char_tiploc() {
        // The case that masked the bug in casual testing: a TIPLOC that
        // happens to be exactly 7 characters needs no padding, so it
        // resolved correctly even before this fix.
        assert_eq!(tiploc_key("WATRLMN"), "WATRLMN");
        assert_eq!(tiploc_key("RAYNSPK"), "RAYNSPK");
    }

    #[test]
    fn tiploc_key_uppercases_so_it_matches_the_sql_sides_upper_trim() {
        assert_eq!(tiploc_key("putney "), "PUTNEY");
    }

    /// The actual regression test for the live journey-page bug (train
    /// `L82877`, 2026-09-14, SWR's Kingston loop): every calling point
    /// whose TIPLOC is shorter than the CIF schedule body's fixed
    /// 7-character field rendered as "Unknown location", because the
    /// padded stored value (`"PUTNEY "`) was looked up verbatim against
    /// `stanox_crs.tiploc`'s trimmed value (`"PUTNEY"`). The 7-character
    /// TIPLOCs on the same journey resolved fine, which is exactly why it
    /// read as "some stations are missing from the reference data" rather
    /// than as a key-format bug.
    ///
    /// The TIPLOCs, their order and their padding are taken verbatim from
    /// that journey's own live response, not invented -- the padded entries
    /// here are precisely the stops the live page showed as "Unknown
    /// location" for a reason this fix addresses.
    #[test]
    fn every_short_padded_tiploc_on_the_real_kingston_loop_journey_resolves_to_its_crs() {
        let service_date: NaiveDate = "2026-09-14".parse().unwrap();
        // Keyed as `crs_for_tiplocs_batch` returns them: UPPER(TRIM(...)).
        let tiploc_to_crs: HashMap<String, String> = [
            ("WATRLMN", "WAT"),
            ("ERLFLD", "EAD"),
            ("WDON", "WIM"),
            ("RAYNSPK", "RAY"),
            ("KGSTON", "KNG"),
            ("STMGTS", "SMG"),
            ("RICHMND", "RMD"),
            ("NSHEEN", "NSH"),
            ("MRTLKE", "MTL"),
            ("BARNES", "BNS"),
            ("PUTNEY", "PUT"),
        ]
        .into_iter()
        .map(|(t, c)| (t.to_string(), c.to_string()))
        .collect();

        let raw = vec![
            raw_cp("WATRLMN"), // exactly 7 -- resolved before this fix too
            raw_cp("ERLFLD "), // Earlsfield
            raw_cp("WDON   "), // Wimbledon
            raw_cp("RAYNSPK"), // exactly 7
            raw_cp("KGSTON "), // Kingston
            raw_cp("STMGTS "), // St Margarets
            raw_cp("RICHMND"), // exactly 7
            raw_cp("NSHEEN "), // North Sheen
            raw_cp("MRTLKE "), // Mortlake
            raw_cp("BARNES "), // Barnes
            raw_cp("PUTNEY "), // Putney
        ];

        let stops = stops_from_calling_points(&raw, &tiploc_to_crs, service_date);

        assert_eq!(
            stops.iter().map(|s| s.crs.as_deref()).collect::<Vec<_>>(),
            vec![
                Some("WAT"),
                Some("EAD"),
                Some("WIM"),
                Some("RAY"),
                Some("KNG"),
                Some("SMG"),
                Some("RMD"),
                Some("NSH"),
                Some("MTL"),
                Some("BNS"),
                Some("PUT"),
            ],
            "every sub-7-character TIPLOC must resolve; before this fix only the \
             exactly-7-character ones (WATRLMN, RAYNSPK, RICHMND) did"
        );
        assert!(
            stops
                .iter()
                .all(|s| s.tiploc.as_deref().is_some_and(|t| t.trim() == t)),
            "the emitted wire `tiploc` must be the bare code, never the padded field"
        );
    }

    /// Regression test for the seed-fixture wire-shape mismatch the UX
    /// accessibility/usability review's §4.2 describes
    /// (docs/superpowers/specs/2026-09-17-full-service-ux-accessibility-usability-review.md):
    /// `.devdata/seed.sql`'s `trains.calling_points` blob used
    /// `crs`/`name`/`plannedArrival`/`plannedDeparture` with full zoned UTC
    /// instants, which is not `RawCallingPoint`'s actual wire shape at all
    /// (`tiploc`/`kind`/`bookedArrival`/`bookedDeparture` as **naive
    /// London wall-clock** `NaiveTime` values, camelCase) -- so a seed row
    /// shaped like the old fixture fails `serde_json::from_value` outright
    /// (`could not build journey stops`, `crates/api/src/routes/train.rs:966`),
    /// and even a corrected shape that mistakenly treated the booked times
    /// as already-UTC would be silently an hour off for any BST service
    /// date.
    ///
    /// This is a DB-free counterpart to the `#[ignore]`d live-database test
    /// `db_tests::build_journey_stops_from_calling_points_json_resolves_tiploc_to_crs_and_kind`
    /// below, which only asserts `scheduled_departure`/`scheduled_arrival`
    /// are `Some` -- not that the UTC *value* is correct. This proves both
    /// halves: (1) the real camelCase wire shape deserializes successfully
    /// into `RawCallingPoint`, and (2) a naive `bookedDeparture`/
    /// `bookedArrival` London wall-clock value on a BST service date
    /// converts through `london_to_utc` to the correct UTC instant -- one
    /// hour earlier, not equal to the naive value reinterpreted as UTC.
    #[test]
    fn real_wire_shape_calling_points_json_deserializes_and_converts_bst_wall_clock_to_utc() {
        let service_date: NaiveDate = "2026-09-17".parse().unwrap(); // BST: UTC+1
        let calling_points_json = serde_json::json!([
            {
                "tiploc": "KNGX",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "dayOffset": 0
            },
            {
                "tiploc": "EDINBUR",
                "kind": "Terminate",
                "bookedArrival": "13:30:00",
                "bookedDeparture": null,
                "dayOffset": 0
            }
        ]);

        let raw: Vec<RawCallingPoint> = serde_json::from_value(calling_points_json).expect(
            "the real trains.calling_points wire shape must deserialize into RawCallingPoint",
        );

        let tiploc_to_crs: HashMap<String, String> = [
            ("KNGX".to_string(), "KGX".to_string()),
            ("EDINBUR".to_string(), "EDB".to_string()),
        ]
        .into_iter()
        .collect();

        let stops = stops_from_calling_points(&raw, &tiploc_to_crs, service_date);

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].crs.as_deref(), Some("KGX"));
        assert_eq!(
            stops[0].scheduled_departure,
            Some("2026-09-17T08:00:00Z".parse().unwrap()),
            "09:00 London wall-clock on a BST date (UTC+1) must convert to 08:00 UTC, not be \
             reinterpreted as already-UTC 09:00 -- the exact regression a zoned-instant \
             `plannedDeparture` fixture shape would mask"
        );
        assert_eq!(stops[1].crs.as_deref(), Some("EDB"));
        assert_eq!(
            stops[1].scheduled_arrival,
            Some("2026-09-17T12:30:00Z".parse().unwrap()),
            "13:30 London wall-clock on a BST date (UTC+1) must convert to 12:30 UTC"
        );
    }

    /// Task 3.6.2's regression test for the whole TIPLOC->CRS->name chain,
    /// DB-free: chains [`stops_from_calling_points`] straight into
    /// [`apply_station_names`] for a stop whose feed data supplies only a
    /// TIPLOC (no CRS, no name -- exactly what `trains.calling_points`
    /// actually carries), proving both halves of the join actually run
    /// server-side rather than leaving a `journeyStopLabel` fallback on the
    /// frontend to do the whole job. Before this fix landed (Task 0.2's
    /// fixture correction plus the tiploc-padding fix above), a broken link
    /// anywhere in this chain surfaced on the train detail page as "Unknown
    /// location"; this test would have caught it without a live Postgres
    /// connection.
    #[test]
    fn tiploc_to_crs_to_name_resolves_end_to_end_for_a_stop_with_only_a_tiploc() {
        let service_date: NaiveDate = "2026-09-17".parse().unwrap();
        let tiploc_to_crs: HashMap<String, String> =
            [("KNGX".to_string(), "KGX".to_string())].into_iter().collect();
        let names: HashMap<String, String> =
            [("KGX".to_string(), "LONDON KINGS CROSS".to_string())].into_iter().collect();

        let raw = vec![raw_cp("KNGX")];
        let mut stops = stops_from_calling_points(&raw, &tiploc_to_crs, service_date);
        assert_eq!(stops[0].name, None, "name is not yet resolved by stops_from_calling_points alone");

        apply_station_names(&mut stops, &names);

        assert_eq!(stops[0].crs.as_deref(), Some("KGX"), "the tiploc->crs half of the join");
        assert_eq!(
            stops[0].name.as_deref(),
            Some("LONDON KINGS CROSS"),
            "the crs->name half of the join -- together, a stop whose only feed \
             data is a bare tiploc must end up with a real display name, not \
             fall through to the frontend's generic fallback"
        );
    }

    /// The train detail page renders its header departure time and its
    /// timetable rows from two **independently written** values, and this
    /// test is the guard that they name the same instant.
    ///
    /// - The header (`frontend/app/train/[uid]/[date]/page.tsx`'s
    ///   `pinScheduledDeparture: train.scheduledDeparture` ->
    ///   `frontend/lib/trackingName.ts`'s `formatTime`) renders the
    ///   `trains.scheduled_departure` **column**, which
    ///   [`crate::data::trains::find_or_create_train_with_schedule_match`]
    ///   binds verbatim from the caller's `pin_scheduled_departure` -- an
    ///   already-zoned `DateTime<Utc>`, never re-converted.
    /// - The timetable row (`frontend/components/JourneyTimeline.tsx`)
    ///   renders `stops[i].scheduled_departure`, which
    ///   [`JourneyStop::from_calling_point`] derives by pushing the
    ///   calling point's **naive London wall-clock** `bookedDeparture`
    ///   through [`london_to_utc`].
    ///
    /// Nothing in the type system ties those two together: a
    /// `scheduled_departure` written as a bare wall-clock time with a `Z`
    /// stapled on ("09:00:00Z" meaning 09:00 *London*) sits happily beside
    /// a `bookedDeparture` of `"09:00:00"` that this module correctly
    /// resolves to 08:00Z, and the page then shows 10:00 in its header and
    /// 09:00 in its table -- a clean one-hour disagreement for the ~7
    /// months of BST, invisible in winter. That is the class of bug this
    /// test exists to catch.
    ///
    /// In production the two cannot actually drift, and it is worth
    /// recording why, because it is a real invariant rather than a
    /// coincidence: `schedule_matching::find_schedule_match` picks the
    /// matched schedule with `schedule_query::match_pin`, which keeps a
    /// candidate only when `|pin_scheduled_departure -
    /// london_to_utc(booked_departure)| <= common::MATCH_TOLERANCE` -- the
    /// *same* conversion this module applies. `attempt_schedule_match`
    /// then writes that match's `calling_points` and the pin instant onto
    /// the `trains` row in one call, so the two columns are always written
    /// together from one agreeing match. An hour of BST error is 60
    /// minutes, far outside the 20-minute tolerance, so it would make the
    /// match fail outright (no `calling_points` at all) rather than
    /// produce a mismatched pair. A fixture or backfill that writes the
    /// two columns by hand bypasses that check entirely -- hence this
    /// test.
    #[test]
    fn origin_stops_scheduled_departure_equals_the_pin_instant_it_was_matched_against() {
        let service_date: NaiveDate = "2026-09-17".parse().unwrap(); // BST: UTC+1

        // Exactly what `trains.calling_points` holds: naive London
        // wall-clock times, camelCase, no zone.
        let raw: Vec<RawCallingPoint> = serde_json::from_value(serde_json::json!([
            {
                "tiploc": "KNGX",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "dayOffset": 0
            },
            {
                "tiploc": "EDINBUR",
                "kind": "Terminate",
                "bookedArrival": "13:30:00",
                "bookedDeparture": null,
                "dayOffset": 0
            }
        ]))
        .expect("the real trains.calling_points wire shape must deserialize");

        // What `trains.scheduled_departure` holds for the same train: the
        // pin instant, stored already-zoned and NEVER re-converted (see
        // `find_or_create_train_with_schedule_match`). A correctly
        // authored one names the same moment as the origin calling point's
        // 09:00 London wall clock, i.e. 08:00 UTC on a BST date.
        let pin_scheduled_departure: DateTime<Utc> = "2026-09-17T08:00:00Z".parse().unwrap();

        let stops = stops_from_calling_points(&raw, &HashMap::new(), service_date);

        assert_eq!(
            stops[0].scheduled_departure,
            Some(pin_scheduled_departure),
            "the origin stop's scheduled_departure (rendered in the timetable) and the pin \
             instant mirrored onto trains.scheduled_departure (rendered in the page header) \
             must be the same instant -- if this fails, the detail page shows two different \
             departure times for one train"
        );

        // The production invariant itself, stated as an assertion:
        // `match_pin` would only ever have produced this pairing if the
        // two were within `MATCH_TOLERANCE` of each other.
        let delta = (pin_scheduled_departure - stops[0].scheduled_departure.unwrap()).abs();
        assert!(
            delta <= common::MATCH_TOLERANCE,
            "schedule_query::match_pin would never have matched this pin to this schedule: \
             delta {delta} exceeds common::MATCH_TOLERANCE"
        );

        // And the specific way it goes wrong: a `scheduled_departure`
        // authored as the wall-clock time with a `Z` stapled on is an hour
        // late, which is both unequal AND outside the match tolerance --
        // i.e. unreachable through the real write path.
        let naively_zoned: DateTime<Utc> = "2026-09-17T09:00:00Z".parse().unwrap();
        assert_ne!(
            stops[0].scheduled_departure,
            Some(naively_zoned),
            "09:00 London on a BST date is 08:00Z, not 09:00Z"
        );
        assert!(
            (naively_zoned - stops[0].scheduled_departure.unwrap()).abs() > common::MATCH_TOLERANCE,
            "an hour of BST error must be outside MATCH_TOLERANCE -- that is why the real \
             schedule-matching path cannot produce this mismatch"
        );
    }

    /// A **characterization** test, not a regression test: it records that
    /// trimming does nothing for the first remaining gap in `tiploc_key`'s
    /// "What this does NOT fix" note (Vauxhall's `VAUXHLM`, Clapham
    /// Junction's `CLPHMJM`/`CLPHMJW`, each called at under a TIPLOC that
    /// is not the one their STANOX-keyed crosswalk row retained). It passes
    /// identically with and without this fix -- that is the point. It
    /// exists so the gap is stated in code rather than only in prose, and
    /// so whoever closes it has an obvious place to come and change the
    /// expectation.
    #[test]
    fn a_line_group_tiploc_absent_from_the_stanox_keyed_crosswalk_is_still_unresolved() {
        let service_date: NaiveDate = "2026-09-14".parse().unwrap();
        // `VAUXHLW` stands in for "whichever sibling TIPLOC of STANOX 87214
        // the crosswalk actually retained" -- which one it is was not
        // verified, and does not matter to what this test pins: the
        // crosswalk can hold only ONE of Vauxhall's TIPLOCs, and the
        // schedule calls at `VAUXHLM`.
        let tiploc_to_crs: HashMap<String, String> = [("VAUXHLW".to_string(), "VXH".to_string())]
            .into_iter()
            .collect();

        let stops = stops_from_calling_points(&[raw_cp("VAUXHLM")], &tiploc_to_crs, service_date);

        assert_eq!(
            stops[0].crs, None,
            "trimming cannot help a 7-character TIPLOC the crosswalk simply does not hold -- \
             closing this needs a TIPLOC-keyed crosswalk, not a query change"
        );
    }

    #[test]
    fn a_tiploc_with_no_crosswalk_row_still_degrades_to_none_rather_than_guessing() {
        // Unchanged behaviour, asserted so the normalization above can't
        // quietly turn a genuine miss into a fabricated match. `SHCKLGJ`
        // (Shacklegate Junction) is one of the real, correctly-unresolvable
        // non-station timing points on the same live journey -- see
        // `tiploc_key`'s own "What this does NOT fix" note.
        let service_date: NaiveDate = "2026-09-14".parse().unwrap();
        let tiploc_to_crs: HashMap<String, String> = [("PUTNEY".to_string(), "PUT".to_string())]
            .into_iter()
            .collect();

        let stops = stops_from_calling_points(&[raw_cp("SHCKLGJ")], &tiploc_to_crs, service_date);

        assert_eq!(stops.len(), 1);
        assert_eq!(stops[0].crs, None);
        assert_eq!(stops[0].tiploc.as_deref(), Some("SHCKLGJ"));
    }

    #[test]
    fn apply_delay_estimates_propagates_the_current_delay_onto_an_unreported_stop() {
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-09T10:00:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-09T10:02:00Z".parse().unwrap()),
            ..blank_stop()
        }];

        apply_delay_estimates(&mut stops, Some(5));

        assert_eq!(
            stops[0].estimated_arrival,
            Some("2026-09-09T10:05:00Z".parse().unwrap())
        );
        assert_eq!(
            stops[0].estimated_departure,
            Some("2026-09-09T10:07:00Z".parse().unwrap())
        );
    }

    #[test]
    fn apply_delay_estimates_never_overwrites_a_stop_with_a_confirmed_actual_time() {
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-09T10:00:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-09T10:02:00Z".parse().unwrap()),
            actual_arrival: Some("2026-09-09T10:03:00Z".parse().unwrap()),
            actual_departure: Some("2026-09-09T10:04:00Z".parse().unwrap()),
            ..blank_stop()
        }];

        apply_delay_estimates(&mut stops, Some(5));

        assert_eq!(
            stops[0].estimated_arrival, None,
            "a confirmed actual_arrival must never get an estimate alongside it"
        );
        assert_eq!(
            stops[0].estimated_departure, None,
            "a confirmed actual_departure must never get an estimate alongside it"
        );
    }

    #[test]
    fn apply_delay_estimates_estimates_arrival_and_departure_independently() {
        // A stop that's been departed from (actual_departure known) but
        // whose arrival was never reported (a PASS-only upstream gap, or
        // simply missing data) still gets an arrival estimate -- the two
        // fields are gated independently, not as a pair.
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-09T10:00:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-09T10:02:00Z".parse().unwrap()),
            actual_departure: Some("2026-09-09T10:02:00Z".parse().unwrap()),
            ..blank_stop()
        }];

        apply_delay_estimates(&mut stops, Some(3));

        assert_eq!(
            stops[0].estimated_arrival,
            Some("2026-09-09T10:03:00Z".parse().unwrap())
        );
        assert_eq!(
            stops[0].estimated_departure, None,
            "actual_departure is already confirmed"
        );
    }

    #[test]
    fn apply_delay_estimates_leaves_every_estimate_none_when_the_current_delay_is_unknown() {
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-09T10:00:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-09T10:02:00Z".parse().unwrap()),
            ..blank_stop()
        }];

        apply_delay_estimates(&mut stops, None);

        assert_eq!(stops[0].estimated_arrival, None);
        assert_eq!(stops[0].estimated_departure, None);
    }

    #[test]
    fn apply_delay_estimates_leaves_a_stop_with_no_scheduled_time_alone() {
        let mut stops = vec![blank_stop()];

        apply_delay_estimates(&mut stops, Some(10));

        assert_eq!(stops[0].estimated_arrival, None);
        assert_eq!(stops[0].estimated_departure, None);
    }

    #[test]
    fn may_have_arrived_is_false_within_the_threshold_of_the_final_stops_estimated_arrival() {
        let stops = vec![JourneyStop {
            estimated_arrival: Some("2026-09-09T10:00:00Z".parse().unwrap()),
            ..blank_stop()
        }];
        let now: DateTime<Utc> = "2026-09-09T10:10:00Z".parse().unwrap(); // 10m past
        assert!(!may_have_arrived(&stops, now));
    }

    #[test]
    fn may_have_arrived_is_true_more_than_the_threshold_past_the_final_stops_estimated_arrival() {
        let stops = vec![JourneyStop {
            estimated_arrival: Some("2026-09-09T10:00:00Z".parse().unwrap()),
            ..blank_stop()
        }];
        let now: DateTime<Utc> = "2026-09-09T10:20:00Z".parse().unwrap(); // 20m past
        assert!(may_have_arrived(&stops, now));
    }

    #[test]
    fn may_have_arrived_is_false_when_the_final_stop_has_no_estimate_at_all() {
        // Covers both a genuinely-already-arrived stop (a confirmed
        // actual_arrival means apply_delay_estimates never set an estimate)
        // and a stop with no schedule/delay data to estimate from.
        let stops = vec![blank_stop()];
        let now: DateTime<Utc> = "2026-09-09T10:20:00Z".parse().unwrap();
        assert!(!may_have_arrived(&stops, now));
    }

    #[test]
    fn may_have_arrived_is_false_for_an_empty_stop_list() {
        let now: DateTime<Utc> = "2026-09-09T10:20:00Z".parse().unwrap();
        assert!(!may_have_arrived(&[], now));
    }

    // --- Per-visit event assignment (`assign_events_to_stops`) ---

    fn stop_at(crs: &str, kind: schedule_query::CallingPointKind) -> JourneyStop {
        JourneyStop {
            crs: Some(crs.to_string()),
            kind: Some(kind),
            ..blank_stop()
        }
    }

    fn event(crs: &str, event_type: &str, at: &str) -> queries::MovementEventRow {
        queries::MovementEventRow {
            loc_crs: crs.to_string(),
            event_type: Some(event_type.to_string()),
            planned_timestamp: Some(at.parse().unwrap()),
            actual_timestamp: Some(at.parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
        }
    }

    /// The real production shape this fix exists for, trimmed to its
    /// load-bearing stops: South Western Railway's Kingston Loop (train
    /// L82877, 2026-09-14), which departs London Waterloo and terminates
    /// back at London Waterloo, calling at Clapham Junction twice on the
    /// way round. `WAT` is the FIRST and the LAST calling point; `CLJ`
    /// appears twice in the middle without the journey ending there.
    fn kingston_loop_stops() -> Vec<JourneyStop> {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        vec![
            stop_at("WAT", Origin),       // 0 -- 07:27 departure
            stop_at("CLJ", Intermediate), // 1 -- 07:36, outbound via Wimbledon
            stop_at("KNG", Intermediate), // 2 -- 07:58, the far side of the loop
            stop_at("RMD", Intermediate), // 3 -- 08:19, coming back via Richmond
            stop_at("CLJ", Intermediate), // 4 -- 08:35, the SECOND call here
            stop_at("WAT", Terminate),    // 5 -- 08:46 arrival, same CRS as stop 0
        ]
    }

    /// THE HEADLINE BUG. Before this fix the overlay keyed events by CRS,
    /// so the terminus's 08:49 ARRIVAL was copied onto the ORIGIN row too
    /// (and the origin's own 07:28 departure was thrown away) -- exactly
    /// what production served for L82877/2026-09-14. Each `WAT` call must
    /// now get its OWN event, resolved by position in the journey.
    #[test]
    fn assign_events_to_stops_gives_a_same_origin_terminus_loop_one_event_per_visit() {
        let stops = kingston_loop_stops();
        let events = vec![
            event("WAT", "DEPARTURE", "2026-09-14T06:28:00Z"),
            event("CLJ", "DEPARTURE", "2026-09-14T06:37:00Z"),
            event("KNG", "DEPARTURE", "2026-09-14T06:59:00Z"),
            event("RMD", "DEPARTURE", "2026-09-14T07:20:00Z"),
            event("CLJ", "DEPARTURE", "2026-09-14T07:36:00Z"),
            event("WAT", "ARRIVAL", "2026-09-14T07:49:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(
            assignment,
            vec![Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)],
            "each of the six reports belongs to exactly one of the six calls, in order"
        );
    }

    /// The same loop, read end to end: the ORIGIN must show a departure and
    /// NO arrival, and the TERMINUS the arrival -- the two facts the
    /// CRS-keyed overlay swapped.
    #[test]
    fn build_journey_overlay_puts_a_loops_arrival_on_the_terminus_not_the_origin() {
        let mut stops = kingston_loop_stops();
        let events = vec![
            event("WAT", "DEPARTURE", "2026-09-14T06:28:00Z"),
            event("WAT", "ARRIVAL", "2026-09-14T07:49:00Z"),
        ];

        overlay_movement_events(&mut stops, &events);

        assert_eq!(
            stops[0].actual_departure,
            Some("2026-09-14T06:28:00Z".parse().unwrap())
        );
        assert_eq!(
            stops[0].actual_arrival, None,
            "the origin never 'arrived' -- that report belongs to the terminus"
        );
        assert_eq!(
            stops[5].actual_arrival,
            Some("2026-09-14T07:49:00Z".parse().unwrap())
        );
        assert_eq!(stops[5].actual_departure, None);
    }

    /// THE FIX THIS EXISTS FOR. A PASS at a genuine booked calling point
    /// (`Intermediate`, with both a scheduled arrival AND departure) must
    /// NOT populate `actual_arrival`/`actual_departure` -- doing so used to
    /// make a train that ran straight through a station render identically
    /// to one that actually stopped there (`frontend/components/
    /// JourneyTimeline.tsx`'s `JourneyStopRow`: `reached = actual !== null`).
    /// `last_event_type` must still record `"PASS"` -- that signal is not
    /// lost, just no longer laundered into the `actual*` fields -- and
    /// `scheduled_arrival`/`scheduled_departure` are untouched (both were
    /// already `Some`, from the real timetable, so the `.or(...)` fallback
    /// is a no-op here anyway).
    #[test]
    fn overlay_movement_events_does_not_set_actual_times_for_a_pass_at_a_booked_stop() {
        use schedule_query::CallingPointKind::Intermediate;
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-14T09:10:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-14T09:11:00Z".parse().unwrap()),
            ..stop_at("SLO", Intermediate)
        }];
        let events = vec![event("SLO", "PASS", "2026-09-14T09:12:00Z")];

        overlay_movement_events(&mut stops, &events);

        assert_eq!(
            stops[0].actual_arrival, None,
            "a PASS at a booked stop must not read as a completed arrival"
        );
        assert_eq!(
            stops[0].actual_departure, None,
            "a PASS at a booked stop must not read as a completed departure"
        );
        assert_eq!(
            stops[0].last_event_type.as_deref(),
            Some("PASS"),
            "the PASS itself must still be recorded, for a follow-up 'Skipped' UI to key off"
        );
        assert_eq!(
            stops[0].scheduled_arrival,
            Some("2026-09-14T09:10:00Z".parse().unwrap()),
            "the real timetabled time is untouched"
        );
        assert_eq!(
            stops[0].scheduled_departure,
            Some("2026-09-14T09:11:00Z".parse().unwrap())
        );
    }

    /// The scoping half of the fix above: a PASS at a stop that is NOT a
    /// genuine two-sided booked call -- here, `Intermediate` but missing a
    /// scheduled arrival, the CIF shape of a timing point with blank public
    /// times (`schedule_query::parse::parse_calling_point`), never a real
    /// public calling point -- keeps the old behavior. There is no "booked
    /// but skipped" stop being misrepresented here, so nothing to fix.
    #[test]
    fn overlay_movement_events_still_sets_actual_times_for_a_pass_at_a_non_booked_point() {
        use schedule_query::CallingPointKind::Intermediate;
        let mut stops = vec![JourneyStop {
            scheduled_arrival: None,
            scheduled_departure: Some("2026-09-14T09:11:00Z".parse().unwrap()),
            ..stop_at("SLO", Intermediate)
        }];
        let events = vec![event("SLO", "PASS", "2026-09-14T09:12:00Z")];

        overlay_movement_events(&mut stops, &events);

        let expected: Option<DateTime<Utc>> = "2026-09-14T09:12:00Z".parse().ok();
        assert_eq!(stops[0].actual_arrival, expected);
        assert_eq!(stops[0].actual_departure, expected);
        assert_eq!(stops[0].last_event_type.as_deref(), Some("PASS"));
    }

    /// Same scoping, the other direction: an `Origin`/`Terminate` stop (no
    /// two-sided booked call to begin with -- `CallingPointKind`'s own doc
    /// comment) also keeps the old behavior on a PASS.
    #[test]
    fn overlay_movement_events_still_sets_actual_times_for_a_pass_at_an_origin() {
        use schedule_query::CallingPointKind::Origin;
        let mut stops = vec![stop_at("WAT", Origin)];
        let events = vec![event("WAT", "PASS", "2026-09-14T09:12:00Z")];

        overlay_movement_events(&mut stops, &events);

        let expected: Option<DateTime<Utc>> = "2026-09-14T09:12:00Z".parse().ok();
        assert_eq!(stops[0].actual_arrival, expected);
        assert_eq!(stops[0].actual_departure, expected);
    }

    // --- `apply_stop_status` (the "Skipped" follow-up) ---

    /// A booked calling point exactly like
    /// `overlay_movement_events_does_not_set_actual_times_for_a_pass_at_a_booked_stop`
    /// -- TRUST reported a PASS, and Darwin's own snapshot says nothing
    /// (empty `skipped_stations`). `StopStatus::Skipped` from the TRUST
    /// signal alone, softly-worded provenance (`SkipSource::Trust`), and
    /// `delay_minutes` -- which the PASS event's own actual-vs-planned diff
    /// populated -- is cleared per this function's own documented decision.
    #[test]
    fn apply_stop_status_marks_a_trust_pass_only_booked_stop_skipped_with_trust_source() {
        use schedule_query::CallingPointKind::Intermediate;
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-14T09:10:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-14T09:11:00Z".parse().unwrap()),
            ..stop_at("SLO", Intermediate)
        }];
        let events = vec![event("SLO", "PASS", "2026-09-14T09:12:00Z")];
        overlay_movement_events(&mut stops, &events);

        apply_stop_status(&mut stops, &[]);

        assert_eq!(stops[0].stop_status, StopStatus::Skipped);
        assert_eq!(stops[0].skip_source, Some(SkipSource::Trust));
        assert_eq!(
            stops[0].delay_minutes, None,
            "a skipped stop's delay_minutes must be suppressed, even though the PASS event \
             populated it"
        );
    }

    /// The Darwin-explicit counterpart: no TRUST event at all for this
    /// stop (a train that hasn't reached it yet, per real-time data), but
    /// the train's own captured Darwin snapshot names this CRS as skipped
    /// today. Must still be `Skipped`, sourced to `SkipSource::Darwin`
    /// alone -- this is the whole reason `skipped_stations` is threaded
    /// into this function at all, independent of any TRUST signal.
    #[test]
    fn apply_stop_status_marks_a_darwin_only_skip_skipped_with_darwin_source() {
        use schedule_query::CallingPointKind::Intermediate;
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-14T09:10:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-14T09:11:00Z".parse().unwrap()),
            ..stop_at("SLO", Intermediate)
        }];

        apply_stop_status(&mut stops, &["SLO".to_string()]);

        assert_eq!(stops[0].stop_status, StopStatus::Skipped);
        assert_eq!(stops[0].skip_source, Some(SkipSource::Darwin));
        assert_eq!(stops[0].delay_minutes, None);
    }

    /// Both signals agreeing -- a real TRUST PASS AND Darwin's own
    /// snapshot naming the same CRS -- sources to `SkipSource::Both`, not
    /// silently collapsed into either single-source variant.
    #[test]
    fn apply_stop_status_marks_agreeing_signals_skipped_with_both_source() {
        use schedule_query::CallingPointKind::Intermediate;
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-14T09:10:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-14T09:11:00Z".parse().unwrap()),
            ..stop_at("SLO", Intermediate)
        }];
        let events = vec![event("SLO", "PASS", "2026-09-14T09:12:00Z")];
        overlay_movement_events(&mut stops, &events);

        apply_stop_status(&mut stops, &["SLO".to_string()]);

        assert_eq!(stops[0].stop_status, StopStatus::Skipped);
        assert_eq!(stops[0].skip_source, Some(SkipSource::Both));
    }

    /// The ordinary, overwhelmingly common case: a booked stop the train
    /// genuinely called at (a reported ARRIVAL), with no skip signal from
    /// either source. Must be `Called`, never `Skipped`, and `skip_source`
    /// stays `None`.
    #[test]
    fn apply_stop_status_marks_a_normal_called_stop_called_with_no_skip_source() {
        use schedule_query::CallingPointKind::Intermediate;
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-14T09:10:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-14T09:11:00Z".parse().unwrap()),
            ..stop_at("SLO", Intermediate)
        }];
        let events = vec![event("SLO", "ARRIVAL", "2026-09-14T09:10:30Z")];
        overlay_movement_events(&mut stops, &events);

        apply_stop_status(&mut stops, &[]);

        assert_eq!(stops[0].stop_status, StopStatus::Called);
        assert_eq!(stops[0].skip_source, None);
        assert!(
            stops[0].delay_minutes.is_some(),
            "a genuinely-called stop's real delay_minutes must NOT be cleared -- only a \
             skipped stop's is"
        );
    }

    /// The case this whole feature must not get wrong in the other
    /// direction: a booked stop with NO signal at all yet (no TRUST event,
    /// not in Darwin's snapshot) must be `Scheduled` -- "hasn't happened
    /// yet" is not the same fact as "confirmed skipped", and conflating
    /// them would falsely accuse an ordinary future stop of being skipped.
    #[test]
    fn apply_stop_status_leaves_a_not_yet_reached_booked_stop_as_scheduled_not_skipped() {
        use schedule_query::CallingPointKind::Intermediate;
        let mut stops = vec![JourneyStop {
            scheduled_arrival: Some("2026-09-14T09:10:00Z".parse().unwrap()),
            scheduled_departure: Some("2026-09-14T09:11:00Z".parse().unwrap()),
            ..stop_at("SLO", Intermediate)
        }];

        apply_stop_status(&mut stops, &[]);

        assert_eq!(stops[0].stop_status, StopStatus::Scheduled);
        assert_eq!(stops[0].skip_source, None);
    }

    /// The `Unknown` gate: an `Origin`/`Terminate` stop, or an
    /// `Intermediate` one missing a scheduled time, is never eligible for
    /// `Skipped` at all -- even a matching Darwin `skipped_stations` entry
    /// must not flip it, because `overlay_movement_events`'s own
    /// `booked_calling_point` gate (the one this function mirrors exactly)
    /// would never have suppressed `actual_arrival`/`actual_departure` for
    /// it in the first place.
    #[test]
    fn apply_stop_status_leaves_a_non_booked_stop_unknown_even_with_a_matching_darwin_entry() {
        use schedule_query::CallingPointKind::Origin;
        let mut stops = vec![stop_at("WAT", Origin)];

        apply_stop_status(&mut stops, &["WAT".to_string()]);

        assert_eq!(stops[0].stop_status, StopStatus::Unknown);
        assert_eq!(stops[0].skip_source, None);
    }

    /// The inverse, and the reason a "has it finished?" check can't just
    /// read the last stop's timestamps blind: while a loop train is still
    /// out on the circuit, the CRS-keyed overlay put its ORIGIN departure
    /// on the TERMINUS row, which read as "the train is at its final stop".
    #[test]
    fn assign_events_to_stops_leaves_a_loops_terminus_unreported_until_the_train_returns() {
        let stops = kingston_loop_stops();
        let events = vec![
            event("WAT", "DEPARTURE", "2026-09-14T06:28:00Z"),
            event("CLJ", "DEPARTURE", "2026-09-14T06:37:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(assignment[0], Some(0), "the origin's own departure");
        assert_eq!(assignment[1], Some(1), "the first call at Clapham Junction");
        assert_eq!(
            assignment[4], None,
            "the second call at Clapham Junction hasn't happened yet"
        );
        assert_eq!(
            assignment[5], None,
            "nothing has been reported at the terminus -- the train is still out on the loop"
        );
    }

    /// A CRS visited twice that is NOT the terminus: the two calls at
    /// Clapham Junction, an hour apart, must not share one event either.
    #[test]
    fn assign_events_to_stops_separates_two_calls_at_one_intermediate_station() {
        let stops = kingston_loop_stops();
        let events = vec![
            event("CLJ", "ARRIVAL", "2026-09-14T06:36:00Z"),
            event("CLJ", "DEPARTURE", "2026-09-14T06:37:00Z"),
            event("KNG", "DEPARTURE", "2026-09-14T06:59:00Z"),
            event("CLJ", "ARRIVAL", "2026-09-14T07:35:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(
            assignment[1],
            Some(1),
            "the FIRST call keeps its own latest report (the 06:37 departure)"
        );
        assert_eq!(
            assignment[4],
            Some(3),
            "the SECOND call gets the 07:35 arrival, not the first call's departure"
        );
    }

    /// Regression: a station called at exactly once still collapses to its
    /// single latest-reported event -- the `DISTINCT ON (UPPER(loc_crs))
    /// ORDER BY received_at DESC` behaviour this replaced, unchanged.
    #[test]
    fn assign_events_to_stops_collapses_a_single_call_to_its_latest_event() {
        use schedule_query::CallingPointKind::{Origin, Terminate};
        let stops = vec![stop_at("RDG", Origin), stop_at("PAD", Terminate)];
        let events = vec![
            event("RDG", "ARRIVAL", "2026-09-14T09:15:00Z"),
            event("RDG", "DEPARTURE", "2026-09-14T09:20:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(assignment, vec![Some(1), None]);
    }

    /// Regression: an ordinary A-to-B journey is assigned exactly as it
    /// always was, one event per stop, in order.
    #[test]
    fn assign_events_to_stops_handles_a_plain_non_loop_journey() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let stops = vec![
            stop_at("RDG", Origin),
            stop_at("SLO", Intermediate),
            stop_at("PAD", Terminate),
        ];
        let events = vec![
            event("RDG", "DEPARTURE", "2026-09-14T09:00:00Z"),
            event("SLO", "PASS", "2026-09-14T09:12:00Z"),
            event("PAD", "ARRIVAL", "2026-09-14T09:30:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(assignment, vec![Some(0), Some(1), Some(2)]);
    }

    /// Regression, design doc §3.3: an event for a location no stop
    /// carries (an unscheduled diversion, or a STANOX that translated to a
    /// CRS neither schedule source names) is silently merged into nothing
    /// -- never onto the nearest stop.
    #[test]
    fn assign_events_to_stops_drops_an_event_for_a_crs_the_journey_never_calls_at() {
        use schedule_query::CallingPointKind::{Origin, Terminate};
        let stops = vec![stop_at("RDG", Origin), stop_at("PAD", Terminate)];
        let events = vec![
            event("RDG", "DEPARTURE", "2026-09-14T09:00:00Z"),
            event("ZZZ", "PASS", "2026-09-14T09:10:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(assignment, vec![Some(0), None]);
    }

    /// A straggling report for a station the train has already left lands
    /// back on THAT station, rather than being dropped or dragged forward
    /// -- the `rposition` fallback. `RDG` appears once here, so the only
    /// candidate is behind the cursor.
    #[test]
    fn assign_events_to_stops_puts_a_straggling_report_back_on_the_stop_it_names() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let stops = vec![
            stop_at("RDG", Origin),
            stop_at("SLO", Intermediate),
            stop_at("PAD", Terminate),
        ];
        let events = vec![
            event("RDG", "DEPARTURE", "2026-09-14T09:00:00Z"),
            event("PAD", "ARRIVAL", "2026-09-14T09:30:00Z"),
            // A corrected Reading report, timestamped after the arrival at
            // Paddington -- it still describes Reading.
            event("RDG", "PASS", "2026-09-14T09:35:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(assignment, vec![Some(2), None, Some(1)]);
    }

    /// The CRS comparison is case-insensitive on both sides, same posture
    /// as every other CRS comparison in this codebase -- the SQL upper-cases
    /// `loc_crs`, but a unit caller (or a future source) may not.
    #[test]
    fn assign_events_to_stops_matches_crs_case_insensitively() {
        use schedule_query::CallingPointKind::{Origin, Terminate};
        let stops = vec![stop_at("rdg", Origin), stop_at("PAD", Terminate)];
        let events = vec![event("RDG", "DEPARTURE", "2026-09-14T09:00:00Z")];

        assert_eq!(assign_events_to_stops(&stops, &events), vec![Some(0), None]);
    }

    /// REVIEW FINDING, and the nastiest case here: a DUPLICATE of the
    /// origin's own departure must not be read as "the train departed a
    /// second time" and walked forward onto the terminus -- which would
    /// re-create, from a duplicate, the very bug this function exists to
    /// fix.
    ///
    /// Duplicates are routine in this data model, not hypothetical.
    /// `trust_schema::dedup::dedup_key` hashes `loc_stanox`, which the live
    /// consumer supplies and
    /// `trust_event_backlog_match::replay_backlog_history` explicitly does
    /// not, so one real event written down both paths lands as two
    /// `train_movement_events` rows under two different dedup keys.
    #[test]
    fn assign_events_to_stops_keeps_a_duplicated_origin_departure_off_the_terminus() {
        let stops = kingston_loop_stops();
        let events = vec![
            event("WAT", "DEPARTURE", "2026-09-14T06:28:00Z"),
            // The same real-world departure, written again by the other
            // ingest path a few seconds later.
            event("WAT", "DEPARTURE", "2026-09-14T06:28:04Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(
            assignment[0],
            Some(1),
            "both reports are the origin's one departure; the later-received one wins"
        );
        assert_eq!(
            assignment[5], None,
            "the train is still sitting at Waterloo -- nothing may reach the terminus"
        );
    }

    /// REVIEW FINDING: a corrected report of a type a stop has already
    /// recorded, while that visit is still open (no departure yet), must be
    /// APPLIED, not dropped.
    #[test]
    fn assign_events_to_stops_applies_a_corrected_repeat_of_an_already_recorded_type() {
        use schedule_query::CallingPointKind::{Origin, Terminate};
        let stops = vec![stop_at("RDG", Origin), stop_at("PAD", Terminate)];
        let events = vec![
            event("RDG", "ARRIVAL", "2026-09-14T09:15:00Z"),
            event("RDG", "ARRIVAL", "2026-09-14T09:16:00Z"),
        ];

        assert_eq!(assign_events_to_stops(&stops, &events), vec![Some(1), None]);
    }

    /// REVIEW FINDING, and the reason grouping and winner-selection use
    /// DIFFERENT orderings: the winner within a visit is the
    /// latest-RECEIVED report, exactly as the old `ORDER BY received_at
    /// DESC` collapse chose it. A correction that revises a timestamp
    /// BACKWARDS still supersedes the original, even though it now sorts
    /// earlier by instant.
    #[test]
    fn assign_events_to_stops_lets_the_latest_received_report_win_even_if_it_moves_time_backwards()
    {
        use schedule_query::CallingPointKind::{Origin, Terminate};
        let stops = vec![stop_at("RDG", Origin), stop_at("PAD", Terminate)];
        let events = vec![
            event("RDG", "DEPARTURE", "2026-09-14T09:20:00Z"),
            // Received second, but reporting an EARLIER actual time.
            event("RDG", "DEPARTURE", "2026-09-14T09:18:00Z"),
        ];

        assert_eq!(
            assign_events_to_stops(&stops, &events),
            vec![Some(1), None],
            "received order picks the winner; timestamps only group visits"
        );
    }

    /// The revisit gap comes from the SCHEDULE, not from one hard-coded
    /// number: two calls booked only 20 minutes apart (closer together than
    /// `DEFAULT_REVISIT_GAP`) still split correctly, because half their own
    /// booked interval is the threshold.
    #[test]
    fn assign_events_to_stops_derives_the_revisit_gap_from_the_schedule() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let scheduled = |mut stop: JourneyStop, at: &str| {
            stop.scheduled_arrival = Some(at.parse().unwrap());
            stop
        };
        let stops = vec![
            scheduled(stop_at("AAA", Origin), "2026-09-14T09:00:00Z"),
            scheduled(stop_at("BBB", Intermediate), "2026-09-14T09:10:00Z"),
            scheduled(stop_at("AAA", Intermediate), "2026-09-14T09:20:00Z"),
            scheduled(stop_at("CCC", Terminate), "2026-09-14T09:30:00Z"),
        ];
        let events = vec![
            event("AAA", "ARRIVAL", "2026-09-14T09:00:00Z"),
            event("AAA", "DEPARTURE", "2026-09-14T09:01:00Z"),
            event("BBB", "DEPARTURE", "2026-09-14T09:11:00Z"),
            event("AAA", "ARRIVAL", "2026-09-14T09:20:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(assignment[0], Some(1), "the first call's own departure");
        assert_eq!(assignment[1], Some(2));
        assert_eq!(
            assignment[2],
            Some(3),
            "a 19-minute gap is a second visit here, because the schedule says these two calls are \
             20 minutes apart -- DEFAULT_REVISIT_GAP alone would have merged them"
        );
        assert_eq!(assignment[3], None);
    }

    /// A station called at THREE times, each with its own arrival and
    /// departure, resolves to three separate visits in order.
    #[test]
    fn assign_events_to_stops_separates_three_calls_at_one_station() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let stops = vec![
            stop_at("AAA", Origin),
            stop_at("BBB", Intermediate),
            stop_at("AAA", Intermediate),
            stop_at("CCC", Intermediate),
            stop_at("AAA", Terminate),
        ];
        let events = vec![
            event("AAA", "DEPARTURE", "2026-09-14T09:00:00Z"),
            event("BBB", "DEPARTURE", "2026-09-14T09:30:00Z"),
            event("AAA", "ARRIVAL", "2026-09-14T10:00:00Z"),
            event("AAA", "DEPARTURE", "2026-09-14T10:02:00Z"),
            event("CCC", "DEPARTURE", "2026-09-14T10:30:00Z"),
            event("AAA", "ARRIVAL", "2026-09-14T11:00:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(
            assignment,
            vec![Some(0), Some(1), Some(3), Some(4), Some(5)],
            "three visits to AAA, one per call, the middle one keeping its own departure"
        );
    }

    /// An event carrying neither an actual nor a planned timestamp can't be
    /// placed by time, so it joins the last visit -- keeping the old
    /// "latest reported event wins" behaviour rather than inventing a
    /// position for it.
    #[test]
    fn assign_events_to_stops_folds_an_untimed_report_into_the_latest_visit() {
        use schedule_query::CallingPointKind::{Origin, Terminate};
        let stops = vec![stop_at("RDG", Origin), stop_at("PAD", Terminate)];
        let untimed = queries::MovementEventRow {
            loc_crs: "RDG".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
        };
        let events = vec![event("RDG", "ARRIVAL", "2026-09-14T09:15:00Z"), untimed];

        assert_eq!(assign_events_to_stops(&stops, &events), vec![Some(1), None]);
    }

    /// An event with no `event_type` at all is still assigned (the overlay
    /// loop's own `_ => {}` arm then declines to derive any times from it)
    /// -- grouping never depends on the type.
    #[test]
    fn assign_events_to_stops_still_places_an_event_with_no_type() {
        use schedule_query::CallingPointKind::{Origin, Terminate};
        let stops = vec![stop_at("RDG", Origin), stop_at("PAD", Terminate)];
        let mut typeless = event("RDG", "DEPARTURE", "2026-09-14T09:20:00Z");
        typeless.event_type = None;
        let events = vec![typeless];

        assert_eq!(assign_events_to_stops(&stops, &events), vec![Some(0), None]);
    }

    /// A stop whose TIPLOC never resolved to a CRS (eight of L82877's
    /// thirty) is never a match target and never absorbs another stop's
    /// event.
    #[test]
    fn assign_events_to_stops_skips_a_stop_with_no_resolved_crs() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let stops = vec![
            stop_at("RDG", Origin),
            JourneyStop {
                crs: None,
                kind: Some(Intermediate),
                ..blank_stop()
            },
            stop_at("PAD", Terminate),
        ];
        let events = vec![
            event("RDG", "DEPARTURE", "2026-09-14T09:00:00Z"),
            event("PAD", "ARRIVAL", "2026-09-14T09:30:00Z"),
        ];

        assert_eq!(
            assign_events_to_stops(&stops, &events),
            vec![Some(0), None, Some(1)]
        );
    }

    /// A station called at three times, 60 minutes apart, where the train
    /// is held on the platform at the FIRST call for 40 minutes -- longer
    /// than the 30-minute `revisit_gap` that spacing derives. The gap alone
    /// would cut that one dwell into two visits, the surplus would consume
    /// the middle call's slot, and the middle call's ARRIVAL would cascade
    /// onto the TERMINUS -- where `confirmed_final_arrival` would read a
    /// train still sitting at its second call as having finished its
    /// journey. `rejoin_split_dwells` prevents the split in the first place.
    #[test]
    fn assign_events_to_stops_does_not_let_a_long_dwell_cascade_onto_the_terminus() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let scheduled = |mut stop: JourneyStop, at: &str| {
            stop.scheduled_arrival = Some(at.parse().unwrap());
            stop
        };
        let stops = vec![
            scheduled(stop_at("AAA", Origin), "2026-09-14T09:00:00Z"),
            scheduled(stop_at("BBB", Intermediate), "2026-09-14T09:50:00Z"),
            scheduled(stop_at("AAA", Intermediate), "2026-09-14T10:00:00Z"),
            scheduled(stop_at("CCC", Intermediate), "2026-09-14T10:30:00Z"),
            scheduled(stop_at("AAA", Terminate), "2026-09-14T11:00:00Z"),
        ];
        let events = vec![
            event("AAA", "ARRIVAL", "2026-09-14T09:00:00Z"),
            // Held on the platform for forty minutes.
            event("AAA", "DEPARTURE", "2026-09-14T09:40:00Z"),
            event("BBB", "DEPARTURE", "2026-09-14T09:52:00Z"),
            // The train is at its SECOND call and has not left yet. Nothing
            // at all has been reported at the terminus.
            event("AAA", "ARRIVAL", "2026-09-14T10:30:00Z"),
        ];

        let mut overlaid = stops.clone();
        overlay_movement_events(&mut overlaid, &events);

        assert_eq!(
            assign_events_to_stops(&stops, &events),
            vec![Some(1), Some(2), Some(3), None, None],
            "the 40-minute dwell is one call, so the second call keeps its own arrival"
        );
        assert!(
            !confirmed_final_arrival(&overlaid),
            "the terminus has reported nothing; the train is still running"
        );
        assert_eq!(
            apply_confirmed_arrival(Some("en_route".to_string()), Some(&overlaid)),
            Some("en_route".to_string())
        );
    }

    /// The reverse order is NOT a dwell and must stay two visits: a loop's
    /// origin DEPARTURE followed an hour later by its terminus ARRIVAL is
    /// exactly the shape `rejoin_split_dwells` must never merge.
    #[test]
    fn assign_events_to_stops_never_rejoins_a_departure_followed_by_an_arrival() {
        let stops = kingston_loop_stops();
        let events = vec![
            event("WAT", "DEPARTURE", "2026-09-14T06:28:00Z"),
            event("WAT", "ARRIVAL", "2026-09-14T07:49:00Z"),
        ];

        assert_eq!(
            assign_events_to_stops(&stops, &events),
            vec![Some(0), None, None, None, None, Some(1)]
        );
    }

    /// A station called at three times where the MIDDLE call has reported
    /// nothing yet: the reports that do exist keep their own calls, and
    /// nothing is invented for the one that is silent.
    #[test]
    fn assign_events_to_stops_tolerates_a_silent_middle_call() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let stops = vec![
            stop_at("AAA", Origin),
            stop_at("BBB", Intermediate),
            stop_at("AAA", Intermediate),
            stop_at("CCC", Intermediate),
            stop_at("AAA", Terminate),
        ];
        let events = vec![
            event("AAA", "DEPARTURE", "2026-09-14T09:00:00Z"),
            event("BBB", "DEPARTURE", "2026-09-14T09:30:00Z"),
            // Nothing at all from the second call at AAA.
            event("CCC", "DEPARTURE", "2026-09-14T10:30:00Z"),
            event("AAA", "ARRIVAL", "2026-09-14T11:00:00Z"),
        ];

        assert_eq!(
            assign_events_to_stops(&stops, &events),
            vec![Some(0), Some(1), None, Some(2), Some(3)],
            "the terminus arrival still reaches the terminus past an unreported middle call"
        );
    }

    /// An untimed report is the one thing that cannot be placed in time, so
    /// it must never be allowed to go FIRST and drag the cursor with it --
    /// `Option`'s own ordering sorts `None` before every real instant, which
    /// it would have done here.
    #[test]
    fn assign_events_to_stops_never_lets_an_untimed_report_jump_the_queue() {
        let stops = kingston_loop_stops();
        let untimed_clj = queries::MovementEventRow {
            loc_crs: "CLJ".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
        };
        let events = vec![
            untimed_clj,
            event("WAT", "DEPARTURE", "2026-09-14T06:28:00Z"),
            event("KNG", "DEPARTURE", "2026-09-14T06:59:00Z"),
            event("WAT", "ARRIVAL", "2026-09-14T07:49:00Z"),
        ];

        let assignment = assign_events_to_stops(&stops, &events);

        assert_eq!(assignment[0], Some(1), "the origin keeps its own departure");
        assert_eq!(assignment[2], Some(2));
        assert_eq!(
            assignment[5],
            Some(3),
            "the terminus arrival still lands on the terminus"
        );
    }

    /// An unexplained extra visit at a station the schedule has no call left
    /// for is dropped rather than overwriting the final call -- which, at a
    /// terminus, could otherwise put another call's ARRIVAL there and make
    /// `confirmed_final_arrival` read a still-running train as finished.
    #[test]
    fn assign_events_to_stops_drops_a_surplus_visit_rather_than_overwriting_the_last_call() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let stops = vec![
            stop_at("AAA", Origin),
            stop_at("BBB", Intermediate),
            stop_at("AAA", Terminate),
        ];
        let events = vec![
            event("AAA", "DEPARTURE", "2026-09-14T09:00:00Z"),
            event("BBB", "DEPARTURE", "2026-09-14T09:30:00Z"),
            event("AAA", "ARRIVAL", "2026-09-14T10:00:00Z"),
            // Empty stock running back out through the terminus an hour
            // later -- a third visit the schedule has no call for.
            event("AAA", "PASS", "2026-09-14T11:00:00Z"),
        ];

        let mut overlaid = stops.clone();
        overlay_movement_events(&mut overlaid, &events);

        assert_eq!(
            assign_events_to_stops(&stops, &events),
            vec![Some(0), Some(1), Some(2)],
            "the terminus keeps its real ARRIVAL; the unexplained fourth report is dropped"
        );
        assert!(
            confirmed_final_arrival(&overlaid),
            "the train really did arrive; a later stock move must not erase that"
        );
    }

    /// `rejoin_split_dwells`'s known limit, pinned rather than papered over:
    /// two genuinely separate calls, the first having lost its DEPARTURE
    /// report and the second its ARRIVAL, look exactly like one long dwell
    /// and are rejoined into a single visit, so one of the two calls shows
    /// nothing. What this test exists to guarantee is the BOUND on that: it
    /// costs a missing report, never an invented arrival, so the journey can
    /// still never read as finished when it isn't.
    #[test]
    fn assign_events_to_stops_rejoin_limit_costs_a_report_but_never_a_false_arrival() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let stops = vec![
            stop_at("ORG", Origin),
            stop_at("AAA", Intermediate),
            stop_at("BBB", Intermediate),
            stop_at("AAA", Intermediate),
            stop_at("TRM", Terminate),
        ];
        let events = vec![
            event("ORG", "DEPARTURE", "2026-09-14T08:30:00Z"),
            // The first call at AAA loses its departure...
            event("AAA", "ARRIVAL", "2026-09-14T09:00:00Z"),
            event("BBB", "DEPARTURE", "2026-09-14T09:30:00Z"),
            // ...and the second loses its arrival.
            event("AAA", "DEPARTURE", "2026-09-14T10:01:00Z"),
        ];

        let mut overlaid = stops.clone();
        overlay_movement_events(&mut overlaid, &events);

        assert_eq!(
            assign_events_to_stops(&stops, &events),
            vec![Some(0), Some(3), Some(2), None, None],
            "the two half-reported calls rejoin into one, so the SECOND shows nothing and the \
             first carries the merged visit's latest report -- the known limit"
        );
        assert!(
            !confirmed_final_arrival(&overlaid),
            "the terminus reported nothing, and nothing may fabricate an arrival there"
        );
    }

    /// The mirror of the case above: `split_into_visits` appends an untimed
    /// report to the last group, so an untimed report of a different type
    /// can stop a rejoin that should have happened. Same bound -- the
    /// terminus ends up with an untimed winner and therefore no
    /// `actual_arrival`, so no false arrival can come of it.
    #[test]
    fn assign_events_to_stops_an_untimed_report_may_suppress_a_rejoin_but_cannot_fake_arrival() {
        use schedule_query::CallingPointKind::{Intermediate, Origin, Terminate};
        let stops = vec![
            stop_at("AAA", Origin),
            stop_at("BBB", Intermediate),
            stop_at("AAA", Intermediate),
            stop_at("CCC", Intermediate),
            stop_at("AAA", Terminate),
        ];
        let events = vec![
            event("AAA", "DEPARTURE", "2026-09-14T09:00:00Z"),
            event("BBB", "DEPARTURE", "2026-09-14T09:20:00Z"),
            event("AAA", "ARRIVAL", "2026-09-14T10:00:00Z"),
            event("AAA", "DEPARTURE", "2026-09-14T10:40:00Z"),
            queries::MovementEventRow {
                loc_crs: "AAA".to_string(),
                event_type: Some("ARRIVAL".to_string()),
                planned_timestamp: None,
                actual_timestamp: None,
                variation_status: None,
            },
        ];

        let mut overlaid = stops.clone();
        overlay_movement_events(&mut overlaid, &events);

        assert!(
            !confirmed_final_arrival(&overlaid),
            "an untimed report carries no actual_arrival, so it can never confirm an arrival"
        );
        assert_eq!(
            apply_confirmed_arrival(Some("en_route".to_string()), Some(&overlaid)),
            Some("en_route".to_string())
        );
    }

    #[test]
    fn assign_events_to_stops_handles_empty_inputs() {
        assert!(assign_events_to_stops(&[], &[]).is_empty());
        assert!(
            assign_events_to_stops(&[], &[event("RDG", "ARRIVAL", "2026-09-14T09:00:00Z")])
                .is_empty()
        );
        assert_eq!(
            assign_events_to_stops(&kingston_loop_stops(), &[]),
            vec![None; 6]
        );
    }

    // --- Sequence-anchored confirmed arrival ---

    fn arrived(stop: JourneyStop, at: &str) -> JourneyStop {
        JourneyStop {
            actual_arrival: Some(at.parse().unwrap()),
            last_event_type: Some("ARRIVAL".to_string()),
            ..stop
        }
    }

    #[test]
    fn confirmed_final_arrival_is_true_for_a_loop_that_got_back_to_its_own_origin_crs() {
        let mut stops = kingston_loop_stops();
        let last = stops.len() - 1;
        stops[last] = arrived(stops[last].clone(), "2026-09-14T07:49:00Z");

        assert!(confirmed_final_arrival(&stops));
    }

    /// The distinction the CRS-only check cannot make: an ARRIVAL at the
    /// journey's FIRST call at `WAT` is not the journey finishing, even
    /// though that stop's CRS is identical to the terminus's.
    #[test]
    fn confirmed_final_arrival_ignores_an_arrival_at_an_earlier_stop_sharing_the_terminus_crs() {
        let mut stops = kingston_loop_stops();
        stops[0] = arrived(stops[0].clone(), "2026-09-14T06:20:00Z");

        assert!(
            !confirmed_final_arrival(&stops),
            "position in the journey decides this, never the CRS code"
        );
    }

    /// Mirrors `trust_schema::journey::apply_movement`'s own rule: only an
    /// ARRIVAL confirms a finished journey. Empty stock running through the
    /// terminus's own location reports a PASS, and must not.
    #[test]
    fn confirmed_final_arrival_is_false_for_a_pass_through_the_terminus() {
        let mut stops = kingston_loop_stops();
        let last = stops.len() - 1;
        stops[last].actual_arrival = Some("2026-09-14T07:49:00Z".parse().unwrap());
        stops[last].last_event_type = Some("PASS".to_string());

        assert!(!confirmed_final_arrival(&stops));
    }

    #[test]
    fn confirmed_final_arrival_is_false_for_an_unreported_terminus_or_no_stops_at_all() {
        assert!(!confirmed_final_arrival(&kingston_loop_stops()));
        assert!(!confirmed_final_arrival(&[]));
    }

    /// The end-to-end fix for the reported bug: a stored `'en_route'` on a
    /// loop service whose terminus has a confirmed arrival now reads as
    /// `'completed'`.
    #[test]
    fn apply_confirmed_arrival_completes_a_stuck_en_route_loop_service() {
        let mut stops = kingston_loop_stops();
        let last = stops.len() - 1;
        stops[last] = arrived(stops[last].clone(), "2026-09-14T07:49:00Z");

        assert_eq!(
            apply_confirmed_arrival(Some("en_route".to_string()), Some(&stops)),
            Some("completed".to_string())
        );
    }

    #[test]
    fn apply_confirmed_arrival_leaves_a_train_that_is_genuinely_still_running_alone() {
        let stops = kingston_loop_stops();
        assert_eq!(
            apply_confirmed_arrival(Some("en_route".to_string()), Some(&stops)),
            Some("en_route".to_string())
        );
    }

    /// Upgrade-only, and only from `'en_route'`: no other status may be
    /// rewritten, and an absent timeline can never be read as evidence
    /// either way.
    #[test]
    fn apply_confirmed_arrival_never_rewrites_any_other_status() {
        let mut stops = kingston_loop_stops();
        let last = stops.len() - 1;
        stops[last] = arrived(stops[last].clone(), "2026-09-14T07:49:00Z");

        for status in ["cancelled", "awaiting_activation", "completed"] {
            assert_eq!(
                apply_confirmed_arrival(Some(status.to_string()), Some(&stops)),
                Some(status.to_string()),
                "{status} must survive the overlay untouched"
            );
        }
        assert_eq!(apply_confirmed_arrival(None, Some(&stops)), None);
        assert_eq!(
            apply_confirmed_arrival(Some("en_route".to_string()), None),
            Some("en_route".to_string()),
            "no journey timeline is not evidence a train did NOT arrive"
        );
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_from_calling_points_json_resolves_tiploc_to_crs_and_kind \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_from_calling_points_json_resolves_tiploc_to_crs_and_kind() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-CP", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-1".to_string(),
                    crs: "EUS".to_string(),
                    tiploc: "TEST-JRN-EUSTON".to_string(),
                    station_name: "EUSTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-2".to_string(),
                    crs: "CRE".to_string(),
                    tiploc: "TEST-JRN-CREWE".to_string(),
                    station_name: "CREWE".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-EUSTON",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-CREWE",
                "kind": "Terminate",
                "bookedArrival": "10:30:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-CP",
            service_date,
            Some(&calling_points),
            None,
            &[],
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].crs.as_deref(), Some("EUS"));
        assert_eq!(
            stops[0].kind,
            Some(schedule_query::CallingPointKind::Origin)
        );
        assert!(stops[0].scheduled_departure.is_some());
        assert_eq!(stops[1].crs.as_deref(), Some("CRE"));
        assert_eq!(
            stops[1].kind,
            Some(schedule_query::CallingPointKind::Terminate)
        );
        assert!(stops[1].scheduled_arrival.is_some());

        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// End-to-end (real Postgres) counterpart to the pure
    /// `every_short_padded_tiploc_on_the_real_kingston_loop_journey_resolves_to_its_crs`
    /// unit test:
    /// proves the whole `crs_for_tiplocs_batch` round-trip -- the SQL
    /// `UPPER(TRIM(tiploc))` on the stored side AND `tiploc_key` on the
    /// Rust side -- resolves a real, sub-7-character, space-padded schedule
    /// TIPLOC. The stored `stanox_crs.tiploc` is written UNPADDED here
    /// because that is exactly what `schedule-reference`'s `parse_ti_lines`
    /// (`line[2..9].trim()`) writes, which is the whole asymmetry the live
    /// bug came from.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_resolves_a_space_padded_sub_seven_char_tiploc \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_resolves_a_space_padded_sub_seven_char_tiploc() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-14".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-PAD", service_date)
                .await
                .expect("find_or_create_train");

        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-JRN-PAD-%'")
            .execute(&pool)
            .await
            .ok();

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-PAD-1".to_string(),
                    crs: "WAT".to_string(),
                    // Exactly 7 characters -- resolved even before the fix.
                    tiploc: "WATRLMN".to_string(),
                    station_name: "LONDON WATERLOO".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-PAD-2".to_string(),
                    crs: "PUT".to_string(),
                    // 6 characters, stored trimmed exactly as
                    // `parse_ti_lines` writes it -- the failing case.
                    tiploc: "PUTNEY".to_string(),
                    station_name: "PUTNEY".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        // TIPLOCs exactly as the CIF schedule body carries them and as
        // `ScheduleCallingPointDto` stores them: the fixed 7-character,
        // space-padded field.
        let calling_points = serde_json::json!([
            {
                "tiploc": "WATRLMN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "07:27:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "PUTNEY ",
                "kind": "Terminate",
                "bookedArrival": "08:26:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-PAD",
            service_date,
            Some(&calling_points),
            None,
            &[],
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].crs.as_deref(), Some("WAT"));
        assert_eq!(
            stops[1].crs.as_deref(),
            Some("PUT"),
            "a space-padded 6-character TIPLOC must resolve against its trimmed \
             stanox_crs row; before this fix it came back None and the page rendered \
             \"Unknown location\""
        );
        assert_eq!(
            stops[1].tiploc.as_deref(),
            Some("PUTNEY"),
            "the emitted wire tiploc must be the bare code, not the padded field"
        );

        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-JRN-PAD-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_falls_back_to_schedule_destination_departures_and_appends_terminus \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_falls_back_to_schedule_destination_departures_and_appends_terminus()
     {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-FB", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-FB'")
            .execute(&pool)
            .await
            .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[
                crate::data::queries::ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "08:00:00".parse().unwrap(),
                    day_offset: 0,
                    train_uid: "TEST-JRN-FB".to_string(),
                    origin_crs: "RDG".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
                crate::data::queries::ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "08:20:00".parse().unwrap(),
                    day_offset: 0,
                    train_uid: "TEST-JRN-FB".to_string(),
                    origin_crs: "SLO".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
            ],
        )
        .await
        .expect("seed schedule_destination_departures");

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-FB", service_date, None, None, &[])
            .await
            .expect("build_journey_stops")
            .expect("Some stops from the fallback source");

        assert_eq!(stops.len(), 3, "RDG + SLO + synthetic WAT terminus");
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(
            stops[0].kind,
            Some(schedule_query::CallingPointKind::Origin)
        );
        assert_eq!(stops[1].crs.as_deref(), Some("SLO"));
        assert_eq!(
            stops[1].kind,
            Some(schedule_query::CallingPointKind::Intermediate)
        );
        assert_eq!(stops[2].crs.as_deref(), Some("WAT"));
        assert_eq!(
            stops[2].kind,
            Some(schedule_query::CallingPointKind::Terminate)
        );
        assert!(
            stops[2].scheduled_arrival.is_none(),
            "no arrival time known from this source yet"
        );

        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-FB'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_returns_none_when_neither_source_has_anything \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_returns_none_when_neither_source_has_anything() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NONE", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-NONE'",
        )
        .execute(&pool)
        .await
        .ok();

        let stops =
            build_journey_stops(&pool, trains_id, "TEST-JRN-NONE", service_date, None, None, &[])
                .await
                .expect("build_journey_stops");

        assert!(stops.is_none());

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_overlays_a_departure_event_with_correct_delay_sign \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_overlays_a_departure_event_with_correct_delay_sign() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-OV", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-OV'")
            .execute(&pool)
            .await
            .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[crate::data::queries::ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "08:00:00".parse().unwrap(),
                day_offset: 0,
                train_uid: "TEST-JRN-OV".to_string(),
                origin_crs: "RDG".to_string(),
                true_origin_crs: Some("RDG".to_string()),
                calling_point_arrival: None,
                destination_arrival: None,
                destination_arrival_day_offset: 0,
            }],
        )
        .await
        .expect("seed schedule_destination_departures");

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k1', '0003', 'DEPARTURE', 'RDG', '2026-09-08T07:00:00Z', \
                     '2026-09-08T07:04:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-OV", service_date, None, None, &[])
            .await
            .expect("build_journey_stops")
            .expect("Some stops");

        assert_eq!(
            stops.len(),
            2,
            "RDG + synthetic WAT terminus (destination_crs differs from last row)"
        );
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(
            stops[0].actual_departure,
            "2026-09-08T07:04:00Z".parse().ok()
        );
        assert_eq!(stops[0].last_event_type.as_deref(), Some("DEPARTURE"));
        assert_eq!(
            stops[0].delay_minutes,
            Some(4),
            "actual 4 minutes after this event's own planned time"
        );
        assert_eq!(stops[1].crs.as_deref(), Some("WAT"));
        assert_eq!(
            stops[1].kind,
            Some(schedule_query::CallingPointKind::Terminate)
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-OV'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Regression test for the final whole-branch review's Finding 2: a
    /// stop with BOTH a booked arrival (10:00 London) and a booked
    /// departure (10:02 London) -- a real Intermediate calling point with
    /// a dwell time -- gets an `ARRIVAL`-only movement event one minute
    /// LATE. Under the old, buggy code (`scheduled_reference =
    /// stop.scheduled_departure.or(stop.scheduled_arrival)`, unconditional
    /// "prefer departure"), this would have paired the actual ARRIVAL
    /// (09:01 UTC) against the booked DEPARTURE (09:02 UTC), computing
    /// `09:01 - 09:02 = -1 minute` (`Some(-1)`, rendering as "1m early"
    /// for a train that arrived late). The fix pairs by
    /// `last_event_type` instead, so this asserts `Some(1)` (correctly
    /// late), NOT `Some(-1)`.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_overlays_an_arrival_event_pairing_arrival_with_arrival_not_departure \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_overlays_an_arrival_event_pairing_arrival_with_arrival_not_departure()
     {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-ARR", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-ARR-1".to_string(),
                    crs: "PAD".to_string(),
                    tiploc: "TEST-JRN-ARR-ORIGIN".to_string(),
                    station_name: "PADDINGTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-ARR-2".to_string(),
                    crs: "TSA".to_string(),
                    tiploc: "TEST-JRN-ARR-MID".to_string(),
                    station_name: "TEST STATION A".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        // Booked arrival 10:00 London, booked departure 10:02 London --
        // 2026-09-08 is within BST (UTC+1), so 09:00Z/09:02Z respectively.
        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-ARR-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-ARR-MID",
                "kind": "Intermediate",
                "bookedArrival": "10:00:00",
                "bookedDeparture": "10:02:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-arr-1', '0001', 'ARRIVAL', 'TSA', '2026-09-08T09:00:00Z', \
                     '2026-09-08T09:01:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-ARR",
            service_date,
            Some(&calling_points),
            None,
            &[],
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].crs.as_deref(), Some("TSA"));
        assert_eq!(stops[1].last_event_type.as_deref(), Some("ARRIVAL"));
        assert_eq!(stops[1].actual_arrival, "2026-09-08T09:01:00Z".parse().ok());
        assert_eq!(
            stops[1].delay_minutes,
            Some(1),
            "must pair actual ARRIVAL (09:01Z) against booked ARRIVAL (09:00Z) -> 1 minute late; \
             the old buggy code paired it against booked DEPARTURE (09:02Z) -> Some(-1), \
             falsely rendering as \"1m early\""
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-ARR-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// §6(d): a `PASS` event sets both `actual_arrival` and
    /// `actual_departure` to the same instant (a passing train's arrival
    /// and departure are the same instant for display purposes), and
    /// `delay_minutes` is computed correctly against whichever scheduled
    /// time is available (the fallback source here only ever has
    /// `scheduled_departure`).
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_overlays_a_pass_event_setting_both_actual_times \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_overlays_a_pass_event_setting_both_actual_times() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-PASS", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-PASS'",
        )
        .execute(&pool)
        .await
        .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[crate::data::queries::ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "08:00:00".parse().unwrap(),
                day_offset: 0,
                train_uid: "TEST-JRN-PASS".to_string(),
                origin_crs: "RDG".to_string(),
                true_origin_crs: Some("RDG".to_string()),
                calling_point_arrival: None,
                destination_arrival: None,
                destination_arrival_day_offset: 0,
            }],
        )
        .await
        .expect("seed schedule_destination_departures");

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-pass-1', '0002', 'PASS', 'RDG', '2026-09-08T07:00:00Z', \
                     '2026-09-08T07:02:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops =
            build_journey_stops(&pool, trains_id, "TEST-JRN-PASS", service_date, None, None, &[])
                .await
                .expect("build_journey_stops")
                .expect("Some stops");

        assert_eq!(stops.len(), 2, "RDG + synthetic WAT terminus");
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(stops[0].last_event_type.as_deref(), Some("PASS"));
        let expected_instant: Option<DateTime<Utc>> = "2026-09-08T07:02:00Z".parse().ok();
        assert_eq!(stops[0].actual_arrival, expected_instant);
        assert_eq!(stops[0].actual_departure, expected_instant);
        assert_eq!(
            stops[0].delay_minutes,
            Some(2),
            "actual 2 minutes after this event's own planned time, via scheduled_departure \
             (the only scheduled time this fallback source has)"
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-PASS'",
        )
        .execute(&pool)
        .await
        .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// §6(e): a movement event whose `loc_crs` matches NO stop in the base
    /// list is silently dropped -- no panic, no stray extra stop, and none
    /// of the real stops' `actual_*`/`delay_minutes` fields get corrupted
    /// by it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_silently_drops_an_event_whose_loc_crs_matches_no_stop \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_silently_drops_an_event_whose_loc_crs_matches_no_stop() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NOMATCH", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-NOMATCH'",
        )
        .execute(&pool)
        .await
        .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[crate::data::queries::ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "08:00:00".parse().unwrap(),
                day_offset: 0,
                train_uid: "TEST-JRN-NOMATCH".to_string(),
                origin_crs: "RDG".to_string(),
                true_origin_crs: Some("RDG".to_string()),
                calling_point_arrival: None,
                destination_arrival: None,
                destination_arrival_day_offset: 0,
            }],
        )
        .await
        .expect("seed schedule_destination_departures");

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        // 'ZZZ' is not RDG (the base stop) nor WAT (the synthetic
        // terminus) -- an unscheduled diversion location, or a
        // STANOX->CRS translation that doesn't line up with either
        // source's own CRS.
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-nomatch-1', '0001', 'ARRIVAL', 'ZZZ', '2026-09-08T09:00:00Z', \
                     '2026-09-08T09:05:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-NOMATCH",
            service_date,
            None,
            None,
            &[],
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops");

        assert_eq!(
            stops.len(),
            2,
            "RDG + synthetic WAT terminus, no stray 'ZZZ' stop appended"
        );
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(stops[0].actual_arrival, None);
        assert_eq!(stops[0].actual_departure, None);
        assert_eq!(stops[0].last_event_type, None);
        assert_eq!(stops[0].delay_minutes, None);
        assert_eq!(stops[1].crs.as_deref(), Some("WAT"));
        assert_eq!(stops[1].actual_arrival, None);
        assert_eq!(stops[1].actual_departure, None);
        assert_eq!(stops[1].last_event_type, None);
        assert_eq!(stops[1].delay_minutes, None);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-NOMATCH'",
        )
        .execute(&pool)
        .await
        .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Regression test for the live journey-page bug (2026-09-10, trains
    /// `L78923`/`C34231`): the real TRUST `TRAIN_MVT_ALL_TOC` feed was
    /// delivering `planned_timestamp`/`actual_timestamp` epoch millis that
    /// were BOTH consistently ~1 hour ahead of true UTC (an upstream feed
    /// issue -- confirmed against `received_at` -- outside this codebase;
    /// nothing in `common::trust_timestamp::parse_trust_epoch_millis_pair`
    /// needed to change). Because the skew hits both of TRUST's own fields equally,
    /// diffing them against EACH OTHER (this test) cancels it out and
    /// yields the true delay, whereas diffing TRUST's `actual` against the
    /// CIF-schedule-derived `scheduled_arrival` (a completely separate,
    /// correctly-BST-converted pipeline that the skew never touched) mixes
    /// two independent bases and manufactures a bogus ~59 minute "late".
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_delay_uses_events_own_planned_timestamp_not_cif_schedule \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_delay_uses_events_own_planned_timestamp_not_cif_schedule() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-SKEW", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-SKEW-1".to_string(),
                    crs: "PAD".to_string(),
                    tiploc: "TEST-JRN-SKEW-ORIGIN".to_string(),
                    station_name: "PADDINGTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-SKEW-2".to_string(),
                    crs: "TSB".to_string(),
                    tiploc: "TEST-JRN-SKEW-MID".to_string(),
                    station_name: "TEST STATION B".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        // CIF schedule (correctly BST-converted): booked arrival 11:23
        // London on 2026-09-08 (BST, UTC+1) -> 10:23:00Z.
        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-SKEW-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-SKEW-MID",
                "kind": "Terminate",
                "bookedArrival": "11:23:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        // The live-investigated scenario: TRUST's own two fields on this
        // event (`planned_timestamp` and `actual_timestamp`) are BOTH ~1
        // hour ahead of true UTC, but only 1 minute apart from EACH OTHER
        // -- this train is genuinely running 1 minute early per TRUST's
        // own self-consistent numbers, even though neither TRUST
        // timestamp lines up at all with the correctly-converted CIF
        // schedule time (10:23:00Z).
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-skew-1', '0001', 'ARRIVAL', 'TSB', '2026-09-08T11:23:00Z', \
                     '2026-09-08T11:22:00Z', 'EARLY', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-SKEW",
            service_date,
            Some(&calling_points),
            None,
            &[],
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].crs.as_deref(), Some("TSB"));
        assert_eq!(
            stops[1].scheduled_arrival,
            "2026-09-08T10:23:00Z".parse().ok(),
            "the DISPLAYED scheduled time is still the correctly-BST-converted CIF value \
             -- this fix only changes the delay-minutes arithmetic, not what's shown as \
             \"scheduled\""
        );
        assert_eq!(stops[1].actual_arrival, "2026-09-08T11:22:00Z".parse().ok());
        assert_eq!(
            stops[1].delay_minutes,
            Some(-1),
            "must diff TRUST's own actual_timestamp (11:22Z) against TRUST's own \
             planned_timestamp (11:23Z) on the SAME movement event row -> 1 minute early. \
             The old buggy code diffed TRUST's actual (11:22Z) against the CIF-derived \
             scheduled_arrival (10:23Z) instead -> Some(59), a bogus ~1 hour \"late\" caused \
             entirely by the upstream TRUST feed's timestamp skew against true UTC."
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-SKEW-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// The non-skewed, legitimate case: TRUST's own `planned_timestamp` for
    /// this event genuinely agrees with the correctly-converted CIF
    /// schedule time. This fix must not change the computed delay for this,
    /// the normal case -- asserts the same value old and new code both
    /// produce.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_delay_unaffected_when_trust_and_cif_timestamps_agree \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_delay_unaffected_when_trust_and_cif_timestamps_agree() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NOSKEW", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-NOSKEW-1".to_string(),
                    crs: "PAD".to_string(),
                    tiploc: "TEST-JRN-NOSKEW-ORIGIN".to_string(),
                    station_name: "PADDINGTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-NOSKEW-2".to_string(),
                    crs: "TSC".to_string(),
                    tiploc: "TEST-JRN-NOSKEW-MID".to_string(),
                    station_name: "TEST STATION C".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        // Booked arrival 10:00 London on 2026-09-08 (BST) -> 09:00:00Z,
        // and TRUST's own planned_timestamp for the same event agrees
        // exactly -- no upstream skew present.
        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-NOSKEW-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "08:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-NOSKEW-MID",
                "kind": "Terminate",
                "bookedArrival": "10:00:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-noskew-1', '0001', 'ARRIVAL', 'TSC', '2026-09-08T09:00:00Z', \
                     '2026-09-08T09:06:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-NOSKEW",
            service_date,
            Some(&calling_points),
            None,
            &[],
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].crs.as_deref(), Some("TSC"));
        assert_eq!(
            stops[1].delay_minutes,
            Some(6),
            "TRUST's own planned_timestamp (09:00Z) matches the CIF schedule (09:00Z) here, \
             so diffing against either basis gives the same, correct 6-minutes-late result"
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-NOSKEW-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// A movement event that's missing its own `planned_timestamp` (some
    /// TRUST messages omit it) must NOT fall back to diffing against the
    /// CIF-derived `scheduled_arrival`/`scheduled_departure` -- that
    /// fallback is exactly the cross-basis bug this fix removes. Instead
    /// `delay_minutes` is `None` ("delay unknown"), matching this
    /// function's established "don't guess when data is incomplete"
    /// convention (the same convention behind the `_ => None` arm and the
    /// "silently drops" test above).
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_delay_is_none_when_events_own_planned_timestamp_is_missing \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_delay_is_none_when_events_own_planned_timestamp_is_missing() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NOPLAN", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-NOPLAN-1".to_string(),
                    crs: "PAD".to_string(),
                    tiploc: "TEST-JRN-NOPLAN-ORIGIN".to_string(),
                    station_name: "PADDINGTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-NOPLAN-2".to_string(),
                    crs: "TSD".to_string(),
                    tiploc: "TEST-JRN-NOPLAN-MID".to_string(),
                    station_name: "TEST STATION D".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-NOPLAN-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "08:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-NOPLAN-MID",
                "kind": "Terminate",
                "bookedArrival": "10:00:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        // `planned_timestamp` explicitly NULL, `actual_timestamp` present.
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-noplan-1', '0001', 'ARRIVAL', 'TSD', NULL, \
                     '2026-09-08T09:06:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-NOPLAN",
            service_date,
            Some(&calling_points),
            None,
            &[],
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].crs.as_deref(), Some("TSD"));
        assert_eq!(
            stops[1].actual_arrival,
            "2026-09-08T09:06:00Z".parse().ok(),
            "the actual time itself is still shown even without a planned_timestamp"
        );
        assert_eq!(
            stops[1].delay_minutes, None,
            "no planned_timestamp on this event -> delay unknown, NOT a fallback diff \
             against the CIF-derived scheduled_arrival (which would reintroduce the \
             cross-basis bug this fix removes)"
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-NOPLAN-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_estimates_only_the_unreported_stop_from_the_current_delay \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_estimates_only_the_unreported_stop_from_the_current_delay() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-EST", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-EST-1".to_string(),
                    crs: "EUS".to_string(),
                    tiploc: "TEST-JRN-EST-EUSTON".to_string(),
                    station_name: "EUSTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-EST-2".to_string(),
                    crs: "CRE".to_string(),
                    tiploc: "TEST-JRN-EST-CREWE".to_string(),
                    station_name: "CREWE".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-EST-EUSTON",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-EST-CREWE",
                "kind": "Terminate",
                "bookedArrival": "10:30:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        // Only the ORIGIN has a reported movement -- CREWE (the terminus)
        // has no live data at all, so it's the one this test expects an
        // estimate on.
        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-est', '0003', 'DEPARTURE', 'EUS', '2026-09-08T09:00:00Z', \
                     '2026-09-08T09:06:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-EST",
            service_date,
            Some(&calling_points),
            Some(6),
            &[],
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].crs.as_deref(), Some("EUS"));
        assert!(
            stops[0].actual_departure.is_some(),
            "the origin's departure was actually reported"
        );
        assert_eq!(
            stops[0].estimated_departure, None,
            "a confirmed actual_departure must never also carry an estimate"
        );

        assert_eq!(stops[1].crs.as_deref(), Some("CRE"));
        assert_eq!(
            stops[1].actual_arrival, None,
            "the terminus was never reported"
        );
        assert_eq!(
            stops[1].estimated_arrival,
            Some(stops[1].scheduled_arrival.unwrap() + Duration::minutes(6)),
            "an unreported stop's estimate is its scheduled time + the current delay"
        );
        // `scheduled_arrival` is 10:30 LONDON time on this (BST) service
        // date -- 09:30 UTC -- so the estimate above is 09:36 UTC.
        assert!(!may_have_arrived(
            &stops,
            "2026-09-08T09:45:00Z".parse().unwrap()
        ));
        assert!(may_have_arrived(
            &stops,
            "2026-09-08T10:00:00Z".parse().unwrap()
        ));

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-EST-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }
}
