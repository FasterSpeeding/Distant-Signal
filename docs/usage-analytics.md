# Usage analytics

First-party, aggregate usage counts (DS-190). No third-party script, no
cookie, no new server: the api counts, Prometheus and DS Postgres keep the
numbers. Nothing stored identifies a person.

There are two parts.

## Phase 1: feature counters (always on)

The api counts the outcomes its own handlers already see, in one counter
with fixed labels (`crates/api/src/usage_events.rs`, `FeatureEvent`):

`distant_signal_api_feature_events_total{feature, outcome}`

| `feature` | `outcome` | Counted in |
|---|---|---|
| `track_train` | `matched`, `pending`, `rejected`, `error` | `POST /Train/track`, `POST /Train/by-uid/{uid}/{date}/track` |
| `trip_plan` | `results`, `no_route`, `rejected`, `error` | `GET /Trips/plan` |
| `trip_plan` | `saved_journey` | `POST /Journeys/itinerary` (the web app's "Save as journey", and DS-MCP's saves) |
| `share_link` | `created`, `viewed`, `not_found` | `POST /Journeys/{id}/share-link`, `GET /Journeys/shared/{token}` |
| `notification_subscribe` | `succeeded`, `rejected`, `error` | `POST /notifications/subscribe` |
| `group_join` | `viewed`, `joined`, `refused` | `GET`/`POST /groups/join/{token}` |
| `connected_app` | `enabled`, `revoked` | `POST`/`DELETE /account/connected-apps/{client}` |

Page loads backed by the api (TfL stop pages, transit networks, lines,
stations) are already counted by route template in
`distant_signal_http_requests_total{endpoint}` (exported as
`exported_endpoint`). So are the Trains section's timetable reads
(`/public/trains/search`, `/public/trains/arrivals`,
`/public/trains/by-headcode`), with their status, so they get no feature
counter of their own: a read has no outcome beyond its status that the
handler sees and the browser does not, and phase 2 counts which view was
used and whether it found anything. The chat runs in the browser against the user's own
Anthropic key, so the api cannot see it; phase 2 counts it when opted in.

Every series is registered at 0 at startup. Lawful basis: legitimate
interests (aggregate, no identifiers); the privacy page says how to object.

## Phase 2: opt-in events

Off by default.

- Signed in: `users.analytics_opt_in`, set on Account, Preferences ("Help
  improve Distant Signal"; `PUT /public/preferences/analytics`).
- Logged out: the browser's `localStorage` key `ds-analytics-opt-in`, set on
  the same page or on `/cookies`.
- A browser sending Global Privacy Control or Do Not Track sends nothing;
  the api also drops any event that carries `Sec-GPC: 1` or `DNT: 1` (the
  frontend proxy forwards both).

The browser sends `POST /api/events` (same origin, `navigator.sendBeacon`, so
the CSP's `connect-src 'self'` covers it) and the proxy forwards it to
`POST /public/events`. The body is `{name, route_template, props?}`; the api
checks it against an allowlist and answers `204` whether or not it counted
it. Its own rate-limit budget: `API_RATE_LIMIT_EVENTS_PER_MINUTE` (60) and
`API_RATE_LIMIT_EVENTS_BURST` (30) per client IP.

| `name` | `props` | Sent when |
|---|---|---|
| `page_view` | none | A page is shown; `route_template` is Next's template (`/lines/[id]`), never the real path |
| `display_mode` | `mode`: `standalone`, `browser` | Once per page load |
| `feature` | `feature`, `step` (below) | A client-visible step of a core feature |
| `empty_state` | `kind`: `trains_station_board_none`, `trains_between_none`, `trains_headcode_none`, `station_search_no_results`, `departures_none`, `journeys_none`, `groups_none`, `line_trains_none` | A list or search comes back empty |
| `api_error` | `status`: `4xx`, `5xx`, `network` | A call from the browser to the api fails |
| `error_boundary` | `digest` (optional) | The error page is shown; Next's digest only, never the message or stack |
| `offline_shown` | none (`route_template` `/offline`) | The offline page was shown; sent on the next page load |

`feature` steps: `track_train` (`submitted`, `succeeded`, `failed`), `plan`
(`searched`, `results`, `no_route`, `failed`, `saved_journey`), `share_link`
(`created`, `copied`), `notifications` (`prompted`, `granted`, `denied`,
`subscribed`, `failed`), `group_join` (`joined`, `failed`), `chat`
(`opened`, `message_sent`), `connected_app` (`enabled`, `revoked`), and:

| `feature` | `step` | Sent when |
|---|---|---|
| `trains` | `station_departures`, `station_arrivals`, `between`, `headcode`, `uid` | A `/trains` search runs in that view (a UID opens its train's page, so it has no outcome) |
| `trains` | `results`, `no_results`, `failed` | What the search found (an unpublished date is `no_results`) |
| `trains_filters` | `calls_at`, `via`, `avoid`, `avoid_stop`, `previously_calls_at`, `previously_stops_at`, `origin`, `terminates_at` | A `/trains` search with that advanced option set, one per option (never which stations; `origin` is Starts at); also a journey leg added from "Add a leg"'s time-window search with it set, told apart by `route_template` (`/journeys/[id]`, `/plan`) |
| `compare_options` | `searched` | A trip plan with "Compare options" on |
| `compare_options` | `alternative_selected`, `alternative_saved` | An option other than the fastest chosen, then saved as a journey |
| `shared_journeys` | `shown`, `opened` | Home's "Shared with you" list shown, and a journey in it opened |
| `leg_stops` | `earlier_expanded`, `later_expanded` | A journey leg's collapsed "Earlier stops" or "Later stops" opened |
| `journey_start` | `plan`, `known`, `untracked` | `/plan`'s way in to a new journey shown, on arrival and on each switch |
| `stop_finder` | `stop_opened`, `all_matches_opened` | A transit network page's "Find a stop": a stop opened from its list, or its "See all *n* matches" row (the A to Z, `/transit/[network]/stops`) |

An event from before the nav knows the session (a page's own effects can
run first on a full load) waits for it, and is sent only if the session
says opted in.

Nothing names a station, line, train, journey or group.

### Storage

```sql
usage_daily(day date, event text, dim text, signed_in boolean, count bigint,
            PRIMARY KEY (day, event, dim, signed_in))
```

`day` is London's date. `dim` is the route template, then each property as
`|key=value` sorted by key (`/plan|feature=plan|step=results`). Each event
is `INSERT ... ON CONFLICT DO UPDATE SET count = count + 1`. No user id, IP
address, user agent or session id. Days older than 13 months are deleted by
the api's hourly retention sweep. The api role has SELECT, INSERT, UPDATE
and DELETE on it (`db-grants.yaml`).

Mirrored into Prometheus:

- `distant_signal_api_usage_events_total{event, signed_in}`
- `distant_signal_api_usage_page_views_total{route}` (one series per route
  template, 55)
- `distant_signal_api_usage_events_dropped_total{reason}`: `privacy_signal`,
  `not_opted_in`, `invalid`, `error`

## Panels

Prometheus keeps 30 days (25 GB) on the cluster. These queries need nothing
new there beyond the existing api scrape. A Grafana dashboard would be a
`ConfigMap` with the `grafana_dashboard: "1"` label in Ranma-Config (the
chart deliberately ships none; see
docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md,
"Point 4").

| Panel | PromQL |
|---|---|
| Feature outcomes per day | `sum by (feature, outcome) (increase(distant_signal_api_feature_events_total[1d]))` |
| Trip plans with no route | `sum(increase(distant_signal_api_feature_events_total{feature="trip_plan",outcome="no_route"}[1d])) / sum(increase(distant_signal_api_feature_events_total{feature="trip_plan",outcome=~"results\|no_route"}[1d]))` |
| Tracked trains matched at once | `sum(increase(distant_signal_api_feature_events_total{feature="track_train",outcome="matched"}[1d])) / sum(increase(distant_signal_api_feature_events_total{feature="track_train",outcome=~"matched\|pending"}[1d]))` |
| Share links opened per link created | `sum(increase(distant_signal_api_feature_events_total{feature="share_link",outcome="viewed"}[7d])) / sum(increase(distant_signal_api_feature_events_total{feature="share_link",outcome="created"}[7d]))` |
| Opt-in page views by page | `topk(20, sum by (route) (increase(distant_signal_api_usage_page_views_total[1d])))` |
| Opt-in events, signed in or not | `sum by (event, signed_in) (increase(distant_signal_api_usage_events_total[1d]))` |
| Dropped events | `sum by (reason) (increase(distant_signal_api_usage_events_dropped_total[1d]))` |
| Api-backed page loads | `sum by (exported_endpoint) (increase(distant_signal_http_requests_total{exported_endpoint=~"/public/tfl/stops/\\{id\\}\|/public/transit/.*",status="200"}[1d]))` |

For the long run, query Postgres (read-only):

```sql
-- Page views per week, by page.
SELECT date_trunc('week', day) AS week, dim AS page, SUM(count) AS views
FROM usage_daily WHERE event = 'page_view'
GROUP BY 1, 2 ORDER BY 1 DESC, 3 DESC;

-- The plan funnel, per month.
SELECT date_trunc('month', day) AS month, split_part(dim, '|step=', 2) AS step, SUM(count)
FROM usage_daily WHERE event = 'feature' AND dim LIKE '%|feature=plan|%'
GROUP BY 1, 2 ORDER BY 1 DESC, 3 DESC;

-- Installed app share.
SELECT dim, SUM(count) FROM usage_daily WHERE event = 'display_mode' GROUP BY 1;
```
