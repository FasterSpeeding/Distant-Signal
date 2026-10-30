# Personal data: retention, deletion and export

UK legal audit LEG-4 and LEG-5 (2026-09-27). This page records what happens to each table of personal data when:

- a user deletes their account;
- a user downloads their data;
- data ages out.

Keep it in step with `crates/api/src/data/account.rs` and `crates/api/src/data/retention.rs`, and with the privacy notice.

## Self-service routes

| Route | What it does |
|---|---|
| `GET /public/account/export` (frontend: `/api/account/export`, the "Download my data" link on `/account`) | Returns every row of personal data held about the caller as one JSON file (UK GDPR Arts. 15 and 20). |
| `DELETE /public/account` (frontend: `/account` → "Delete my account") | Deletes the account and all personal data in one transaction (Art. 17). |

The export is `data::account::export_account`. It is sent with `Content-Disposition: attachment` and `Cache-Control: no-store`.

- Each row is the table's own columns (snake_case).
- Credentials are left out and listed in the file's `omitted` field:
  - hashed session ids;
  - push encryption keys;
  - hashed link tokens.
- Other group members' names are not included.

The delete route has three guards:

- It needs a logged-in session.
- It needs the JSON body `{"confirm": "delete my account"}` (case-insensitive).
- It needs a same-origin `Origin` or `Referer`. The check is strict, as for `logout`: if both headers are absent, the request is refused. This is on top of the router-wide L4 `reject_cross_origin_cookie_mutation` layer.

The response clears the session cookie. The frontend then clears the chat's browser-only data: the MCP OAuth tokens and the Anthropic key in `localStorage`.

Neither route touches the user's single sign-on (Authentik or Discord) identity. The UI tells the user to close that account separately.

## How deletion works

Every foreign key to `users(id)` has an explicit `ON DELETE` action (migrations `20260927060000` and `20260927060100`). `data::account::delete_account` then does four things:

1. Locks the user row.
2. Leaves every group the user belongs to. This uses the same code path as "Leave group" (`groups::remove_member_in_tx`):
   - For an owned group with other members, ownership passes to the longest-standing admin, or if there is none, the longest-standing member.
   - An owned group with no other members is deleted.
   - The user's shared trains and journeys in that group are removed.
   - The user's follows of journeys shared into that group are removed, and so are other members' follows of the user's journeys there.
3. Deletes share links that point at the user's journeys. `unlisted_links` is polymorphic and has no foreign key to `journeys`. Anyone following those journeys through the link stops following them (`journey_follows` cascades from the link and from the journey).
4. Runs `DELETE FROM users`. Everything else cascades.

The inactive-account sweep (below) uses the same function.

## Per-table behaviour

"Cascade" means the row is deleted with the account. "Export" is the key in the export JSON.

| Table | Personal data | On account deletion | Retention while the account exists | Export |
|---|---|---|---|---|
| `users` | OIDC subject id, name, username, IdP groups (no email: the legacy column is dropped by `20261026090000_drop_users_email.sql`), created and last-login times, the usage-analytics choice (`analytics_opt_in`, off by default) | Deleted | Kept while the account exists. Optional inactive-account deletion is **off by default** (`INACTIVE_ACCOUNT_RETENTION_DAYS`). | `account` |
| `sessions` | Hashed session token, expiry | Cascade | Deleted when expired (`session_ttl_days`, 14), hourly | `sessions` (without id or token) |
| `oidc_login_state` | PKCE verifier, nonce, CSRF state, return path. Not linked to a user. | n/a | Deleted after 15 minutes | n/a |
| `push_subscriptions` | Push endpoint URL, encryption keys, timestamps (including the last push delivered), a device label worked out from the browser's User-Agent ("Chrome on Android"; the header itself is not kept) | Cascade | Deleted when the push service reports it gone, beyond 20 per user, or when the user has not logged in **and** the subscription has not been renewed for `STALE_PUSH_SUBSCRIPTION_DAYS` (365) | `pushSubscriptions` (without keys) |
| `pinned_lines`, `pinned_stations`, `pinned_operators` | Pins | Cascade | Life of the account | `pinnedLines`, `pinnedStations`, `pinnedOperators` |
| `custom_lines` | User-defined lines | Cascade, which also removes its group grants, so group members lose access | Life of the account | `customLines` |
| `custom_line_group_grants` | Grants of the user's custom line to a group (`granted_by`) | Cascade | Life of the line or group | `customLineGroupGrants` |
| `train_subscriptions` | Tracked trains: date, origin, destination, operator, platform, custom name | Cascade | Deleted `PAST_TRAVEL_RETENTION_DAYS` (548, about 18 months) after `service_date`. A tracked train only journey legs use is also deleted when the last of those legs (or its journey) is removed, unless the user also tracks it on its own. | `trackedTrains` |
| `tracked_train_tickets` | Ticket records: operator, ticket type, origin and destination, source | Cascade | Attached to a tracked train: deleted with it. Standalone: deleted 548 days after the departure date (or the creation date if there is no departure date). | `tickets` |
| `train_notification_state` | Last push sent per tracked train | Cascade | Deleted with its tracked train | `notificationState.trains` |
| `line_notification_state` | Last push sent per pinned line | Cascade | Life of the account | `notificationState.lines` |
| `journeys`, `journey_legs` | Journeys and their legs: stations, dates, time windows | Cascade | A journey with no leg dated in the last 548 days is deleted, with its share link | `journeys` (legs nested) |
| `journey_revisions`, `journey_revision_legs` | Each journey's booked plan and every change since: trains, stations, booked times, ticket ids, what happened to each leg | Cascade (via the journey) | Deleted with the journey | `journeys[].revisions` (legs nested) |
| `journey_breaks` | Times a journey stopped working (cancellation, missed connection, a train marked missed), the alternatives suggested and what was done | Cascade | Deleted with the journey | `journeyBreaks` |
| `notification_preferences` | Which kinds of push to send, the delay threshold and the quiet hours (time zone included) | Cascade | Life of the account | `notificationPreferences` |
| `notification_holds` | Line and train pushes held for the user's quiet hours: the push text, the line or tracked train, its status and when it was decided | Cascade | Deleted once sent, or dropped when no longer news; at most a day | `notificationState.held` |
| `journey_leg_notification_state` | Last push sent per journey leg | Cascade | Deleted with its leg | `notificationState.journeyLegs` |
| `journey_templates`, `journey_template_legs` | Recurring journey definitions | Cascade | Life of the account (never pruned by age) | `journeyTemplates` (legs nested) |
| `journey_template_skipped_dates` | "Not this date" markers | Cascade (via the template) | Markers dated more than 548 days ago are deleted | `journeyTemplates[].skipped_dates` |
| `groups` | Group name. `created_by` is the creator. | The group survives for other members, and `created_by` becomes NULL. A group the user owned alone is deleted. | Life of the group | `groupMemberships` (name, role, `group_created_by_you`) |
| `group_members` | The user's membership and role | Cascade, after ownership hand-over | Life of the membership | `groupMemberships` |
| `group_trains`, `group_journeys` | Trains and journeys the user shared into a group (`added_by`) | Cascade | Deleted when the user leaves, or with the train or journey | `groupSharedTrains`, `groupSharedJourneys` |
| `journey_follows` | Journeys shared with the user (through a group or a share link) that they follow: which journey, when, and through which group or link (hashed token) | Cascade | Deleted when the user unfollows or leaves the group, when the journey is unshared from the group or deleted, when the link is revoked or regenerated, and by the hourly sweep once the link has expired | `journeyFollows` (journey id, time, group or `link`; no token) |
| `group_invite_links` | Invite links the user created (hashed token) | Cascade | Expire after 7 days. Revoked or expired rows are deleted 30 days later. | `groupInviteLinks` (without token) |
| `unlisted_links` | Share links the user created (hashed token) | Cascade, plus any link naming the user's journeys | Revoked or expired rows are deleted after 30 days, and with their journey | `shareLinks` (without token) |
| `delegated_grants` | Apps the user connected on the Connected apps page: client, scopes, created, last-used and revoked times ([connected-apps.md](connected-apps.md)) | Cascade | Revoked after 90 days unused; revoked rows are deleted 365 days after revocation (`DELEGATED_REVOKED_GRANT_RETENTION_DAYS`) | `connectedApps` |
| `delegated_grant_confirmations` | One-time consent tokens (hashed), bound to a session | Cascade, and with the session | Deleted when used, replaced, or 10 minutes after issue (hourly sweep) | `connectedAppConfirmations` (without token or session) |
| `delegated_deletions` | What a connected app deleted: journey id, custom name, dates, stations, train UIDs | Cascade | Deleted after 30 days | `connectedAppDeletions` |
| `journey_idempotency_keys` | `Idempotency-Key`s of itinerary saves, with the saved response | Cascade | Deleted after 24 hours | `journeySaveKeys` |

Ticket uploads (PDF, pkpass, zip) are parsed in memory and never stored.

## Settings

All three are on the api (`crates/api/src/data/config.rs`), and are set in the chart as `api.*` in `values.yaml`. The sweep runs hourly, in `session_cleanup_sweep_loop`, every `SESSION_CLEANUP_INTERVAL_SECS`. Setting a value to 0 disables that limit.

| Env var | Chart value | Default | Effect |
|---|---|---|---|
| `PAST_TRAVEL_RETENTION_DAYS` | `api.pastTravelRetentionDays` | 548 | Deletes these after the given number of days: tracked trains, standalone tickets, journeys and template skip markers. |
| `STALE_PUSH_SUBSCRIPTION_DAYS` | `api.stalePushSubscriptionDays` | 365 | Deletes push subscriptions of users who have been absent this long. |
| `INACTIVE_ACCOUNT_RETENTION_DAYS` | `api.inactiveAccountRetentionDays` | **0 (off)** | Deletes whole accounts with no login and no live session for this long, at most 100 per run. |

`INACTIVE_ACCOUNT_RETENTION_DAYS` is off by default because the app holds no email addresses, so a user cannot be warned first. Before enabling it (the audit suggests 730), state it in the privacy notice.

The `/account` page says "18 months" and "up to 14 days" (backups). If `PAST_TRAVEL_RETENTION_DAYS` or the backup retention changes, update that copy too: the account, account-deleted and privacy pages, and the delete-account dialog.

## Usage analytics

Not personal data, recorded here because the privacy page describes it.
Design and queries: [usage-analytics.md](usage-analytics.md).

| What | Where | Retention |
|---|---|---|
| Opt-in event counts (`POST /public/events`): one row per London day, event, dimension (route template and enumerated properties) and signed-in or not. No user id, IP address, user agent or session id. | `usage_daily` | Days older than 13 months are deleted by the hourly sweep (`data::usage::prune`, from `retention::prune_personal_data`; not switchable) |
| The same counts, and the always-on feature counters (`distant_signal_api_feature_events_total`), with fixed labels only | Prometheus | Prometheus' own retention (30 days or 25 GB on the cluster) |
| A signed-in user's choice | `users.analytics_opt_in` | With the account; in the export's `account` |
| A logged-out browser's choice, and the offline page's pending count | `localStorage` `ds-analytics-opt-in`, `ds-analytics-offline-pending` (the browser only) | Until cleared |

Turning analytics off stops future counts. Past counts are aggregates with
nothing that links them to a person, so there is nothing to export or delete
for one user; account deletion leaves them.

## Application logs

The api's and the frontend's container logs (requests, errors) can hold an
IP address or a user id. Loki keeps DS application logs for **up to 31 days**
(Ranma-Config's `{namespace="distant-signal"}` retention rule, from
2026-10-31; before that they fell under Loki's global default of a year).
The privacy page says so; keep it in step if that changes.

## Schedule feed SFTP server logs

The schedulefeed pod's SFTP server (SFTPGo, [schedule-feed-sftp.md](schedule-feed-sftp.md))
is open to the internet. Its log holds the source IP of every connection, any
username tried, and what was transferred; IPs are personal data when the
client is a person. Lawful basis: legitimate interests (securing the one
public-facing service, and evidencing what the timetable supplier delivered).
Loki keeps these lines in three tiers (Ranma-Config `logging.yaml`, Alloy's
`audit` stream label and Loki's `retention_stream`, per Ranma's
`docs/specs/sftp-audit-observability.md`):

| Lines | Retention | Why |
|---|---|---|
| Connections that never authenticate (scanners), and everything else in the container's log without an audit label | 31 days (the distant-signal namespace rule) | No reason to keep strangers' addresses |
| Failed logins with a username, and defender bans (`audit="sftp-security"`) | 90 days | Long enough to see slow brute force and investigate an incident reported weeks later |
| Successful logins, uploads and file commands, and schedule-ingest's `schedule_ingest::audit` decision lines (`audit="sftp-delivery"`) | 400 days | A year of deliveries plus a month for an annual supplier review or a dispute; in practice only the supplier's own addresses |

Not stored at all: Alloy drops the kubelet's liveness-probe connections and
"failed to accept" lines from the pod network (10.42.0.0/16), and SFTPGo's
event-manager debug reloads, before they reach Loki.

The schedule-ingest decision line and the `schedule_feed_ingests` /
`corpus_deliveries` rows hold file names, sizes and hashes only, no personal
data. The privacy page's "Connections to our file-transfer server" entry
states the three periods; keep it in step if they change.

## Backups

Deleted or pruned data can stay in backups for **up to 14 days**. The account, account-deleted and privacy pages and the delete-account dialog say so (decided 2026-09-30).

- The daily `pg_dump` is age-encrypted and kept for 7 days (Ranma-Config `distant-signal.yaml`).
- The chart's point-in-time recovery (`postgresql.pgbackrest.enabled`; [postgres-pitr.md](postgres-pitr.md)) keeps WAL and backups in an encrypted pgBackRest repository: a weekly full backup, daily differentials, and `repo.retentionFullType: time` with `retentionFull: 7`. That keeps whatever is needed to restore to any moment in the last 7 days, which includes the weekly full backup taken before that window. So a row deleted just after a full backup stays in the repository until the next-but-one full backup expires it: up to about 14 days.

If either retention changes, keep the 14-day figure (or a shorter one) true, and update the pages above.
