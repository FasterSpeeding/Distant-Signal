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
3. Deletes share links that point at the user's journeys. `unlisted_links` is polymorphic and has no foreign key to `journeys`.
4. Runs `DELETE FROM users`. Everything else cascades.

The inactive-account sweep (below) uses the same function.

## Per-table behaviour

"Cascade" means the row is deleted with the account. "Export" is the key in the export JSON.

| Table | Personal data | On account deletion | Retention while the account exists | Export |
|---|---|---|---|---|
| `users` | OIDC subject id, name, username, verified email (if any), IdP groups, created and last-login times | Deleted | Kept while the account exists. Optional inactive-account deletion is **off by default** (`INACTIVE_ACCOUNT_RETENTION_DAYS`). | `account` |
| `sessions` | Hashed session token, expiry | Cascade | Deleted when expired (`session_ttl_days`, 14), hourly | `sessions` (without id or token) |
| `oidc_login_state` | PKCE verifier, nonce, CSRF state, return path. Not linked to a user. | n/a | Deleted after 15 minutes | n/a |
| `push_subscriptions` | Push endpoint URL, encryption keys, timestamps | Cascade | Deleted when the push service reports it gone, beyond 20 per user, or when the user has not logged in **and** the subscription has not been renewed for `STALE_PUSH_SUBSCRIPTION_DAYS` (365) | `pushSubscriptions` (without keys) |
| `pinned_lines`, `pinned_stations`, `pinned_operators` | Pins | Cascade | Life of the account | `pinnedLines`, `pinnedStations`, `pinnedOperators` |
| `custom_lines` | User-defined lines | Cascade, which also removes its group grants, so group members lose access | Life of the account | `customLines` |
| `custom_line_group_grants` | Grants of the user's custom line to a group (`granted_by`) | Cascade | Life of the line or group | `customLineGroupGrants` |
| `train_subscriptions` | Tracked trains: date, origin, destination, operator, platform, custom name | Cascade | Deleted `PAST_TRAVEL_RETENTION_DAYS` (548, about 18 months) after `service_date` | `trackedTrains` |
| `tracked_train_tickets` | Ticket records: operator, ticket type, origin and destination, source | Cascade | Attached to a tracked train: deleted with it. Standalone: deleted 548 days after the departure date (or the creation date if there is no departure date). | `tickets` |
| `train_notification_state` | Last push sent per tracked train | Cascade | Deleted with its tracked train | `notificationState.trains` |
| `line_notification_state` | Last push sent per pinned line | Cascade | Life of the account | `notificationState.lines` |
| `journeys`, `journey_legs` | Journeys and their legs: stations, dates, time windows | Cascade | A journey with no leg dated in the last 548 days is deleted, with its share link | `journeys` (legs nested) |
| `journey_leg_notification_state` | Last push sent per journey leg | Cascade | Deleted with its leg | `notificationState.journeyLegs` |
| `journey_templates`, `journey_template_legs` | Recurring journey definitions | Cascade | Life of the account (never pruned by age) | `journeyTemplates` (legs nested) |
| `journey_template_skipped_dates` | "Not this date" markers | Cascade (via the template) | Markers dated more than 548 days ago are deleted | `journeyTemplates[].skipped_dates` |
| `groups` | Group name. `created_by` is the creator. | The group survives for other members, and `created_by` becomes NULL. A group the user owned alone is deleted. | Life of the group | `groupMemberships` (name, role, `group_created_by_you`) |
| `group_members` | The user's membership and role | Cascade, after ownership hand-over | Life of the membership | `groupMemberships` |
| `group_trains`, `group_journeys` | Trains and journeys the user shared into a group (`added_by`) | Cascade | Deleted when the user leaves, or with the train or journey | `groupSharedTrains`, `groupSharedJourneys` |
| `group_invite_links` | Invite links the user created (hashed token) | Cascade | Expire after 7 days. Revoked or expired rows are deleted 30 days later. | `groupInviteLinks` (without token) |
| `unlisted_links` | Share links the user created (hashed token) | Cascade, plus any link naming the user's journeys | Revoked or expired rows are deleted after 30 days, and with their journey | `shareLinks` (without token) |

Ticket uploads (PDF, pkpass, zip) are parsed in memory and never stored.

## Settings

All three are on the api (`crates/api/src/data/config.rs`), and are set in the chart as `api.*` in `values.yaml`. The sweep runs hourly, in `session_cleanup_sweep_loop`, every `SESSION_CLEANUP_INTERVAL_SECS`. Setting a value to 0 disables that limit.

| Env var | Chart value | Default | Effect |
|---|---|---|---|
| `PAST_TRAVEL_RETENTION_DAYS` | `api.pastTravelRetentionDays` | 548 | Deletes these after the given number of days: tracked trains, standalone tickets, journeys and template skip markers. |
| `STALE_PUSH_SUBSCRIPTION_DAYS` | `api.stalePushSubscriptionDays` | 365 | Deletes push subscriptions of users who have been absent this long. |
| `INACTIVE_ACCOUNT_RETENTION_DAYS` | `api.inactiveAccountRetentionDays` | **0 (off)** | Deletes whole accounts with no login and no live session for this long, at most 100 per run. |

`INACTIVE_ACCOUNT_RETENTION_DAYS` is off by default because most accounts have no email address, so a user cannot be warned first. Before enabling it (the audit suggests 730), state it in the privacy notice.

The `/account` page says "18 months" and "7 days". If `PAST_TRAVEL_RETENTION_DAYS` or the backup retention changes, update that copy too.

## Backups

The daily `pg_dump` is age-encrypted and kept for 7 days (Ranma-Config `distant-signal.yaml`). Deleted or pruned data therefore leaves every backup within 7 days. The account and deletion pages say so.
