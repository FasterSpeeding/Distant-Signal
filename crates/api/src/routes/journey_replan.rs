//! Mid-journey changes: alternatives, "I missed this train", replacing
//! part of a journey, and its revisions
//! (docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md §7).
//!
//! Every route here is cookie-only (`AuthenticatedUser`) and owner-only:
//! a journey that is not the caller's answers 404, as every other journey
//! write does. None is a delegated route (D-8): a delegated client reads
//! the additive fields of `GET /Journeys/{id}` only.
//!
//! | Route | What |
//! |---|---|
//! | `GET .../legs/{legId}/alternatives?cause=` | Ranked alternatives from one leg |
//! | `POST .../legs/{legId}/replace` | Replace legs with given ones |
//! | `GET .../breaks/current?src=push` | The open break, its stored suggestions, `stale` |
//! | `POST .../breaks/{breakId}/dismiss` | Dismiss the open break |
//! | `POST .../replan/preview` | Re-plan the open break's suggestions now |
//! | `POST .../replan/accept` | Take a suggestion (`Idempotency-Key`) |
//! | `GET .../revisions` | Every plan, newest first |
//! | `POST .../revisions/{no}/revert` | Back to an earlier plan (the Undo) |
//! | `PUT .../suggestions-muted` | Mute suggestion pushes for this journey |
//!
//! `GET /Journeys/{id}` gains `openBreak`, `revisionCount`,
//! `suggestionsMuted` and per leg `replacesLegIds` through
//! [`detail_extras`]; there, for the owner, a break is opened (or marked
//! recovered) on read, so the in-app banner works without the sweep.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::alternative_ranking::{self, Context, TicketInfo, Weights};
use crate::data::journey_alternatives::{
    self, Planned, SetInputs, Suggestion, SuggestionSet, TriggerTrain,
};
use crate::data::journey_breaks::{self, BreakRow, Cause, Detected, LegFacts};
use crate::data::journey_feasibility::{self, JourneyTimings, LegTiming, TrainLegTiming};
use crate::data::journey_replace::{self, Replaced, Replacement};
use crate::data::journey_revisions::{self, Reason, RevisionMeta};
use crate::data::{journeys, train_tracking};
use crate::routes::journey_itinerary::RouteError;
use crate::routes::trips::{ReplanQuery, ReplanSlot};

pub fn router() -> Router {
    Router::new()
        .route(
            "/Journeys/{journey_id}/legs/{leg_id}/alternatives",
            axum::routing::get(get_alternatives),
        )
        .route(
            "/Journeys/{journey_id}/legs/{leg_id}/replace",
            axum::routing::post(post_replace),
        )
        .route(
            "/Journeys/{journey_id}/breaks/current",
            axum::routing::get(get_current_break),
        )
        .route(
            "/Journeys/{journey_id}/breaks/{break_id}/dismiss",
            axum::routing::post(post_dismiss),
        )
        .route(
            "/Journeys/{journey_id}/replan/preview",
            axum::routing::post(post_preview),
        )
        .route(
            "/Journeys/{journey_id}/replan/accept",
            axum::routing::post(post_accept),
        )
        .route(
            "/Journeys/{journey_id}/revisions",
            axum::routing::get(get_revisions),
        )
        .route(
            "/Journeys/{journey_id}/revisions/{revision_no}/revert",
            axum::routing::post(post_revert),
        )
        .route(
            "/Journeys/{journey_id}/suggestions-muted",
            axum::routing::put(put_suggestions_muted),
        )
}

/// How long after a change the plan before it can be gone back to
/// whatever its trains are doing: the sheet's 30-second Undo, with room for
/// a slow network (§8.3, §14 Q12).
const UNDO_WINDOW: Duration = Duration::minutes(2);

/// Now, as every route here judges it: `london_now`, which a test can pin.
fn now_utc() -> DateTime<Utc> {
    crate::routes::london_now().with_timezone(&Utc)
}

fn internal(operation: &'static str) -> impl Fn(anyhow::Error) -> RouteError {
    move |err| {
        if let Some((status, body)) = crate::unavailable::response_for(&err) {
            return RouteError::Legacy(status, body);
        }
        tracing::error!(error = ?err, operation, "journey replan request failed");
        RouteError::coded(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            format!("failed to {operation}"),
        )
    }
}

fn not_found() -> RouteError {
    RouteError::coded(
        StatusCode::NOT_FOUND,
        "not_found",
        "no journey with that id",
    )
}

fn render(result: Result<Response, RouteError>) -> Response {
    result.unwrap_or_else(|err| err.render(false, "bad_request"))
}

/// The caller's journey, else 404.
async fn require_owner(app: &App, journey_id: i64, user_id: &str) -> Result<(), RouteError> {
    match journeys::journey_owner(&app.database, journey_id)
        .await
        .map_err(internal("read journey owner"))?
    {
        Some(owner) if owner == user_id => Ok(()),
        _ => Err(not_found()),
    }
}

// ---------------------------------------------------------------------------
// The journey as the break logic sees it
// ---------------------------------------------------------------------------

/// A journey's legs, their trains and their live timings, read once.
pub(crate) struct Analysis {
    pub legs: Vec<journeys::JourneyLegWithNamesRow>,
    pub states: Vec<Option<train_tracking::TrackedTrainState>>,
    pub timings: Option<JourneyTimings>,
    pub facts: Vec<LegFacts>,
    pub now: DateTime<Utc>,
}

impl Analysis {
    fn timing(&self, index: usize) -> Option<&TrainLegTiming> {
        match self.timings.as_ref()?.timings.get(index)? {
            LegTiming::Train(timing) => Some(timing),
            LegTiming::Untracked { .. } | LegTiming::Unknown => None,
        }
    }

    fn is_train(&self, index: usize) -> bool {
        self.legs[index].kind == journeys::LEG_KIND_TRAIN
    }

    fn board_crs(&self, index: usize) -> Option<String> {
        self.timing(index)
            .map(|t| t.board_crs.clone())
            .or_else(|| self.legs[index].origin_crs.clone())
            .or_else(|| {
                self.states[index]
                    .as_ref()
                    .and_then(|s| s.pin_origin_crs.clone())
            })
            .map(|crs| crs.trim().to_ascii_uppercase())
    }

    fn alight_crs(&self, index: usize) -> Option<String> {
        self.timing(index)
            .map(|t| t.alight_crs.clone())
            .or_else(|| self.legs[index].destination_crs.clone())
            .or_else(|| {
                self.states[index].as_ref().and_then(|s| {
                    s.pin_destination_crs
                        .clone()
                        .or_else(|| s.schedule_destination_crs.clone())
                })
            })
            .map(|crs| crs.trim().to_ascii_uppercase())
    }

    /// When the traveller can board leg `index`'s train, from the leg
    /// before it: its live arrival plus the change time.
    fn ready_for(&self, index: usize) -> Option<DateTime<Utc>> {
        let previous = index.checked_sub(1)?;
        let timings = self.timings.as_ref()?;
        let next = self.timing(index);
        let (arrival, from_crs) = match timings.timings.get(previous)? {
            LegTiming::Train(t) => (t.live_arr, Some(t.alight_crs.as_str())),
            LegTiming::Untracked {
                arrival: Some(arrival),
                ..
            } => (*arrival, None),
            LegTiming::Untracked { .. } | LegTiming::Unknown => return None,
        };
        let change = next
            .and_then(|next| timings.change_minutes(from_crs, arrival, next))
            .unwrap_or(5);
        Some(arrival + Duration::minutes(i64::from(change)))
    }
}

/// Reads a journey for the break logic: legs, train states with their
/// stops, live timings, and which legs are travelled.
pub(crate) async fn analyse(
    app: &App,
    journey_id: i64,
    now: DateTime<Utc>,
) -> anyhow::Result<Analysis> {
    let legs = journeys::list_legs_for_journey(&app.database, journey_id).await?;
    let ids: Vec<i64> = legs
        .iter()
        .filter_map(|leg| leg.train_subscription_id)
        .collect();
    let by_id = train_tracking::get_by_tracking_ids(&app.database, &ids).await?;
    let (indexes, states): (Vec<usize>, Vec<train_tracking::TrackedTrainState>) = legs
        .iter()
        .enumerate()
        .filter_map(|(index, leg)| Some((index, by_id.get(&leg.train_subscription_id?)?.clone())))
        .unzip();
    let built = crate::routes::train::attach_journey_stops_batch(app, states).await;
    let mut leg_states: Vec<Option<train_tracking::TrackedTrainState>> =
        legs.iter().map(|_| None).collect();
    for (index, state) in indexes.into_iter().zip(built) {
        leg_states[index] = Some(state);
    }
    let inputs = crate::routes::journeys::leg_inputs(&legs, &leg_states);
    let timings = journey_feasibility::journey_timings(&app.database, &inputs, now).await;
    let leg_ids: Vec<i64> = legs.iter().map(|leg| leg.id).collect();
    let mut conn = app.database.acquire().await?;
    let travelled = journey_breaks::travelled_legs(&mut conn, &leg_ids).await?;
    drop(conn);
    let facts = legs
        .iter()
        .zip(&leg_states)
        .map(|(leg, state)| LegFacts {
            leg_id: leg.id,
            is_train: leg.kind == journeys::LEG_KIND_TRAIN && leg.train_subscription_id.is_some(),
            trains_id: state.as_ref().and_then(|s| s.trains_id),
            whole_cancelled: state
                .as_ref()
                .is_some_and(|s| s.status.as_deref() == Some("cancelled")),
            skipped: false,
            travelled: travelled.contains(&leg.id),
        })
        .collect();
    Ok(Analysis {
        legs,
        states: leg_states,
        timings,
        facts,
        now,
    })
}

// ---------------------------------------------------------------------------
// Planning the alternatives
// ---------------------------------------------------------------------------

/// Why alternatives are wanted from a leg (`?cause=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AskCause {
    Missed,
    Cancelled,
    Connection,
    Choice,
}

/// How many next departures are searched one by one besides the options
/// set (a Pareto set keeps only the earliest arrival per number of changes,
/// so the next few trains, what a traveller who just missed one wants to
/// see, need their own searches).
const NEXT_DEPARTURES: usize = 3;

/// The next `count` ways there from `query`'s time: `fastest` searches,
/// each from a minute after the previous one's departure, leaving out an
/// itinerary whose first train is `skip_uid` (the train being replaced).
async fn next_departures(
    app: &App,
    mut query: ReplanQuery,
    count: usize,
    skip_uid: Option<&str>,
    slot: ReplanSlot,
) -> Result<Vec<crate::data::trip_planning_itinerary::PlannedItinerary>, RouteError> {
    let mut found = Vec::new();
    for _ in 0..=count {
        let Some(list) = crate::routes::trips::plan_for_journey(app, &query, slot)
            .await
            .map_err(RouteError::from)?
        else {
            break;
        };
        let Some(first) = list.into_iter().next() else {
            break;
        };
        let next_min = first.departure_min + 1;
        let first_uid = first.legs.iter().find_map(|leg| match leg {
            crate::data::trip_planning_itinerary::PlannedLeg::Train { train_uid, .. } => {
                Some(train_uid.as_str())
            }
            crate::data::trip_planning_itinerary::PlannedLeg::Transfer { .. } => None,
        });
        if first_uid.is_none() || first_uid != skip_uid {
            found.push(first);
        }
        if found.len() >= count || next_min <= query.depart_after_min {
            break;
        }
        query.depart_after_min = next_min;
    }
    Ok(found)
}

/// Plans and ranks the alternatives from leg `index` of `analysis` (§5.6):
/// the journey's final rail destination by the options search, the next
/// booked train leg by a rejoin search, and the replaced trains' operators
/// by a same-operator search (R1). `cause`/`fault` are the break's
/// (`user_choice`/`none` for a free choice).
#[expect(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "long but linear: inputs, three searches, ranking inputs, the set; each argument is \
              an independent input"
)]
pub(crate) async fn plan_suggestions(
    app: &App,
    journey_id: i64,
    analysis: &Analysis,
    index: usize,
    cause: &str,
    fault: &str,
    fingerprint: &str,
    slot: ReplanSlot,
) -> Result<SuggestionSet, RouteError> {
    let started = std::time::Instant::now();
    let trigger_label = match slot {
        ReplanSlot::Request => "request",
        ReplanSlot::Sweep => "sweep",
    };
    let now = analysis.now;
    let legs = &analysis.legs;
    let Some(last_train) = (0..legs.len()).rev().find(|&i| analysis.is_train(i)) else {
        return Err(RouteError::coded(
            StatusCode::UNPROCESSABLE_ENTITY,
            "no_train_leg",
            "This journey has no train to find another for.",
        ));
    };
    if index > last_train || !analysis.is_train(index) {
        return Err(RouteError::coded(
            StatusCode::UNPROCESSABLE_ENTITY,
            "not_a_train_leg",
            "Other trains can be found for a train leg only.",
        ));
    }
    let (Some(origin), Some(destination)) =
        (analysis.board_crs(index), analysis.alight_crs(last_train))
    else {
        return Err(RouteError::coded(
            StatusCode::UNPROCESSABLE_ENTITY,
            "no_stations",
            "This leg has no stations to search from yet.",
        ));
    };
    let timing = analysis.timing(index);
    let ready = analysis.ready_for(index);
    let depart_after = match cause {
        "user_choice" => ready
            .or_else(|| timing.map(|t| t.sched_dep - Duration::minutes(60)))
            .unwrap_or(now)
            .max(now),
        _ => ready.unwrap_or(now).max(now),
    };
    let date = timing.map_or(legs[index].service_date, |t| t.date);
    let depart_after_min =
        crate::data::trip_plan_live::service_minute(date, depart_after).unwrap_or(0);
    let train_legs_replaced = (index..=last_train)
        .filter(|&i| analysis.is_train(i))
        .count();
    let max_changes = u32::try_from(train_legs_replaced)
        .unwrap_or(0)
        .max(crate::data::trip_planning_itinerary::DEFAULT_MAX_CHANGES);
    let last_leg_id = legs[last_train].id;

    // The replaced trains' operators, and the tickets on them.
    let replaced_uids: Vec<String> = (index..=last_train)
        .filter_map(|i| analysis.timing(i).map(|t| t.uid.clone()))
        .collect();
    let operators =
        crate::data::train_operator::operators_for_trains(&app.database, &replaced_uids, date)
            .await
            .unwrap_or_default();
    let mut replaced_operators: Vec<String> = replaced_uids
        .iter()
        .filter_map(|uid| operators.get(uid).map(|op| op.code.clone()))
        .collect();
    replaced_operators.dedup();
    let subscription_ids: Vec<i64> = (index..=last_train)
        .filter_map(|i| legs[i].train_subscription_id)
        .collect();
    let tickets: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT ticket_type, operator FROM tracked_train_tickets WHERE tracked_train_id = ANY($1)",
    )
    .bind(&subscription_ids)
    .fetch_all(&app.database)
    .await
    .map_err(|err| internal("read tickets")(err.into()))?;
    let tickets: Vec<TicketInfo> = tickets
        .into_iter()
        .map(|(ticket_type, operator)| TicketInfo {
            ticket_type,
            operator_atoc: operator
                .as_deref()
                .and_then(crate::data::delay_repay_rules::atoc_for_operator_text)
                .map(str::to_string),
        })
        .collect();

    // The three searches.
    let mut plans: Vec<Planned> = Vec::new();
    let main = crate::routes::trips::plan_for_journey(
        app,
        &ReplanQuery {
            origin: origin.clone(),
            destination: destination.clone(),
            date,
            depart_after_min,
            results: "options",
            max_changes,
            operator_rule: None,
        },
        slot,
    )
    .await
    .map_err(RouteError::from)?;
    plans.extend(main.into_iter().flatten().map(|itinerary| Planned {
        itinerary,
        date,
        through_leg_id: last_leg_id,
        rejoins_leg_id: None,
    }));
    let excluded_uid = timing.map(|t| t.uid.clone());
    let next = next_departures(
        app,
        ReplanQuery {
            origin: origin.clone(),
            destination: destination.clone(),
            date,
            depart_after_min,
            results: "fastest",
            max_changes,
            operator_rule: None,
        },
        NEXT_DEPARTURES,
        excluded_uid.as_deref(),
        slot,
    )
    .await?;
    plans.extend(next.into_iter().map(|itinerary| Planned {
        itinerary,
        date,
        through_leg_id: last_leg_id,
        rejoins_leg_id: None,
    }));
    if let Some(next) = (index + 1..=last_train).find(|&i| analysis.is_train(i))
        && let (Some(next_timing), Some(next_board)) =
            (analysis.timing(next), analysis.board_crs(next))
        && next_board != origin
    {
        let change = analysis
            .timings
            .as_ref()
            .and_then(|t| t.change_minutes(None, next_timing.live_dep, next_timing))
            .unwrap_or(5);
        let deadline = next_timing.live_dep - Duration::minutes(i64::from(change));
        let rejoin = crate::routes::trips::plan_for_journey(
            app,
            &ReplanQuery {
                origin: origin.clone(),
                destination: next_board,
                date,
                depart_after_min,
                results: "fastest",
                max_changes,
                operator_rule: None,
            },
            slot,
        )
        .await
        .map_err(RouteError::from)?;
        plans.extend(
            rejoin
                .into_iter()
                .flatten()
                .filter(|itinerary| {
                    crate::data::trip_plan_live::service_instant(date, itinerary.arrival_min)
                        .is_some_and(|arrival| arrival <= deadline)
                })
                .map(|itinerary| Planned {
                    itinerary,
                    date,
                    through_leg_id: legs[next - 1].id,
                    rejoins_leg_id: Some(legs[next].id),
                }),
        );
    }
    // The ticket's "same operator" search: those operators' CIF trains,
    // buses and ferries (one code covers all three, from the graph's
    // `schedule_services` operators), with the Tube and any transit free to
    // use between them (operator filtering spec §5.6, §6.3).
    let same_operators = schedule_query::OperatorMask::of(
        replaced_operators
            .iter()
            .filter_map(|code| schedule_query::OperatorId::from_atoc(code)),
    );
    if !same_operators.is_empty() {
        let same = next_departures(
            app,
            ReplanQuery {
                origin: origin.clone(),
                destination: destination.clone(),
                date,
                depart_after_min,
                results: "fastest",
                max_changes,
                operator_rule: Some(
                    trip_planner::OperatorRule::only(same_operators).cif_trains_only(),
                ),
            },
            2,
            excluded_uid.as_deref(),
            slot,
        )
        .await?;
        plans.extend(same.into_iter().map(|itinerary| Planned {
            itinerary,
            date,
            through_leg_id: last_leg_id,
            rejoins_leg_id: None,
        }));
    }

    // Booked arrival at the destination, from the compensation baseline.
    let booked_arrival: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT l.booked_arrival FROM journey_revisions r \
         JOIN journey_revision_legs l ON l.revision_id = r.id \
         WHERE r.journey_id = $1 AND r.is_compensation_baseline AND l.kind = 'train' \
         ORDER BY l.position DESC LIMIT 1",
    )
    .bind(journey_id)
    .fetch_optional(&app.database)
    .await
    .map_err(|err| internal("read booked arrival")(err.into()))?
    .flatten();
    let booked_departure = timing.map(|t| t.sched_dep);

    let mut codes: Vec<String> = vec![origin.clone(), destination.clone()];
    for planned in &plans {
        for leg in &planned.itinerary.legs {
            if let crate::data::trip_planning_itinerary::PlannedLeg::Train {
                origin_crs,
                destination_crs,
                ..
            } = leg
            {
                codes.extend(origin_crs.iter().cloned());
                codes.extend(destination_crs.iter().cloned());
            }
        }
    }
    let state = analysis.states[index].as_ref();
    codes.extend(analysis.alight_crs(index));
    codes.extend(state.and_then(|s| s.schedule_destination_crs.clone()));
    codes.sort();
    codes.dedup();
    let names = journey_alternatives::station_names(&app.database, &codes).await;
    let mut operator_codes: Vec<String> = plans
        .iter()
        .flat_map(|p| &p.itinerary.legs)
        .filter_map(|leg| match leg {
            crate::data::trip_planning_itinerary::PlannedLeg::Train { operator, .. } => {
                operator.clone()
            }
            crate::data::trip_planning_itinerary::PlannedLeg::Transfer { .. } => None,
        })
        .collect();
    operator_codes.sort();
    operator_codes.dedup();
    let operator_names = journey_alternatives::operator_names(&app.database, &operator_codes).await;
    let name_of = |crs: Option<String>| crs.and_then(|c| names.get(c.trim()).cloned());
    let trigger = Some(TriggerTrain {
        leg_id: legs[index].id,
        departure: booked_departure,
        origin_name: names.get(&origin).cloned(),
        destination_name: name_of(analysis.alight_crs(index)),
        train_destination_name: name_of(state.and_then(|s| s.schedule_destination_crs.clone())),
    });
    let constraint = alternative_ranking::ticket_constraint(&tickets, &replaced_operators);
    let set = journey_alternatives::build_set(
        plans,
        &SetInputs {
            fingerprint: fingerprint.to_string(),
            from_leg_id: legs[index].id,
            origin_crs: origin,
            destination_crs: destination,
            depart_after,
            booked_arrival,
            trigger,
            exclude_train: timing.map(|t| (t.uid.clone(), t.date)),
            context: Context {
                constraint,
                entitlement: alternative_ranking::entitlement(cause, fault, booked_departure),
                replaced_operators,
                weights: Weights::default(),
            },
            now,
        },
        &names,
        &operator_names,
    );
    metrics::histogram!(
        common::metrics::metric_name("api_journey_break_suggestions_seconds"),
        "trigger" => trigger_label
    )
    .record(started.elapsed().as_secs_f64());
    metrics::counter!(
        common::metrics::metric_name("api_journey_break_suggestions_total"),
        "result" => if set.suggestions.is_empty() { "none" } else { "found" }
    )
    .increment(1);
    Ok(set)
}

// ---------------------------------------------------------------------------
// GET /Journeys/{id}'s additions
// ---------------------------------------------------------------------------

/// The open break as `GET /Journeys/{id}` serves it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenBreakSummary {
    pub id: i64,
    pub cause: String,
    pub fault: String,
    pub severity: i16,
    pub trigger_leg_id: Option<i64>,
    pub detected_at: DateTime<Utc>,
    pub suggestions_ready: bool,
    pub stale: bool,
    /// The sheet's headline facts, when suggestions are stored: where, to
    /// where, and the best option's arrival.
    pub station_name: Option<String>,
    pub destination_name: Option<String>,
    pub suggestion_count: usize,
    pub first_arrival: Option<DateTime<Utc>>,
}

impl OpenBreakSummary {
    fn from_row(row: &BreakRow, now: DateTime<Utc>) -> Self {
        let set: Option<SuggestionSet> = row
            .suggestions
            .clone()
            .and_then(|value| serde_json::from_value(value).ok());
        Self {
            id: row.id,
            cause: row.cause.clone(),
            fault: row.fault.clone(),
            severity: row.severity,
            trigger_leg_id: row.trigger_leg_id,
            detected_at: row.detected_at,
            suggestions_ready: set.is_some(),
            stale: set
                .as_ref()
                .is_none_or(|set| set.is_stale(&row.fingerprint, now)),
            station_name: set.as_ref().and_then(|s| s.origin_name.clone()),
            destination_name: set.as_ref().and_then(|s| s.destination_name.clone()),
            suggestion_count: set.as_ref().map_or(0, |s| s.suggestions.len()),
            first_arrival: set
                .as_ref()
                .and_then(|s| s.suggestions.first().map(|f| f.arrival)),
        }
    }
}

/// What `GET /Journeys/{id}` gains (flattened into its response).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetailExtras {
    /// The open break, `null` when the journey works.
    pub open_break: Option<OpenBreakSummary>,
    /// How many plans the journey has had (1: only the booked one).
    pub revision_count: i64,
    /// Suggestion pushes muted for this journey (owner only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestions_muted: Option<bool>,
    /// Per current leg, the legs of the plan before it that it replaced.
    #[serde(skip)]
    pub replaces: HashMap<i64, Vec<i64>>,
}

/// One leg as [`detail_extras`] judges it.
pub(crate) struct DetailLeg {
    pub id: i64,
    pub is_train: bool,
    pub trains_id: Option<i64>,
    pub cancelled: bool,
    pub skipped: bool,
}

/// [`DetailExtras`] for a journey page read. For the owner
/// (`owner_user_id`), the page's own judgement opens a break (or ends one
/// the plan has recovered from) so the in-app banner never waits for the
/// sweep; nobody else's read writes anything. Best effort: a failure logs
/// and leaves the fields empty.
pub(crate) async fn detail_extras(
    app: &App,
    journey_id: i64,
    owner_user_id: Option<&str>,
    legs: &[DetailLeg],
    timings: Option<&JourneyTimings>,
    now: DateTime<Utc>,
) -> DetailExtras {
    match detail_extras_inner(app, journey_id, owner_user_id, legs, timings, now).await {
        Ok(extras) => extras,
        Err(err) => {
            tracing::warn!(error = ?err, journey_id, "could not read the journey's breaks or revisions");
            DetailExtras::default()
        }
    }
}

async fn detail_extras_inner(
    app: &App,
    journey_id: i64,
    owner_user_id: Option<&str>,
    legs: &[DetailLeg],
    timings: Option<&JourneyTimings>,
    now: DateTime<Utc>,
) -> anyhow::Result<DetailExtras> {
    let pool = &app.database;
    let mut open = journey_breaks::open_break(pool, journey_id).await?;
    if let Some(user_id) = owner_user_id
        && let Some(timings) = timings
        && open.as_ref().is_none_or(|b| b.cause != "user_missed")
    {
        let ids: Vec<i64> = legs.iter().map(|l| l.id).collect();
        let mut conn = pool.acquire().await?;
        let travelled = journey_breaks::travelled_legs(&mut conn, &ids).await?;
        drop(conn);
        let facts: Vec<LegFacts> = legs
            .iter()
            .map(|leg| LegFacts {
                leg_id: leg.id,
                is_train: leg.is_train,
                trains_id: leg.trains_id,
                whole_cancelled: leg.cancelled,
                skipped: leg.skipped,
                travelled: travelled.contains(&leg.id),
            })
            .collect();
        match journey_breaks::detect(&facts, &timings.timings, &timings.connections) {
            Some(found) => {
                let detected = Detected {
                    cause: found.cause,
                    fault: "operator",
                    severity: found.cause.base_severity(),
                    trigger_leg_id: Some(found.trigger_leg_id),
                    trigger_trains_id: found.trigger_trains_id,
                    break_position: i32::try_from(found.index + 1)?,
                    fingerprint: journey_breaks::fingerprint(
                        found.cause,
                        found.index,
                        &timings.timings,
                    ),
                };
                let (row, _) =
                    journey_breaks::upsert_open(pool, journey_id, user_id, &detected, false)
                        .await?;
                open = Some(row);
            }
            None => {
                if let Some(row) = open.take() {
                    let mut conn = pool.acquire().await?;
                    journey_breaks::resolve(&mut conn, row.id, "recovered", None, None).await?;
                }
            }
        }
    }
    let summary = journey_revisions::summary(pool, journey_id).await?;
    let mut replaces = HashMap::new();
    for id in &summary.new_leg_ids {
        replaces.insert(*id, summary.replaced_leg_ids.clone());
    }
    let suggestions_muted = match owner_user_id {
        Some(_) => Some(journey_breaks::suggestions_muted(pool, journey_id).await?),
        None => None,
    };
    Ok(DetailExtras {
        open_break: open
            .as_ref()
            .map(|row| OpenBreakSummary::from_row(row, now)),
        revision_count: summary.revision_count,
        suggestions_muted,
        replaces,
    })
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AlternativesParams {
    cause: Option<AskCause>,
}

/// The cause and fault a request plans with: an open break's when it is on
/// this leg, else the asked cause's.
fn plan_cause(ask: AskCause, open: Option<&BreakRow>, leg_id: i64) -> (String, String) {
    if let Some(open) = open.filter(|b| b.trigger_leg_id == Some(leg_id)) {
        return (open.cause.clone(), open.fault.clone());
    }
    match ask {
        AskCause::Missed => ("user_missed".to_string(), "user".to_string()),
        AskCause::Cancelled => ("cancelled".to_string(), "operator".to_string()),
        AskCause::Connection => ("connection_missed".to_string(), "operator".to_string()),
        AskCause::Choice => ("user_choice".to_string(), "none".to_string()),
    }
}

/// Who is at fault for missing train leg `index` (§4.2, §14 Q5): the
/// railway when the change onto it no longer works on live times, else the
/// traveller. A first leg has no change onto it, so it is always the
/// traveller's.
fn missed_fault(analysis: &Analysis, index: usize) -> &'static str {
    let connection_onto = analysis
        .timings
        .as_ref()
        .and_then(|t| t.connections.get(index).cloned().flatten());
    journey_breaks::user_missed_fault(connection_onto.as_ref())
}

/// `GET /Journeys/{id}/legs/{legId}/alternatives?cause=missed|cancelled|connection|choice`:
/// ranked alternatives from one train leg, live, whatever the leg's
/// `windowSearched` (§7). Not stored, and never opens a break: "I missed
/// this train" shows these, and only taking one (`.../replace` with
/// `cause: missed`) records the leg as missed. Opening the sheet to look is
/// not saying the train was missed.
async fn get_alternatives(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, leg_id)): Path<(i64, i64)>,
    Query(params): Query<AlternativesParams>,
) -> Response {
    render(
        async {
            require_owner(&app, journey_id, &user.id).await?;
            let now = now_utc();
            let analysis = analyse(&app, journey_id, now)
                .await
                .map_err(internal("read journey"))?;
            let index = analysis
                .legs
                .iter()
                .position(|leg| leg.id == leg_id)
                .ok_or_else(not_found)?;
            if analysis.facts[index].travelled {
                return Err(leg_travelled());
            }
            let open = journey_breaks::open_break(&app.database, journey_id)
                .await
                .map_err(internal("read break"))?;
            let on_leg = open
                .as_ref()
                .is_some_and(|b| b.trigger_leg_id == Some(leg_id));
            let ask = params.cause.unwrap_or(AskCause::Choice);
            let (cause, mut fault) = plan_cause(ask, open.as_ref(), leg_id);
            if !on_leg && ask == AskCause::Missed {
                fault = missed_fault(&analysis, index).to_string();
            }
            let fingerprint = open
                .as_ref()
                .filter(|b| b.trigger_leg_id == Some(leg_id))
                .map(|b| b.fingerprint.clone())
                .unwrap_or_default();
            let set = plan_suggestions(
                &app,
                journey_id,
                &analysis,
                index,
                &cause,
                &fault,
                &fingerprint,
                ReplanSlot::Request,
            )
            .await?;
            Ok(Json(set).into_response())
        }
        .await,
    )
}

fn leg_travelled() -> RouteError {
    RouteError::coded(
        StatusCode::CONFLICT,
        "leg_travelled",
        "This leg is already travelled.",
    )
}

/// A break with its suggestions, as the break routes answer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BreakResponse {
    id: i64,
    journey_id: i64,
    cause: String,
    fault: String,
    severity: i16,
    status: String,
    trigger_leg_id: Option<i64>,
    detected_at: DateTime<Utc>,
    opened_via: Option<String>,
    /// Why the fault is what it is, when the data overrode the traveller
    /// (§14 Q5): "Your earlier train was late, so this counts as a missed
    /// connection."
    fault_note: Option<&'static str>,
    suggestions: Option<SuggestionSet>,
    stale: bool,
}

impl BreakResponse {
    fn from_row(row: BreakRow, now: DateTime<Utc>) -> Self {
        let set: Option<SuggestionSet> = row
            .suggestions
            .clone()
            .and_then(|value| serde_json::from_value(value).ok());
        let stale = set
            .as_ref()
            .is_none_or(|set| set.is_stale(&row.fingerprint, now));
        let suggestions = set.map(|mut set| {
            set.stale = stale;
            set
        });
        Self {
            fault_note: (row.cause == "user_missed" && row.fault == "operator")
                .then_some("Your earlier train was late, so this counts as a missed connection."),
            id: row.id,
            journey_id: row.journey_id,
            cause: row.cause,
            fault: row.fault,
            severity: row.severity,
            status: row.status,
            trigger_leg_id: row.trigger_leg_id,
            detected_at: row.detected_at,
            opened_via: row.opened_via,
            suggestions,
            stale,
        }
    }
}

/// Plans `row`'s suggestions now and stores them on it (raising its
/// severity to what they show, §6.2). Returns the updated row.
pub(crate) async fn replan_break(
    app: &App,
    row: &BreakRow,
    analysis: &Analysis,
    slot: ReplanSlot,
) -> Result<BreakRow, RouteError> {
    let index = row
        .trigger_leg_id
        .and_then(|id| analysis.legs.iter().position(|leg| leg.id == id))
        .ok_or_else(|| {
            RouteError::coded(
                StatusCode::CONFLICT,
                "break_gone",
                "This journey has changed since; reload it.",
            )
        })?;
    let set = plan_suggestions(
        app,
        row.journey_id,
        analysis,
        index,
        &row.cause,
        &row.fault,
        &row.fingerprint,
        slot,
    )
    .await?;
    let cause = Cause::parse(&row.cause).unwrap_or(Cause::ConnectionMissed);
    let severity = journey_breaks::severity(cause, set.best_delay());
    let value =
        serde_json::to_value(&set).map_err(|err| internal("encode suggestions")(err.into()))?;
    journey_breaks::store_suggestions(&app.database, row.id, &value, severity)
        .await
        .map_err(internal("store suggestions"))?;
    journey_breaks::get_break(&app.database, row.journey_id, row.id)
        .await
        .map_err(internal("read break"))?
        .ok_or_else(not_found)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentBreakParams {
    src: Option<String>,
}

/// `GET /Journeys/{id}/breaks/current[?src=push]`: the open break with its
/// stored suggestions (rendered at once, dimmed while `stale`, §5.4) and
/// `stale`. Stamps `opened_at` once, `opened_via` `push` with `src=push`
/// (the deep link), else `in_app`. 404 with none open.
async fn get_current_break(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
    Query(params): Query<CurrentBreakParams>,
) -> Response {
    render(
        async {
            require_owner(&app, journey_id, &user.id).await?;
            let row = journey_breaks::open_break(&app.database, journey_id)
                .await
                .map_err(internal("read break"))?
                .ok_or_else(|| {
                    RouteError::coded(StatusCode::NOT_FOUND, "no_break", "Nothing has broken.")
                })?;
            let via = if params.src.as_deref() == Some("push") {
                "push"
            } else {
                "in_app"
            };
            if journey_breaks::stamp_opened(&app.database, row.id, via)
                .await
                .map_err(internal("stamp break"))?
            {
                metrics::counter!(
                    common::metrics::metric_name("api_journey_break_opened_total"),
                    "via" => via
                )
                .increment(1);
            }
            Ok(Json(BreakResponse::from_row(row, now_utc())).into_response())
        }
        .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DismissBody {
    reason: String,
}

/// `POST /Journeys/{id}/breaks/{breakId}/dismiss` `{reason: willMakeIt |
/// notTravelling | other}`. 204; 404 when it is not this journey's open
/// break.
async fn post_dismiss(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, break_id)): Path<(i64, i64)>,
    Json(body): Json<DismissBody>,
) -> Response {
    render(
        async {
            if !["willMakeIt", "notTravelling", "other"].contains(&body.reason.as_str()) {
                return Err(RouteError::coded(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    "reason must be willMakeIt, notTravelling or other.",
                ));
            }
            require_owner(&app, journey_id, &user.id).await?;
            journey_breaks::get_break(&app.database, journey_id, break_id)
                .await
                .map_err(internal("read break"))?
                .filter(|row| row.status == "open")
                .ok_or_else(not_found)?;
            let mut conn = app
                .database
                .acquire()
                .await
                .map_err(|err| internal("dismiss break")(err.into()))?;
            journey_breaks::resolve(&mut conn, break_id, "dismissed", None, Some(&body.reason))
                .await
                .map_err(internal("dismiss break"))?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewBody {
    break_id: Option<i64>,
    from_leg_id: Option<i64>,
}

/// `POST /Journeys/{id}/replan/preview` `{breakId?, fromLegId?}`: plans the
/// open break's suggestions now and stores them on it (the page calls this
/// when stored ones are stale). With no open break, `fromLegId` plans a
/// free choice from that leg, not stored. Rate-limited like `/Trips/plan`.
async fn post_preview(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
    Json(body): Json<PreviewBody>,
) -> Response {
    render(
        async {
            require_owner(&app, journey_id, &user.id).await?;
            let now = now_utc();
            let analysis = analyse(&app, journey_id, now)
                .await
                .map_err(internal("read journey"))?;
            let open = journey_breaks::open_break(&app.database, journey_id)
                .await
                .map_err(internal("read break"))?;
            match (open, body.break_id, body.from_leg_id) {
                (Some(row), Some(id), _) if row.id == id => {
                    let row = replan_break(&app, &row, &analysis, ReplanSlot::Request).await?;
                    Ok(Json(BreakResponse::from_row(row, now)).into_response())
                }
                (Some(row), None, from) if from.is_none() || from == row.trigger_leg_id => {
                    let row = replan_break(&app, &row, &analysis, ReplanSlot::Request).await?;
                    Ok(Json(BreakResponse::from_row(row, now)).into_response())
                }
                (_, Some(_), _) => Err(RouteError::coded(
                    StatusCode::CONFLICT,
                    "break_gone",
                    "That break is no longer open; reload the journey.",
                )),
                (_, None, Some(leg_id)) => {
                    let index = analysis
                        .legs
                        .iter()
                        .position(|leg| leg.id == leg_id)
                        .ok_or_else(not_found)?;
                    let set = plan_suggestions(
                        &app,
                        journey_id,
                        &analysis,
                        index,
                        "user_choice",
                        "none",
                        "",
                        ReplanSlot::Request,
                    )
                    .await?;
                    Ok(Json(set).into_response())
                }
                (_, None, None) => Err(RouteError::coded(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    "Give a breakId or a fromLegId.",
                )),
            }
        }
        .await,
    )
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AcceptBody {
    break_id: i64,
    suggestion_id: String,
    /// The suggestions' fingerprint the page showed.
    #[serde(default)]
    fingerprint: Option<String>,
}

/// The `Idempotency-Key` header and the request's hash.
fn idempotency(
    headers: &HeaderMap,
    body: &impl Serialize,
) -> Result<Option<(String, String)>, RouteError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let key = value.to_str().unwrap_or_default().trim();
    if key.is_empty()
        || key.len() > crate::routes::journey_itinerary::MAX_IDEMPOTENCY_KEY_LENGTH
        || !key.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(RouteError::coded(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            "Idempotency-Key must be 1 to 200 visible ASCII characters.",
        ));
    }
    let canonical = serde_json::to_vec(body).map_err(|err| internal("hash request")(err.into()))?;
    Ok(Some((
        key.to_string(),
        format!("{:x}", Sha256::digest(&canonical)),
    )))
}

/// The answer to a replacement: 200 with a `ReplaceResponse`, or the
/// errors every replacement shares.
fn replaced_response(outcome: Replaced) -> Result<Response, RouteError> {
    match outcome {
        Replaced::Done { response, .. } => Ok(Json(response).into_response()),
        Replaced::Replay(value) => Ok(Json(value).into_response()),
        Replaced::NotFound => Err(not_found()),
        Replaced::BadRange => Err(RouteError::coded(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "throughLegId comes before the leg being replaced.",
        )),
        Replaced::Travelled => Err(leg_travelled()),
        Replaced::AtCap => Err(RouteError::coded(
            StatusCode::BAD_REQUEST,
            "too_many_legs",
            format!(
                "A journey has at most {} legs.",
                journeys::MAX_LEGS_PER_JOURNEY
            ),
        )),
        Replaced::IdempotencyConflict => Err(RouteError::coded(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "This Idempotency-Key was already used for a different request.",
        )),
    }
}

/// The trains of `suggestion`, found or created as shared `trains` rows.
async fn suggestion_trains(
    app: &App,
    suggestion: &Suggestion,
) -> Result<HashMap<(String, NaiveDate), i64>, RouteError> {
    let mut found = HashMap::new();
    for (uid, date) in suggestion.train_key() {
        let id = crate::data::trains::find_or_create_train(&app.database, &uid, date)
            .await
            .map_err(internal("find or create train"))?;
        found.insert((uid, date), id);
    }
    Ok(found)
}

/// `409 suggestion_stale` with the fresh suggestions in the body.
fn stale_response(row: BreakRow, now: DateTime<Utc>) -> Response {
    metrics::counter!(common::metrics::metric_name(
        "api_journey_replan_stale_total"
    ))
    .increment(1);
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "suggestion_stale",
            "message": "That train no longer works. Here are the latest options.",
            "break": BreakResponse::from_row(row, now),
        })),
    )
        .into_response()
}

/// `POST /Journeys/{id}/replan/accept` `{breakId, suggestionId, fingerprint}`
/// (optional `Idempotency-Key`): takes one suggestion (§4.4). When the
/// stored suggestions are stale they are planned again first; the chosen
/// one is found again by its trains, else `409 suggestion_stale` with the
/// fresh list. The booked plan is kept as a revision; the answer names the
/// revision to Undo to.
#[expect(
    clippy::too_many_lines,
    reason = "long but linear: check, re-plan if stale, replace, count"
)]
async fn post_accept(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
    headers: HeaderMap,
    Json(body): Json<AcceptBody>,
) -> Response {
    render(
        async {
            let idempotency = idempotency(&headers, &body)?;
            require_owner(&app, journey_id, &user.id).await?;
            let now = now_utc();
            let row = journey_breaks::get_break(&app.database, journey_id, body.break_id)
                .await
                .map_err(internal("read break"))?
                .ok_or_else(not_found)?;
            if row.status != "open" {
                // A repeat of an accept that already went through.
                if let Some((key, hash)) = &idempotency
                    && let Some(value) = journey_replace::replay(&app.database, &user.id, key, hash)
                        .await
                        .map_err(internal("replay accept"))?
                {
                    return Ok(Json(value).into_response());
                }
                return Err(RouteError::coded(
                    StatusCode::CONFLICT,
                    "break_gone",
                    "That break is no longer open; reload the journey.",
                ));
            }
            let stored: Option<SuggestionSet> = row
                .suggestions
                .clone()
                .and_then(|value| serde_json::from_value(value).ok());
            let chosen = stored
                .as_ref()
                .and_then(|set| set.suggestions.iter().find(|s| s.id == body.suggestion_id))
                .cloned();
            let Some(chosen) = chosen else {
                return Err(RouteError::coded(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "No such suggestion; reload the journey.",
                ));
            };
            let fresh_enough = stored.as_ref().is_some_and(|set| {
                !set.is_stale(&row.fingerprint, now)
                    && body
                        .fingerprint
                        .as_ref()
                        .is_none_or(|f| *f == set.fingerprint)
            }) && chosen
                .first_train_departure()
                .is_none_or(|departure| departure >= now - Duration::minutes(1));
            let (row, chosen, rank) = if fresh_enough {
                let rank = stored
                    .as_ref()
                    .and_then(|set| set.suggestions.iter().position(|s| s.id == chosen.id))
                    .unwrap_or(0);
                (row, chosen, rank)
            } else {
                let analysis = analyse(&app, journey_id, now)
                    .await
                    .map_err(internal("read journey"))?;
                let row = replan_break(&app, &row, &analysis, ReplanSlot::Request).await?;
                let fresh: Option<SuggestionSet> = row
                    .suggestions
                    .clone()
                    .and_then(|value| serde_json::from_value(value).ok());
                let key = chosen.train_key();
                let found = fresh.as_ref().and_then(|set| {
                    set.suggestions
                        .iter()
                        .position(|s| s.train_key() == key)
                        .map(|rank| (set.suggestions[rank].clone(), rank))
                });
                match found {
                    Some((suggestion, rank)) => (row, suggestion, rank),
                    None => return Ok(stale_response(row, now)),
                }
            };
            let trains = suggestion_trains(&app, &chosen).await?;
            let Some(new_legs) = chosen.itinerary_legs(&trains) else {
                return Err(internal("resolve suggestion legs")(anyhow::anyhow!(
                    "a suggestion train was not resolved"
                )));
            };
            let from_leg_id = stored
                .as_ref()
                .map_or(row.trigger_leg_id.unwrap_or_default(), |set| {
                    set.from_leg_id
                });
            let cause = Cause::parse(&row.cause).unwrap_or(Cause::ConnectionMissed);
            let outcome = journey_replace::replace(
                &app.database,
                &Replacement {
                    journey_id,
                    user_id: &user.id,
                    from_leg_id,
                    through_leg_id: Some(chosen.through_leg_id),
                    legs: &new_legs,
                    meta: RevisionMeta {
                        reason: Reason::Replace,
                        cause: row.cause.clone(),
                        fault: row.fault.clone(),
                        break_id: Some(row.id),
                        actor: "owner",
                    },
                    trigger_fate: cause.trigger_fate(),
                    now,
                    idempotency: idempotency
                        .as_ref()
                        .map(|(key, hash)| (key.as_str(), hash.as_str())),
                },
            )
            .await
            .map_err(internal("replace legs"))?;
            if matches!(outcome, Replaced::Done { .. }) {
                let pick = chosen
                    .picks
                    .first()
                    .map_or("more", |pick| pick.metric_label());
                let fit = chosen.fit.map_or("none", |fit| match fit {
                    alternative_ranking::Fit::Covered => "covered",
                    alternative_ranking::Fit::CoveredIfCancelled => "covered_if_cancelled",
                    alternative_ranking::Fit::Check => "check",
                    alternative_ranking::Fit::LikelyNewTicket => "likely_new_ticket",
                });
                metrics::counter!(
                    common::metrics::metric_name("api_journey_replan_accepted_total"),
                    "via" => row.opened_via.clone().unwrap_or_else(|| "in_app".to_string()),
                    "rank" => match rank { 0 => "1", 1 => "2", 2 => "3", _ => "more" },
                    "pick" => pick,
                    "fit" => fit
                )
                .increment(1);
            }
            replaced_response(outcome)
        }
        .await,
    )
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplaceBody {
    cause: AskCause,
    legs: Vec<crate::routes::journey_itinerary::ItineraryLegRequest>,
    #[serde(default)]
    through_leg_id: Option<i64>,
}

/// `POST /Journeys/{id}/legs/{legId}/replace` `{cause, legs, throughLegId?}`
/// (optional `Idempotency-Key`): replaces the leg (through `throughLegId`)
/// with `legs`, each as `POST /Journeys/itinerary` takes them. With a break
/// open on the leg, it is taken as that break's alternative; otherwise it
/// is the traveller's own choice (§4.2's `user_choice`), which before the
/// journey starts just updates the booked plan (§4.3).
async fn post_replace(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, leg_id)): Path<(i64, i64)>,
    headers: HeaderMap,
    Json(body): Json<ReplaceBody>,
) -> Response {
    render(
        async {
            let idempotency = idempotency(&headers, &body)?;
            if body.legs.is_empty()
                || i64::try_from(body.legs.len()).unwrap_or(i64::MAX)
                    > journeys::MAX_LEGS_PER_JOURNEY
            {
                return Err(RouteError::coded(
                    StatusCode::BAD_REQUEST,
                    "invalid_itinerary",
                    format!(
                        "Replace with between 1 and {} legs.",
                        journeys::MAX_LEGS_PER_JOURNEY
                    ),
                ));
            }
            let today = crate::routes::london_today();
            let mut valid = Vec::with_capacity(body.legs.len());
            for leg in &body.legs {
                valid.push(crate::routes::journey_itinerary::validate_leg(leg, today)?);
            }
            require_owner(&app, journey_id, &user.id).await?;
            crate::routes::journey_itinerary::check_legs_call(&app, &valid).await?;
            let resolved = crate::routes::journey_itinerary::resolve_legs(&app, &valid).await?;
            let open = journey_breaks::open_break(&app.database, journey_id)
                .await
                .map_err(internal("read break"))?
                .filter(|row| row.trigger_leg_id == Some(leg_id));
            let (meta, fate) = if let Some(row) = &open {
                (
                    RevisionMeta {
                        reason: Reason::Replace,
                        cause: row.cause.clone(),
                        fault: row.fault.clone(),
                        break_id: Some(row.id),
                        actor: "owner",
                    },
                    Cause::parse(&row.cause).map_or("not_used", Cause::trigger_fate),
                )
            } else {
                let (cause, fault) = plan_cause(body.cause, None, leg_id);
                let fault = if body.cause == AskCause::Missed {
                    // Taking another train after "I missed this train" is
                    // what records the leg as missed; the data decides
                    // whose fault it was, as for a break (§4.2).
                    let analysis = analyse(&app, journey_id, now_utc())
                        .await
                        .map_err(internal("read journey"))?;
                    let index = analysis
                        .legs
                        .iter()
                        .position(|leg| leg.id == leg_id)
                        .ok_or_else(not_found)?;
                    missed_fault(&analysis, index).to_string()
                } else if fault == "user" {
                    fault
                } else {
                    "none".to_string()
                };
                let fate = match body.cause {
                    AskCause::Missed | AskCause::Connection => "missed",
                    AskCause::Cancelled => "cancelled",
                    AskCause::Choice => "not_used",
                };
                (
                    RevisionMeta {
                        reason: Reason::Replace,
                        cause: if cause == "user_missed" || cause == "user_choice" {
                            cause
                        } else {
                            // A cause with no break open is the
                            // traveller's own call.
                            "user_choice".to_string()
                        },
                        fault,
                        break_id: None,
                        actor: "owner",
                    },
                    fate,
                )
            };
            let outcome = journey_replace::replace(
                &app.database,
                &Replacement {
                    journey_id,
                    user_id: &user.id,
                    from_leg_id: leg_id,
                    through_leg_id: body.through_leg_id,
                    legs: &resolved,
                    meta,
                    trigger_fate: fate,
                    now: now_utc(),
                    idempotency: idempotency
                        .as_ref()
                        .map(|(key, hash)| (key.as_str(), hash.as_str())),
                },
            )
            .await
            .map_err(internal("replace legs"))?;
            replaced_response(outcome)
        }
        .await,
    )
}

/// `GET /Journeys/{id}/revisions`: every plan, newest first, each with its
/// legs and their fates (§7, §8.2 "Earlier plans").
async fn get_revisions(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
) -> Response {
    render(
        async {
            require_owner(&app, journey_id, &user.id).await?;
            let revisions = journey_revisions::list_revisions(&app.database, journey_id)
                .await
                .map_err(internal("list revisions"))?;
            let now = now_utc();
            let latest_at = revisions.first().map(|r| r.created_at);
            let latest_no = revisions.first().map(|r| r.revision_no);
            let body: Vec<serde_json::Value> = revisions
                .iter()
                .map(|revision| {
                    let mut value = serde_json::to_value(revision).unwrap_or_default();
                    value["canRevert"] = serde_json::Value::Bool(
                        Some(revision.revision_no) != latest_no
                            && revert_allowed(revision, latest_no, latest_at, now),
                    );
                    value
                })
                .collect();
            Ok(Json(serde_json::json!({ "revisions": body })).into_response())
        }
        .await,
    )
}

/// Whether the plan `revision` can be gone back to (§14 Q12): the one just
/// replaced, within [`UNDO_WINDOW`] of the change; any earlier one while its
/// trains are all still to leave.
fn revert_allowed(
    revision: &journey_revisions::Revision,
    latest_no: Option<i32>,
    latest_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    if latest_no == Some(revision.revision_no + 1)
        && latest_at.is_some_and(|at| now - at <= UNDO_WINDOW)
    {
        return true;
    }
    revision.legs.iter().all(|leg| {
        leg.kind != journeys::LEG_KIND_TRAIN || leg.booked_departure.is_some_and(|d| d > now)
    })
}

/// `POST /Journeys/{id}/revisions/{no}/revert` (optional
/// `Idempotency-Key`): back to plan `no` (the sheet's Undo), written as a
/// new revision. The legs from the first one that differs are replaced by
/// that plan's; a break the undone change accepted is open again. `409
/// revision_gone` once that plan no longer works (§14 Q12).
#[expect(
    clippy::too_many_lines,
    reason = "long but linear: find, check, rebuild the legs, replace, reopen"
)]
async fn post_revert(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, revision_no)): Path<(i64, i32)>,
    headers: HeaderMap,
) -> Response {
    render(
        async {
            let idempotency = idempotency(&headers, &revision_no)?;
            require_owner(&app, journey_id, &user.id).await?;
            let now = now_utc();
            let revisions = journey_revisions::list_revisions(&app.database, journey_id)
                .await
                .map_err(internal("list revisions"))?;
            let latest = revisions.first().ok_or_else(not_found)?;
            let target = revisions
                .iter()
                .find(|r| r.revision_no == revision_no && r.revision_no != latest.revision_no)
                .ok_or_else(not_found)?;
            if !revert_allowed(
                target,
                Some(latest.revision_no),
                Some(latest.created_at),
                now,
            ) {
                return Err(RouteError::coded(
                    StatusCode::CONFLICT,
                    "revision_gone",
                    "That plan no longer works.",
                ));
            }
            let current = journeys::list_legs_for_journey(&app.database, journey_id)
                .await
                .map_err(internal("list legs"))?;
            // The first position where the plans differ; legs before it stay.
            let first_diff = current
                .iter()
                .zip(&target.legs)
                .position(|(leg, old)| old.source_leg_id != Some(leg.id))
                .unwrap_or(current.len().min(target.legs.len()));
            let (Some(from), Some(through)) = (current.get(first_diff), current.last()) else {
                return Err(RouteError::coded(
                    StatusCode::CONFLICT,
                    "revision_gone",
                    "That plan no longer works.",
                ));
            };
            let mut legs = Vec::new();
            for old in &target.legs[first_diff.min(target.legs.len())..] {
                if old.kind == journeys::LEG_KIND_TRAIN {
                    let Some(uid) = &old.train_uid else {
                        return Err(RouteError::coded(
                            StatusCode::CONFLICT,
                            "revision_gone",
                            "That plan has a leg with no train.",
                        ));
                    };
                    let trains_id = crate::data::trains::find_or_create_train(
                        &app.database,
                        uid,
                        old.service_date,
                    )
                    .await
                    .map_err(internal("find or create train"))?;
                    legs.push(journeys::ItineraryLeg::Train {
                        trains_id,
                        service_date: old.service_date,
                        origin_crs: old.origin_crs.clone(),
                        destination_crs: old.destination_crs.clone(),
                    });
                } else {
                    let (Some(departure), Some(arrival), Some(mode)) = (
                        old.planned_departure,
                        old.planned_arrival,
                        &old.transit_mode,
                    ) else {
                        continue;
                    };
                    legs.push(journeys::ItineraryLeg::Untracked(journeys::UntrackedLeg {
                        transit_mode: mode.clone(),
                        origin_place: old.origin_place.clone(),
                        destination_place: old.destination_place.clone(),
                        line_name: old.line_name.clone(),
                        planned_departure: departure,
                        planned_arrival: arrival,
                        origin_stop_id: None,
                        destination_stop_id: None,
                    }));
                }
            }
            let reopen = latest.break_id;
            let outcome = journey_replace::replace(
                &app.database,
                &Replacement {
                    journey_id,
                    user_id: &user.id,
                    from_leg_id: from.id,
                    through_leg_id: Some(through.id),
                    legs: &legs,
                    meta: RevisionMeta {
                        reason: Reason::Revert,
                        cause: "user_choice".to_string(),
                        fault: "none".to_string(),
                        break_id: None,
                        actor: "owner",
                    },
                    trigger_fate: "not_used",
                    now,
                    idempotency: idempotency
                        .as_ref()
                        .map(|(key, hash)| (key.as_str(), hash.as_str())),
                },
            )
            .await
            .map_err(internal("revert"))?;
            if matches!(outcome, Replaced::Done { .. })
                && let Some(break_id) = reopen
            {
                let mut conn = app
                    .database
                    .acquire()
                    .await
                    .map_err(|err| internal("reopen break")(err.into()))?;
                journey_breaks::reopen(&mut conn, break_id)
                    .await
                    .map_err(internal("reopen break"))?;
            }
            replaced_response(outcome)
        }
        .await,
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MutedBody {
    muted: bool,
}

/// `PUT /Journeys/{id}/suggestions-muted` `{muted}`: "Stop suggestions for
/// this journey" (§6.5). Pushes only; the banner still shows. 204.
async fn put_suggestions_muted(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
    Json(body): Json<MutedBody>,
) -> Response {
    render(
        async {
            if journey_breaks::set_suggestions_muted(
                &app.database,
                journey_id,
                &user.id,
                body.muted,
            )
            .await
            .map_err(internal("mute suggestions"))?
            {
                Ok(StatusCode::NO_CONTENT.into_response())
            } else {
                Err(not_found())
            }
        }
        .await,
    )
}

/// Every alternative's response field, for the changelog's sake: the set
/// as served (`SuggestionSet`) and the replace answer
/// (`ReplaceResponse`) are the wire types.
#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn a_replace_answer_names_the_undo_revision() {
        let value = serde_json::to_value(journey_replace::ReplaceResponse {
            journey_id: 4,
            revision_no: 3,
            new_leg_ids: vec![9],
            undo_revision_no: Some(2),
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "journeyId": 4, "revisionNo": 3, "newLegIds": [9], "undoRevisionNo": 2
            })
        );
    }

    #[test]
    fn plan_cause_follows_an_open_break_on_the_leg() {
        assert_eq!(
            plan_cause(AskCause::Missed, None, 1),
            ("user_missed".to_string(), "user".to_string())
        );
        assert_eq!(
            plan_cause(AskCause::Choice, None, 1),
            ("user_choice".to_string(), "none".to_string())
        );
    }
}

#[cfg(test)]
mod db_tests;
