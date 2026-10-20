# Connected apps (delegated access): operator guide

A registered client, today only the Distant-Signal-MCP (ds-mcp), can act
for a Distant Signal (DS) user who connected it on the Connected apps page:
list and read their journeys, save an itinerary, and delete journeys it
saved itself. Design and threat model:
[the delegated-access spec](superpowers/specs/2026-10-14-ds-mcp-delegated-access-design.md).
Client-facing contract: [api-changelog.md](api-changelog.md), 2026-10-15.

**It is off by default.** With `api.delegated.clients` empty (the default)
the api binds no delegated listener, the chart creates no delegated Service,
and every delegated bearer is refused.

## How a request is accepted

1. ds-mcp exchanges the user's Authentik token for a 5-minute token from the
   `distant-signal-api` provider (`iss`
   `https://sso.cursed.solutions/application/o/distant-signal-api/`, `aud`
   and `azp` `distant-signal-api`, RS256, a `scope` claim, no `act`, no
   `sid`).
2. It calls the api's **delegated listener** (`api.delegated.port`, 8082) on
   the `<release>-api-delegated` ClusterIP Service. That listener marks every
   request it accepts; a delegated token arriving any other way (the
   tunnel, the Ingress, the frontend proxy, the main Service) is answered
   `401` and logged `event=delegated_rejected reason=public_listener`. The
   NetworkPolicy admits the port only from `networkPolicy.apiExtraIngressNamespaces`
   (narrowed by `apiExtraIngressPodLabels`); with none listed nothing can
   reach it. The frontend proxy never forwards `Authorization`.
3. A router-wide gate picks the client by the token's unverified `iss` and
   `aud`, then verifies it with that client's own verifier (signature
   against the provider's JWKS, exact `iss`, `aud`, `exp`, `nbf`, a required
   `iat`, not a logout token). It refuses a cookie and a bearer together,
   and any route other than the four delegated ones.
4. The rate limiter charges the user's and the client's delegated buckets.
5. The handler's extractor loads the user's active grant for the client,
   refuses a token issued before the grant or before the user's last "log
   out other sessions", expires a grant unused for 90 days, and requires the
   route's scope in all of the token, the grant and the client's
   `maxScopes`. An empty `scope` grants nothing.

`issuerUrl` must equal the token's `iss` exactly, trailing slash included.

## Turning it on

1. Authentik: the `distant-signal-api` provider and application (spec §9.6,
   done in production on 2026-10-14). The application has no group binding
   (decision D-18): any Authentik user may be exchanged for, and the DS grant
   gates use.
2. Chart values:

   ```yaml
   api:
     delegated:
       clients:
         - name: ds-mcp
           displayName: Distant Signal assistant
           issuerUrl: https://sso.cursed.solutions/application/o/distant-signal-api/
           audience: distant-signal-api
           maxScopes: [journeys:read, journeys:write, journeys:delete]
           enabled: true
           serviceGroup: srv-ds-mcp
   networkPolicy:
     apiExtraIngressNamespaces: [ds-mcp]
   ```

   The api refuses to start if a delegated audience equals the SSO or
   internal client id (or another client's), or a delegated issuer equals
   the SSO or internal issuer.
3. Deploy. The migration `20261015010000_delegated_access` is expand-only;
   with per-service roles on, re-run the role setup so the notifier gets
   its column grants on `delegated_grants`.
4. ds-mcp points its delegated calls at
   `http://<release>-api-delegated.<namespace>.svc:8082`.

## Users' side

- `/account/connected-apps/ds-mcp` is the consent page ds-mcp links to. It
  needs a DS login (which creates the DS account on a first visit), shows
  who is logged in and what the app may do, and issues a single-use
  confirmation token bound to the session (10 minutes). Allow creates the
  grant with the ticked scopes only. The page cannot be framed.
- After a grant is created the notifier pushes "<app> connected" to each of
  the user's devices, and every page shows a banner for 7 days.
- `/account/connected-apps` (and the account page's link) lists grants,
  with Disconnect, and "Recently deleted by the assistant": what a client
  deleted in the last 30 days.

## Revocation

| Trigger | Effect |
|---|---|
| Disconnect | Grant revoked (`user`) |
| Admin `POST /api/admin/users/revoke-sessions` | Every grant revoked (`admin`), with the sessions |
| Authentik back-channel logout on `distant-signal` | Every grant revoked (`backchannel`) |
| ds-mcp's own logout: `POST /delegated-grants/revoke {sub}` with its `srv-ds-mcp` token | That client's grant revoked (`client_logout`); always `204` |
| 90 days unused | Revoked (`inactive`) on the next request or by the hourly sweep |
| "Log out other sessions" | Grant kept; tokens issued before it refused |
| Account deletion | Rows cascade |
| **Kill switch:** `enabled: false` for the client | Every token for it `401` at once, and its listener is not bound if no client is enabled |

For a suspected ds-mcp compromise: set `enabled: false` and deploy, rotate
the `distant-signal-api` secret in Authentik, and if needed revoke every
grant:

```sql
UPDATE delegated_grants SET revoked_at = now(), revoked_reason = 'client_disabled'
WHERE client = 'ds-mcp' AND revoked_at IS NULL;
```

`journeys.created_via = 'ds-mcp'` finds every journey the client saved, and
`delegated_deletions` (30 days) what it deleted.

## Limits

Per user: 60 reads, 10 saves and 5 deletes a minute (bursts 20, 5, 3); per
client, 20 times that (`api.rateLimit.delegated`). Per user per rolling 24
hours: 50 saves and 20 deletes (`api.delegated.dailySaveLimit`,
`dailyDeleteLimit`), answered `429 daily_limit` with `Retry-After`. An
itinerary may have 1 to 20 legs, the journey's own leg cap, for a
delegated client as for the web app.

## Itinerary errors

`POST /Journeys/itinerary` answers a delegated client with JSON `{error,
message}`; `message` is a sentence ds-mcp can show the user as is. The web
app gets the same message as plain text.

| Status | `error` | When |
|---|---|---|
| 400 | `idempotency_key_required` | No, or a malformed, `Idempotency-Key` |
| 400 | `invalid_itinerary` | Bad JSON, 0 or more than 20 legs, a bad UID, CRS code or name, a date past the tracking horizon, the journey or tracked-train cap |
| 409 | `idempotency_conflict` | The key was used for a different body |
| 422 | `train_does_not_call_at_origin` | The train doesn't stop at `originCrs` on that date (a pass is not a stop) |
| 422 | `train_does_not_call_at_destination` | The train doesn't stop at `destinationCrs` on that date |
| 422 | `destination_before_origin` | It stops at both, but reaches `destinationCrs` first |
| 422 | `boarding_not_allowed` | `originCrs` is set-down only for this train, or its terminus |
| 422 | `alighting_not_allowed` | `destinationCrs` is pick-up only for this train (a sleeper's `U` stops) |
| 429 | `daily_limit`, `rate_limited` | With `Retry-After` |

The 422 checks run only on legs with `originCrs` or `destinationCrs` and
only when the timetable has the train (`schedule_calling_points_full` for
that UID and date). A train not in it yet is saved unchecked, as before,
and resolves through tracking. The first failing leg wins; with more than
one leg its message starts `Leg N: `. Nothing is saved on any error, and
the `Idempotency-Key` stays unused. Code: `crates/api/src/data/leg_validation.rs`.

## Retention

The hourly maintenance pass (the api loop, or the `api-maintenance`
CronJob) revokes grants unused for 90 days, deletes revoked grants 365 days
after revocation (`api.delegated.revokedGrantRetentionDays`,
`DELEGATED_REVOKED_GRANT_RETENTION_DAYS`), `delegated_deletions` after 30 days, `Idempotency-Key`s
after 24 hours and expired confirmation tokens. See
[personal-data-retention.md](personal-data-retention.md).

## Observability

- Audit log lines (`audit=true`): `delegated_request` (every delegated
  request: client, user, grant, `jti`, route, status, scopes),
  `delegated_rejected` (with a reason and a hash of the `sub`),
  `delegated_grant_created` and `delegated_grant_refused` (with a hash of the
  session id), `delegated_grant_revoked`, `delegated_journey_saved`,
  `delegated_delete` (WARN), `delegated_revoke_denied`. Tokens and request
  bodies are never logged.
- Metrics: `distant_signal_api_delegated_requests_total{client,route,status}`,
  `distant_signal_api_delegated_rejected_total{client,reason}`,
  `distant_signal_api_delegated_grants_active{client}`,
  `distant_signal_api_rate_limited_total{caller="delegated_user"|"delegated_client"}`,
  and the notifier's `grant_notice` cycle.
