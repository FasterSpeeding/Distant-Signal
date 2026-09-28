# TRUST reason codes, cancellation and stop status

This page covers what the train-detail responses serve about why a train is
not running as booked, and how that compares with Darwin/LDBWS
(`GetServiceDetails`).

## Where the fields appear

The fields are served by:

- `GET /Train/by-uid/{uid}/{date}` (`PublicTrainState`);
- `GET /Train/{trackingId}` and each journey leg's `trackedTrainState`
  (`TrackedTrainState`);
- each `liveStatus` of `GET /public/lines/{id}/trains`, apart from the
  per-stop fields.

All of them are additive and nullable.

| Field | Meaning |
|---|---|
| `operatorCode`, `operatorName` | ATOC code from the CIF schedule (`BX`), and its name from `tocs` (`data::train_operator`) |
| `cancelled` | `status == "cancelled"`, the TRUST-derived status. A reinstated train is not cancelled. |
| `cancelReasonCode`, `cancelReason` | The latest `0002` `canx_reason_code`, and its glossary text. Served only while `cancelled` is true. |
| `changeOfOriginReasonCode`, `changeOfOriginReason` | The latest `0006` `reason_code`, and its text |
| `journeyStops[].status`, `journeyStops[].lateMinutes` | Per-stop LDBWS-style status (below) |
| `journeyStops[].board` | This train's row on the stop's live LDBWS departure board, including Darwin's `delayReason` (below). Not on the line trains list. |

Example of a cancelled train:

```json
{
  "trainUid": "C11052",
  "status": "cancelled",
  "operatorCode": "SW",
  "operatorName": "South Western Railway",
  "cancelled": true,
  "cancelReasonCode": "TG",
  "cancelReason": "Driver",
  "changeOfOriginReasonCode": null,
  "changeOfOriginReason": null,
  "journeyStops": [
    { "crs": "WAT", "status": "Departed", "lateMinutes": null },
    { "crs": "CLJ", "status": "Cancelled", "lateMinutes": null }
  ]
}
```

## Delay reasons come from the live board, not TRUST

TRUST's open movement feed carries a reason code only on a `0002`
Cancellation and a `0006` Change of Origin. A `0003` Movement has none,
because delay attribution happens later in TRUST DA and is not in the feed.
So there is **no TRUST delay reason**.

Darwin's own passenger text is served instead, per stop, from the LDBWS
departure boards `poller-ldbws` samples (about 560 stations):
`journeyStops[].board` (`crates/api/src/data/stop_board.rs`).

```json
"board": {
  "delayReason": "This train has been delayed by a points failure",
  "cancelReason": null,
  "isCancelled": false,
  "delayMinutes": 4,
  "estimated": "17:04",
  "observedAt": "2026-09-28T16:01:12Z"
}
```

- `board` is `null` unless the stop's station board, polled in the last 10
  minutes, lists exactly one row that is this train. Matching uses the
  stop's TIPLOC (embedded in the LDBWS serviceID), the booked departure
  (within 5 minutes) and the Retail Service ID (RSID), which both the board
  and the CIF schedule carry. Without an RSID it falls back to destination,
  operator and time within 2 minutes. Any tie is `null`, never a guess.
- Departures only. A terminating stop, an unsampled station, a stop the
  train has already left, and a train not yet on the board (a busy
  station's 10-row board can reach as little as 20 minutes ahead) are all
  `null`. **`null` means not known, never on time.**
- `delayMinutes` is Darwin's `etd - std`: `0` for "On time" or early,
  `null` for "Delayed" and "Cancelled". It is separate from the stop's own
  `delayMinutes`, which is TRUST's.
- `cancelReason` and `isCancelled` here are Darwin's, for this service at
  this station. The train-level `cancelReason` is TRUST's glossary text.
- Relaying the text needs the National Rail Enquiries attribution, as for
  any LDBWS data.

## Reason text

Codes map to text through `reference-data/delay-attribution-reasons.tsv`,
converted from Network Rail's "Historic Delay Attribution Glossary"
(https://www.networkrail.co.uk/wp-content/uploads/2021/08/Historic-Delay-Attribution-Glossary.xlsx,
August 2021, 365 codes). The attribution is on `/attribution`. The file
carries no licence text of its own. On 2026-09-27 the operator decided to
use it on the basis that Network Rail's transparency data is published under
OGL v3.0.

Rules (`data::train_reasons`):

- The text is the glossary's description, verbatim.
- The code is always served. The text is `null` when:
  - the glossary lacks the code. It is from 2021, so later codes are
    missing, and a code redefined since may carry its 2021 meaning.
  - the code is `PD` ("System generated cancellation"). This is about 70% of
    all cancellations, nearly all `ON CALL`: planned cancellations of
    schedules that were never going to run.
  - the code is `ZW` ("Unattributed Cancellations System Roll-ups Only").

  Both are system codes, not causes. A client can word them itself, for
  example "planned cancellation" for `PD`.

### How this differs from Darwin

| | Darwin / LDBWS | DS (TRUST) |
|---|---|---|
| Source | TOC-entered, a curated list of about 500 numeric reason codes with passenger prose ("This train has been cancelled because of a shortage of train crew") | Network Rail delay attribution code (two characters), typed in TRUST at the time |
| Wording | For passengers | Industry attribution text ("Driver", "Late arrival of booked inward stock …", "Exclusion commercially agreed …") |
| Delay reason | Yes | No. Darwin's own text is served per stop as `journeyStops[].board.delayReason` (see above). |
| Stability | Updated by the TOC as the cause is understood | The code as first entered. TRUST DA can re-attribute it later, and that change never reaches the feed. |
| Per stop | The reason is per service | The reason is per train. For `EN ROUTE`, the location it was cancelled from is stored (`train_reasons.loc_stanox`) but not served. |

The two vocabularies do not map onto each other, so DS makes no attempt to
translate one into the other.

## Per-stop status

`journeyStops[].status` is one of `OnTime`, `Late` (with `lateMinutes`),
`Cancelled`, `NoReport`, `Arrived`, `Departed` or `Scheduled`. The table of
rules and their LDBWS equivalents is in
`crates/api/src/data/stop_live_status.rs`.

In short, it differs from Darwin in three ways:

- The estimates are TRUST's current delay carried forward, not Darwin
  forecasts.
- `Scheduled` is used where Darwin would say "On time" without live data.
- There is no `Delayed` state.

## Pipeline

1. `trust-backlog-consumer` (`src/reasons.rs`) turns each coded `0002`/`0006`
   into a `common::TrainReasonMessage`. It posts them, best-effort, to
   `POST /private/train-reasons` before each backlog batch. The URL is
   derived from `API_INGEST_URL`, and the service-account group is the same
   one.
2. `api` files each reason against the shared `trains` row. It finds the row
   by uid, creating it if needed, or else by TRUST `train_id`. It writes to
   `train_reasons`: one row per (train, message type), the newest winning.
   The table cascades from `trains`, so it shares that table's 30-day
   retention.
3. The text is looked up at read time.

History: only messages processed after deploy have reasons.
`train_movement_events.raw_body` was never populated, so nothing can be
backfilled.

Rollout order: deploy `api` first. A new consumer against an old `api` gets a
404 on the reasons POST. That is logged and counted
(`trust_backlog_consumer_errors_total{operation="post_train_reasons"}`) and
never blocks the backlog.
