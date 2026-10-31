//! Usage analytics (DS-190): aggregate, first-party, no identifiers.
//!
//! Two parts, both disclosed on the privacy page:
//!
//! - **Phase 1, feature counters** ([`FeatureEvent`], always on). The api
//!   counts what its own handlers already see (a train tracked, a trip
//!   planned, a share link opened...) in one Prometheus counter,
//!   `distant_signal_api_feature_events_total{feature,outcome}`. Both
//!   labels are fixed enums: no ids, no free text, no user. Legitimate
//!   interests; the privacy page says how to object.
//! - **Phase 2, opt-in events** ([`validate`], `POST /public/events`). A
//!   browser that has opted in (and sends no `Sec-GPC: 1` or `DNT: 1`)
//!   reports page views by route template and a few enumerated feature
//!   steps. The api checks each against the allowlist below and adds one
//!   to that day's `usage_daily` row; nothing about the visitor is stored.
//!   Mirrored into `distant_signal_api_usage_events_total{event,signed_in}`
//!   and, for page views, `distant_signal_api_usage_page_views_total{route}`.
//!
//! The allowlist is mirrored by the web app in `frontend/lib/usageEvents.ts`;
//! `tests::the_frontend_mirrors_the_allowlist` keeps the two in step, and
//! `tests::route_templates_match_the_frontend_pages` keeps
//! [`ROUTE_TEMPLATES`] in step with `frontend/app`'s pages.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Phase 1's counter, under the `distant_signal_` prefix.
pub const FEATURE_EVENTS_METRIC: &str = "api_feature_events_total";
/// Phase 2's mirror of `usage_daily`, by event and signed-in or not.
pub const USAGE_EVENTS_METRIC: &str = "api_usage_events_total";
/// Phase 2's page views by route template (about 55 series).
pub const USAGE_PAGE_VIEWS_METRIC: &str = "api_usage_page_views_total";
/// Events the api did not count, by [`DropReason`].
pub const USAGE_EVENTS_DROPPED_METRIC: &str = "api_usage_events_dropped_total";

/// A server-side feature outcome (phase 1). Each is one
/// `(feature, outcome)` label pair; [`FeatureEvent::ALL`] is the whole set,
/// registered at 0 at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureEvent {
    /// `POST /Train/track` or `/Train/by-uid/.../track`: tracked and
    /// matched to a timetabled train at once.
    TrackTrainMatched,
    /// Tracked, but no timetabled train matched yet (the sweep retries).
    TrackTrainPending,
    /// Refused: invalid request, or over a cap.
    TrackTrainRejected,
    TrackTrainError,
    /// `GET /Trips/plan` answered with at least one journey.
    TripPlanResults,
    /// Answered with no journey.
    TripPlanNoRoute,
    /// Refused as invalid (400).
    TripPlanRejected,
    TripPlanError,
    /// `POST /Journeys/itinerary`: a planned trip saved as a journey.
    TripPlanSavedJourney,
    /// `POST /Journeys/{id}/share-link`.
    ShareLinkCreated,
    /// `GET /Journeys/shared/{token}`: a live link opened.
    ShareLinkViewed,
    /// ... an unknown, revoked or expired link.
    ShareLinkNotFound,
    /// `POST /notifications/subscribe`: push turned on for a device.
    NotificationSubscribeSucceeded,
    /// Refused as invalid (400), e.g. a push endpoint that is not allowed.
    NotificationSubscribeRejected,
    NotificationSubscribeError,
    /// `GET /groups/join/{token}`: an invite opened.
    GroupJoinViewed,
    /// `POST /groups/join/{token}`: joined.
    GroupJoinJoined,
    /// ... an unknown, revoked or expired invite, or the group is full.
    GroupJoinRefused,
    /// `POST /account/connected-apps/{client}`: an app connected.
    ConnectedAppEnabled,
    /// `DELETE /account/connected-apps/{client}`.
    ConnectedAppRevoked,
}

impl FeatureEvent {
    pub const ALL: &'static [FeatureEvent] = &[
        FeatureEvent::TrackTrainMatched,
        FeatureEvent::TrackTrainPending,
        FeatureEvent::TrackTrainRejected,
        FeatureEvent::TrackTrainError,
        FeatureEvent::TripPlanResults,
        FeatureEvent::TripPlanNoRoute,
        FeatureEvent::TripPlanRejected,
        FeatureEvent::TripPlanError,
        FeatureEvent::TripPlanSavedJourney,
        FeatureEvent::ShareLinkCreated,
        FeatureEvent::ShareLinkViewed,
        FeatureEvent::ShareLinkNotFound,
        FeatureEvent::NotificationSubscribeSucceeded,
        FeatureEvent::NotificationSubscribeRejected,
        FeatureEvent::NotificationSubscribeError,
        FeatureEvent::GroupJoinViewed,
        FeatureEvent::GroupJoinJoined,
        FeatureEvent::GroupJoinRefused,
        FeatureEvent::ConnectedAppEnabled,
        FeatureEvent::ConnectedAppRevoked,
    ];

    /// The `(feature, outcome)` labels.
    pub fn labels(self) -> (&'static str, &'static str) {
        match self {
            FeatureEvent::TrackTrainMatched => ("track_train", "matched"),
            FeatureEvent::TrackTrainPending => ("track_train", "pending"),
            FeatureEvent::TrackTrainRejected => ("track_train", "rejected"),
            FeatureEvent::TrackTrainError => ("track_train", "error"),
            FeatureEvent::TripPlanResults => ("trip_plan", "results"),
            FeatureEvent::TripPlanNoRoute => ("trip_plan", "no_route"),
            FeatureEvent::TripPlanRejected => ("trip_plan", "rejected"),
            FeatureEvent::TripPlanError => ("trip_plan", "error"),
            FeatureEvent::TripPlanSavedJourney => ("trip_plan", "saved_journey"),
            FeatureEvent::ShareLinkCreated => ("share_link", "created"),
            FeatureEvent::ShareLinkViewed => ("share_link", "viewed"),
            FeatureEvent::ShareLinkNotFound => ("share_link", "not_found"),
            FeatureEvent::NotificationSubscribeSucceeded => ("notification_subscribe", "succeeded"),
            FeatureEvent::NotificationSubscribeRejected => ("notification_subscribe", "rejected"),
            FeatureEvent::NotificationSubscribeError => ("notification_subscribe", "error"),
            FeatureEvent::GroupJoinViewed => ("group_join", "viewed"),
            FeatureEvent::GroupJoinJoined => ("group_join", "joined"),
            FeatureEvent::GroupJoinRefused => ("group_join", "refused"),
            FeatureEvent::ConnectedAppEnabled => ("connected_app", "enabled"),
            FeatureEvent::ConnectedAppRevoked => ("connected_app", "revoked"),
        }
    }

    /// Adds one to this outcome's series.
    pub fn record(self) {
        let (feature, outcome) = self.labels();
        metrics::counter!(
            common::metrics::metric_name(FEATURE_EVENTS_METRIC),
            "feature" => feature,
            "outcome" => outcome
        )
        .increment(1);
    }

    /// Records the outcome of a handler's result: `ok` picks the outcome
    /// of a success, a 4xx is `rejected`, anything else `error`.
    pub fn record_result<T, E>(
        result: &Result<T, (axum::http::StatusCode, E)>,
        ok: impl FnOnce(&T) -> FeatureEvent,
        rejected: FeatureEvent,
        error: FeatureEvent,
    ) {
        match result {
            Ok(value) => ok(value).record(),
            Err((status, _)) if status.is_client_error() => rejected.record(),
            Err(_) => error.record(),
        }
    }
}

/// Why an event was not counted (`USAGE_EVENTS_DROPPED_METRIC`'s `reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// The request carried `Sec-GPC: 1` or `DNT: 1`.
    PrivacySignal,
    /// A signed-in caller whose account has not opted in.
    NotOptedIn,
    /// Not JSON, or not on the allowlist.
    Invalid,
    /// Writing the count failed.
    Error,
}

impl DropReason {
    pub const ALL: &'static [DropReason] = &[
        DropReason::PrivacySignal,
        DropReason::NotOptedIn,
        DropReason::Invalid,
        DropReason::Error,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DropReason::PrivacySignal => "privacy_signal",
            DropReason::NotOptedIn => "not_opted_in",
            DropReason::Invalid => "invalid",
            DropReason::Error => "error",
        }
    }

    pub fn record(self) {
        metrics::counter!(
            common::metrics::metric_name(USAGE_EVENTS_DROPPED_METRIC),
            "reason" => self.label()
        )
        .increment(1);
    }
}

/// Every page of the web app, as Next's route templates (`(home)` is `/`),
/// plus two that are not pages: `/offline` (the service worker's offline
/// page) and `/_not-found`. A path the web app cannot match to one of these
/// is never sent.
pub const ROUTE_TEMPLATES: &[&str] = &[
    "/",
    "/_not-found",
    "/accessibility",
    "/account",
    "/account/connected-apps",
    "/account/connected-apps/[client]",
    "/account/deleted",
    "/account/notifications",
    "/account/preferences",
    "/attribution",
    "/chat",
    "/chat/callback",
    "/connect-claude",
    "/connect-claude/authorize",
    "/contact",
    "/cookies",
    "/groups",
    "/groups/[id]",
    "/groups/join/[token]",
    "/groups/new",
    "/incidents",
    "/incidents/[id]",
    "/journeys",
    "/journeys/[id]",
    "/journeys/shared/[token]",
    "/journeys/templates",
    "/journeys/templates/[id]",
    "/lines",
    "/lines/[id]",
    "/lines/[id]/edit",
    "/lines/[id]/history",
    "/lines/[id]/timetable",
    "/lines/new",
    "/network/history",
    "/offline",
    "/operators",
    "/operators/[code]/history",
    "/plan",
    "/privacy",
    "/stations",
    "/stations/[crs]",
    "/status",
    "/terms",
    "/tfl/stops/[id]",
    "/track",
    "/track/mine/add-ticket",
    "/train/[uid]/[date]",
    "/train/by-id/[trackingId]",
    "/trains",
    "/transit",
    "/transit/[network]",
    "/transit/[network]/incidents/[id]",
    "/transit/[network]/lines/[line]",
    "/transit/[network]/lines/[line]/timetable",
    "/transit/[network]/stops",
    "/transit/[network]/stops/[stop]",
];

/// A property's allowed values.
#[derive(Debug, Clone, Copy)]
pub enum PropValues {
    /// One of these.
    OneOf(&'static [&'static str]),
    /// Next's error digest: an opaque hash, 1 to 64 of `[A-Za-z0-9@_-]`.
    Digest,
    /// One of the steps [`FEATURE_STEPS`] lists for the event's `feature`.
    FeatureStep,
}

/// One event the api accepts: its name, and each property it may carry
/// with that property's values. `required` properties must be present.
#[derive(Debug, Clone, Copy)]
pub struct EventSpec {
    pub name: &'static str,
    pub props: &'static [(&'static str, PropValues, bool)],
}

/// The feature steps the web app reports, as `feature` → steps. Only what
/// the browser sees; the api's own outcomes are [`FeatureEvent`].
pub const FEATURE_STEPS: &[(&str, &[&str])] = &[
    ("track_train", &["submitted", "succeeded", "failed"]),
    (
        "plan",
        &["searched", "results", "no_route", "failed", "saved_journey"],
    ),
    ("share_link", &["created", "copied"]),
    (
        "notifications",
        &["prompted", "granted", "denied", "subscribed", "failed"],
    ),
    ("group_join", &["joined", "failed"]),
    ("chat", &["opened", "message_sent"]),
    ("connected_app", &["enabled", "revoked"]),
    // `/trains`: a search run in one of its views (the step names the
    // view), then what came back. A UID opens its train's page, so it has
    // no outcome of its own here.
    (
        "trains",
        &[
            "station_departures",
            "station_arrivals",
            "between",
            "headcode",
            "uid",
            "results",
            "no_results",
            "failed",
        ],
    ),
    // A `/trains` search with that advanced option set, or a journey leg
    // added from "Add a leg"'s time-window search with it (DS-215; the
    // route template tells them apart): its kind only, never the stations.
    (
        "trains_filters",
        &[
            "calls_at",
            "via",
            "avoid",
            "avoid_stop",
            "previously_calls_at",
            "previously_stops_at",
            "origin",
            "terminates_at",
        ],
    ),
    // A `/plan` search with an operator filter (2026-10-31): its kind
    // only, never the operators; the "Add London Underground" button; and
    // a saved journey's filter changed or removed on its page.
    (
        "plan_filters",
        &[
            "operator_only",
            "operator_avoid",
            "operator_prefer",
            "operator_add_underground",
            "journey_operator_filter_changed",
            "journey_operator_filter_removed",
        ],
    ),
    // The planner's "Compare options": a search with it on, and an option
    // other than the fastest chosen, then saved as a journey.
    (
        "compare_options",
        &[
            "searched",
            "alternative_selected",
            "alternative_saved",
            "operator_option_selected",
        ],
    ),
    // Home's "Shared with you" list: shown, and one of its journeys opened.
    ("shared_journeys", &["shown", "opened"]),
    // A journey leg's collapsed "Earlier stops" or "Later stops" opened.
    ("leg_stops", &["earlier_expanded", "later_expanded"]),
    // `/plan`'s way in to a new journey, each time one is shown.
    ("journey_start", &["plan", "known", "untracked"]),
    // A transit network page's "Find a stop": a stop opened from it, or its
    // "See all matches" row.
    ("stop_finder", &["stop_opened", "all_matches_opened"]),
];

/// Empty or no-result states the web app reports (`empty_state`'s `kind`).
pub const EMPTY_STATE_KINDS: &[&str] = &[
    "trains_station_board_none",
    "trains_between_none",
    "trains_headcode_none",
    "station_search_no_results",
    "departures_none",
    "journeys_none",
    "groups_none",
    "line_trains_none",
];

/// `api_error`'s status classes: a failed call from the browser.
pub const API_ERROR_STATUSES: &[&str] = &["4xx", "5xx", "network"];

/// `display_mode`'s values: installed app or a browser tab.
pub const DISPLAY_MODES: &[&str] = &["standalone", "browser"];

/// Every feature named in [`FEATURE_STEPS`].
const FEATURES: &[&str] = &[
    "track_train",
    "plan",
    "share_link",
    "notifications",
    "group_join",
    "chat",
    "connected_app",
    "trains",
    "trains_filters",
    "plan_filters",
    "compare_options",
    "shared_journeys",
    "leg_stops",
    "journey_start",
    "stop_finder",
];

/// The event names and their properties.
pub const EVENTS: &[EventSpec] = &[
    EventSpec {
        name: "page_view",
        props: &[],
    },
    EventSpec {
        name: "display_mode",
        props: &[("mode", PropValues::OneOf(DISPLAY_MODES), true)],
    },
    EventSpec {
        name: "feature",
        props: &[
            ("feature", PropValues::OneOf(FEATURES), true),
            ("step", PropValues::FeatureStep, true),
        ],
    },
    EventSpec {
        name: "empty_state",
        props: &[("kind", PropValues::OneOf(EMPTY_STATE_KINDS), true)],
    },
    EventSpec {
        name: "api_error",
        props: &[("status", PropValues::OneOf(API_ERROR_STATUSES), true)],
    },
    EventSpec {
        name: "error_boundary",
        props: &[("digest", PropValues::Digest, false)],
    },
    EventSpec {
        name: "offline_shown",
        props: &[],
    },
];

/// `POST /public/events`'s body. `props` values are strings; anything
/// else is refused.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventBody {
    pub name: String,
    pub route_template: String,
    #[serde(default)]
    pub props: Option<BTreeMap<String, String>>,
}

/// A checked event, ready to count: `usage_daily`'s `event` and `dim`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidEvent {
    pub event: &'static str,
    /// The route template, then each property as `|key=value`, by key.
    pub dim: String,
    /// The route template, for the page-view counter.
    pub route: &'static str,
}

/// The longest body `POST /public/events` reads.
pub const MAX_BODY_BYTES: usize = 1024;

/// Checks `body` against the allowlist. `None` for anything not on it.
pub fn validate(body: &EventBody) -> Option<ValidEvent> {
    let spec = EVENTS.iter().find(|spec| spec.name == body.name)?;
    let route = *ROUTE_TEMPLATES
        .iter()
        .find(|route| **route == body.route_template)?;
    let empty = BTreeMap::new();
    let props = body.props.as_ref().unwrap_or(&empty);
    if props
        .keys()
        .any(|key| !spec.props.iter().any(|(k, _, _)| k == key))
    {
        return None;
    }
    let mut dim = route.to_string();
    // `spec.props` is not sorted; the dim is, so one event has one dim.
    let mut present: Vec<(&str, &str)> = Vec::new();
    for (key, values, required) in spec.props {
        match props.get(*key) {
            None if *required => return None,
            None => {}
            Some(value) => {
                let ok = match values {
                    PropValues::OneOf(allowed) => allowed.contains(&value.as_str()),
                    PropValues::Digest => is_digest(value),
                    PropValues::FeatureStep => props.get("feature").is_some_and(|feature| {
                        FEATURE_STEPS
                            .iter()
                            .any(|(f, steps)| f == feature && steps.contains(&value.as_str()))
                    }),
                };
                if !ok {
                    return None;
                }
                present.push((key, value.as_str()));
            }
        }
    }
    present.sort_unstable();
    for (key, value) in present {
        dim.push('|');
        dim.push_str(key);
        dim.push('=');
        dim.push_str(value);
    }
    Some(ValidEvent {
        event: spec.name,
        dim,
        route,
    })
}

fn is_digest(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'_' | b'-'))
}

/// Mirrors one counted event into Prometheus.
pub fn record_usage(event: &ValidEvent, signed_in: bool) {
    metrics::counter!(
        common::metrics::metric_name(USAGE_EVENTS_METRIC),
        "event" => event.event,
        "signed_in" => if signed_in { "true" } else { "false" }
    )
    .increment(1);
    if event.event == "page_view" {
        metrics::counter!(
            common::metrics::metric_name(USAGE_PAGE_VIEWS_METRIC),
            "route" => event.route
        )
        .increment(1);
    }
}

/// Registers every series above at 0, so `increase()` sees the first
/// event after a restart. Call once the recorder is installed.
pub fn register() {
    for event in FeatureEvent::ALL {
        let (feature, outcome) = event.labels();
        metrics::counter!(
            common::metrics::metric_name(FEATURE_EVENTS_METRIC),
            "feature" => feature,
            "outcome" => outcome
        )
        .increment(0);
    }
    for spec in EVENTS {
        for signed_in in ["true", "false"] {
            metrics::counter!(
                common::metrics::metric_name(USAGE_EVENTS_METRIC),
                "event" => spec.name,
                "signed_in" => signed_in
            )
            .increment(0);
        }
    }
    for route in ROUTE_TEMPLATES {
        metrics::counter!(
            common::metrics::metric_name(USAGE_PAGE_VIEWS_METRIC),
            "route" => *route
        )
        .increment(0);
    }
    for reason in DropReason::ALL {
        metrics::counter!(
            common::metrics::metric_name(USAGE_EVENTS_DROPPED_METRIC),
            "reason" => reason.label()
        )
        .increment(0);
    }
}

/// Whether the request asks not to be tracked: `Sec-GPC: 1` (Global
/// Privacy Control) or `DNT: 1`. The web app checks the same in the
/// browser and sends nothing; this is the server's own check.
pub fn has_privacy_signal(headers: &axum::http::HeaderMap) -> bool {
    ["sec-gpc", "dnt"].iter().any(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.trim() == "1")
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::*;

    fn body(name: &str, route: &str, props: &[(&str, &str)]) -> EventBody {
        EventBody {
            name: name.to_string(),
            route_template: route.to_string(),
            props: (!props.is_empty()).then(|| {
                props
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect()
            }),
        }
    }

    #[test]
    fn a_page_view_is_its_route_template() {
        let event = validate(&body("page_view", "/lines/[id]", &[])).unwrap();
        assert_eq!(event.event, "page_view");
        assert_eq!(event.dim, "/lines/[id]");
    }

    #[test]
    fn properties_join_the_dim_sorted_by_key() {
        let event = validate(&body(
            "feature",
            "/plan",
            &[("step", "results"), ("feature", "plan")],
        ))
        .unwrap();
        assert_eq!(event.dim, "/plan|feature=plan|step=results");
        let event = validate(&body("api_error", "/", &[("status", "5xx")])).unwrap();
        assert_eq!(event.dim, "/|status=5xx");
    }

    #[test]
    fn the_allowlist_refuses_anything_else() {
        for refused in [
            // Unknown event.
            body("click", "/", &[]),
            // A real path, not a template.
            body("page_view", "/lines/northern", &[]),
            body("page_view", "/stations/KGX", &[]),
            // Unknown template.
            body("page_view", "/admin", &[]),
            // A page that is gone: `/journeys/new` redirects to `/plan`.
            body("page_view", "/journeys/new", &[]),
            // Properties on an event that takes none.
            body("page_view", "/", &[("line", "northern")]),
            // Unknown property.
            body("api_error", "/", &[("status", "5xx"), ("path", "/x")]),
            // Missing required property.
            body("api_error", "/", &[]),
            // Value off the list.
            body("api_error", "/", &[("status", "503")]),
            body("display_mode", "/", &[("mode", "fullscreen")]),
            body("empty_state", "/", &[("kind", "anything")]),
            // A step of another feature.
            body(
                "feature",
                "/plan",
                &[("feature", "plan"), ("step", "joined")],
            ),
            body(
                "feature",
                "/plan",
                &[("feature", "nope"), ("step", "results")],
            ),
            body(
                "feature",
                "/trains",
                &[("feature", "trains"), ("step", "shown")],
            ),
            // Free text where a digest goes.
            body("error_boundary", "/", &[("digest", "TypeError: x is null")]),
            body("error_boundary", "/", &[("digest", &"1".repeat(65))]),
        ] {
            assert_eq!(validate(&refused), None, "{refused:?}");
        }
    }

    #[test]
    fn an_error_boundary_takes_a_digest_or_none() {
        assert_eq!(
            validate(&body("error_boundary", "/plan", &[])).unwrap().dim,
            "/plan"
        );
        assert_eq!(
            validate(&body(
                "error_boundary",
                "/plan",
                &[("digest", "2914387@E1")]
            ))
            .unwrap()
            .dim,
            "/plan|digest=2914387@E1"
        );
    }

    #[test]
    fn every_feature_step_is_accepted() {
        for (feature, steps) in FEATURE_STEPS {
            assert!(FEATURES.contains(feature), "{feature}");
            for step in *steps {
                let event = validate(&body(
                    "feature",
                    "/",
                    &[("feature", feature), ("step", step)],
                ));
                assert!(event.is_some(), "{feature}:{step}");
            }
        }
        assert_eq!(FEATURES.len(), FEATURE_STEPS.len());
    }

    #[test]
    fn unknown_fields_in_the_body_are_refused() {
        let parsed: Result<EventBody, _> =
            serde_json::from_str(r#"{"name":"page_view","route_template":"/","user":"x"}"#);
        assert!(parsed.is_err());
        let parsed: Result<EventBody, _> = serde_json::from_str(
            r#"{"name":"api_error","route_template":"/","props":{"status":5}}"#,
        );
        assert!(parsed.is_err());
    }

    #[test]
    fn privacy_signals_are_gpc_or_dnt_set_to_one() {
        let mut headers = axum::http::HeaderMap::new();
        assert!(!has_privacy_signal(&headers));
        headers.insert("dnt", "0".parse().unwrap());
        assert!(!has_privacy_signal(&headers));
        headers.insert("dnt", "1".parse().unwrap());
        assert!(has_privacy_signal(&headers));
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("sec-gpc", "1".parse().unwrap());
        assert!(has_privacy_signal(&headers));
    }

    #[test]
    fn every_series_is_registered_at_zero_and_bounded() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        metrics::with_local_recorder(&recorder, register);
        let rendered = handle.render();
        for line in [
            r#"distant_signal_api_feature_events_total{feature="track_train",outcome="matched"} 0"#,
            r#"distant_signal_api_usage_events_total{event="page_view",signed_in="false"} 0"#,
            r#"distant_signal_api_usage_page_views_total{route="/lines/[id]"} 0"#,
            r#"distant_signal_api_usage_events_dropped_total{reason="privacy_signal"} 0"#,
        ] {
            assert!(rendered.contains(line), "{line} missing from {rendered}");
        }
        let series = rendered
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .count();
        assert!(series < 120, "{series} series");
    }

    fn repo_root() -> PathBuf {
        common::manifest_dir!().join("../..")
    }

    /// Every `page.tsx` under `frontend/app` as a route template.
    fn page_templates(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().unwrap().is_dir() {
                if name == "api" {
                    continue;
                }
                // A route group, `(home)`, adds no segment.
                let next = if name.starts_with('(') {
                    prefix.to_string()
                } else {
                    format!("{prefix}/{name}")
                };
                page_templates(&entry.path(), &next, out);
            } else if name == "page.tsx" {
                out.insert(if prefix.is_empty() {
                    "/".to_string()
                } else {
                    prefix.to_string()
                });
            }
        }
    }

    #[test]
    fn route_templates_match_the_frontend_pages() {
        let app = repo_root().join("frontend/app");
        if !app.is_dir() {
            // A crate-only checkout (the image build): nothing to compare.
            return;
        }
        let mut pages = BTreeSet::new();
        page_templates(&app, "", &mut pages);
        pages.insert("/offline".to_string());
        pages.insert("/_not-found".to_string());
        let ours: BTreeSet<String> = ROUTE_TEMPLATES.iter().map(|r| (*r).to_string()).collect();
        assert_eq!(
            ours, pages,
            "ROUTE_TEMPLATES must list exactly the web app's pages"
        );
    }

    #[test]
    fn the_frontend_mirrors_the_allowlist() {
        let path = repo_root().join("frontend/lib/usageEvents.ts");
        let Ok(source) = std::fs::read_to_string(&path) else {
            return;
        };
        let mut expected: Vec<&str> = Vec::new();
        expected.extend(ROUTE_TEMPLATES);
        expected.extend(EVENTS.iter().map(|spec| spec.name));
        expected.extend(FEATURES);
        for (_, steps) in FEATURE_STEPS {
            expected.extend(*steps);
        }
        expected.extend(EMPTY_STATE_KINDS);
        expected.extend(API_ERROR_STATUSES);
        expected.extend(DISPLAY_MODES);
        for value in expected {
            assert!(
                source.contains(&format!("'{value}'")),
                "frontend/lib/usageEvents.ts does not name '{value}'"
            );
        }
    }
}
