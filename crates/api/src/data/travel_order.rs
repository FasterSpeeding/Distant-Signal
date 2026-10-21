//! The order `/track/mine`'s own lists come in: by when the travel happens,
//! not when it was saved (DS-112). `GET /Journeys/mine` and `GET
//! /Train/mine` both serve, in SQL so their `MINE_LIST_LIMIT` keeps the
//! rows that matter:
//!
//! 1. what is still to come or under way, soonest departure first (so a
//!    journey or train in progress leads, then the next one out);
//! 2. then what has been travelled, most recent departure first;
//!
//! with the row's id, newest first, breaking a tie.
//!
//! "Travelled" is: the last arrival (the timetabled one, plus the train's
//! running delay) is in the past, or every leg is finished (a train leg's
//! `train_current_state.status` is `completed`, an untracked leg's planned
//! arrival has passed).
//!
//! Timetabled times come from `schedule_destination_departures`, the public
//! (GBTT) time where the timetable has one, else the working one. That
//! table only holds the service dates schedule-reference has published and
//! not yet replaced, so each caller falls back (see each list's own doc
//! comment), finally to the start or the end of the service date. Each
//! lookup is one probe of
//! `schedule_destination_departures_train_uid_service_date_scheduled_idx`,
//! and an arrival is only looked up for a train running today or yesterday
//! (London): one dated earlier has arrived, one dated later has not.

/// The London local start of `date` (a SQL `date` expression), as a
/// `timestamptz`: the departure of a leg or train whose time is unknown.
pub(crate) fn day_start_sql(date: &str) -> String {
    format!("(({date})::timestamp AT TIME ZONE 'Europe/London')")
}

/// Whether train `uid` on service date `date` has reached station `crs`
/// (SQL expressions): its timetabled arrival there plus `delay_minutes`
/// (its running delay, a SQL `int` expression, `NULL` for none; early
/// running is ignored) has passed, else, with no timetabled arrival, the
/// service date (London) is over. Never `NULL`.
pub(crate) fn train_arrived_sql(uid: &str, date: &str, crs: &str, delay_minutes: &str) -> String {
    let today = "(now() AT TIME ZONE 'Europe/London')::date";
    let arrival = scheduled_arrival_sql(uid, date, crs);
    format!(
        "(CASE WHEN ({date}) < {today} - 1 THEN TRUE \
               WHEN ({date}) > {today} THEN FALSE \
               ELSE COALESCE({arrival} \
                                 + GREATEST(COALESCE({delay_minutes}, 0), 0) * INTERVAL '1 minute', \
                             ((({date}) + 1)::timestamp AT TIME ZONE 'Europe/London')) <= now() \
          END)"
    )
}

/// The timetabled departure of train `uid` on service date `date` from
/// station `crs` (SQL expressions), as a `timestamptz`: the public time,
/// else the working one. `NULL` when the timetable for that date is not
/// held or the train does not call there.
pub(crate) fn scheduled_departure_sql(uid: &str, date: &str, crs: &str) -> String {
    format!(
        "(SELECT ((sdd_dep.service_date + sdd_dep.day_offset) \
                  + COALESCE(sdd_dep.public_departure, sdd_dep.scheduled)) \
                 AT TIME ZONE 'Europe/London' \
          FROM schedule_destination_departures sdd_dep \
          WHERE sdd_dep.train_uid = ({uid}) AND sdd_dep.service_date = ({date}) \
            AND sdd_dep.origin_crs = UPPER({crs}) \
          ORDER BY sdd_dep.day_offset, sdd_dep.scheduled \
          LIMIT 1)"
    )
}

/// The timetabled arrival of train `uid` on service date `date` at station
/// `crs` (SQL expressions), as a `timestamptz`: the public time, else the
/// working one. An intermediate call is read from its own row; the
/// terminus, which has no departure row of its own, from any row's
/// `destination_arrival`. `NULL` when neither is held.
fn scheduled_arrival_sql(uid: &str, date: &str, crs: &str) -> String {
    format!(
        "COALESCE( \
            (SELECT ((sdd_call.service_date + sdd_call.day_offset) \
                     + COALESCE(sdd_call.public_calling_point_arrival, \
                                sdd_call.calling_point_arrival)) \
                    AT TIME ZONE 'Europe/London' \
             FROM schedule_destination_departures sdd_call \
             WHERE sdd_call.train_uid = ({uid}) AND sdd_call.service_date = ({date}) \
               AND sdd_call.origin_crs = UPPER({crs}) \
               AND COALESCE(sdd_call.public_calling_point_arrival, \
                            sdd_call.calling_point_arrival) IS NOT NULL \
             ORDER BY sdd_call.day_offset, sdd_call.scheduled \
             LIMIT 1), \
            (SELECT ((sdd_end.service_date + sdd_end.destination_arrival_day_offset) \
                     + COALESCE(sdd_end.public_destination_arrival, \
                                sdd_end.destination_arrival)) \
                    AT TIME ZONE 'Europe/London' \
             FROM schedule_destination_departures sdd_end \
             WHERE sdd_end.train_uid = ({uid}) AND sdd_end.service_date = ({date}) \
               AND sdd_end.destination_crs = UPPER({crs}) \
               AND COALESCE(sdd_end.public_destination_arrival, \
                            sdd_end.destination_arrival) IS NOT NULL \
             LIMIT 1))"
    )
}

/// The `ORDER BY` keys both lists share, over a `travelled` boolean and a
/// `departs_at` time or date (SQL expressions): still to come soonest
/// first, then travelled most recent first. The caller appends its id,
/// newest first, as the tie-break.
pub(crate) fn order_by_sql(travelled: &str, departs_at: &str) -> String {
    format!(
        "{travelled} ASC, \
         CASE WHEN {travelled} THEN NULL ELSE {departs_at} END ASC, \
         {departs_at} DESC"
    )
}
