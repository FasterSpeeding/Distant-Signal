//! A journey's alternatives as served and stored
//! (docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md §5.6,
//! §7, §17): what the planner found from the break point, ranked
//! (`alternative_ranking`), each with the legs a one-tap accept saves.
//!
//! A [`SuggestionSet`] is the response of the alternatives, preview and
//! "I missed this train" routes and the JSON stored on an open break
//! (`journey_breaks.suggestions`), so a deep link renders the stored set at
//! once and an accept saves exactly what was shown.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::data::alternative_ranking::{self, Candidate, CandidateTrain, Context, Fit, Pick};
use crate::data::journeys::{ItineraryLeg, UntrackedLeg};
use crate::data::trip_plan_live;
use crate::data::trip_planning_itinerary::{PlannedItinerary, PlannedLeg};

/// Suggestions older than this are stale (§5.4): 120 seconds, or
/// `JOURNEY_BREAKS_SUGGESTIONS_MAX_AGE_SECS` (chart
/// `api.journeyBreaks.suggestionsMaxAgeSecs`).
pub fn suggestions_max_age() -> Duration {
    static MAX_AGE: std::sync::LazyLock<Duration> = std::sync::LazyLock::new(|| {
        Duration::seconds(trip_plan_live::env_num(
            "JOURNEY_BREAKS_SUGGESTIONS_MAX_AGE_SECS",
            120_i64,
        ))
    });
    *MAX_AGE
}

/// Suggestions whose first train leaves within this of now are stale.
pub const FIRST_TRAIN_DUE_WITHIN: Duration = Duration::minutes(2);

/// At most this many alternatives are kept (§5.6).
pub const MAX_SUGGESTIONS: usize = 9;

/// One leg of an alternative: a train to track, or a Tube ride, bus or
/// walk saved as an untracked leg (as `POST /Journeys/itinerary` takes it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SuggestionLeg {
    Train {
        train_uid: String,
        service_date: NaiveDate,
        origin_crs: Option<String>,
        origin_name: Option<String>,
        destination_crs: Option<String>,
        destination_name: Option<String>,
        /// Live (else timetabled) times at the leg's two calls.
        departure: DateTime<Utc>,
        arrival: DateTime<Utc>,
        /// Timetabled (public) times, for "+N min".
        scheduled_departure: DateTime<Utc>,
        scheduled_arrival: DateTime<Utc>,
        operator: Option<String>,
        operator_name: Option<String>,
        platform: Option<String>,
        cancelled: bool,
        /// Set when the change onto this train works only because it is
        /// running late (additive, DS-141): it could leave earlier. Absent
        /// otherwise, and from a set saved before.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relies_on_late_departure: Option<trip_plan_live::LateDepartureReliance<DateTime<Utc>>>,
    },
    Untracked {
        transit_mode: String,
        origin_name: Option<String>,
        destination_name: Option<String>,
        line_name: Option<String>,
        origin_stop_id: Option<String>,
        destination_stop_id: Option<String>,
        planned_departure: DateTime<Utc>,
        planned_arrival: DateTime<Utc>,
    },
}

/// One ranked alternative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// Stable within its set (`s1`, `s2`, ...): what an accept names.
    pub id: String,
    pub departure: DateTime<Utc>,
    pub arrival: DateTime<Utc>,
    pub change_count: u32,
    /// Minutes after the booked arrival at the same destination; `None`
    /// when the booked arrival is unknown or the destination differs.
    pub delay_minutes: Option<i64>,
    /// The last current leg this alternative replaces.
    pub through_leg_id: i64,
    /// The later booked leg it gets you to in time for, kept (§5.5).
    pub rejoins_leg_id: Option<i64>,
    pub legs: Vec<SuggestionLeg>,
    pub cost: i64,
    pub fit: Option<Fit>,
    pub fit_label: Option<String>,
    pub fit_reason: Option<String>,
    pub picks: Vec<Pick>,
    pub pick_labels: Vec<String>,
    /// One of the (at most three) cards; the rest are "More options".
    pub card: bool,
    /// The best route on only the journey's preferred operators
    /// (additive, 2026-10-31): their codes. Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_only: Option<Vec<String>>,
    /// The journey's operator filter left no suggestion, so this one uses
    /// other operators (additive, 2026-10-31). Absent when `false`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outside_operator_filter: bool,
}

impl Suggestion {
    /// The trains, in order: what identifies an alternative across two
    /// plans (an accept of a stale set finds it again by these).
    pub fn train_key(&self) -> Vec<(String, NaiveDate)> {
        self.legs
            .iter()
            .filter_map(|leg| match leg {
                SuggestionLeg::Train {
                    train_uid,
                    service_date,
                    ..
                } => Some((train_uid.clone(), *service_date)),
                SuggestionLeg::Untracked { .. } => None,
            })
            .collect()
    }

    /// The first train's departure, if any.
    pub fn first_train_departure(&self) -> Option<DateTime<Utc>> {
        self.legs.iter().find_map(|leg| match leg {
            SuggestionLeg::Train { departure, .. } => Some(*departure),
            SuggestionLeg::Untracked { .. } => None,
        })
    }

    /// The legs to save, resolved by `resolve_train` (train UID and date to
    /// the shared `trains` row id).
    pub fn itinerary_legs(
        &self,
        trains_ids: &HashMap<(String, NaiveDate), i64>,
    ) -> Option<Vec<ItineraryLeg>> {
        self.legs
            .iter()
            .map(|leg| match leg {
                SuggestionLeg::Train {
                    train_uid,
                    service_date,
                    origin_crs,
                    destination_crs,
                    ..
                } => Some(ItineraryLeg::Train {
                    trains_id: *trains_ids.get(&(train_uid.clone(), *service_date))?,
                    service_date: *service_date,
                    origin_crs: origin_crs.clone(),
                    destination_crs: destination_crs.clone(),
                }),
                SuggestionLeg::Untracked {
                    transit_mode,
                    origin_name,
                    destination_name,
                    line_name,
                    origin_stop_id,
                    destination_stop_id,
                    planned_departure,
                    planned_arrival,
                } => Some(ItineraryLeg::Untracked(UntrackedLeg {
                    transit_mode: transit_mode.clone(),
                    origin_place: origin_name.clone(),
                    destination_place: destination_name.clone(),
                    line_name: line_name.clone(),
                    planned_departure: *planned_departure,
                    planned_arrival: *planned_arrival,
                    origin_stop_id: origin_stop_id.clone(),
                    destination_stop_id: destination_stop_id.clone(),
                })),
            })
            .collect()
    }
}

/// The train whose leg no longer works, as the sheet and the push name it
/// ("the 10:16 to Charing Cross").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerTrain {
    pub leg_id: i64,
    /// Its timetabled departure from the leg's boarding station.
    pub departure: Option<DateTime<Utc>>,
    pub origin_name: Option<String>,
    /// The leg's own alighting station.
    pub destination_name: Option<String>,
    /// The train's own destination (what a train is named by).
    pub train_destination_name: Option<String>,
}

/// The alternatives for one break point, as served and stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionSet {
    pub generated_at: DateTime<Utc>,
    /// The break's fingerprint when planned (§5.4); empty with no break.
    pub fingerprint: String,
    /// The first leg replaced.
    pub from_leg_id: i64,
    pub origin_crs: String,
    pub origin_name: Option<String>,
    pub destination_crs: String,
    pub destination_name: Option<String>,
    pub depart_after: DateTime<Utc>,
    /// The booked arrival at the destination (the compensation baseline's).
    pub booked_arrival: Option<DateTime<Utc>>,
    /// The sheet's one-line ticket hint (§17.5).
    pub hint: Option<String>,
    /// The leg's train, when it is a train leg.
    #[serde(default)]
    pub trigger: Option<TriggerTrain>,
    /// Cards first, then the rest by cost.
    pub suggestions: Vec<Suggestion>,
    /// Set by the route on read: stored suggestions that need a re-plan.
    #[serde(default)]
    pub stale: bool,
}

impl SuggestionSet {
    /// Whether these suggestions need re-planning (§5.4): planned for other
    /// inputs, too old, or the first one's first train is about to leave.
    pub fn is_stale(&self, fingerprint: &str, now: DateTime<Utc>) -> bool {
        self.fingerprint != fingerprint
            || now - self.generated_at > suggestions_max_age()
            || self
                .suggestions
                .first()
                .and_then(Suggestion::first_train_departure)
                .is_some_and(|departure| departure - now < FIRST_TRAIN_DUE_WITHIN)
    }

    /// The best (first) alternative's minutes after the booked arrival:
    /// `Some(None)` when nothing was found (severity 4, §6.2).
    pub fn best_delay(&self) -> Option<Option<i64>> {
        match self.suggestions.first() {
            None => Some(None),
            Some(best) => best.delay_minutes.map(Some),
        }
    }
}

/// Where the alternatives are planned to, besides the destination: a later
/// booked leg the traveller could still make (§5.6, the rejoin search).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejoinTarget {
    pub leg_id: i64,
    /// The last current leg before it (what a rejoin replaces through).
    pub through_leg_id: i64,
    pub boarding_crs: String,
    /// Arrive by this to make the change (its live departure less the
    /// minimum change time).
    pub deadline: DateTime<Utc>,
}

/// One planned itinerary, with what the route knows about it.
#[derive(Debug, Clone)]
pub struct Planned {
    pub itinerary: PlannedItinerary,
    /// The search's service date (the itinerary's minutes are on it).
    pub date: NaiveDate,
    pub through_leg_id: i64,
    pub rejoins_leg_id: Option<i64>,
    /// From the journey's "prefer" operator filter's own search
    /// (2026-10-31): the preferred operators, for the suggestion's
    /// `operatorOnly`.
    pub operator_only: Option<Vec<String>>,
    /// The fallback when the journey's operator filter left nothing
    /// (2026-10-31, spec §5.6): served flagged `outsideOperatorFilter`.
    pub outside_operator_filter: bool,
}

fn instant(date: NaiveDate, minute: u32) -> Option<DateTime<Utc>> {
    trip_plan_live::service_instant(date, minute)
}

/// The planner's transfer mode as an untracked leg's (the web planner's
/// `transferAsUntracked`), `None` for a change within one station.
fn transfer_mode(mode: &str) -> Option<&'static str> {
    Some(match mode.to_ascii_uppercase().as_str() {
        "WALK" => "walk",
        "TUBE" => "tube",
        "METRO" => "metro",
        "BUS" => "bus",
        "FERRY" => "ferry",
        _ => return None,
    })
}

/// Station names for every code in `codes` (best effort).
pub async fn station_names(pool: &PgPool, codes: &[String]) -> HashMap<String, String> {
    if codes.is_empty() {
        return HashMap::new();
    }
    sqlx::query_as::<_, (String, String)>(
        "SELECT TRIM(crs), name FROM stations WHERE crs = ANY($1::bpchar[])",
    )
    .bind(codes)
    .fetch_all(pool)
    .await
    .map_or_else(
        |err| {
            tracing::warn!(error = ?err, "could not read station names for alternatives");
            HashMap::new()
        },
        |rows| rows.into_iter().collect(),
    )
}

/// Operator names by ATOC code (best effort).
pub async fn operator_names(pool: &PgPool, codes: &[String]) -> HashMap<String, String> {
    if codes.is_empty() {
        return HashMap::new();
    }
    sqlx::query_as::<_, (String, String)>(
        "SELECT atoc_code::text, name FROM tocs WHERE atoc_code::text = ANY($1)",
    )
    .bind(codes)
    .fetch_all(pool)
    .await
    .map_or_else(
        |err| {
            tracing::warn!(error = ?err, "could not read operator names for alternatives");
            HashMap::new()
        },
        |rows| rows.into_iter().collect(),
    )
}

/// An itinerary's legs as [`SuggestionLeg`]s, `None` when a train leg's
/// times cannot be placed. A transfer starts when the train before it
/// arrives (else ends when the next leaves) and lasts its minutes; a change
/// within one station is left out, as the web planner saves it.
#[expect(
    clippy::too_many_lines,
    reason = "one match over the two leg kinds, each mapped field by field"
)]
fn suggestion_legs(
    planned: &Planned,
    names: &HashMap<String, String>,
    operator_names: &HashMap<String, String>,
) -> Option<Vec<SuggestionLeg>> {
    let date = planned.date;
    let legs = &planned.itinerary.legs;
    let mut out = Vec::with_capacity(legs.len());
    for (index, leg) in legs.iter().enumerate() {
        match leg {
            PlannedLeg::Train {
                train_uid,
                service_date,
                origin_crs,
                destination_crs,
                departure_min,
                arrival_min,
                public_departure,
                public_arrival,
                public_departure_day_offset,
                public_arrival_day_offset,
                scheduled_departure,
                scheduled_arrival,
                departure_day_offset,
                arrival_day_offset,
                operator,
                live,
                ..
            } => {
                let clock = |time: chrono::NaiveTime, offset: u8| {
                    use chrono::TimeZone;
                    let local = (*service_date + Duration::days(i64::from(offset))).and_time(time);
                    chrono_tz::Europe::London
                        .from_local_datetime(&local)
                        .earliest()
                        .map(|at| at.with_timezone(&Utc))
                };
                let scheduled_dep = match public_departure {
                    Some(time) => clock(*time, public_departure_day_offset.unwrap_or(0)),
                    None => clock(*scheduled_departure, *departure_day_offset),
                }?;
                let scheduled_arr = match public_arrival {
                    Some(time) => clock(*time, public_arrival_day_offset.unwrap_or(0)),
                    None => clock(*scheduled_arrival, *arrival_day_offset),
                }?;
                let leg_live = live.as_ref().and_then(|l| l.as_deref());
                let name = |crs: &Option<String>| {
                    crs.as_ref()
                        .and_then(|code| names.get(code.trim()).cloned())
                };
                out.push(SuggestionLeg::Train {
                    train_uid: train_uid.clone(),
                    service_date: *service_date,
                    origin_crs: origin_crs.clone(),
                    origin_name: name(origin_crs),
                    destination_crs: destination_crs.clone(),
                    destination_name: name(destination_crs),
                    departure: instant(date, *departure_min).unwrap_or(scheduled_dep),
                    arrival: instant(date, *arrival_min).unwrap_or(scheduled_arr),
                    scheduled_departure: scheduled_dep,
                    scheduled_arrival: scheduled_arr,
                    operator: operator.clone(),
                    operator_name: operator
                        .as_ref()
                        .and_then(|code| operator_names.get(code).cloned()),
                    platform: leg_live.and_then(|l| l.platform.clone()),
                    cancelled: leg_live.is_some_and(|l| l.cancelled),
                    relies_on_late_departure: leg_live
                        .and_then(|l| l.relies_on_late_departure.as_ref())
                        .map(|r| trip_plan_live::LateDepartureReliance {
                            scheduled_departure: scheduled_dep,
                            expected_departure: instant(date, *departure_min)
                                .unwrap_or(scheduled_dep),
                            delay_minutes: r.delay_minutes,
                            slack_if_on_time_minutes: r.slack_if_on_time_minutes,
                        }),
                });
            }
            PlannedLeg::Transfer {
                mode,
                origin_crs,
                destination_crs,
                minutes,
                line,
                origin_stop,
                destination_stop,
                ..
            } => {
                let Some(transit_mode) = transfer_mode(mode) else {
                    continue;
                };
                let minutes = Duration::minutes(i64::from((*minutes).max(0)));
                let before = legs[..index].iter().rev().find_map(|l| match l {
                    PlannedLeg::Train { arrival_min, .. } => instant(date, *arrival_min),
                    PlannedLeg::Transfer { .. } => None,
                });
                let after = legs[index + 1..].iter().find_map(|l| match l {
                    PlannedLeg::Train { departure_min, .. } => instant(date, *departure_min),
                    PlannedLeg::Transfer { .. } => None,
                });
                let Some(start) = before.or_else(|| after.map(|a| a - minutes)) else {
                    continue;
                };
                let place = |stop: &Option<Box<crate::data::tfl_stops::TflStopSummary>>,
                             crs: &Option<String>| {
                    stop.as_ref().map(|s| s.name.clone()).or_else(|| {
                        crs.as_ref()
                            .map(|code| names.get(code.trim()).cloned().unwrap_or(code.clone()))
                    })
                };
                out.push(SuggestionLeg::Untracked {
                    transit_mode: transit_mode.to_string(),
                    origin_name: place(origin_stop, origin_crs),
                    destination_name: place(destination_stop, destination_crs),
                    line_name: line.clone(),
                    origin_stop_id: origin_stop.as_ref().map(|s| s.id.clone()),
                    destination_stop_id: destination_stop.as_ref().map(|s| s.id.clone()),
                    planned_departure: start,
                    planned_arrival: start + minutes,
                });
            }
        }
    }
    Some(out)
}

/// Ranking's view of one alternative.
fn candidate(planned: &Planned, legs: &[SuggestionLeg]) -> Option<Candidate> {
    let itinerary = &planned.itinerary;
    let departure = instant(planned.date, itinerary.departure_min)?;
    let arrival = instant(planned.date, itinerary.arrival_min)?;
    let mut tube_or_bus = 0;
    let mut walk_minutes = 0;
    for leg in &itinerary.legs {
        if let PlannedLeg::Transfer { mode, minutes, .. } = leg {
            match mode.to_ascii_uppercase().as_str() {
                "TUBE" | "BUS" | "METRO" => tube_or_bus += 1,
                "WALK" => walk_minutes += u32::try_from((*minutes).max(0)).unwrap_or(0),
                _ => {}
            }
        }
    }
    Some(Candidate {
        departure,
        arrival,
        changes: itinerary.change_count,
        tube_or_bus_transfers: tube_or_bus,
        walk_minutes,
        min_slack_minutes: None,
        rejoins: planned.rejoins_leg_id.is_some(),
        relies_on_late_departure: legs.iter().any(|leg| {
            matches!(
                leg,
                SuggestionLeg::Train {
                    relies_on_late_departure: Some(_),
                    ..
                }
            )
        }),
        trains: legs
            .iter()
            .filter_map(|leg| match leg {
                SuggestionLeg::Train {
                    operator,
                    departure,
                    ..
                } => Some(CandidateTrain {
                    operator: operator.clone(),
                    departure: *departure,
                }),
                SuggestionLeg::Untracked { .. } => None,
            })
            .collect(),
    })
}

/// Everything [`build_set`] needs besides the plans.
#[derive(Debug, Clone)]
pub struct SetInputs {
    pub fingerprint: String,
    pub from_leg_id: i64,
    pub origin_crs: String,
    pub destination_crs: String,
    pub depart_after: DateTime<Utc>,
    pub booked_arrival: Option<DateTime<Utc>>,
    pub trigger: Option<TriggerTrain>,
    /// The train being replaced (UID, date): never offered as its own
    /// alternative.
    pub exclude_train: Option<(String, NaiveDate)>,
    pub context: Context,
    pub now: DateTime<Utc>,
}

/// Ranks `plans` into a [`SuggestionSet`]: duplicates (the same trains)
/// dropped, options that leave before `depart_after` or are cancelled on
/// live times dropped, at most [`MAX_SUGGESTIONS`] kept, cards first.
#[expect(
    clippy::implicit_hasher,
    clippy::too_many_lines,
    reason = "callers always use the default hasher; one linear pass from plans to the ranked set"
)]
pub fn build_set(
    plans: Vec<Planned>,
    inputs: &SetInputs,
    names: &HashMap<String, String>,
    operator_names: &HashMap<String, String>,
) -> SuggestionSet {
    let mut seen: Vec<Vec<(String, NaiveDate)>> = Vec::new();
    let mut kept: Vec<(Planned, Vec<SuggestionLeg>, Candidate)> = Vec::new();
    for planned in plans {
        if planned.itinerary.live_feasible == Some(false) {
            continue;
        }
        let Some(legs) = suggestion_legs(&planned, names, operator_names) else {
            continue;
        };
        let key: Vec<(String, NaiveDate)> = legs
            .iter()
            .filter_map(|leg| match leg {
                SuggestionLeg::Train {
                    train_uid,
                    service_date,
                    ..
                } => Some((train_uid.clone(), *service_date)),
                SuggestionLeg::Untracked { .. } => None,
            })
            .collect();
        // The same route from the "prefer" search: the kept one is it.
        if planned.operator_only.is_some()
            && let Some(index) = seen.iter().position(|seen| *seen == key)
        {
            kept[index]
                .0
                .operator_only
                .clone_from(&planned.operator_only);
            continue;
        }
        if key.is_empty()
            || seen.contains(&key)
            || inputs
                .exclude_train
                .as_ref()
                .is_some_and(|excluded| key.first() == Some(excluded))
        {
            continue;
        }
        let Some(candidate) = candidate(&planned, &legs) else {
            continue;
        };
        if candidate.departure < inputs.depart_after - Duration::minutes(1) {
            continue;
        }
        seen.push(key);
        kept.push((planned, legs, candidate));
    }
    let candidates: Vec<Candidate> = kept.iter().map(|(_, _, c)| c.clone()).collect();
    let ranking = alternative_ranking::rank(&candidates, &inputs.context);
    let suggestions: Vec<Suggestion> = ranking
        .order
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .enumerate()
        .map(|(position, ranked)| {
            let (planned, legs, candidate) = &kept[ranked.index];
            let ends_at_destination = legs.iter().rev().find_map(|leg| match leg {
                SuggestionLeg::Train {
                    destination_crs, ..
                } => Some(destination_crs.as_deref() == Some(inputs.destination_crs.as_str())),
                SuggestionLeg::Untracked { .. } => None,
            });
            let delay_minutes = inputs
                .booked_arrival
                .filter(|_| ends_at_destination == Some(true) && planned.rejoins_leg_id.is_none())
                .map(|booked| (candidate.arrival - booked).num_minutes());
            Suggestion {
                id: format!("s{}", position + 1),
                departure: candidate.departure,
                arrival: candidate.arrival,
                change_count: candidate.changes,
                delay_minutes,
                through_leg_id: planned.through_leg_id,
                rejoins_leg_id: planned.rejoins_leg_id,
                legs: legs.clone(),
                cost: ranked.cost,
                fit: ranked.fit,
                fit_label: ranked.fit_label.map(str::to_string),
                fit_reason: ranked.fit_reason,
                pick_labels: ranked.picks.iter().map(|p| p.label().to_string()).collect(),
                picks: ranked.picks,
                card: ranked.card,
                operator_only: planned.operator_only.clone(),
                outside_operator_filter: planned.outside_operator_filter,
            }
        })
        .collect();
    SuggestionSet {
        generated_at: inputs.now,
        fingerprint: inputs.fingerprint.clone(),
        from_leg_id: inputs.from_leg_id,
        origin_name: names.get(&inputs.origin_crs).cloned(),
        origin_crs: inputs.origin_crs.clone(),
        destination_name: names.get(&inputs.destination_crs).cloned(),
        destination_crs: inputs.destination_crs.clone(),
        depart_after: inputs.depart_after,
        booked_arrival: inputs.booked_arrival,
        hint: ranking.hint.map(str::to_string),
        trigger: inputs.trigger.clone(),
        suggestions,
        stale: false,
    }
}
