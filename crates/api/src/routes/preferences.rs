//! `/public/preferences`: which lines/stations/operators are pinned to the
//! home page. Fully session-gated, both read and write -- unlike `/public/lines`,
//! whose *reads* stay unauthenticated (see
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
//! Non-goals), pinned lines/stations are per-user state with no useful
//! anonymous reading, so every handler here requires a resolved session.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::auth::delegated::{DelegatedOrSessionUser, PreferencesReadScope};
use crate::data::{custom_lines, notification_settings, preferences, queries, reference};

pub fn router() -> Router {
    Router::new()
        .route("/preferences", axum::routing::get(get_preferences))
        .route(
            "/preferences/pinned-lines",
            axum::routing::put(put_pinned_lines),
        )
        .route(
            "/preferences/pinned-lines/{line_id}/alerts",
            axum::routing::put(put_pinned_line_alerts),
        )
        .route(
            "/preferences/pinned-stations",
            axum::routing::put(put_pinned_stations),
        )
        .route(
            "/preferences/pinned-operators",
            axum::routing::put(put_pinned_operators),
        )
        .route(
            "/preferences/notifications",
            axum::routing::get(get_notification_preferences).put(put_notification_preferences),
        )
}

/// `GET`/`PUT /public/preferences/notifications`: the user's push
/// settings beyond the browser's own permission (mid-journey changes spec
/// 2026-10-22 §6.5; DS-153). A switch per kind of push, each on unless
/// turned off: `journeySuggestions` (suggested trains when a journey
/// breaks), `lineStatus`, `trainDelay`, `trainCancel`, `predictedStart`,
/// `stationSkip` and `templateAttention`; `delayThresholdMinutes` (5, 10,
/// 15 or 30; `null` for the default, 15); `quietHours` (`{start, end,
/// timezone}`, `HH:MM` local times and an IANA zone, or `null` for none);
/// `snoozedUntil` (pushes held until then as in quiet hours, at most a
/// week ahead; `null` when not snoozed).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one switch per kind of push, as the wire has them"
)]
struct NotificationPreferences {
    journey_suggestions: bool,
    line_status: bool,
    train_delay: bool,
    train_cancel: bool,
    predicted_start: bool,
    station_skip: bool,
    template_attention: bool,
    delay_threshold_minutes: Option<i16>,
    quiet_hours: Option<QuietHoursBody>,
    snoozed_until: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuietHoursBody {
    start: String,
    end: String,
    timezone: String,
}

/// A `PUT` body: only the fields present change, so a client that knows
/// only `journeySuggestions` keeps working. `null` clears the threshold
/// (back to the default) or the quiet hours. Unknown fields are refused.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[expect(
    clippy::option_option,
    reason = "absent (unchanged) and null (cleared) are different requests"
)]
struct NotificationPreferencesUpdate {
    journey_suggestions: Option<bool>,
    line_status: Option<bool>,
    train_delay: Option<bool>,
    train_cancel: Option<bool>,
    predicted_start: Option<bool>,
    station_skip: Option<bool>,
    template_attention: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    delay_threshold_minutes: Option<Option<i16>>,
    #[serde(default, deserialize_with = "present")]
    quiet_hours: Option<Option<QuietHoursBody>>,
    #[serde(default, deserialize_with = "present")]
    snoozed_until: Option<Option<chrono::DateTime<chrono::Utc>>>,
}

/// A field that is present, `null` or not: `Some(None)` for `null`.
#[expect(
    clippy::option_option,
    reason = "absent (unchanged) and null (cleared) are different requests"
)]
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer).map(Some)
}

const QUIET_TIME_FORMAT: &str = "%H:%M";

fn parse_quiet_time(value: &str) -> Option<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(value.trim(), QUIET_TIME_FORMAT).ok()
}

/// Checks a `PUT` body and turns it into the change to store, or the
/// sentence a 400 says.
fn validate_notification_update(
    body: NotificationPreferencesUpdate,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<notification_settings::NotificationSettingsChange, String> {
    if let Some(Some(until)) = body.snoozed_until
        && until > now + notification_settings::MAX_SNOOZE
    {
        return Err("Notifications can be paused for up to 7 days.".to_string());
    }
    if let Some(Some(minutes)) = body.delay_threshold_minutes
        && !notification_settings::DELAY_THRESHOLDS.contains(&minutes)
    {
        return Err("The delay threshold must be 5, 10, 15 or 30 minutes.".to_string());
    }
    let quiet_hours = match body.quiet_hours {
        None => None,
        Some(None) => Some(None),
        Some(Some(quiet)) => {
            let (Some(start), Some(end)) =
                (parse_quiet_time(&quiet.start), parse_quiet_time(&quiet.end))
            else {
                return Err("Quiet hours need a start and an end time, such as 22:00.".to_string());
            };
            if start == end {
                return Err("Quiet hours can't start and end at the same time.".to_string());
            }
            let timezone = quiet.timezone.trim();
            if timezone.parse::<chrono_tz::Tz>().is_err() {
                return Err("That time zone isn't one we know.".to_string());
            }
            Some(Some(notification_settings::QuietHours {
                start,
                end,
                timezone: timezone.to_string(),
            }))
        }
    };
    Ok(notification_settings::NotificationSettingsChange {
        journey_suggestions: body.journey_suggestions,
        line_status: body.line_status,
        train_delay: body.train_delay,
        train_cancel: body.train_cancel,
        predicted_start: body.predicted_start,
        station_skip: body.station_skip,
        template_attention: body.template_attention,
        delay_threshold_minutes: body.delay_threshold_minutes,
        quiet_hours,
        // A time already past is no snooze: stored as such, read as null.
        snoozed_until: body.snoozed_until,
    })
}

impl From<notification_settings::NotificationSettings> for NotificationPreferences {
    fn from(settings: notification_settings::NotificationSettings) -> Self {
        Self {
            journey_suggestions: settings.journey_suggestions,
            line_status: settings.line_status,
            train_delay: settings.train_delay,
            train_cancel: settings.train_cancel,
            predicted_start: settings.predicted_start,
            station_skip: settings.station_skip,
            template_attention: settings.template_attention,
            delay_threshold_minutes: settings.delay_threshold_minutes,
            quiet_hours: settings.quiet_hours.map(|quiet| QuietHoursBody {
                start: quiet.start.format(QUIET_TIME_FORMAT).to_string(),
                end: quiet.end.format(QUIET_TIME_FORMAT).to_string(),
                timezone: quiet.timezone,
            }),
            snoozed_until: settings.snoozed_until,
        }
    }
}

/// Also a delegated route (`preferences:read`, DS-153): a connected app the
/// user granted it may read these settings, never change them.
async fn get_notification_preferences(
    State(app): State<App>,
    user: DelegatedOrSessionUser<PreferencesReadScope>,
) -> Result<Json<NotificationPreferences>, (StatusCode, String)> {
    let settings = notification_settings::get(&app.database, &user.user_id)
        .await
        .map_err(internal_error)?;
    Ok(Json(settings.into()))
}

/// `PUT`: applies the fields present and answers with every setting.
async fn put_notification_preferences(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(body): Json<NotificationPreferencesUpdate>,
) -> Result<Json<NotificationPreferences>, (StatusCode, String)> {
    let change = validate_notification_update(body, chrono::Utc::now())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let settings = notification_settings::update(&app.database, &user.id, &change)
        .await
        .map_err(internal_error)?;
    Ok(Json(settings.into()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreferencesResponse {
    pinned_lines: Vec<String>,
    pinned_stations: Vec<String>,
    pinned_operators: Vec<String>,
    /// Of `pinned_lines`, the ones whose status pushes are off
    /// (`PUT /preferences/pinned-lines/{lineId}/alerts`, DS-153). Additive.
    silent_pinned_lines: Vec<String>,
}

async fn get_preferences(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<PreferencesResponse>, (StatusCode, String)> {
    let pinned_line_ids = preferences::list_pinned_line_ids(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    // Only the custom lines THIS caller may read count as "known" here.
    // `PUT /preferences/pinned-lines` validates nothing (see
    // `preferences::replace_pinned_lines`), so resolving pins against the
    // instance-wide `list_custom_lines` made this route an existence
    // oracle: pin a guessed id -- and ids are deterministic slugs of
    // user-chosen names (`custom_lines::slugify`) -- and it echoed back iff
    // somebody on the instance really owned a line by that name. Scoping
    // the lookup to the pinned ids also replaces a full-table read with one
    // indexed lookup, and still lets a group-shared line stay pinned, which
    // `list_custom_lines_for_user` alone would not.
    let pinned_custom_ids: Vec<String> = pinned_line_ids
        .iter()
        .filter(|id| id.starts_with(custom_lines::CUSTOM_LINE_ID_PREFIX))
        .cloned()
        .collect();
    let readable_custom_ids = if pinned_custom_ids.is_empty() {
        HashSet::new()
    } else {
        custom_lines::readable_custom_line_ids(&app.database, &pinned_custom_ids, &user.id)
            .await
            .map_err(internal_error)?
    };
    // TfL lines live in neither `app.config.lines` (the static catalogue)
    // nor `custom_lines` -- they're ingested straight into `line_status`
    // with `source = 'tfl'` (see `queries::upsert_tfl_line_status`).
    // Without this, a pin on a TfL line (e.g. `tfl-victoria`) is written
    // fine by `replace_pinned_lines` -- which validates nothing -- but
    // silently dropped here on every read, so it renders unstarred again
    // after the next fetch/reload.
    let tfl = queries::tfl_line_summaries(&app.database)
        .await
        .map_err(internal_error)?;
    // Transit lines (`metrolink:blue`, transit spec §12.2) likewise: known
    // while their network is served and has the line.
    let transit = queries::transit_line_ids(
        &app.database,
        &pinned_line_ids,
        &app.config.transit_networks,
    )
    .await
    .map_err(internal_error)?;
    let pinned_lines = filter_known_pinned_lines(
        pinned_line_ids,
        app.config.lines.iter().map(|l| l.id.clone()),
        readable_custom_ids,
        tfl.into_iter().map(|l| l.id).chain(transit),
    );
    let silent_pinned_lines = preferences::list_silent_pinned_line_ids(&app.database, &user.id)
        .await
        .map_err(internal_error)?
        .into_iter()
        .filter(|id| pinned_lines.contains(id))
        .collect();

    let pinned_station_candidates = preferences::list_pinned_station_crs(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    let pinned_stations =
        preferences::filter_existing_station_crs(&app.database, &pinned_station_candidates)
            .await
            .map_err(internal_error)?;

    let pinned_operator_codes = preferences::list_pinned_operator_codes(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    // Every real ATOC code plus the synthetic "TfL" row is "known" here --
    // unlike `filter_known_pinned_lines`, there is no ownership/visibility
    // distinction to make (an operator code is a public reference concept,
    // not a private or group-scoped resource), so this filter exists purely
    // to drop a stale/foreign code, the same hygiene role
    // `filter_existing_station_crs` plays for stations.
    let tocs = reference::get_all_tocs(&app.database)
        .await
        .map_err(internal_error)?;
    let known_operator_codes = tocs
        .into_iter()
        .map(|t| t.code)
        .chain(std::iter::once(common::TFL_OPERATOR.to_string()));
    let pinned_operators =
        filter_known_pinned_operators(pinned_operator_codes, known_operator_codes);

    Ok(Json(PreferencesResponse {
        pinned_lines,
        pinned_stations,
        pinned_operators,
        silent_pinned_lines,
    }))
}

/// Upper bound on how many ids/codes a single `PUT /preferences/pinned-*`
/// body may contain, enforced by [`reject_if_over_pin_limit`] before any of
/// these ever reach `preferences::replace_pinned_*`.
///
/// There is no legitimate reason for a real user to pin thousands of
/// lines/stations/operators -- the whole catalogue of real lines, stations,
/// and operator codes is itself only in the hundreds -- so 500 is generous
/// headroom over any real use while still capping the row-by-row insert
/// cost of `replace_pinned_*`'s per-element loop (see that function's own
/// doc comment) to something bounded and cheap, repeatable per request by
/// any logged-in user.
const MAX_PINNED_ITEMS: usize = 500;

/// Returns a clean `400` when `items` exceeds [`MAX_PINNED_ITEMS`], so an
/// oversized `PUT` body is rejected before it ever reaches
/// `preferences::replace_pinned_*`'s unbounded per-row insert loop, rather
/// than being accepted and paid for as thousands of single-row inserts in
/// one transaction.
fn reject_if_over_pin_limit(items: &[String], noun: &str) -> Result<(), (StatusCode, String)> {
    if items.len() > MAX_PINNED_ITEMS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "too many pinned {noun} ({} submitted, {MAX_PINNED_ITEMS} max)",
                items.len()
            ),
        ));
    }
    if items.iter().any(|item| item.len() > MAX_PINNED_ID_LENGTH) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("a pinned {noun} id is too long ({MAX_PINNED_ID_LENGTH} bytes max)"),
        ));
    }
    Ok(())
}

/// Upper bound, in bytes, on any single id/code in a
/// `PUT /preferences/pinned-*` body (2026-09-26 review, L10).
/// [`MAX_PINNED_ITEMS`] bounded how MANY ids one request could store, but
/// each id was an unvalidated free-form string (`pinned_lines`/
/// `pinned_operators` don't validate against any catalogue on write -- see
/// `preferences::replace_pinned_lines`), so 500 multi-megabyte strings were
/// still accepted and persisted. Real ids are short: the longest catalogue
/// line id is under 50 bytes, operator codes are 2-3, and a custom line's
/// id is `custom-` plus a slug of its name. 256 leaves generous room for a
/// long custom-line name's slug.
const MAX_PINNED_ID_LENGTH: usize = 256;

async fn put_pinned_lines(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(ids): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    reject_if_over_pin_limit(&ids, "lines")?;

    preferences::replace_pinned_lines(&app.database, &user.id, &ids)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /preferences/pinned-lines/{lineId}/alerts` `{notify}`: whether a
/// pinned line's status changes are pushed (DS-153). Pinning and alerting
/// are separate: a line can stay on the home page without pushes. 204;
/// 404 when the caller has not pinned that line. Unknown fields are
/// refused.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PinnedLineAlerts {
    notify: bool,
}

async fn put_pinned_line_alerts(
    State(app): State<App>,
    user: AuthenticatedUser,
    axum::extract::Path(line_id): axum::extract::Path<String>,
    Json(body): Json<PinnedLineAlerts>,
) -> Result<StatusCode, (StatusCode, String)> {
    if line_id.len() > MAX_PINNED_ID_LENGTH {
        return Err((StatusCode::NOT_FOUND, "That line isn't pinned.".to_string()));
    }
    if preferences::set_pinned_line_notify(&app.database, &user.id, &line_id, body.notify)
        .await
        .map_err(internal_error)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "That line isn't pinned.".to_string()))
    }
}

async fn put_pinned_stations(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(crs_codes): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    reject_if_over_pin_limit(&crs_codes, "stations")?;

    if crs_codes.iter().any(|crs| crs.len() != 3) {
        return Err((
            StatusCode::BAD_REQUEST,
            "station codes must be exactly 3 characters".to_string(),
        ));
    }

    preferences::replace_pinned_stations(&app.database, &user.id, &crs_codes)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn put_pinned_operators(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(codes): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    reject_if_over_pin_limit(&codes, "operators")?;

    preferences::replace_pinned_operators(&app.database, &user.id, &codes)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used as a map_err callback, which passes the error by value"
)]
fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    if let Some(unavailable) = crate::unavailable::response_for(&err) {
        return unavailable;
    }
    tracing::error!(error = ?err, "preferences operation failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}

/// Filters `pinned_line_ids` down to ones that still resolve to a real
/// line, dropping stale ids for lines that have since been removed/renamed.
/// A line is "real" if it appears in the static catalogue, among the custom
/// lines THE CALLER MAY READ, or among the `TfL` lines `crates/poller-tfl` has
/// ingested -- all three are valid targets of
/// `PUT /preferences/pinned-lines`, which itself validates nothing (see
/// `preferences::replace_pinned_lines`), so this is the only place a stale
/// or foreign id gets caught.
///
/// `custom_ids` being caller-scoped is load-bearing, not incidental: an
/// instance-wide custom-line list here turns an unvalidated pin into a
/// probe for whether another user owns a line with a given id. See
/// `get_preferences`.
///
/// Factored out of `get_preferences` so the "`TfL` ids count as known" rule
/// is unit-testable without a database, unlike the three id sources
/// themselves, which each need one to produce for real.
fn filter_known_pinned_lines(
    pinned_line_ids: Vec<String>,
    catalogue_ids: impl IntoIterator<Item = String>,
    custom_ids: impl IntoIterator<Item = String>,
    tfl_ids: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let known_line_ids: HashSet<String> = catalogue_ids
        .into_iter()
        .chain(custom_ids)
        .chain(tfl_ids)
        .collect();
    pinned_line_ids
        .into_iter()
        .filter(|id| known_line_ids.contains(id))
        .collect()
}

/// Filters `pinned_codes` down to ones that still resolve to a real
/// operator -- a real `tocs` row's code, or the synthetic `"TfL"` row.
/// Unlike `filter_known_pinned_lines`, there is no per-caller visibility
/// distinction: every operator code is public reference data, so a single
/// flat known-codes set (not a caller-scoped one) is correct here.
fn filter_known_pinned_operators(
    pinned_codes: Vec<String>,
    known_codes: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let known: HashSet<String> = known_codes.into_iter().collect();
    pinned_codes
        .into_iter()
        .filter(|code| known.contains(code))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(json: serde_json::Value) -> Result<NotificationPreferencesUpdate, serde_json::Error> {
        serde_json::from_value(json)
    }

    fn validate_notification_update_now(
        body: NotificationPreferencesUpdate,
    ) -> Result<notification_settings::NotificationSettingsChange, String> {
        validate_notification_update(body, chrono::Utc::now())
    }

    #[test]
    fn a_snooze_runs_for_at_most_a_week() {
        let now: chrono::DateTime<chrono::Utc> = "2099-01-10T12:00:00Z".parse().unwrap();
        let check = |until: &str| {
            validate_notification_update(
                update(serde_json::json!({ "snoozedUntil": until })).unwrap(),
                now,
            )
        };
        assert!(check("2099-01-10T13:00:00Z").is_ok());
        assert!(check("2099-01-17T12:00:00Z").is_ok());
        assert!(check("2099-01-17T12:00:01Z").is_err());
        let cleared = validate_notification_update(
            update(serde_json::json!({ "snoozedUntil": null })).unwrap(),
            now,
        )
        .unwrap();
        assert_eq!(cleared.snoozed_until, Some(None));
    }

    #[test]
    fn a_notification_update_changes_only_the_fields_present() {
        let change = validate_notification_update_now(
            update(serde_json::json!({ "journeySuggestions": false })).unwrap(),
        )
        .unwrap();
        assert_eq!(
            change,
            notification_settings::NotificationSettingsChange {
                journey_suggestions: Some(false),
                ..Default::default()
            }
        );
        // null clears the threshold and the quiet hours; absent leaves them.
        let change = validate_notification_update_now(
            update(serde_json::json!({ "delayThresholdMinutes": null, "quietHours": null }))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(change.delay_threshold_minutes, Some(None));
        assert_eq!(change.quiet_hours, Some(None));
    }

    #[test]
    fn a_notification_update_checks_the_threshold_and_the_quiet_hours() {
        let check = |json| validate_notification_update(update(json).unwrap(), chrono::Utc::now());
        assert!(check(serde_json::json!({ "delayThresholdMinutes": 30 })).is_ok());
        assert!(check(serde_json::json!({ "delayThresholdMinutes": 20 })).is_err());
        let quiet = |start: &str, end: &str, zone: &str| serde_json::json!({ "quietHours": { "start": start, "end": end, "timezone": zone } });
        let ok = check(quiet("22:00", "07:00", "Europe/London")).unwrap();
        assert_eq!(
            ok.quiet_hours,
            Some(Some(notification_settings::QuietHours {
                start: "22:00:00".parse().unwrap(),
                end: "07:00:00".parse().unwrap(),
                timezone: "Europe/London".to_string(),
            }))
        );
        assert!(check(quiet("22:00", "22:00", "Europe/London")).is_err());
        assert!(check(quiet("25:00", "07:00", "Europe/London")).is_err());
        assert!(check(quiet("22:00", "07:00", "Mars/Olympus")).is_err());
        // Unknown fields are refused, at the top level and inside.
        assert!(update(serde_json::json!({ "everything": false })).is_err());
        assert!(
            update(serde_json::json!({
                "quietHours": { "start": "22:00", "end": "07:00", "timezone": "UTC", "days": 1 }
            }))
            .is_err()
        );
    }

    #[test]
    fn a_pinned_tfl_line_survives_the_known_ids_filter() {
        // This is the regression case: before TfL ids were folded into
        // `known_line_ids`, a pin on a TfL line -- written fine by
        // `replace_pinned_lines`, which validates nothing -- was silently
        // dropped on every read, so a starred TfL line looked unstarred
        // again after the next fetch/reload.
        let pinned = vec!["tfl-victoria".to_string(), "northern".to_string()];
        let result = filter_known_pinned_lines(
            pinned,
            vec!["northern".to_string()],
            vec![],
            vec!["tfl-victoria".to_string()],
        );
        assert_eq!(
            result,
            vec!["tfl-victoria".to_string(), "northern".to_string()]
        );
    }

    #[test]
    fn a_pinned_custom_line_survives_the_known_ids_filter() {
        let pinned = vec!["custom-my-commute".to_string()];
        let result = filter_known_pinned_lines(
            pinned,
            vec![],
            vec!["custom-my-commute".to_string()],
            vec![],
        );
        assert_eq!(result, vec!["custom-my-commute".to_string()]);
    }

    /// The existence-oracle regression: `custom_ids` is the set of custom
    /// lines THE CALLER may read, so another user's private line -- which
    /// this caller can still *pin*, since the write path validates nothing
    /// -- must not echo back and confirm it exists.
    #[test]
    fn a_pinned_custom_line_the_caller_cannot_read_is_dropped_rather_than_confirmed() {
        let pinned = vec!["custom-someone-elses-commute".to_string()];
        let result = filter_known_pinned_lines(pinned, vec![], vec![], vec![]);
        assert!(
            result.is_empty(),
            "pinning a guessed id must not reveal whether anyone owns it"
        );
    }

    #[test]
    fn a_pinned_id_unknown_to_every_source_is_dropped() {
        // e.g. a line withdrawn from the catalogue, or a TfL line that
        // left the feed and was pruned from `line_status`
        // (`queries::upsert_tfl_line_status`).
        let pinned = vec!["long-gone-line".to_string()];
        let result = filter_known_pinned_lines(pinned, vec![], vec![], vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn a_pinned_real_operator_code_survives_the_known_codes_filter() {
        let pinned = vec!["SW".to_string()];
        let result =
            filter_known_pinned_operators(pinned, vec!["SW".to_string(), "VT".to_string()]);
        assert_eq!(result, vec!["SW".to_string()]);
    }

    #[test]
    fn a_pinned_tfl_code_survives_the_known_codes_filter() {
        let pinned = vec!["TfL".to_string()];
        let result = filter_known_pinned_operators(pinned, vec!["TfL".to_string()]);
        assert_eq!(result, vec!["TfL".to_string()]);
    }

    #[test]
    fn a_pinned_code_unknown_to_every_source_is_dropped() {
        let pinned = vec!["ZZ".to_string()];
        let result = filter_known_pinned_operators(pinned, vec!["SW".to_string()]);
        assert!(result.is_empty());
    }

    /// The regression case for the review finding: an array at exactly the
    /// cap is accepted (no off-by-one), so this pins down the boundary
    /// before the "too many" case below pins down the rejection.
    #[test]
    fn a_pin_array_at_exactly_the_cap_is_accepted() {
        let items: Vec<String> = (0..MAX_PINNED_ITEMS).map(|i| i.to_string()).collect();
        assert!(reject_if_over_pin_limit(&items, "lines").is_ok());
    }

    /// The core regression case: before this cap existed, an unbounded
    /// array was accepted straight into `replace_pinned_*`'s row-by-row
    /// insert loop -- cheap, repeatable resource exhaustion for any logged-in
    /// user. One element over the cap must now get a clean 400, not be
    /// accepted and paid for as thousands of single-row inserts in one
    /// transaction.
    #[test]
    fn a_pin_array_over_the_cap_is_rejected_with_a_clean_400() {
        let items: Vec<String> = (0..=MAX_PINNED_ITEMS).map(|i| i.to_string()).collect();
        let err = reject_if_over_pin_limit(&items, "lines")
            .expect_err("an over-cap array must be rejected");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(
            err.1.contains("too many pinned lines"),
            "error message should name what was over the limit: {}",
            err.1
        );
    }

    /// L10 (2026-09-26 review): each pinned id is capped at
    /// `MAX_PINNED_ID_LENGTH` BYTES (not characters), exactly at the
    /// boundary.
    #[test]
    fn a_pinned_id_over_the_byte_length_cap_is_rejected() {
        let at_cap = vec!["a".repeat(MAX_PINNED_ID_LENGTH)];
        assert!(reject_if_over_pin_limit(&at_cap, "lines").is_ok());

        let over = vec!["SW".to_string(), "a".repeat(MAX_PINNED_ID_LENGTH + 1)];
        let err = reject_if_over_pin_limit(&over, "operators")
            .expect_err("an over-long id must be rejected");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(
            err.1.contains("pinned operators id is too long"),
            "{}",
            err.1
        );

        // 86 three-byte characters: 86 chars but 258 bytes.
        let multibyte = vec!["\u{20ac}".repeat(86)];
        assert!(multibyte[0].chars().count() < MAX_PINNED_ID_LENGTH);
        assert!(reject_if_over_pin_limit(&multibyte, "stations").is_err());
    }
}

/// End-to-end version of the `tests` module's regression case, exercising
/// the real `preferences`/`queries` DB round trip that `get_preferences`
/// itself makes, rather than hand-built inputs: writes a `TfL` line status
/// row (as `crates/poller-tfl` -> `queries::upsert_tfl_line_status` would),
/// pins it via `preferences::replace_pinned_lines` (the real write path,
/// same as `PUT /preferences/pinned-lines`), then reads it back through
/// `preferences::list_pinned_line_ids` + `queries::tfl_line_summaries` +
/// `filter_known_pinned_lines` -- the same three calls `get_preferences`
/// makes, minus the axum plumbing (constructing a full `App` needs OIDC
/// config this test has no need of).
#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::data::{preferences, queries};

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_pinned_tfl_line_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip \
                -- --ignored --test-threads=1`"]
    async fn a_pinned_tfl_line_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip()
     {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO users (id, name) VALUES ('TEST-PREFS-USER', 'Test Rider') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES ('TEST-TFL-PIN', 'test tfl pin line', 'tube', '{TfL}', '[]', 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture tfl line");

        // The real write path: identical to what `PUT /preferences/pinned-lines`
        // does, and validates nothing -- see `preferences::replace_pinned_lines`.
        preferences::replace_pinned_lines(
            &pool,
            "TEST-PREFS-USER",
            &["TEST-TFL-PIN".to_string(), "TEST-UNKNOWN-LINE".to_string()],
        )
        .await
        .expect("pin lines");

        // The real read path: identical to what `get_preferences` does.
        let pinned_line_ids = preferences::list_pinned_line_ids(&pool, "TEST-PREFS-USER")
            .await
            .expect("list pinned line ids");
        let tfl = queries::tfl_line_summaries(&pool)
            .await
            .expect("tfl_line_summaries");
        let pinned_lines = filter_known_pinned_lines(
            pinned_line_ids,
            vec![],
            vec![],
            tfl.into_iter().map(|l| l.id),
        );

        sqlx::query("DELETE FROM pinned_lines WHERE user_id = 'TEST-PREFS-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM line_status WHERE line_id = 'TEST-TFL-PIN'")
            .execute(&pool)
            .await
            .expect("cleanup fixture tfl line");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-PREFS-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");

        assert!(
            pinned_lines.contains(&"TEST-TFL-PIN".to_string()),
            "a pinned TfL line should survive the read path, not be silently dropped"
        );
        assert!(
            !pinned_lines.contains(&"TEST-UNKNOWN-LINE".to_string()),
            "a pinned id with no matching line anywhere should still be dropped"
        );
    }

    /// Transit spec §12.2 (plan 4.6): a pinned transit line survives the
    /// read while its network is served and has the line; an unknown line,
    /// or any line of a network not served, is dropped.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL (a migrated database)"]
    async fn a_pinned_transit_line_survives_the_read_while_its_network_is_served() {
        let pool = ds_store::test_support::connect().await;
        let network = "test-pin";
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        ds_store::test_support::transit::seed(&pool, network, today).await;
        ds_store::test_support::seed_user(&pool, "TEST-PREFS-TRANSIT").await;
        let pins = [
            "test-pin:blue".to_owned(),
            "test-pin:nope".to_owned(),
            "northern".to_owned(),
        ];
        preferences::replace_pinned_lines(&pool, "TEST-PREFS-TRANSIT", &pins)
            .await
            .unwrap();
        let read = |served: Vec<String>| {
            let pool = pool.clone();
            async move {
                let pinned = preferences::list_pinned_line_ids(&pool, "TEST-PREFS-TRANSIT")
                    .await
                    .unwrap();
                let transit = queries::transit_line_ids(&pool, &pinned, &served)
                    .await
                    .unwrap();
                filter_known_pinned_lines(pinned, vec!["northern".to_owned()], vec![], transit)
            }
        };
        let served = read(vec![network.to_owned()]).await;
        let dark = read(vec![]).await;

        sqlx::query("DELETE FROM pinned_lines WHERE user_id = 'TEST-PREFS-TRANSIT'")
            .execute(&pool)
            .await
            .unwrap();
        ds_store::test_support::cleanup_user(&pool, "TEST-PREFS-TRANSIT").await;
        ds_store::test_support::transit::cleanup(&pool, network).await;

        assert!(served.contains(&"test-pin:blue".to_owned()));
        assert!(!served.contains(&"test-pin:nope".to_owned()));
        assert!(served.contains(&"northern".to_owned()));
        assert_eq!(dark, ["northern"]);
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_pinned_operator_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip \
                -- --ignored --test-threads=1`"]
    async fn a_pinned_operator_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip()
     {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO users (id, name) VALUES ('TEST-PREFS-OPERATOR-USER', 'Test Rider') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        sqlx::query(
            "INSERT INTO tocs (atoc_code, name, legal_name, fetched_at) VALUES ('ZP', 'Z Prefs Rail', 'Z Prefs Rail', NOW()) \
             ON CONFLICT (atoc_code) DO UPDATE SET name = EXCLUDED.name",
        )
        .execute(&pool)
        .await
        .expect("seed fixture toc");

        preferences::replace_pinned_operators(
            &pool,
            "TEST-PREFS-OPERATOR-USER",
            &[
                "ZP".to_string(),
                "ZZ-UNKNOWN".to_string(),
                "TfL".to_string(),
            ],
        )
        .await
        .expect("pin operators");

        // This test's job is only the write/read round trip through the
        // real table (replace_pinned_operators -> list_pinned_operator_codes),
        // matching the scope
        // a_pinned_tfl_line_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip
        // has for pinned_lines. filter_known_pinned_operators (the "is this
        // code still real" hygiene step get_preferences applies on top of
        // this list) is deliberately NOT re-exercised here -- it needs no
        // database at all and is already covered by this file's own pure
        // unit tests in Step 7. So this list is expected to still contain
        // the unknown code -- that filtering happens one layer up, in the
        // route handler, not in list_pinned_operator_codes itself.
        let pinned_operator_codes =
            preferences::list_pinned_operator_codes(&pool, "TEST-PREFS-OPERATOR-USER")
                .await
                .expect("list pinned operator codes");

        sqlx::query("DELETE FROM pinned_operators WHERE user_id = 'TEST-PREFS-OPERATOR-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM tocs WHERE atoc_code = 'ZP'")
            .execute(&pool)
            .await
            .expect("cleanup fixture toc");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-PREFS-OPERATOR-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");

        assert!(pinned_operator_codes.contains(&"ZP".to_string()));
        assert!(pinned_operator_codes.contains(&"TfL".to_string()));
        assert!(pinned_operator_codes.contains(&"ZZ-UNKNOWN".to_string()));
    }
}
