# Journey breaks and suggestion pushes

Runbook for mid-journey changes (design:
`docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md`).

## What runs where

- **The api** judges journeys and plans alternatives. A journey page read
  (`GET /Journeys/{id}`, by its owner) opens or ends a break at once. With
  `api.journeyBreaks.enabled`, the break sweep (`crates/api/src/journey_break_sweep.rs`)
  also runs every `api.journeyBreaks.intervalSecs` (60) under the advisory
  lock `journey_breaks` (`dslpjbrk`), so one api replica runs it whatever
  `API_BACKGROUND_LOOPS` says. It looks at today's and yesterday's journeys
  with a train leg leaving inside the live window (at most
  `maxJourneysPerTick`), opens, updates or ends each break, and plans its
  suggestions through its own planning slot (never a user's).
- **The notifier** (`crates/notifier/src/break_notices.rs`) reads open
  breaks with suggestions every `notifier.journeyBreaksPollIntervalSecs`
  (15) and pushes one notice per break, again only when it gets worse. A
  notice is that train's push: the train's own delay or cancel push stays
  quiet for it.

## The kill switch

`api.journeyBreaks.enabled` (default `false`) turns both the sweep and the
notices on or off. Off, the in-app parts still work: the banner on a
journey page, "I missed this train", the alternatives sheet, accept and
Undo. Turning it off stops pushes at the next notifier restart; open breaks
stay until they are acted on, recover or expire (04:00 after the journey's
last service date).

## Opt-outs

- Per user: `notification_preferences.journey_suggestions`
  (`/account/preferences`, "Suggest other trains when a journey breaks").
- Per journey: `journeys.suggestions_muted` ("Stop suggestions for this
  journey" in the sheet).

Either stops the push only; the train's own push then goes out as before.

## Metrics

| Metric | Labels |
|---|---|
| `api_journey_breaks_detected_total` | `cause`, `fault` |
| `api_journey_breaks_resolved_total` | `status` |
| `api_journey_break_suggestions_seconds` | `trigger` (`request`, `sweep`) |
| `api_journey_break_suggestions_total` | `result` (`found`, `none`, `error`) |
| `api_journey_break_opened_total` | `via` (`push`, `in_app`) |
| `api_journey_replan_accepted_total` | `via`, `rank`, `pick`, `fit` |
| `api_journey_replan_stale_total` | |
| `api_journey_breaks_sweep_seconds` | |
| `notifier_journey_break_notice_total` | `result` (`sent`, `opt_out`, `muted`, `not_worse`, `acted`, `unconfirmed`, `in_app`) |

## The funnel

Aggregates only; no per-user data leaves the database.

```sql
SELECT date_trunc('week', detected_at) AS week,
       count(*) AS breaks,
       count(*) FILTER (WHERE notified_at IS NOT NULL) AS notified,
       count(*) FILTER (WHERE notified_at IS NOT NULL AND opened_via = 'push') AS opened_from_push,
       count(*) FILTER (WHERE opened_at IS NOT NULL) AS opened,
       count(*) FILTER (WHERE status = 'accepted') AS accepted,
       percentile_cont(0.5) WITHIN GROUP (ORDER BY resolved_at - detected_at)
           FILTER (WHERE status = 'accepted') AS median_time_to_accept,
       count(*) FILTER (WHERE status = 'dismissed') AS dismissed,
       count(*) FILTER (WHERE status = 'recovered') AS recovered
FROM journey_breaks
GROUP BY 1 ORDER BY 1 DESC;

SELECT dismiss_reason, count(*) FROM journey_breaks
WHERE status = 'dismissed' GROUP BY 1 ORDER BY 2 DESC;
```

Success for the first month: at least half of notified breaks opened, and
at least a third of opened breaks accepted.
