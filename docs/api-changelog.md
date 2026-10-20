# API changelog

Changes to the Distant Signal (DS) HTTP API that a client such as DS-MCP
needs to know about. Newest first. Field names are as served (camelCase).

## 2026-10-20: `/Trips/plan` uses fixed links in both directions

No shape change. The timetable's fixed links (Tube, walk, transfer, bus,
ferry) were used only in the direction the feed happens to list them, so
half of them were missing: Vauxhall to Euston went by train via Waterloo
East and Charing Cross because only Euston to Vauxhall by Tube was listed.
Each link now runs both ways, with the same minutes and hours. Plans can
change: a `transfer` leg (`mode` `TUBE`, `WALK`, and so on) may now appear
where trains were offered before, e.g. Vauxhall to Euston is one 22-minute
`TUBE` leg (27 minutes evenings and Sundays). A small curated set adds
links the feed lacks, so far Vauxhall to Victoria (`TUBE`, 11 minutes;
`reference-data/supplementary-fixed-links.csv`). Transfer legs still carry
no line name.

## 2026-10-20: `trackingPhase` on group and shared rows

Additive; no existing field changes meaning. The group and shared lists
now carry what the owner's own lists already did, so a shared train reads
the same on every page (it used to say "Pending match" in a group while
its owner saw "Planned").

- **`GET /groups/{id}/trains` and `GET /groups/shared-trains`.** Each row
  gains `serviceMode`, `liveTracking`, `trackingPhase` and
  `firstReportExpectedAt`, exactly as on `GET /Train/mine` rows.
- **`GET /groups/{id}/journeys` and `GET /groups/shared-journeys`.** Each
  row gains `trackingPhase` and `firstReportExpectedAt` for the leg it
  summarises (the journey's first train leg, else its first leg), plus
  `kind` (`train` or `untracked`), `transitMode`, `plannedDeparture`,
  `plannedArrival` and `untrackedStatus` (`planned`, `inProgress`,
  `done`). `kind: "untracked"` means the journey has no train legs; the
  four untracked fields are `null` on a train leg. `serviceMode` and
  `liveTracking` are present only when that leg has a train picked; read
  `kind` and `resolutionStatus: null` (no train picked yet) before
  treating a row as unmatched.
- **Privacy.** Nothing new is exposed: the phase comes from the
  subscription's `resolutionStatus`, live `status`, `trainUid`,
  `serviceDate` and public timetable data, all already on these rows. The
  rows still carry no `trackedAt`, `addedAt`, tickets or notification
  setting.
- **For ds-mcp.** Nothing has to change. The delegated Phase 3 group reads,
  when they ship, get these fields too: read `trackingPhase` and, for a
  journey, `kind`/`untrackedStatus`, the same way as on the owner's lists.

## 2026-10-20: hand-entered train legs are checked against the timetable

A train leg typed by hand is refused with `422` when its train can't
serve it. Checked: each `POST /Journeys/itinerary` train leg with `originCrs` or
`destinationCrs`; a `knownTrain` leg's `originCrs`/`destinationCrs` on
`POST /Journeys` and `POST /Journeys/{id}/legs`; and the leg's own origin
and destination on `POST /Journeys/{id}/legs/{legId}/train`.

- **Rules.** The train must stop at the origin and later at the
  destination on `serviceDate`. A passing point is not a stop. Boarding
  must be allowed at the origin and getting off at the destination. A
  request stop allows both. An absent `originCrs` or `destinationCrs`
  means the train's own end, which always passes.
- **Codes** (`error` for a delegated client; the web app gets the
  `message` as plain text): `train_does_not_call_at_origin`,
  `train_does_not_call_at_destination`, `destination_before_origin`,
  `boarding_not_allowed` (set-down-only stop or terminus) and
  `alighting_not_allowed` (pick-up-only stop, such as a sleeper's
  northbound Watford, Crewe and Preston). `message` names the train, the
  station and the date. On an itinerary with more than one leg it starts
  `Leg N: ` (1-based, untracked legs counted). Nothing is saved, and the `Idempotency-Key` stays
  unused.
- **Not in the timetable yet.** A UID with no schedule for that date (a
  late STP or VSTP service, a date not published yet) is accepted
  unchecked, as before. Saved legs are never re-checked.
- **For ds-mcp.** Handle `422` on `POST /Journeys/itinerary`: show
  `message` and don't retry the same legs. Legs taken as-is from
  `/Trips/plan` always pass. Full table:
  [connected-apps.md](connected-apps.md#itinerary-errors).

## 2026-10-20: untracked journey legs, `trackingPhase`

Additive; no existing request or field changes meaning. Spec:
`docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md`.

- **Untracked legs.** A journey leg for transport we have no data for
  (Tube, bus, walk, ferry). `POST /Journeys` (`leg`) and `POST
  /Journeys/{id}/legs` take `"mode": "untracked"` with `transitMode`
  (`tube`, `bus`, `tram`, `rail`, `walk`, `ferry`, `taxi`, `cycle`,
  `other`), optional `lineName` (60 characters), `originName` and
  `destinationName` (100 each, free text), and required
  `plannedDeparture`/`plannedArrival` (RFC3339; arrival not before
  departure, at most 24 hours later; departure at most 28 days ahead and 7
  days back). `trackingId` is `null`. No tracked-train cap applies.
- **`POST /Journeys/itinerary`.** A leg may be `{"kind": "untracked",
  "transitMode": ..., "plannedDeparture": ..., "plannedArrival": ...}`
  (the same optional fields), with no `trainUid`/`serviceDate`/CRS codes.
  A leg with no `kind` is a train leg, as before. An itinerary still needs
  at least one train leg (400 `invalid_itinerary` otherwise), so the
  top-level `trackingId` stays a number: it is now the first TRAIN leg's.
  Each `legs[]` entry gains `kind`; its `trackingId` is `null` for an
  untracked leg. A train-only body hashes for `Idempotency-Key` exactly as
  before. Delegated clients may send untracked legs; they count toward the
  20-leg cap and the daily save limit like any leg.
- **Reading legs.** Every leg of `GET /Journeys/{id}` (and the share-link
  route) gains `kind` (`train` or `untracked`), `transitMode`, `lineName`,
  `plannedDeparture`, `plannedArrival` and `untrackedStatus` (`planned`,
  `inProgress`, `done`, from the clock alone), all `null` on a train leg.
  An untracked leg's `originName`/`destinationName` are its place names,
  its `originCrs`/`destinationCrs`, `trackedTrainState` and `legSkip`
  always `null`. **Read `kind` before treating `trackedTrainState: null` as
  "needs a train picked".** `GET /Journeys/mine` rows gain the same fields
  for their current leg; a finished untracked leg is never the current
  leg.
- **Refused on an untracked leg.** `GET .../legs/{legId}/candidates` and
  `POST .../legs/{legId}/train`: 400 "This leg isn't a train leg, so it has
  no train to pick."
- **Templates.** Saving a journey as a template copies its train legs only;
  one with no train legs is a 400.
- **`trackingPhase`, `firstReportExpectedAt`.** On `GET /Train/{id}`, `GET
  /Train/mine` rows, `GET /Train/by-uid/{uid}/{date}`, every
  `trackedTrainState` of `GET /Journeys/{id}`, and `GET /Journeys/mine`
  rows (`null` for an open or untracked current leg). Values:
  `timetableOnly` (bus or ferry), `live` (Network Rail reported a
  movement, cancellation or arrival), `noTimetable` (no timetable for the
  train and no report: it can't be tracked), `planned` (timetabled, first
  report not due yet), `awaitingReport` (due; the old waiting states).
  `firstReportExpectedAt` is 45 minutes before the train's timetabled
  departure from its own origin until TRUST activates it, then that
  departure; `null` once live or when unknown. Both are `null` if the
  lookup fails. `resolutionStatus` is unchanged.
- **For ds-mcp.** Nothing has to change. To show a saved train as planned
  rather than waiting, read `trackingPhase`. To save a door-to-door plan,
  send its Tube or walk hops as untracked legs.

## 2026-10-20: whole-journey names in journey lists

Additive; no existing field changes meaning. A journey is named from its
first leg's origin to its LAST leg's destination, on its first leg's date
(`docs/style-guide.md`, "Journey names"). The list payloads only carried
one leg's ends, so a planned multi-leg journey read as ending at its
first change.

- **`GET /Journeys/mine`.** Each row gains `firstOriginCrs`,
  `firstOriginName`, `lastDestinationCrs`, `lastDestinationName` (names
  `null` when no station row resolves), `firstServiceDate`
  (`YYYY-MM-DD`) and `legCount`. `originCrs`, `destinationCrs`,
  `serviceDate` and the rest still describe the current leg, as before.
- **`GET /groups/{id}/journeys` and `GET /groups/shared-journeys`.**
  Each row gains `firstOriginCrs`, `firstOriginName`,
  `lastDestinationCrs` and `lastDestinationName`, from the journey's legs.
  The `pin*` fields are still the first leg's train pin, empty until that
  leg has a train.
- **Push notifications** (no API change): a multi-leg delay or
  cancellation names an unnamed journey "your journey to <last
  destination>", numbers the leg by its position (no gaps after a deleted
  leg), and uses station names instead of codes when both ends resolve.
- **For ds-mcp.** Nothing breaks. To name a journey the way the site does,
  use `customName`, else `firstOriginName` (or `firstOriginCrs`) to
  `lastDestinationName` (or `lastDestinationCrs`) and `firstServiceDate`.

## 2026-10-20: `journeyStops[].passingPoint`

Additive. Every `journeyStops` entry (train detail, tracked trains,
journey legs) now carries `passingPoint: boolean`: `true` where the
schedule books no stop, only a pass (`workingPass`) or no time at all.
It comes from the schedule and never changes with live data.

- **Fix.** A TRUST movement at a passing point used to fill its
  `scheduledArrival`/`scheduledDeparture` from TRUST's planned time, so
  once the train had gone by the location looked like a booked call
  (and could get `stopStatus: Skipped`). Those now stay `null` at a
  passing point and `stopStatus` stays `Unknown`. `actualArrival`,
  `actualDeparture`, `delayMinutes` and `lastEventType: PASS` are still
  set there: they are the train's progress.
- **For ds-mcp.** To list a train's stops, drop entries with
  `passingPoint: true`. Keep them only for a working-timetable or
  position view.

## 2026-10-20: `/Trips/plan` searches both service dates (`acrossMidnight`)

Additive; a request without the new parameter is unchanged.

- **`acrossMidnight=true`**, only with `dayOffset=1`. The route also
  searches `date + 1`'s trains from the same clock time and merges the two
  plans, so a time just after midnight sees the late trains of `date` and
  the first trains of `date + 1`. Example: `date=<Friday>&arriveBy=00:30&
  dayOffset=1&acrossMidnight=true`.
  - The ordering rules are unchanged. `fastest` is the one best journey:
    the latest departure for arrive-by, the earliest arrival for
    depart-after. `options` is one journey per change count, each better
    than the ones with fewer changes.
  - Identical journeys are kept once.
- **Times.** Every time in the body is on `date`'s clock, so `date + 1`'s
  trains show `dayOffset: 1`. Each train leg still names its own
  `serviceDate`, with day offsets relative to it.
- **New top-level `serviceDates`.** The dates searched: `[date]`, or
  `[date, date + 1]`. It is always present.
- **400s.** `acrossMidnight=true` without `dayOffset=1` (`acrossMidnight
  needs dayOffset=1 ...`), and any value other than `true` or `false`.
- **Other fields.** `live` sums both dates' counts and is `applied` when
  either date's live data applied. `provisional` follows the later date.
- **For ds-mcp.** Nothing to change. To plan "arrive by 00:30 on Saturday",
  send `date=<Friday>&arriveBy=00:30&dayOffset=1&acrossMidnight=true`.

## 2026-10-20: incident state from the validity windows

Spec: `docs/superpowers/specs/2026-10-20-incident-validity-state-design.md`.
New fields and a new filter value are additive; `state=active` (and
`cleared=false`) now returns fewer rows.

- **`state` on every incident.** `GET /public/incidents` rows and `GET
  /public/incidents/{id}` gain `state`: `active`, `upcoming`, `ended` or
  `cleared`. First match wins: `cleared` when RDM cleared it; `ended` when
  the feed stopped listing it (`sourceRemovedAt`); then by the incident's
  own `validityPeriods`: `active` while one covers now (or there are none),
  `upcoming` while none has started yet (or now is between two), `ended`
  once every one has finished. Until now "active" only meant "not cleared
  and still listed", so planned work next week read as active.
- **`startsAt`** (RFC3339 or `null`): when the next window starts, for
  `upcoming`. **`validityEndedAt`** (RFC3339 or `null`): when the last
  window finished, once every one has; `sourceRemovedAt` still says when a
  removed one was last listed.
- **`state=upcoming`** is a new filter value. `state=active` and the
  legacy `cleared=false` now exclude upcoming incidents and listed ones
  whose windows have all finished; `state=ended` now includes the latter.
  `total` counts the same rows as the filter.
- **Unchanged.** `isCleared`, `sourceRemovedAt`, `validityPeriods` and
  `isNow` (which means "open-ended", not "covers now"). The line status
  routes were already gated on the same windows.
- **For ds-mcp.** Prefer `state` over deriving one from `isCleared` and
  `sourceRemovedAt`; a client that ignores it keeps working.

## 2026-10-19: `/Trips/plan` overnight times (`dayOffset`)

Additive; no existing request or field changes meaning. Spec:
`docs/superpowers/specs/2026-10-19-trips-plan-arrive-by-design.md`.

- **`dayOffset=0|1`.** `1` puts `departAfter` or `arriveBy` on the
  morning after `date`, still searching `date`'s trains. "Arrive by 00:30
  on Saturday" is `date=<Friday>&arriveBy=00:30&dayOffset=1`, which finds
  Friday's late trains. Absent or empty is `0`. These are 400s: any other
  value (`dayOffset must be 0 or 1, not '2'`), and `dayOffset` without
  `departAfter` or `arriveBy`.
- **Top-level `departAfter`.** A new top-level key, `{time, dayOffset}`:
  the effective start of a depart-after search (`00:00:00` when none was
  sent), `null` for arrive-by. The top-level `arriveBy` already had this
  shape and now carries the offset.
- **Unchanged, for reference.** Arrive-by itself dates from 2026-09-29.
  `fastest` returns the latest departure that arrives in time. `options`
  lists fewest changes first, each departing strictly later.
  `departAfter` with `arriveBy` is a 400.
- **For ds-mcp.** Nothing to change. A `plan_journey` with an arrival time
  after midnight can now be sent as the previous `date` with
  `dayOffset=1` instead of going to the local planner.

## 2026-10-15: delegated access follow-ups

- **`createdVia` is owner-only.** `GET /Journeys/{id}` includes it only
  for the journey's owner; the public `GET /Journeys/shared/{token}` and a
  group member's view no longer carry the field at all. `GET
  /Journeys/mine` (always the caller's own) is unchanged.
- **Leg limit.** `POST /Journeys/itinerary` takes 1 to 20 legs from every
  caller, a delegated client included (it was 8 for a delegated client).
- **Retention.** Revoked delegated grants are kept 365 days after
  revocation (was 90), configurable with
  `DELEGATED_REVOKED_GRANT_RETENTION_DAYS` (`api.delegated.revokedGrantRetentionDays`).
  No API shape changes.

## 2026-10-15: delegated access (connected apps), Phase 1

Off by default: nothing below exists for a delegated client until the chart
lists it in `api.delegated.clients`. Spec:
`docs/superpowers/specs/2026-10-14-ds-mcp-delegated-access-design.md`;
operator doc: `docs/connected-apps.md`.

**For ds-mcp (the delegated client).** Send a `distant-signal-api` token
(`iss=https://sso.cursed.solutions/application/o/distant-signal-api/`,
`aud=distant-signal-api`, from RFC 8693 token exchange) as
`Authorization: Bearer` to the **delegated listener**,
`http://<release>-api-delegated.<namespace>.svc:8082`. The same token on the
public port is `401 invalid_token`. Never send a cookie with it (`400
ambiguous_credentials`). Routes, each needing its scope in the token, in the
user's grant and in the client's `maxScopes`:

| Route | Scope | Notes |
|---|---|---|
| `GET /Journeys/mine` | `journeys:read` | Unchanged shape, plus `createdVia` per row |
| `GET /Journeys/{id}` | `journeys:read` | Own journeys only (a group-shared one is `404`); `shareLink` always `null` |
| `POST /Journeys/itinerary` | `journeys:write` | Body below. `Idempotency-Key` header required (1 to 200 visible ASCII characters) |
| `DELETE /Journeys/{id}` | `journeys:delete` | Only journeys this client saved (`createdVia` = client name) and not shared: else `404 not_found` or `409 journey_shared`. `204` on success |

Every other route refuses a delegated token with `403 insufficient_scope`.
Errors are JSON `{error, message}` with stable codes: `400
ambiguous_credentials | idempotency_key_required | invalid_itinerary`, `401
invalid_token` (bad, expired, or issued before the grant or a "log out other
sessions"; exchange again once), `403 delegation_not_granted` (no active
grant, or no DS account: send the user to
`https://<ds>/account/connected-apps/ds-mcp`), `403 insufficient_scope`
(with `WWW-Authenticate: Bearer error="insufficient_scope",
scope="..."`), `404 not_found`, `409 idempotency_conflict | journey_shared`,
`429 rate_limited | daily_limit` (with `Retry-After`), and the existing
`503 service_unavailable`. Limits: per user 60 reads, 10 saves and 5 deletes
a minute (bursts 20, 5, 3), 20 times that per client, and 50 saves and 20
deletes per rolling 24 hours.

`POST /delegated-grants/revoke {"sub": "..."}`, with the client's existing
internal-OAuth token (group `srv-ds-mcp`), revokes that user's grant for
that client only. Always `204` for a recognised service; `401` without a
valid token, `403` for a token in no client's `serviceGroup`. Either port.

**`POST /Journeys/itinerary`** (also for the web app, with the cookie; the
key is optional there and errors are plain text):

```json
{"customName": "optional", "legs": [
  {"trainUid": "C12345", "serviceDate": "2026-10-20", "originCrs": "CRE", "destinationCrs": "PRE"}]}
```

1 to 8 legs for a delegated client (20 with a cookie); `originCrs` and
`destinationCrs` optional; unknown fields refused. One transaction. The
response is `POST /Journeys`' shape plus the legs: `{journeyId, legId,
trackingId, resolutionStatus: null, legs: [{legId, trackingId}], createdVia}`
(`legId`/`trackingId` are the first leg's). A repeat with the same key and
body returns the first response; keys are kept 24 hours per user.

**Additive for everyone.** `GET /Journeys/mine` rows and `GET
/Journeys/{id}` (and the share-link route) gain `createdVia`: `"web"` or the
client's name. `POST /public/admin/users/revoke-sessions` gains
`grantsRevoked`. The account export gains `connectedApps`,
`connectedAppConfirmations`, `connectedAppDeletions` and `journeySaveKeys`.
New cookie routes under `/public/account/`: `connected-apps` (GET),
`connected-apps-notice` (GET), `connected-apps/{client}/consent` (GET) and
`connected-apps/{client}` (POST, DELETE).

## 2026-10-14: `GET /public/chatbot/access` removed

Removal. The embedded chatbot (`/chat`) is open to every logged-in user,
so the access check it answered (`200 {"allowed": true, "access": ...}`,
`401` with no session, `403 {"error": "chatbot_not_available"}` outside
the SSO group) has nothing left to say beyond "is there a session". The
route is gone and now answers `404`; use `GET /public/auth/session`
(`authenticated`) instead. Its only caller was DS's own frontend. The
`CHATBOT_ACCESS` and `CHATBOT_ACCESS_GROUP` settings went with it.

## 2026-10-14: incident search text and total

Additive, on `GET /public/incidents`.

**`q`.** Optional free text, matched against each incident's summary and
description. Every word must match, each as a prefix ("sig fail wok"
finds "Signalling failure at Woking"), with English stemming; case and
punctuation don't matter, and only the first 8 words count. Blank (or
punctuation only, or only stop words such as "the") is no filter. Over
200 characters is a `400`. It combines with every other filter, and the
order stays newest `firstSeenAt` first.

**`total`.** The response gains `total`: how many incidents match the
filters in all, across every page. It is on the first page only and
`null` on a page fetched with `after`.

## 2026-10-14: transit rail links

Additive. New: `GET /public/transit/rail-links?crs=MAN` (3600 s):
`{"links": [{network, networkName, stopId, stopName, kind, note}]}`, the
tram stops of every served network linked to that National Rail station
(`kind` `interchange` or `nearby`), by network then stop name. `crs` is
three letters, either case; anything else is a `400`. Unlike the other
transit routes this is never a `404`: with no network served, or no link,
it is `{"links": []}`.

A stop's `railLinks` entries gain `stationName`, the National Rail
station's name, or `null` when the station list doesn't have the CRS.

## 2026-10-14: transit disruptions and line status

Additive, on the `/public/transit` routes below, and as dark as they are
(nothing is served for a network not listed in `API_TRANSIT_NETWORKS`).
Statuses come from the network's published disruptions (for Metrolink,
TfGM's SIRI-SX situations in the Bus Open Data Service), never from the
timetable.

**`status`.** Each line in `GET /public/transit/{network}`'s `lines`, and
`GET /public/transit/{network}/lines/{line}`, carry `status`: `{state,
asOf, statuses: [{severity, description, reason, situationId}], upcoming:
[{from, to, summary, situationId}]}`. The network also carries `status:
{state, asOf}`. `state` is:

- `current`: the disruptions were read under 30 minutes ago. `statuses`
  is worst first, and the first is the line's headline. A line with
  nothing published reads severity 27, "No Reported Disruption" (not
  "Good Service": nothing is known about the service itself);
  `Information` statuses follow it and never replace it.
- `unavailable`: the disruptions are older than 30 minutes (`asOf` says
  when they were read). `statuses` and `upcoming` are empty.
- `none`: the network has no disruptions feed. Show no status.

`severity` is the same number as `statusSeverity` elsewhere in this API
(Suspended 2, Part Suspended 3, Severe Delays 6, Reduced Service 7, Minor
Delays 9, Information 26, No Reported Disruption 27). A transit line's
status is not served by `/Line/{ids}/Status` or
`/Line/Mode/{modes}/Status`.

**Situations.** New: `GET /public/transit/{network}/situations` (60 s):
`{state, asOf, current: [...], upcoming: [...]}`, each situation `{id,
summary, description, infoUrl, planned, reason, scope, lines, stops,
advice, validity: [{start, end}], starts}`. `scope` is `line`, `stop` or
`network`; `lines` are public line IDs; `stops` station IDs; `starts` is
an upcoming situation's next start (within 7 days). `description` is plain
text as published: render it as text, never as HTML. `current` is ordered
by validity start, `upcoming` by `starts`. Both are empty unless `state`
is `current`.

A line's response adds `situations` (the same shape): the current and
upcoming situations that change its status or name one of its stops. A
stop's response adds `notices`: the current situations naming it (a lift
out of service, the stop closed). The line and stop routes' cache drops
from 300 s to 60 s.

## 2026-10-14: transit networks (Manchester Metrolink), scheduled only

New endpoints, additive only, under `/public/transit`, for transit networks
that are neither National Rail nor TfL (Manchester Metrolink first).
Unauthenticated, read-only, the same body for every caller.

**Dark until enabled.** A network is served only once the operator lists
it (`API_TRANSIT_NETWORKS`). Until then every route for it answers `404`
and `GET /public/transit/networks` returns `{"networks": []}`. Metrolink
is not listed by default.

**Scheduled, never live.** Boards and timetables come from the published
timetable. Their bodies carry `"realtime": false` and `"kind":
"scheduled"`, and no row has an expected time, delay, cancellation or
status. A scheduled tram is not one known to be running.

Path segments: `{network}` and `{line}` are local IDs (`metrolink`,
`blue`); `{stop}` is a station's NaPTAN code (`9400ZZMAPIC`). Bodies give a
line's public ID, `<network>:<line>` (`metrolink:blue`); that form in a
path is a `404`.

| Route | Returns | Cache |
| --- | --- | --- |
| `GET /public/transit/networks` | `{"networks": [{id, name, mode, attribution, feedVersion, horizonFrom, horizonTo, timetableAsOf, situationsAsOf}]}` | 300 s |
| `GET /public/transit/{network}` | the network as above, plus `lines: [{id, lineId, name, colour}]` | 60 s |
| `GET /public/transit/{network}/lines/{line}` | `{id, lineId, name, colour, directions: [{direction, headsign, stations: [{id, name}]}]}`; `direction` is `outbound` or `inbound` | 300 s |
| `GET /public/transit/{network}/lines/{line}/timetable` | `{date, direction, stations: [{id, name}], trips: [{tripId, headsign, times}], outsideHorizon, realtime, kind}`; `times` has one `HH:MM` or `null` per station | 300 s |
| `GET /public/transit/{network}/stops` | `{"stops": [{id, name, lat, lon, lines, railLinks}]}`; `lines` are public line IDs; `railLinks: [{crs, tiploc, kind, note}]`, `kind` `interchange` or `nearby` | 3600 s |
| `GET /public/transit/{network}/stops/{stop}` | a stop as above, plus `platforms: [{id, indicator}]` | 300 s |
| `GET /public/transit/{network}/stops/{stop}/departures` | `{date, departures, nextCursor, outsideHorizon, realtime, kind}` | 60 s |

Timetable parameters: `date` (`YYYY-MM-DD`, default today), `direction`
(default `outbound`), `from` and `to` (`HH:MM`, default the three hours
from the start of the last hour). A bad value is a `400`.

Departures parameters: `date` (default today), `after` (`HH:MM`, default
now today, else midnight), `limit` (default 20, at most 100), `cursor`
(a previous page's `nextCursor`). Each departure: `scheduledDeparture`
(RFC 3339 UK time), `tripId`, `line` (`{id, lineId, name, colour}`, or
`null` for a replacement bus), `destination`, `platformId`, `platform`
(the platform's indicator, such as "To Ashton", or `null`),
`replacementBus`. A departure after midnight from the previous day's
timetable is listed on the day it departs. A date outside the stored
timetable (about 60 days) gives no departures and `"outsideHorizon":
true`, not "no service".

**Attribution.** `attribution` is `{licence, text}`. For Metrolink:
"Contains public sector information licensed under the Open Government
Licence v3.0. Timetables and disruptions: Bus Open Data Service
(Department for Transport). Stops: NaPTAN (Department for Transport)."
Show it wherever the data is shown.

## 2026-10-12: recently cleared incidents

New endpoint, additive only: `GET /public/incidents/recently-cleared`.
Unauthenticated; the same body for every caller.

It lists National Rail incidents that the National Rail Knowledgebase has
cleared but still lists, cleared since 02:00 UK time today, newest clear
first. These are for information only: a cleared incident never affects a
line's status. The feed purges cleared incidents nightly at about 23:00
UTC, so the list is usually empty overnight.

Query parameters (at most one):

| Parameter | Meaning |
| --- | --- |
| `line` | A catalogue line ID. Incidents whose matched lines include it. An unknown, TfL or custom line ID is a `400` (`unknown line`). |
| `station` | A 3-letter station code (any case). Incidents matched to any catalogue line that calls there, as `GET /StopPoint/{crs}/Disruption` places live disruptions. Anything else is a `400`. |
| neither | Every recently cleared incident. |

Both at once is a `400` (`use either line or station, not both`); so is
any other parameter.

Response: `{"results": [item, ...]}`, each item:

| Field | Type | Meaning |
| --- | --- | --- |
| `incidentId` | string | As in `GET /public/incidents/{incidentId}`. |
| `summary` | string | The clearing summary as published, often `CLEARED: ...`. |
| `description` | string | The clearing text as published (HTML, usually "... has now ended"). |
| `operators` | string[] | ATOC codes. |
| `affectedLines` | `{id, name}[]` | The catalogue lines it was matched to. |
| `clearedAt` | RFC 3339 | When DS first saw it cleared (the first history row of its current cleared run; a reopen starts a new one). |
| `lastListedAt` | RFC 3339 | When the feed last listed it. |
| `expectedUntil` | RFC 3339 or `null` | The end time the last uncleared text stated ("Disruption is expected until 22:30"), as an instant. Show it only while it is still in the future: then the incident cleared early. |
| `expectedUntilEndOfDay` | boolean | The last uncleared text, published the same rail day, said "expected until the end of the day" (or "of service"). |

The stated end is read from the text with a pattern over the
Knowledgebase's own wording, not the AI summary. An end on a later day
("until 04:00 on Tuesday 8 September") is not reported.

Example: `{"results": [{"incidentId": "0071A58BD9B04B06B365FFBE1E02CD94",
"summary": "CLEARED: Residual disruption between Brockenhurst and
Bournemouth", "description": "<p>Disruption ... has now ended.</p>",
"operators": ["SW"], "affectedLines": [{"id": "swr-south-west-main",
"name": "South West Main Line"}], "clearedAt": "2026-10-02T21:26:00+00:00",
"lastListedAt": "2026-10-02T22:55:12+00:00", "expectedUntil":
"2026-10-02T21:30:00+00:00", "expectedUntilEndOfDay": false}]}`

## 2026-10-12: the internal `/private/*` routes are removed (ingest release B)

No public route changes. The internal ingest and internal-read routes
under `/private/*` are deleted: every `/private/...` path now answers the
ordinary `404` of an unknown route. They were never part of the public
API: only DS's own producers called them, with an internal service
token, and since release A (2026-10-10) every producer writes Postgres or
a Redis stream directly and reads Postgres directly
(docs/ingest-phase5-runbook.md).

Removed: `/private/station-samples`, `/full-coverage-stats`,
`/full-coverage-window-stats`, `/station-full-coverage-samples`,
`/tfl-line-status`, `/incidents`, `/stations`, `/tocs`,
`/schedule-calling-points-full`, `/schedule-destination-departures`,
`/schedule-network-departures`, `/schedule-line-population`,
`/schedule-services`, `/schedule-reference-publishes`,
`/schedule-feed-ingests`, `/corpus-locations`, `/fixed-links`,
`/tiploc-crs`, `/tiploc-locations`, `/stanox-crs` (the `/private` one;
`/public/stanox-crs` stays), `/trust-event-backlog`, `/train-reasons`,
`/train-events`, `/train-forward-signals`, `/tracked-trains` and
`/sample-stations`.

DS-MCP is unaffected: its `srv-ds-mcp` bearer token still buys its own
rate-limit budget on the public routes.

## 2026-10-12: "station code" and "login" in API error text

No shape changes or status code changes. Error bodies the API writes
use the site's terms (docs/style-guide.md, "Terminology"):

- Station code validation errors say "station code" instead of "CRS
  code": "Station codes are three letters, like WOK or EUS." (pins,
  track-by-UID, tickets, journeys, journey templates); "{field} must be
  a 3-letter station code" (`/public/trains/search`, `callingAt` on
  departures); "'XYZ' is not a valid 3-letter station code" (custom
  lines); "use a three-letter station code" (line timetable `from`/`to`);
  "'XYZ' is not a recognised station code" (trip planning, including
  `via:` errors).
- The plain-text error bodies of `/public/auth/login` and
  `/public/auth/callback` say "login failed", "login was not completed"
  and "login temporarily unavailable" instead of "sign-in ...".

A client matching on the old wording ("CRS code", "sign-in") must
match the new text.

## 2026-10-10: no em dashes in API-written text

No shape changes. Text the API and aggregator write themselves no longer
uses an em dash or a "--":

- Validation errors (400/409 bodies) are two sentences: "That departure
  is too far ahead. Trains can be tracked up to 28 days before they run."
  instead of "... too far ahead — trains can ...". Likewise the station
  code, departure age, custom and group name length, group full, train
  identifier, template leg and track-by-UID messages, and the window
  search error ("... to search a window, or pick a specific known
  departure instead.").
- The Delay Repay `disclaimer` (estimate) and top-level `disclaimer`
  (route) say "Distant Signal never submits a claim on your behalf." as
  their own sentence, in place of "-- this app never submits ...".
- A line-status `reason` with a high-confidence "resolved" extraction
  ends `(reported resolved, showing residual impact)` instead of
  `(reported resolved — showing residual impact)`.

Third-party text (National Rail incident summaries and descriptions,
TfL reasons) is passed through unchanged. Line-status history already
stored keeps the old wording.

## 2026-10-09: plainer line-status `reason` text

No shape changes. The aggregator's free-text `reason` on line statuses
(`/public/status` and the line endpoints) reads differently:

- The `(most cited: X)` suffix on sample-inferred reasons is now a
  sentence, `Main cause: X.`
- The `(shared trunk — also affects other lines)` suffix is now `Also
  affects other lines on this route.`
- An extraction escalation no longer appends `reported more severe than
  automatically classified: …`. The severity still rises; only the text
  is gone.

Line-status history already stored keeps the old wording.

## 2026-10-09: `identity` (the train's headcode) on train search rows

Additive only: one new key on every row. Nothing else changes.

| Field | Type | Meaning |
| --- | --- | --- |
| `identity` | string or `null` | The CIF Train Identity, i.e. the timetabled headcode (`BS` record, columns 33-36), e.g. `"1S00"`. |

- Where: each `results[]` row of `GET /public/trains/search`, and each
  candidate of `GET /Journeys/{journeyId}/legs/{legId}/candidates` (the
  same row renderer).
- Always present. `null` when the schedule's Train Identity is blank, or
  for a row published before DS stored it (filled at that service date's
  next publish). It is never omitted.
- It is the timetabled identity of the schedule that won for the service
  date. A live train can run under a different headcode, so do not use it
  to match TRUST/TD reports; use `uid` and the service date.
- Example: `{"uid": "C40001", "scheduled": "08:22", ..., "operator": "GW",
  "identity": "1A23", ...}`.

A client should treat a missing `identity` (an older DS) as `null`.

## 2026-10-08: the same tracking limits for every leg type

The 28-day horizon and the cap of 100 upcoming tracked trains used to
apply only to pins and track-by-uid. They now apply to every way of
tracking. No request or response shape changes; the new refusals are
`400` (`text/plain`) with the pins' own wording.

- Horizon (a `serviceDate` after today+28, London):
  `That departure is too far ahead — trains can be tracked up to 28 days
  before they run.` Now checked for `knownTrain` and `window` legs on
  `POST /Journeys` and `POST /Journeys/{id}/legs`, the picked train on
  `POST /Journeys/{id}/legs/{legId}/train`, and the date on
  `POST /JourneyTemplates/{id}/materialize`. `POST
  /Train/by-uid/{uid}/{date}/track` now uses this wording too (it said
  "That train is too far ahead").
- Cap (100 or more subscriptions dated today or later):
  `You're already tracking 100 upcoming trains, which is the maximum.
  Remove some to make room.` Now checked on the same routes. A
  `knownTrain` leg or a train pick for a train the user already tracks
  reuses that subscription and is still allowed at the cap. A `window`
  leg (and a template run, whose legs are window legs) is checked even
  though it holds no subscription until its train is picked.
- Only creation is checked. Existing journeys, legs and subscriptions
  past either limit are untouched, and the notifier's scheduled template
  runs and auto-commits are not checked.
- Group shares are unaffected: sharing links an existing subscription or
  journey and creates neither.

## 2026-10-08: `provisional` for far-ahead service dates

Additive only: two new top-level keys on three responses, and one on
`GET /public/trains/search/dates`. Nothing is filtered or changed.

### The rule

A service date is **provisional** when it is more than N days after
today (London): `serviceDate > today + N`, i.e. `serviceDate >=
provisionalFrom` where `provisionalFrom = today + N + 1`. N is the
deployment setting `SCHEDULE_PROVISIONAL_AFTER_DAYS` (0-60, default 7, the
old publish horizon), so by default today..today+7 are firm and
today+8..today+28 are provisional.

Why: late STP overlays and cancellations (engineering works, replacement
buses) reach the CIF only a few weeks before the day, so a far date's
timetable can still change between daily deliveries. No per-row field in
the data says whether a date's short-term plan is complete
(`schedule_services.stp` says which record won, `P`/`O`/`N`, not whether
another is coming), so the rule is distance from today, applied once per
response.

### New keys

| Key | Type | Meaning |
| --- | --- | --- |
| `provisional` | boolean | `true` when the response's service date is `provisionalFrom` or later. |
| `provisionalFrom` | `"YYYY-MM-DD"` | The first provisional service date, so a client can mark dates without its own copy of N. Moves at London midnight. |

Both are top-level, once per response (every row, journey and leg in the
response shares its one service date):

- `GET /public/trains/search`: next to `results` and `nextCursor`, for the
  searched `date` (today when omitted). Rows are unchanged. Every page of
  one search carries the same values.
- `GET /Train/by-uid/{uid}/{date}`: next to the train's own fields, for
  `{date}`. Distinct from `delayProvisional`, which is about a delay
  forecast, not the timetable.
- `GET /Trips/plan`: next to `segments`/`journeys`, for `date`. Applies to
  every journey and leg in the plan; there is no per-journey or per-leg
  copy.
- `GET /public/trains/search/dates`: gains `provisionalFrom` only (it may be
  later than `to` when N is raised).

Example (today 2026-10-08, default N=7),
`GET /public/trains/search?station=PAD&date=2026-10-20`:
`{"results": [...], "nextCursor": null, "provisional": true,
"provisionalFrom": "2026-10-16"}`; the same search for `2026-10-15` has
`"provisional": false`.

A client should treat a missing `provisional` (an older DS) as `false`.
Errors (`400`/`404`, `text/plain`) are unchanged and carry neither key.

## 2026-10-08: `/public/trains/search` dates beyond 7 days, and the arrival at `stops_at`

Additive only: a request that worked before gets the same answer, plus
the new `stopsAt*` fields when it sets `stops_at`.

### `date`: any published date

- `date` is now accepted when it is within 7 days of today (as before) OR
  within the range of service dates DS holds timetable rows for. DS
  publishes today plus 28 days ahead (`SCHEDULE_FORWARD_PUBLISH_DAYS`) and
  keeps 8 days back, so in practice the range is today−8 to today+28. It
  follows the publish window and retention on its own. Querying a far date costs the same as
  querying today.
- A date outside both is a `400` (`text/plain`) naming both ranges:
  `date must be between 2026-09-30 and 2026-10-15: schedule data is
  published for 2026-09-30 to 2026-10-14`. With no rows at all, it is the
  text from before plus a suffix: `date must be within 7 days ago and 7
  days from today: no schedule data is published`.
- Unchanged: a date inside the range with no rows of its own (not yet
  published, or a gap) is still a `404`, `no CIF-derived schedule data has
  been published for YYYY-MM-DD`; a malformed date is still `400 date must
  be YYYY-MM-DD`. `GET /public/trains/resolve` keeps the plain ±7-day
  window.

### New: `GET /public/trains/search/dates`

`200 {"from": "2026-09-30", "to": "2026-10-15", "publishedFrom":
"2026-09-30", "publishedTo": "2026-10-14"}`. `from`/`to` (inclusive) bound
the `date`s the search accepts; `publishedFrom`/`publishedTo` bound the
dates with rows (`null` when there are none). Dates are London service
dates.

### New row fields with `stops_at`

Only when the request sets `stops_at`, each `results[]` row gains the
arrival at that station. Without `stops_at` the keys are absent.

| Field | Type | Meaning |
| --- | --- | --- |
| `stopsAtArrival` | `"HH:MM"` or `null` | Public (GBTT) arrival at `stops_at`. |
| `stopsAtArrivalDayOffset` | integer or `null` | Days after `date` (the service date) that `stopsAtArrival` falls on. |
| `stopsAtWorkingArrival` | `"HH:MM"` or `null` | Working-timetable arrival there: the time `arrival_from`/`arrival_to` compare. |
| `stopsAtWorkingArrivalDayOffset` | integer or `null` | Days after `date` for `stopsAtWorkingArrival`. |

- **Which call.** The call is the same one the `stops_at` filter matched:
  the EARLIEST call at `stops_at` that comes after `station` in the journey,
  allows alighting, and, when given, has its working arrival inside
  `arrival_from`/`arrival_to`. If `stops_at` is the train's terminus and no
  earlier call qualifies, it is the terminus (then the fields equal
  `publicDestinationArrival`/`destinationArrival` and their offset). So a
  train calling at `stops_at` twice reports the first call, unless an
  arrival bound excludes it, in which case it reports the call that
  satisfied the bound.
- **Day offsets** count from the service date, like `dayOffset`. A stop
  reached after midnight has `1`. An arrival before midnight at a stop left
  after midnight has the earlier day. A public time that rounds across
  midnight (23:59H working, 00:00 public) is on the next day.
- **Null.** A time and its offset are `null` together, when that call has
  no such time stored (a public time before the schedule's next publish).
- Example, `?station=PAD&stops_at=RDG&date=2026-10-20`:
  `{"uid": "C12345", "stationCrs": "PAD", "publicDeparture": "23:48",
  "dayOffset": 0, "destinationCrs": "BRI", "publicDestinationArrival":
  "01:32", "destinationArrivalDayOffset": 1, "stopsAtArrival": "00:15",
  "stopsAtArrivalDayOffset": 1, "stopsAtWorkingArrival": "00:14",
  "stopsAtWorkingArrivalDayOffset": 1, ...}`

## 2026-10-08: track trains four weeks ahead

The tracking horizon follows the timetable window below: a train can now
be tracked up to 28 days before it runs (was 7). No request or response
shape changes.

- `POST /Train/by-uid/{uid}/{date}/track`, `POST /Train/track` and a
  `pin`-mode leg on `POST /Journeys` accept a date up to today+28
  (London). Later dates are still `400`, now reading
  `... trains can be tracked up to 28 days before they run.` A pin's
  `scheduled_departure` may be up to 29 days after the request (the extra
  day covers a service running past midnight).
- A far pin is accepted as `resolutionStatus: "pending"` and normally
  schedule-matches the day before it runs, when that date's line
  timetable is published; nothing changes for the client in between.
- Notifications are unchanged: a far tracked train is notified only for
  real movement (delay, cancellation) and the same-day skip check.
- The per-user cap of 100 upcoming tracked trains is unchanged; with the
  longer window it covers more days ahead.
- Unchanged: `GET /public/trains/resolve` keeps its 7-day window (it
  resolves live departure-board rows, which never reach that far), and
  `GET /public/trains/search` keeps its own window (see its entry
  above).
- `SCHEDULE_FORWARD_PUBLISH_DAYS` now accepts 28-60 (was 7-60): it can no
  longer be set below the tracking horizon.

## 2026-10-08: timetable data four weeks ahead

DS now publishes its timetable products for today through today+28 (was
today+7). No request or response shape changes; dates that used to `404`
for lack of data now answer.

- `GET /Trips/plan`: a `date` up to today+28 now plans. Past that it is
  still `404`, `no CIF-derived schedule data has been published for
  YYYY-MM-DD yet`.
- `GET /Train/by-uid/{uid}/{date}`: a train's scheduled stops and its
  bus/ferry mode are available up to today+28.
- `GET /public/trains/search`: the rows exist up to today+28. The route
  itself accepted only dates within 7 days of today until the
  "any published date" change to `date` above; with it, its range
  (and `GET /public/trains/search/dates`'s `publishedTo`) reaches
  today+28 with nothing else to change.
- Tracking a train (`/Train/by-uid/{uid}/{date}/track`, pins) now
  reaches today+28 too: see "track trains four weeks ahead" above.
  `GET /public/trains/resolve` keeps its 7-day forward limit.
- A far date shows the timetable as planned at the latest daily
  delivery. Late changes (engineering-works replacements, cancellations)
  often reach the timetable only a few weeks ahead, so results for a
  date 2-4 weeks away can change between deliveries. (Since the
  `provisional` entry above, responses mark such dates.)
- The window is a deployment setting (`SCHEDULE_FORWARD_PUBLISH_DAYS`,
  28-60 since the tracking change above, default 28). Dates past today+7 appear after the first delivery
  following the deploy (deliveries land around 20:00 UTC).

## 2026-10-08: `GET /public/ready` and graceful shutdown

- New `GET /public/ready`, the readiness probe: `200 {"status":"ready"}`,
  or `503 {"status":"draining"}` while the pod is shutting down. Only with
  `API_READINESS_CHECKS_DB=true` (default false; meant for 2+ replicas) it
  also answers `503 {"status":"database_unreachable"}` when the database
  did not answer `SELECT 1` within 2 s (checked at most every 5 s).
  `Cache-Control: no-store`. `GET /public/health` is unchanged (liveness).
- On shutdown the api finishes in-flight requests (up to
  `API_SHUTDOWN_DRAIN_SECS`, default 20 s) instead of resetting them. A
  keep-alive connection is closed after its current response, so a client
  must reconnect for its next request, as on any closed connection.

## 2026-10-07: live status and origin names on the schedule lists

Additive only: every existing field is unchanged. Applies to each row of
`GET /public/trains/search` (`results[]`) and of
`GET /public/stations/{crs}/schedule-departures`.

### New fields on both

- `live`: the same compact object as `live` on
  `GET /public/lines/{id}/trains?view=summary`:
  `{"status", "delayMinutes", "delayProvisional", "cancelled",
  "lastReportedLocation"}`, for the row's train on its service date.
  `null` when the train has no live state (not activated by TRUST yet, or
  never looked up), which is common for later trains. Example:
  `"live": {"status": "en_route", "delayMinutes": 4, "delayProvisional":
  false, "cancelled": false, "lastReportedLocation": "Reading"}`.
- `originName`: the station name of `originCrs` (the schedule's true first
  calling point), or `null` when the code is `null` or has no station.

### New fields on `GET /public/trains/search` only

- `dayOffset`: days after the searched `date` (the service date) that the
  departure from `station` falls on: `1` for a train that started before
  midnight and calls at `station` after it. The same convention as
  `/schedule-departures`' existing `dayOffset`: it moves the departure's
  calendar day, never the service date, so `GET /Train/by-uid/{uid}/{date}`
  still takes the searched `date`. `0` otherwise.

### New fields on `GET /public/stations/{crs}/schedule-departures` only

- `originCrs`: the schedule's true origin CRS (the same meaning as on
  `/public/trains/search`), or `null` when it does not resolve to a
  station. That route's `dayOffset` already existed.
- With `?date=` (the entry below), `live`, `originCrs` and `originName`
  are looked up for the requested service date, like the rows themselves.

## 2026-10-07: `?date=` on station schedule departures

### New query parameter (`GET /public/stations/{crs}/schedule-departures`)

- `date` (optional, `YYYY-MM-DD`): serve that London service date's
  CIF-derived bucket instead of today's. Only today or yesterday is
  accepted; any other date, or a malformed one, is a `400`. Omitting it is
  unchanged (today).
- Every row in a response has the requested date as its CIF service date;
  `dayOffset` counts from it. So a row's `/Train/by-uid/{uid}/{date}` link
  uses the requested date, and its departure falls on `date + dayOffset`.
- Why: a service that left its origin last evening can call at a station
  after midnight. Its rows are in yesterday's bucket with `dayOffset >= 1`,
  and today's bucket cannot hold them. DS's `/track` picker asks for
  yesterday between London 00:00 and 02:00 and keeps those rows still due.
- An older server ignores the parameter and returns today's bucket.

## 2026-10-07: `/Trips/plan` waypoint groups (`waypoints=group:LON`), `GET /Trips/station-groups`

Design: `docs/superpowers/specs/2026-10-07-trips-plan-or-group-vias-design.md`,
"Waypoint groups". This closes `viaStop` with `LON` for DS-MCP: send it as
`waypoints=group:LON`. The entry below (OR vias) still applies; this one
replaces its "waypoints take single stations" 400.

### Changed parameter: `waypoints` entries may be OR choices

`waypoints=ALT[|ALT...][,ALT[|ALT...]...]`, the same syntax as `via`: commas
separate the waypoints (ordered, each one a stop), `|` the alternatives of
one waypoint, and an alternative is a CRS, a `tiploc:` code or `group:NAME`
(any case). `|` may be raw or `%7C`.

- **Semantics:** the journey must STOP at (call at, alight at, or walk
  into) ANY one station of each waypoint, in order. A train running
  through without calling does not count, as for a single waypoint.
- **Which member:** plain earliest arrival. The member a real train reaches
  first on the best journey wins (the latest-departing one for
  `arriveBy`), with no preference order among members. The member can
  differ per journey.
- **Dwell and changes:** no group-specific rule. Staying aboard the train
  that reached the member is no change (`continuesPreviousTrain: true` on
  the next segment). Otherwise the member's own minimum change time
  applies (5 minutes at a no-interchange sentinel), and `maxChanges`
  counts the whole journey.
- **Ends:** a choice that is the FIRST waypoint and includes the origin is
  satisfied at the origin, and a LAST one including the destination at
  the destination. Such a waypoint gets NO segment (see
  `waypointSatisfiedBy`). One holding both ends is satisfied at the
  origin. A member equal to an end anywhere else is a real stop (the
  journey must return to it). A single-station waypoint equal to an end
  is still a 400.
- **Avoid lists:** a member in `avoid`, `avoidStop` or `avoidChange` is
  dropped from the choice. It is a 400 only if no member is left.
- **Limits:** a choice is ONE waypoint against the cap (20) and in the
  `results=options` guard `(waypoints + 1) * (2 * vias + 1) * (maxChanges
  + 2)`. A choice has at most 24 stations, and the waypoints together at
  most 54, groups expanded and single stations counting 1. That allows 3
  `group:LON`, or 2 and 18 single stations. This budget is separate from
  the vias'.
- Single-station waypoints behave, and are answered, exactly as before.

### New and changed fields

- `segments[s].originCrs`/`destinationCrs`: a choice's label
  (`group:LON`, `KGX|EUS`) where a segment starts or ends at it. Each
  journey's legs show the actual station.
- `journeys[j].waypointSatisfiedBy` (ONLY when at least one waypoint is a
  choice): one entry per requested waypoint, in order:
  - `crs`: the waypoint as requested (its label);
  - `matchedCrs`: the station the journey stopped at (a CRS, or a
    `tiploc:` code);
  - `segment`: the index of the segment that ends there (the next one
    starts there). For `how: "origin"` it is 0; for `"destination"`, the
    last segment;
  - `how`: `call` (a train took the traveller there), `walk` (a transfer
    leg into it), `origin` or `destination` (an end of the trip satisfied
    it).
- `stationGroups` also lists the groups named in `waypoints`.

```json
"segments": [{"originCrs": "CBG", "destinationCrs": "group:LON", "...": "..."},
             {"originCrs": "group:LON", "destinationCrs": "BTN", "...": "..."}],
"journeys": [{"waypointSatisfiedBy": [
  {"crs": "group:LON", "matchedCrs": "KGX", "segment": 0, "how": "call"}
]}]
```

With `origin=KGX&waypoints=group:LON&destination=CBG` there is one segment
(KGX to CBG), and `waypointSatisfiedBy` is
`[{"crs": "group:LON", "matchedCrs": "KGX", "segment": 0, "how": "origin"}]`.

### New 400s

- `waypoints: 'X' stands for 25 stations, at most 24 allowed in one waypoint`
- `waypoints: the waypoints stand for 56 stations together, at most 54 allowed`
- `waypoints: 'group:XYZ' is not a known station group (known: group:LON)`
- `waypoints: 'KGX|' has an empty alternative`
- `waypoints: every station of 'KGX|EUS' is in an avoid list; a trip
  cannot stop at one and avoid them all`
- `waypoints: 'group:LON' and 'KGX' are next to each other and share 'KGX';
  one stop there would satisfy both, so name stations they do not share or
  put another waypoint between them`: two adjacent waypoints, at least one
  a choice, that share a station (after avoided members are dropped).
- An unknown station in a choice: `ORIGIN -> LABEL: 'ZZZ' is not a
  recognised station CRS code` (as for single waypoints).
- Removed: `waypoints: '...' is a choice of stations; waypoints take single
  stations ...`.

### New endpoint: `GET /Trips/station-groups`

Every group `group:NAME` accepts, for a picker. No parameters, no
authentication, `Cache-Control: public, max-age=3600` (the groups change
only with a deploy).

```json
{"groups": [{"group": "LON", "code": "group:LON", "name": "London Terminals",
  "members": [{"crs": "BFR", "name": "London Blackfriars"}, "..."]}]}
```

`code` is the entry to send. `members` are in a fixed order, and a
member's `name` may be `null`.

## 2026-10-07: `/Trips/plan` OR-choice vias and station groups (`group:LON`)

Design: `docs/superpowers/specs/2026-10-07-trips-plan-or-group-vias-design.md`.
This closes the last gap keeping DS-MCP's `plan_journey` on its local engine
for a `LON` group code in `via` or `avoid`/`avoidStop` (`dsTripPlanEligible`).
`viaStop` is closed by the entry above (waypoint groups).

### Changed parameter: `via` entries may be OR choices

`via=ALT[|ALT...][,ALT[|ALT...]...]`. Commas still separate the vias, which
are ordered and at most 3, and must ALL be passed. `|` separates the
alternatives of one via; ANY one of them satisfies it. An alternative is a
CRS, a `tiploc:` code or a named group `group:NAME` (any case). `|` may be
sent raw or as `%7C`.

- `via=group:LON`: through any of the eighteen London Terminals (BFR CST
  CHX CTK EUS FST KGX LST LBG MYB MOG OLD PAD STP VXH VIC WAT WAE).
- `via=KGX|EUS|STP`: through any of the three.
- `via=group:LON|CBG,PBO`: through a London terminal or Cambridge, then
  Peterborough.
- What counts as passing a member is what counts for a single via: a
  call, a timing-point pass, a change or a walk.
- **Limits:** at most 24 stations in one via, and 54 across all vias, with
  groups expanded. A choice counts as ONE via against the cap of 3 and in
  the `results=options` search-size guard.
- **Ends:** a member equal to the origin or the destination satisfies the
  via there. Every journey passes it, so for example `origin=KGX` with
  `via=group:LON` always holds. A single-station via equal to an end is
  still a 400.
- **With `avoid`:** avoided members are never used. The via is a 400 only
  when EVERY station of it is in `avoid`.
- Single-station vias behave exactly as before.

### Changed parameter: avoid lists take groups

`avoid`, `avoidStop` and `avoidChange` accept `group:NAME` entries, which
avoid (or don't stop at, or don't change at) every member. A group counts as
one entry against the cap of 8 per list. An avoid list already means "none
of these", so it needs no `|`: `|` there is a 400. An avoided member that is
the origin, the destination or a waypoint is a 400 naming the group.

### New and changed fields

- `stationGroups` (top level, always present): each group the request
  named, mapped to its members, e.g. `{"LON": ["BFR", "CST", ...]}`.
  It is `{}` when no group was named.
- `via`: each via's label as applied, e.g. `["group:LON", "KGX|EUS", "PBO"]`.
- `avoid`/`avoidStop`/`avoidChange`: as applied, so with groups expanded
  into their member CRS codes.
- `journeys[j].viaSatisfiedBy[k].matchedCrs`: the station that satisfied
  via `k`, which for a choice is the member the journey passed. `crs` stays
  the via as requested (its label).

```json
"via": ["group:LON"],
"stationGroups": {"LON": ["BFR", "CST", "CHX", "...", "WAE"]},
"journeys": [{"viaSatisfiedBy": [
  {"crs": "group:LON", "matchedCrs": "KGX", "segment": 0, "leg": 1, "how": "call"}
]}]
```

- `noResultReason` (`constraint: "via"`) carries the label in `values`
  (`["group:LON"]`). Its message reads "... passes through any of
  group:LON ..." or "any of KGX, EUS".

### New 400s

- `via: 'group:XYZ' is not a known station group (known: group:LON)`
- `via: 'KGX|' has an empty alternative`
- `via: 'Z00|...' stands for 25 stations, at most 24 allowed in one via`
- `via: the vias stand for 60 stations together, at most 54 allowed`
- `via: 'KGX|EUS' is given twice in a row; name it once`: the same station
  set twice in a row, however it is written.
- `via: every station of 'KGX|EUS' is also in avoid; a trip cannot both
  pass through one and avoid them all`
- `via: 'ZZZ' is not a recognised station CRS code`: this now also names
  an unknown alternative.
- `avoid: 'KGX|EUS' uses '|', which only via takes; ...`
- `avoid: 'group:LON' includes 'PAD', which is the destination; a trip
  cannot avoid it`
- (Superseded by the waypoint-groups entry above: waypoints now take
  choices.) `waypoints: 'GROUP:LON' is a choice of stations; ...`

## 2026-10-07: walks between bus stops and their stations in the planner

Design: `docs/superpowers/specs/2026-10-06-tiploc-locations-design.md`
("Walking links to the parent station").

### Changed behaviour (`GET /Trips/plan`, no new fields)

- A journey can now change between a bus stop or ferry terminal and its
  parent station (`parentCrs`): bus, then walk, then train, and the reverse.
  The walk is an ordinary transfer leg: `{"kind": "transfer", "mode":
  "WALK", "originCrs": "tiploc:HTRBUS2", "destinationCrs": "HXX",
  "minutes": 5}`. So a transfer leg's `originCrs`/`destinationCrs` can now
  be a `tiploc:` code, as train legs' already could. Clients that branch
  on `leg.kind` need no change.
- Walking on from a bus or ferry is now a change off it. It owes the same
  5-minute buffer (configurable) as changing at the stop, before the walk
  starts. This applies to every walking or transfer link (e.g. a
  rail-replacement bus, then a walk to the Underground), and arrive-by
  already charged it. `minutes` is the walk alone. The buffer shows only
  in the times.

## 2026-10-07: Delay Repay borderline zone, outcome, ticket-aware bands and schemes

Design: `docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md`,
"Decisions (2026-10-07)". Applies to `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`
and to every item of `GET /Train/tickets/mine` (the two now serve the same
fields from one function).

### New fields on the response (and each ticket list item)

- `outcome`: `arrived`, `departedOnly` or `notReached`; `null` while not
  known yet (en route, or a destination that reports nothing).
  - `departedOnly`: TRUST reported only a departure at the destination;
    the delay is final, measured on that departure.
  - `notReached`: cancelled before the destination (TRUST `0002` at origin,
    on call, out of plan, or en route from an earlier call), a TRUST pass
    through it, or Darwin's cancelled call once TRUST has reported the
    train beyond it. `delayMinutes`, `delayBasis` and `estimate` are then
    `null` and `provisional` is `false`; `measuredAtCrs` still names the
    destination. Show no percentage: eligibility depends on the
    replacement journey.
- `measuredAtName`: `measuredAtCrs`'s station name, when known.
- `ownScheme`: `true` for an operator running its own compensation scheme
  (Elizabeth line, London Overground, Merseyrail, Grand Central). `estimate`
  is then `null`; show the claim link only.
- `schemeOperator`: the operator the scheme was matched to (`"LNER"`,
  `"Caledonian Sleeper"`, ...), or `null` for the DR15 default.
- `rulesCheckedOn`: `YYYY-MM-DD` the scheme table was last checked
  (`2026-10-07`).

### Changed and new fields on `estimate`

- **`percentage` is now nullable**: `null` exactly when `borderline`.
- `borderline` (bool) and `thresholdMinutes`: while provisional, a
  projection less than 3 minutes above a band threshold (15–17, 30–32,
  60–62, 120–122; Heathrow Express 31–33, 61–63) is borderline. Show
  "could go either way" with the threshold, not a percentage. Final
  estimates are never borderline.
- `scheme` may also be `HX` (Heathrow Express's own scheme: 25% of the
  ticket for more than 30 minutes, 50% for more than 60; `bandMinutes` 30
  or 60 meaning "more than").
- `ticketKind`: `single`, `return` or `unknown`, from the ticket's
  free-text type (whole words `return`/`rtn`, `single`/`sgl`/`sngl`).
- `fareBasis` may also be `ticket` (Heathrow Express). At 120+ minutes it
  is `single` for a single ticket (already refunded in full from 60
  minutes) and `return` otherwise.
- `roomSupplementPercentage`: Caledonian Sleeper only, 50 at 30–59
  minutes and 100 at 60+; else `null`.

### Changed behaviour

- Scheme table: DR30 now also covers Caledonian Sleeper, Hull Trains and
  Lumo (with LNER, CrossCountry, ScotRail); Heathrow Express has its own
  scheme; the four own-scheme operators above get no estimate. Each has its
  own `claimUrl`.
- When the ticket's `operator` text names no known operator, the train's
  CIF ATOC code decides the scheme (before falling back to DR15). A ticket
  without an operator now gets an estimate when the train's code is known.

## 2026-10-07: a line's full-day timetable; `running` follows each train's whole run

Design: `docs/superpowers/specs/2026-10-06-line-page-trains-design.md` §5.

### New: `GET /public/lines/{id}/timetable`

One page of the line's trains for a service date, cursor-paged. Same read
gate, date default (London today) and `404` (no population for the date)
as `/trains`. Parameters (all optional):

| parameter | meaning |
|-----------|---------|
| `date` | `YYYY-MM-DD` |
| `scope` | as `/trains`; **default `line,shared`** |
| `dir` | `up`, `down`, `loop` (comma list); `direction` is accepted too |
| `from` | a station CRS: only trains with a public call there, listed (and paged) by their departure from it |
| `to` | a station CRS: only trains with a public call there -- after `from` when both are given (`to` must differ from `from`) |
| `at` | `HH:MM` (hours `00`–`47`): trains from this time on |
| `after` | the previous page's `nextCursor`, unchanged |
| `limit` | 1–200, default 50 |

```json
{
  "lineId": "swr-south-west-main",
  "date": "2026-10-07",
  "scopeApplied": true,
  "scopes": ["line", "shared"],
  "directions": ["down"],
  "from": "WOK",
  "to": "WEY",
  "at": "09:00",
  "stations": [{"crs": "WAT", "name": "London Waterloo", "role": "terminus"}],
  "counts": {"line": {"down": 41, "up": 40}, "shared": {"down": 12}},
  "trains": [{
    "uid": "L80147", "operator": "SW", "serviceMode": "train", "liveTracking": true,
    "scope": "line", "direction": "down",
    "lineDue": {"time": "08:35", "dayOffset": 0},
    "origin": {"crs": "WAT", "name": "London Waterloo"},
    "destination": {"crs": "WEY", "name": "Weymouth"},
    "onLineStops": [{"crs": "WAT", "time": "08:35", "dayOffset": 0}],
    "live": null,
    "time": {"time": "09:02", "dayOffset": 0},
    "arrival": {"time": "11:10", "dayOffset": 0}
  }],
  "nextCursor": "542.L80151"
}
```

- Each train is a `view=summary` train plus `time` (its departure from
  `from`, else its `lineDue`) and `arrival` (its arrival at `to`, `null`
  without `to`). Trains are ordered by `time`, then `uid`; a train with no
  public call on the line is not listed.
- `nextCursor`: pass it back as `after` with the same other parameters
  for the next page; `null` on the last page. Opaque -- don't build one.
- `counts`: the whole day's trains under `scope`, `from` and `to`, per
  scope and direction (`none` without one) -- before `dir`, `at` and the
  cursor, for direction tabs.
- `live` is looked up for the page's trains only.
- A malformed parameter is a `400`.

### Changed: `view=summary`'s `running`

`running` used to consider only trains that reached the line in the six
hours before `at`, so a long run (Edinburgh to Plymouth) dropped out of
it. It now uses each train's whole on-line run: due on the line at or
before `at`, last on-line call no more than three hours (the late-running
grace, as before) before it. Without a window (`from`/`to`), every listed
train now carries its `onLineStops`, `origin` and `destination` (they
were only filled for trains near `at`).

Both views now read a table derived from the population at publish time
when it has the line and date (`line_train_summaries`), and the
population itself otherwise; the responses are the same either way.

## 2026-10-07: named bus stops and timing points; bus stops in the planner

Design: `docs/superpowers/specs/2026-10-06-tiploc-locations-design.md`.

### New fields on `JourneyStop`

`locationType` (`station`, `bus_stop`, `ferry_terminal`, `junction`,
`siding`, `passing_point`, `other`, or `null` when nothing is known) and
`parentCrs` (a bus stop's or ferry terminal's station, else `null`).

### Changed behaviour

- `JourneyStop.name` is now set for stops with no station name: bus stops
  (`"Heathrow Terminal 3 (bus stop)"`), ferry terminals (`"Brodick (ferry
  terminal)"`), timing points (`"Marylebone 10 Signal"`). `null` only for a
  TIPLOC no source knows.
- A destination with no CRS (station boards, train search `destinationCrs`)
  is now keyed `tiploc:HTRBUS3` instead of `~HTRBUS3`, and its
  `destinationName` is set. Rows published before the change keep the `~`
  key, also named, until the next daily publish.
- `GET /Trips/plan`: `origin`, `destination`, `waypoints`, `via` and the
  avoid lists also take a bus stop's or ferry terminal's `tiploc:` code (any
  case). A `tiploc:` via is satisfied only by a call there (or a walk into
  it): nothing runs through a bus stop.
  `originCrs`/`destinationCrs` on segments and legs carry that code for such
  an end. A change to or from a bus or ferry costs 5 extra minutes per bus or
  ferry side (configurable), never at the start or end.

### New parameter

`GET /public/stations?q=...&stops=true` appends up to 10 matching bus stops
and ferry terminals: `{"code": "tiploc:KESWICK", "name": "Keswick (bus)",
"kind": "bus"}` (`parentCrs` when the stop has a station). Without
`stops=true` the response is
unchanged.

## 2026-10-06: `view=summary` on a line's trains

Design: `docs/superpowers/specs/2026-10-06-line-page-trains-design.md`.

`GET /public/lines/{id}/trains` without `view` (or with `view=full`) is
**unchanged**: a bare array of every train of the day with its calling
points (about 20 MB for a main line). The summary parameters below are
ignored there.

New, opt-in: `GET /public/lines/{id}/trains?view=summary`, the slim
windowed view the line page uses. Parameters (all optional):

| parameter | meaning |
|-----------|---------|
| `date` | `YYYY-MM-DD`, default London today (as before) |
| `scope` | `line`, `shared`, `touch` (comma list) or `all`; **default `line,shared`** in this view |
| `from`, `to` | `HH:MM`, hours `00`–`47` (`24:00`+ is the next morning of the service date). Keeps trains whose `lineDue` (first public call on the line) is in `[from, to)`. `to` at or before `from` (both before `24:00`) wraps past midnight: `23:00`–`01:00` is two hours. A missing bound is open. |
| `direction` | `up`, `down`, `loop` (comma list) |
| `at` | `HH:MM` (as `from`): also return `running`, the trains between their first and last on-line public call at that moment (last call pushed back by a known delay; not cancelled or completed). Looks back at most 6 hours. |
| `limit` | 1–2000, default 500: the most `trains` returned (`truncated` says when more matched) |

An unknown `view` and a malformed summary parameter are a `400`; no
population for the date is the usual `404`.

```json
{
  "lineId": "swr-south-west-main",
  "date": "2026-10-06",
  "scopeApplied": true,
  "scopes": ["line", "shared"],
  "window": {"from": "13:30", "to": "15:30"},
  "at": "14:00",
  "directions": null,
  "stations": [{"crs": "WAT", "name": "London Waterloo", "role": "terminus"}],
  "counts": {"line": {"down": 9, "up": 8}, "shared": {"down": 31, "up": 29}},
  "truncated": false,
  "trains": [{
    "uid": "L80147",
    "operator": "SW",
    "serviceMode": "train",
    "liveTracking": true,
    "scope": "line",
    "direction": "down",
    "lineDue": {"time": "13:35", "dayOffset": 0},
    "origin": {"crs": "WAT", "name": "London Waterloo"},
    "destination": {"crs": "WEY", "name": "Weymouth"},
    "onLineStops": [{"crs": "WAT", "time": "13:35", "dayOffset": 0}],
    "live": {"status": "en_route", "delayMinutes": 3, "delayProvisional": false,
             "cancelled": false, "lastReportedLocation": "WOK"}
  }],
  "running": []
}
```

- Trains are sorted by `lineDue`, then `uid`. Times are public (GBTT),
  `HH:MM`, with the day offset after the service date.
- `onLineStops`: the train's public calls at the line's catalogue
  stations (a line's `crs_aliases` count as their station).
- `origin`/`destination`: the first/last calling point whose TIPLOC
  resolves to a real station (depots, junctions and `X..` pseudo-CRS are
  walked past); when none does, the first/last on-line call. Never null
  for a train that calls on the line.
- `serviceMode` and `liveTracking`: as on the default view's entries (see
  "buses and ferries" below) -- the published `schedule_services` mode,
  else the population's CIF Train Status (`5` `replacementBus`, `B` `bus`,
  `S`/`4` `ferry`, otherwise `train`).
- `live` is looked up only for the trains in `trains` and `running`;
  `null` when TRUST has no record.
- `counts`: trains in the window per scope and direction (`none` without
  one), before the `direction` filter, for tab counts.
- `stations`: the line's catalogue stations, with `role`.
- `running` is present only with `at`.
- `scopeApplied: false` (and header `x-scope-applied: false`) for a
  population published before train membership: nothing is filtered,
  `scope`/`direction` are null, and `lineDue` is worked out from the
  calling points.

Also fixed: `GET /public/lines/{id}/trains` (default view, with or
without `scope`) could time out on a large population (a per-entry
re-read of the whole population in SQL).

## 2026-10-06: buses and ferries (`serviceMode`, `liveTracking`); no writes on train-page reads

The CIF timetable carries buses and ferries alongside trains (about 9% of
weekday schedules, 19% on Saturday, 25% on Sunday), and Network Rail's
live feed never reports any of them. Every surface used to present them as
trains forever waiting for a movement report.

### New fields: `serviceMode`, `liveTracking`

Added (always present from this release) wherever a scheduled service is
described:

- `GET /Train/by-uid/{uid}/{date}`;
- `GET /Train/{trackingId}`, `GET /Train/mine` items and journey legs'
  tracked-train states;
- `GET /public/trains/search` rows and
  `GET /Journeys/{journeyId}/legs/{legId}/candidates` rows;
- `GET /public/stations/{crs}/schedule-departures` rows;
- `GET /public/lines/{id}/trains` entries (top level, beside `liveStatus`
  and the membership fields; a bus or ferry is never a line's own train,
  so it comes back `scope: "shared"` or `"touch"`, never `"line"`);
- `GET /Trips/plan` train legs;
- `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`.

```json
"serviceMode": "replacementBus",
"liveTracking": false
```

- `serviceMode` is `train`, `replacementBus` (CIF Train Status `5` or
  Train Category `BR`), `bus` (any other bus: status `B` or category
  `BS`) or `ferry` (status `S`/`4`). It is per `(uid, service date)`: an
  STP overlay can make a train a replacement bus on some dates only.
- `liveTracking` is `false` exactly when `serviceMode` is not `train`: no
  live position, delay, platform or arrival will ever arrive for it.
- **`/Trips/plan` legs keep `kind: "train"`** for a bus or ferry leg, so
  a client branching on `kind` keeps working; read `serviceMode` to label
  it.
- A schedule the backend has no mode for (not yet published for that
  date) reads as `train`, which is the old behaviour.

### Delay Repay

`GET /Train/{trackingId}/tickets/{ticketId}/delay-repay` for a bus or
ferry returns `delayMinutes: null`, `estimate: null` and a new
`unmeasurableReason` string saying the delay cannot be measured;
`claimUrl` and `disclaimer` are still populated. `unmeasurableReason` is
`null` for a train.

### Changed behaviour

- **Tracking a bus or ferry.** Allowed, timetable-only: the subscription
  is schedule-matched from CIF at once and stays `schedule_matched` (it
  can never become `resolved`). It gets no live alerts and no station-skip
  alerts.
- **Recurring-journey auto-commit** picks a train whenever one fits the
  leg, and a bus or ferry only when none does.
- **`GET /Train/by-uid/{uid}/{date}` no longer creates a `trains` row.**
  For a published schedule nobody has tracked and TRUST has not reported,
  it returns a read-only schedule view with **`trainsId: 0`** (no shared
  row exists; still a number, as DS-MCP's schema requires) and every live
  field `null`. Once the train is tracked or reported, `trainsId` is the
  real row's id. Clients must not treat `trainsId` as stable across that
  transition (none did: it was never a tracking id).

## 2026-10-06: train membership (`scope`) on a line's trains

Design: `docs/superpowers/specs/2026-10-06-line-membership-design.md`.

A line's population (`GET /public/lines/{id}/trains` and `/schedule`) is
every train touching one of its stations: at a hub, mostly other routes'
trains. Each entry now says how it belongs to the line.

### New optional query parameter: `scope`

`GET /public/lines/{id}/trains?scope=` and `GET /public/lines/{id}/schedule?scope=`
filter the entries (in SQL) by membership:

- `line`: the line's own trains;
- `shared`: runs a stretch of the line but is another route's or another
  operator's train (CrossCountry along the South West Main Line);
- `touch`: only touches the line (a hub call, a crossing);
- comma-separated combinations (`line,shared`), or `all`.

**Without `scope` the response is unchanged**: every entry, no filter. The
line page passes `scope=line,shared`; touch-only trains are reached from
station pages. An unknown value is a `400`.

When `scope` is given, the response carries the header
`x-scope-applied: true`, or `false` when the line's population was
published before membership existed: then it was NOT filtered and every
entry is returned. (A header, because both bodies are bare arrays.)

### New fields on `/trains` entries

Each omitted when absent (on an older population, all of them):

```json
{
  "scope": "line",
  "direction": "down",
  "runFirstCrs": "WAT",
  "runLastCrs": "WEY",
  "lineDue": { "time": "23:35:00", "dayOffset": 0 }
}
```

- `direction`: `down`/`up` by the line's catalogue station order (the
  order of `GET /public/lines/{id}/definition`'s `stations`), `loop` when
  the run starts and ends at the same station.
- `runFirstCrs`/`runLastCrs`: the first and last line station of the
  train's run along the line (absent for `touch`).
- `lineDue`: the train's first public call at one of the line's stations,
  Europe/London local time, `dayOffset` days after the service date.

`/schedule` entries carry the same data under the population's own
snake-case names: `scope`, `direction`, `run_first_crs`, `run_last_crs`,
`line_due` (`{"time", "day_offset"}`).

## 2026-10-06: 503 + `Retry-After` when DS is temporarily unavailable

Every route, public and `/private`, now answers **503 Service Unavailable**
instead of 500 when it could not reach its database (or Redis, or an
upstream HTTP dependency it calls): the pool timed out or is closed, the
connection was refused, reset or cut, or Postgres said it is shutting down,
starting up or out of connections (SQLSTATE class 08, 57P01, 57P02, 57P03,
53300). Every other failure is still a 500. On 2026-10-01 a six-hour
Postgres outage made every route answer 500.

```http
HTTP/1.1 503 Service Unavailable
Content-Type: application/json
Retry-After: 30

{"error":"service_unavailable","retryable":true,"message":"Distant Signal is temporarily unavailable. Please retry shortly."}
```

- Match on `error == "service_unavailable"` (or `retryable == true`), never
  on `message`.
- `Retry-After` is in seconds (30 by default; the operator's
  `api.timeouts.unavailableRetryAfterSecs`, 1-3600). Wait at least that
  long, with backoff and jitter, before retrying.
- Every other 503 now also carries `Retry-After`, with its own plain-text
  body as before: `/Trips/plan` shedding load ("too many trip plans are
  being computed right now"), a ticket parse with every parse slot busy,
  sign-in while the identity provider cannot be reached, and a schedule
  publish rolled back by its statement timeout. All 503s are retryable.
- A 500 still means a bug or a bad request the server could not classify;
  retrying it is unlikely to help.
- For DS-MCP: a 503 from `/Trips/plan` means "DS is temporarily
  unavailable", not "no route found" (still a 200 with no itineraries, or a
  404 when no schedule is published for the date).

## 2026-10-06: `/Trips/plan` pass-through `via`; `maxChanges` up to 6

Design:
`docs/superpowers/specs/2026-10-06-trips-plan-via-and-max-changes-design.md`.
This closes the two gaps for which DS-MCP's `plan_journey` still used its
local engine (`dsTripPlanEligible`).

### New parameter: `via`

`GET /Trips/plan?...&via=CRS[,CRS]`: at most 3 stations every journey
must pass through, in this order.

- **What counts.** Staying aboard a train that runs through without
  stopping or calls there, or changing or walking there. This is
  `plan_journey`'s `via` (pass through, stopping or not).
- **Vias and waypoints.** `waypoints` (must call, in order) and `via` are
  separate ordered lists that may interleave.
- **Bus stops.** A via may be a bus stop's or ferry terminal's `tiploc:`
  code (see the 2026-10-07 entry above); only a call there satisfies it.
- **With the bus and ferry buffer.** A change on a bus or ferry side still
  costs the extra minutes, whether or not it happens at a via.
- **With the avoid lists.** `avoidStop=X` with `via=X` means "pass X
  without stopping". `avoid=X` with `via=X` is a 400.
- **Timing points only.** Running through is seen only where CIF records a
  passing point, at timing points. Elsewhere only a call there counts.
- **400s:**
  - an unknown or group code (`via: 'ZZZ' is not a recognised station CRS
    code`);
  - more than 3 vias;
  - the same via twice in a row;
  - a via equal to the origin or the destination.

### New fields

- Top level: `via`. The vias as applied, uppercased and in order. Always
  present (`[]`).
- `journeys[j].viaSatisfiedBy`: present only when `via` is given.
  `[{crs, segment, leg, how}]`, one entry per via in order.
  `segments[segment].itineraries[j].legs[leg]` is the leg that first
  passed it. `how` is one of:
  - `call`: the train called there;
  - `pass`: it ran through without calling, or past a cancelled call;
  - `walk`: a transfer leg into it.

```json
"via": ["ZVP"],
"journeys": [{"changeCount": 0, "departure": {...}, "arrival": {...},
  "totalDurationMinutes": 60,
  "viaSatisfiedBy": [{"crs": "ZVP", "segment": 0, "leg": 0, "how": "pass"}]}]
```

### New `noResultReason.constraint`: `via`

An itinerary exists without the vias, but none with them. `values` is the
single via whose removal alone is enough, or every via. Every segment
carries it. A `maxChanges` cap, or a time or route that fails even without
the vias, is reported as before instead.

### Changed: `maxChanges` accepts 0 to 6 (was 0 to 4)

Out of range is a 400: `maxChanges must be a whole number from 0 to 6
(default 2), not '7'`.

With `results=options`, a new 400 applies when
`(waypoints + 1) * (2 * vias + 1) * (maxChanges + 2)` is over the
deployment's bound: 252 by default, configurable in the chart as
`api.tripPlanMaxOptionsSearchSize` (8-504). The message is `... is too
large a search (... = N, at most 252); use fewer waypoints or vias, a
lower maxChanges, or results=fastest`. The limits at the default:

- without vias: all 20 waypoints, at any `maxChanges`;
- with 1 via: 20, 13, 11 and 9 waypoints at `maxChanges` 2, 4, 5 and 6;
- with 3 vias: 8, 5, 4 and 3 waypoints at `maxChanges` 2, 4, 5 and 6.

`fastest` is not affected. DS-MCP: on this 400, fall back to the local
engine.

## 2026-10-06: incidents on the lines they name; `upcoming` on line status

Design:
`docs/superpowers/specs/2026-10-06-incident-line-evidence-design.md`.

### New field: `upcoming`

`GET /Line/Mode/{mode}/Status` and `GET /Line/{ids}/Status` reports gain
`upcoming`: disruptions announced for the line that have not started yet,
soonest first, at most 5. Always present; `[]` when there are none (and
always `[]` for a `TfL` line).

```json
"upcoming": [
  {
    "from": "2026-10-10T23:00:00+00:00",
    "to": "2026-10-11T23:00:00+00:00",
    "summary": "Industrial action to affect TransPennine Express services on Sunday 11, 18 and 25 October",
    "incidentId": "1D3D4694..."
  }
]
```

- `from`/`to` are RFC3339; `to` is exclusive and may be `null`. A whole-day
  event runs from one Europe/London midnight to the next.
- An entry is a note, not a status: it never changes `lineStatuses` or a
  severity. When the day comes, the incident appears in `lineStatuses` as
  usual (and drops out of `upcoming`).
- Listed: unplanned incidents with a high-confidence period that has not
  started yet, for industrial action at any distance, otherwise starting
  within 14 days. `incidentId` is the `GET /public/incidents/{incidentId}`
  id.

### Changed behaviour (no shape change)

- **Which lines an incident is on.** An unplanned incident is on the lines
  its text gives evidence for: the stations it names (resolved against the
  station reference data), a line keyword or brand, or a closed section.
  It is on every line of its operator only for a network-wide notice
  (industrial action, a reduced timetable, "across the ... network", ...)
  or when it names no place at all. Before, most incidents were on every
  line of their operator ("(operator-wide report)" in `reason`). This
  changes `lineStatuses`, `affectedLines` on `GET /public/incidents` rows
  (and its `line` filter), and the detail's `currentlyAffectsLines` alike.
  Archived rows keep their stored `affectedLines` until
  `backfill_incident_lines` is re-run. `LN`/`WM` incidents
  now reach the `LM` lines, and `ZN` ("National Rail") incidents reach the
  lines whose stations they name.
- **How long an unplanned incident shows.** Until the 02:00 rail-day
  boundary after it was listed, reopened or last re-worded (it used to be
  after it was first seen, ever). After that, only while a dated period of
  it is in progress (a stated end date or a weekly schedule window), or
  while it says it lasts "until further notice" (then at most Reduced
  Service, with "(long-running notice)" in `reason`).
- **Not in effect today.** A long-running notice whose schedule window
  excludes now (a Sunday under a Monday-Saturday window) still shows, with
  a `validityPeriods[0].fromDate` at its next window start and
  `isNow: false`; live data does not escalate it, and the line can also
  show its own `ldbws-inferred` status.
- **A notice about the future** (every period still to come, e.g. a strike
  next week) is no longer a status at all; see `upcoming`.

## 2026-10-06: incidents can be "Ended (no longer listed)"

Design:
`docs/superpowers/specs/2026-10-06-incident-source-removal-design.md`.

RDM's Knowledgebase feed drops incidents (nightly, and planned work is never
cleared) without ever setting `ClearedIncident`. DS used to show such an
incident as active forever. An incident is now in one of three states:

| State | `isCleared` | `sourceRemovedAt` |
| --- | --- | --- |
| Active | `false` | `null` |
| Cleared (RDM cleared it) | `true` | `null` |
| Ended (the feed stopped listing it without clearing it) | `false` | when the feed last listed it |

### New field

- `sourceRemovedAt` (RFC3339 or `null`) on `GET /public/incidents` rows and
  on `GET /public/incidents/{incidentId}`.

A client that treats `isCleared: false` as "live" should also check that
`sourceRemovedAt` is `null`.

### New filter

- `GET /public/incidents?state=active|cleared|ended`. Any other value is a
  `400`.

### Changed meaning

- `cleared=false` now returns Active incidents only, not Ended ones.
  `cleared=true` is unchanged. `cleared` is kept as the legacy spelling of
  `state=active`/`state=cleared`.
- Passing both `state` and `cleared` is a `400`.

### History

A change of `isCleared` alone now adds an entry to the detail's `history`,
so the history shows when RDM cleared an incident. Becoming Ended adds no
entry.

## 2026-10-01: public times, phase 2

Design: `docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md`
(§9 decisions, §11 what phase 2 shipped).

Every delay DS serves is now measured against the **public** timetable
(the times a passenger is sold, GBTT), at the passenger's own stop. The
trip planner plans on public times. Working-timetable (WTT) times stay
internal, for matching, except where noted.

### Deprecation: `scheduled*` (working timetable)

These fields still carry the **working** (WTT) time, truncated to the
minute, for one more release. After that they are switched to the public
time or removed. Read `public*` instead now, and `working*` where a WTT time
is wanted:

| Where | Deprecated field | Use instead |
| --- | --- | --- |
| `JourneyStop` (train page `journeyStops[]`, journey legs) | `scheduledArrival`, `scheduledDeparture` | `publicArrival`, `publicDeparture`; `workingArrival`, `workingDeparture` |
| Trip-plan train legs (`GET /Trips/plan`) | `scheduledDeparture`, `scheduledArrival` (and their `departureDayOffset`/`arrivalDayOffset`) | `publicDeparture`, `publicArrival` (and `publicDepartureDayOffset`/`publicArrivalDayOffset`) |
| Timetable board rows, `/public/trains/search` rows, leg candidates | `scheduled`, `destinationArrival` | `publicDeparture`, `publicDestinationArrival`; leg candidates also `legPublicDestinationArrival` (below) |

A `public*` field is `null` where the call has no public time in that
direction (the departure of a set-down-only stop, the arrival of a
pick-up-only one), and until the next schedule publish for a schedule
stored before public times were.

### Changed meaning

**Per-stop delay, `JourneyStop.delayMinutes`** (train page, journey detail):
- Now measured against the public time, on the side of the stop's latest
  TRUST report: an arrival against the public arrival, a departure against
  the public departure.
- New `delayBasis` names the baseline (see the table below).
- A train arriving at Euston at 06:21, booked 06:19 working and 06:22
  public, now shows `-1`, where it used to show `2`.

**Per-stop status, `JourneyStop.status` (`Late`/`OnTime`) and `lateMinutes`:**
- The estimate is now compared with the public time (else the working
  time on a side with no public time).
- `estimatedArrival`/`estimatedDeparture` are unchanged: the working time
  plus TRUST's running delay.

**Train-level `delayMinutes`** is the delay at the passenger's own stop,
against the public timetable. It is measured once the train has reported
at that stop, and forecast before then. The forecast is the stop's working
time plus TRUST's running delay, compared with its public time, so a
terminus's recovery margin counts. Where it is served, and what "own stop"
means there:

| Endpoint | Field | Stop |
| --- | --- | --- |
| `GET /Train/{trackingId}` | `delayMinutes` | the pin destination |
| `GET /Train/mine` | `[].delayMinutes` | the pin destination |
| `GET /Journeys/mine` | `[].delayMinutes` | the current leg's destination |
| `GET /Journeys/{id}` (and its share-link twin) | each leg's tracked-train `delayMinutes` | that subscription's pin destination |
| `GET /groups/{id}/trains`, `GET /groups/shared-trains` | `delayMinutes` | the sharer's pin destination |
| `GET /groups/{id}/journeys`, `GET /groups/shared-journeys` | `delayMinutes` | the first leg's pin destination |
| `GET /Train/by-uid/{uid}/{date}` | `delayMinutes` | no stop of the user's own: the latest call the train reported at (an arrival or departure, never a pass); before any, TRUST's running delay with `delayBasis: "working"` |
| `GET /public/lines/{id}/trains` | `liveStatus.delayMinutes` | as `by-uid` |

Each of these rows gains:
- `delayBasis`: see the table below.
- `delayProvisional` (bool): `true` while `delayMinutes` is a forecast.

**Push notifications.** The notifier judges the 15-minute threshold on
the delay at each subscriber's own stop (their journey leg's destination,
else the pin destination), against the public timetable. The push copy
("now running about N minutes late") uses the same figure. A train 16
minutes late on the working timetable into a terminus with 2 minutes of
recovery margin no longer notifies; one late at a junction but recovering
before the subscriber's stop is judged at that stop.

**Delay Repay: `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`
and `GET /Train/tickets/mine`:**
- `delayMinutes` is the delay against the public arrival at the ticket's
  destination. It was the train's latest working-timetable delay anywhere,
  passes included.
- The destination is the ticket's `destinationCrs`, else the tracked
  train's pin destination, else the train's terminus.
- Final once the train has arrived there. Before that it is a projection,
  `provisional: true`: the current delay carried to the destination's
  working arrival, compared with its public arrival.
- New on the response and on each ticket-list item:
  - `provisional` (bool);
  - `delayBasis`;
  - `measuredAtCrs`: where the delay was measured, `null` when no delay is
    known.
- `estimate` gains:
  - `provisional` (bool), the same as the response's.
  - `fareBasis`: `"single"`, or `"return"` in the 120-minute band.
- `estimate.disclaimer` for a provisional estimate starts with
  "Provisional: the train has not reached your destination yet…", and is
  otherwise the same caveat as before.
- New 120-minute band: `bandMinutes: 120`, `percentage: 100`,
  `fareBasis: "return"`, for both DR15 and DR30.
- The bands are DR15 15/30/60/120, and DR30 30/60/120.

**Trip planning, `GET /Trips/plan`:**
- The search, arrive-by deadlines, waypoint chaining and minimum change
  times now run on public times. A call with no public time in that
  direction falls back to its working time.
- Itineraries can shift by ±1–3 minutes, and a change that only worked
  on WTT can disappear (or the reverse).
- `totalDurationMinutes` is public-to-public.
- Leg `scheduledDeparture`/`scheduledArrival` are still WTT (deprecated,
  above); `publicDeparture`/`publicArrival` are what the search used.
- Live overlay, `legs[].live.delayMinutes`/`arrivalDelayMinutes`: against
  the public time. A TRUST-reported call uses the delay measured from that
  report's own fields; the CIF time is never subtracted from a TRUST
  actual.

**Journey leg candidates, `GET /Journeys/{journeyId}/legs/{legId}/candidates`:**
- A leg can now end at a set-down-only stop. Before, it could not be
  found.
- A leg never ends at a pick-up-only stop, and never boards at a
  set-down-only one.
- New per row: `publicDeparture`, `publicDestinationArrival`, and
  `legPublicDestinationArrival` (the public arrival at the leg's own
  destination, `"HH:MM"`, `null` when unknown).

**`/public/trains/search` with `stops_at`:** a pick-up-only call at
`stops_at` no longer matches (you cannot get off there); a set-down-only
one now does. Boarding rows are unchanged.

### `delayBasis` values

| Value | Baseline |
| --- | --- |
| `public` | TRUST's own public time for the movement (`gbtt_timestamp`) |
| `publicSchedule` | The public time from the CIF schedule at that call. Used when TRUST sent none: movements matched from the TRUST backlog, movements stored before 2026-10-01, and every forecast. Applied as TRUST's working time plus the schedule's `public - working` gap, never as a TRUST instant minus a CIF one. |
| `working` | The working timetable: the call has no public time in that direction (a pass, the departure of a set-down-only stop), or no schedule row is available |

### Unchanged

- Station boards' `delayMinutes` (LDBWS `etd - std`, already public).
- Line status, operator and network statistics (LDBWS departures).
- Full-coverage statistics: TRUST's `timetable_variation`, an operational,
  working-timetable measure.
- TRUST-to-schedule matching and `GET /public/trains/resolve`, which stay on
  WTT.

## 2026-10-01: public times, phase 1

Design: `docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md`
(§10 what phase 1 shipped).

All additive: no existing field changed meaning in phase 1. DS now stores
the public (GBTT) times and the passenger direction of every call from the
CIF schedule, and serves them next to the working-timetable (WTT) times.

**Null until the next publish.** The new schedule columns refill on the
next schedule-reference publish (and `trains.calling_points` for a train
matched after it). Until then a `public*` field is `null` even where the
call has a public time. A client must treat `null` as "not known", not as
"no public call".

### `scheduled*` stays WTT

`scheduledArrival`/`scheduledDeparture` (`JourneyStop`, trip-plan legs) and
`scheduled`/`destinationArrival` (board and search rows) keep their
meaning: the **working** time, truncated to the minute. They are kept for
one release and then switched to the public time or removed; phase 2
(above) deprecates them.

### `JourneyStop`: public times, working times, direction

On every `JourneyStop` (train page `journeyStops[]`, journey legs),
flattened onto the stop, not nested:

| Field | Type | Meaning | `null` when |
| --- | --- | --- | --- |
| `publicArrival` | RFC 3339 UTC instant | The public (GBTT) arrival: what a passenger timetable and the station screens show. | No public arrival here (the origin, a pick-up-only stop, a passing point), or the schedule predates the next publish. |
| `publicDeparture` | RFC 3339 UTC instant | The public departure. | No public departure here (the terminus, a set-down-only stop, a passing point), or the schedule predates the next publish. |
| `workingArrival` | RFC 3339 UTC instant | The exact WTT arrival, with `:30` seconds for a half-minute (`H`). | No booked arrival (the origin, a passing point). |
| `workingDeparture` | RFC 3339 UTC instant | The exact WTT departure, `:30` for a half-minute. | No booked departure (the terminus, a passing point). |
| `workingPass` | RFC 3339 UTC instant | The WTT pass time. Set only on a passing point (the train runs through without stopping), which has no other time. | Any call that is not a pass, and a pass in a schedule stored before the next publish. |
| `canBoard` | bool | A passenger can board here. `false` at a set-down-only (`D`) stop. | Never. |
| `canAlight` | bool | A passenger can alight here. `false` at a pick-up-only (`U`) stop. | Never. |
| `requestStop` | bool | A request stop (`R`): the train calls only if asked. | Never. |

- Every instant is dated on its own day. A stop that dwells across
  midnight has its departure on the next day. A public time that rounds
  across midnight (a 23:59H WTT arrival is a 00:00 public one) is on the
  next day too.
- For a schedule stored before the next publish, `workingArrival`/
  `workingDeparture` are rebuilt from the minute WTT time plus the stored
  half-minute flag. Where neither is stored (the
  `schedule_calling_points_full` fallback), they are the minute WTT time.
- `canBoard`/`canAlight`/`requestStop` are always booleans. Where the
  schedule predates the direction columns, DS fills them in as before
  phase 1: the origin is boardable only, the terminus alightable only, an
  untimed passing point neither, every other call both; `requestStop` is
  `false`.
- A client that may also talk to a server older than phase 1 sees the
  fields absent: read a missing `canBoard`/`canAlight` as `true`, a missing
  `requestStop` as `false`, and a missing `public*`/`working*` as `null`.
- Set-down-only (`D`) stops are now in the stored schedule, with
  `canBoard: false`; before phase 1 they were left out of it. `OP`
  (operational) and `N` (not advertised) stops are still left out.

### Board, search and trip-plan rows

Times here are local (Europe/London) clock times, like the `scheduled*`
fields beside them.

| Where | Field | Format | Meaning |
| --- | --- | --- | --- |
| Schedule-departure board rows, `GET /public/stations/{crs}/schedule-departures` | `publicDeparture` | `"HH:MM"` | Public departure at the board's station. `dayOffset` is the WTT departure's; the public time is minutes from it. |
| `GET /public/trains/search` rows | `publicDeparture` | `"HH:MM"` | Public departure at the searched station (`stationCrs`). |
| `GET /public/trains/search` rows | `publicDestinationArrival` | `"HH:MM"` | Public arrival at the train's destination (`destinationCrs`). |
| Trip-plan train legs, `GET /Trips/plan` (`kind: "train"`) | `publicDeparture` | `"HH:MM:SS"` | Public departure at the leg's boarding call. |
| Trip-plan train legs | `publicArrival` | `"HH:MM:SS"` | Public arrival at the leg's alighting call. |
| Trip-plan train legs | `publicDepartureDayOffset` | integer | Days past `serviceDate` of `publicDeparture`. |
| Trip-plan train legs | `publicArrivalDayOffset` | integer | Days past `serviceDate` of `publicArrival`. |

- Each `public*` time is `null` where the CIF has no public time for that
  call in that direction, and until the next publish for a schedule stored
  before public times were.
- A day offset is `null` exactly when its time is. It is usually the WTT
  time's own offset (`departureDayOffset`/`arrivalDayOffset`), one more
  where rounding crosses midnight.
- In phase 1 the trip planner still searched on WTT and attached the public
  times afterwards; phase 2 (above) moved the search onto them.

### Trip planning and direction

- The planner (`GET /Trips/plan`, including waypoints, arrive-by and the
  live overlay) never boards a train where `canBoard` is false and never
  alights where `canAlight` is false. It still rides through those stops.
- So an itinerary can now end at a set-down-only stop (before, the stop
  was not in the schedule at all), and one that alighted at a pick-up-only
  stop is gone.
