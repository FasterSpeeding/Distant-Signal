# Session revocation: admin revoke and Authentik back-channel logout

A login creates a session that lasts `SESSION_TTL_DAYS` (default 14). The
session carries the user's Authentik `groups` claim as it was at login
(`users.groups`), and every group-gated feature reads it from there. This
covers the chatbot (`CHATBOT_ACCESS_GROUP`, only while `CHATBOT_ACCESS` is
`group`; with `CHATBOT_ACCESS=authenticated` any live session gets the
chatbot) and admin (`ADMIN_GROUP`).

Code: `crates/api/src/routes/admin.rs`, `crates/api/src/routes/auth.rs`
(`backchannel_logout`), `crates/api/src/auth/backchannel_logout.rs` and
`crates/api/src/data/users.rs` (`revoke_all_sessions`).

## How to end a session

| Who | How | Effect |
| --- | --- | --- |
| The user | Account menu, "log out other sessions" (`POST /api/auth/sessions/revoke-others`) | Ends all of their sessions and reissues one for the current browser |
| An admin | `POST /api/admin/users/revoke-sessions` | Ends all of the target user's sessions |
| Authentik | Back-channel logout to `POST /api/auth/backchannel-logout` | Ends all of that user's sessions |

All three set `users.sessions_invalidated_at` and delete the user's `sessions`
rows in one transaction. The next request from a revoked browser is anonymous.
The user can log in again immediately unless they are also disabled in
Authentik.

## When a group change takes effect

`users.groups` is overwritten from the ID token on every login and on no other
occasion. It is never merged, so a removed group disappears on the next login.

- **Adding** someone to a group takes effect at their next login. They can log
  out and back in to get it straight away.
- **Removing** someone from a group, or disabling them, does NOT take effect
  on existing sessions by itself. Those sessions keep the old groups for up to
  `SESSION_TTL_DAYS`. To make it take effect now:
  1. Remove the group (or disable the user) in Authentik.
  2. Revoke their sessions here, either through an admin revoke or through
     Authentik's back-channel logout (see below).
  3. Their next login reads the new groups. A disabled user can't log in.

The admin check itself uses `users.groups`. An admin removed from
`ADMIN_GROUP` keeps admin rights until their next login, unless another admin
revokes their sessions.

## Admin revoke

Off by default. Set `api.adminGroup` in the chart (`ADMIN_GROUP` env) to an
Authentik group, e.g. `distant-signal-admins`. When it is empty, the endpoint
returns 403 to everyone. Startup logs which state applies.

The request must come from a logged-in browser session whose `groups` contain
the admin group. It must also carry a same-origin `Origin` (or `Referer`), the
same strict check `logout` uses. There is no admin UI. From the devtools
console on the site:

```js
await fetch('/api/admin/users/revoke-sessions', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ username: 'their-authentik-username' }), // or { userId: '<OIDC sub>' }
}).then(r => r.json())
// => { userId: '...', sessionsRevoked: 2 }
```

`userId` is the user's Authentik subject (`sub`, stored as `users.id`).
`username` is their Authentik username (`preferred_username`) as of their
last sign-in to Distant Signal, matched case-insensitively. Distant Signal
does not request or store email addresses, so there is no lookup by email;
a body with an `email` field gets `400 email_lookup_removed_use_username`.

| Status | Meaning |
| --- | --- |
| 200 | Revoked; `sessionsRevoked` is the number of rows deleted (0 is fine) |
| 400 | Body must contain exactly one of `userId` or `username` (`give_exactly_one_of_userId_or_username`), or used the removed `email` field |
| 401 | Caller not logged in |
| 403 | Cross-site request, caller not in `ADMIN_GROUP`, or `ADMIN_GROUP` unset |
| 404 | No such user (username matching is case-insensitive, against the username stored at their last sign-in) |
| 409 | More than one user has that username (`username_ambiguous`); use `userId` |

Each revocation is logged at WARN with `audit=true event=admin_session_revoke
admin_user_id=… target_user_id=… sessions_revoked=…`. A non-admin attempt is
logged with `event=admin_session_revoke_denied`.

## Authentik back-channel logout

The endpoint implements OpenID Connect Back-Channel Logout 1.0. Authentik
POSTs `logout_token=<JWT>` whenever an Authentik session that logged in to
Distant Signal is deleted. That happens when the user logs out of Authentik,
an admin deletes the session, or the user is deactivated (which deletes all
their sessions).

The api validates the token before doing anything:
- the signature, against the JWKS discovered from `SSO_ISSUER_URL`;
- the `typ` header, which must be `logout+jwt` if present;
- `iss` must equal `SSO_ISSUER_URL`, and `aud` must contain `SSO_CLIENT_ID`;
- `iat` must be present, not in the future and at most 1 hour old, and `exp`
  (if present) must not have passed;
- the back-channel logout `events` member must be present;
- `sub` must be present;
- there must be no `nonce`.

It then ends all of that user's sessions. Responses:
- 200 for a valid token, including one for an unknown user;
- 400 for an invalid token;
- 500 if the database write fails (Authentik retries).

Every response carries `Cache-Control: no-store`. Each revocation is logged at
INFO with `audit=true event=backchannel_logout user_id=…`. A rejected token is
logged at WARN with the reason.

**Per user, not per session.** Authentik sends one token per Authentik session,
with both `sub` and `sid`. The api doesn't store `sid` and ends every session
of the user. As a result, logging out of Authentik on one device also logs
that user out of Distant Signal on their other devices. A token carrying only
`sid` is refused with 400. Authentik 2026.8 always sends `sub`.

### Limits

- **Authentik only notifies while the access token from that login is still
  valid.** In 2026.8, `providers/oauth2/signals.py` sends the logout only for
  unexpired `AccessToken` rows of the ending session. The default
  `access_token_validity` is `hours=1`, so a session deleted more than an hour
  after the Distant Signal login produces no back-channel call. The provider
  setting below raises it to match the session TTL. Distant Signal discards
  the access token at login and never uses it, so a longer validity doesn't
  widen what a leaked Distant Signal database or log can do.
- It doesn't cover group changes. Removing a group ends no Authentik session.
  Use an admin revoke.
- The provider must sign with an asymmetric key (RS256). Without a signing
  key, Authentik uses HS256 with the client secret, which the JWKS can't
  verify, and every token is rejected (400, `UnknownKey` or `Malformed` in the
  api log).

### Authentik configuration (for the Ranma-Config session)

These settings are on the production Authentik (2026.8.1,
`https://sso.cursed.solutions`). They apply to the OAuth2/OpenID provider
behind the `distant-signal` application (issuer
`https://sso.cursed.solutions/application/o/distant-signal/`), not the
`distant-signal-internal` one.

1. **Provider > Edit > Advanced protocol settings** (blueprint attrs in
   brackets):
   - **Logout URI** (`logout_uri`):
     `https://ds.cursed.solutions/api/auth/backchannel-logout`
   - **Logout Method** (`logout_method`): `Back-channel` (`backchannel`, the
     model default)
   - **Access token validity** (`access_token_validity`): `days=14`, equal to
     `api.sessionTtlDays` / `SESSION_TTL_DAYS`. See Limits.
   - **Signing Key** (`signing_key`): must be set to an RSA certificate. It
     almost certainly is already, since logins verify ID tokens against the
     JWKS.
2. **Network path.** The Authentik worker sends the POST from the `authentik`
   namespace to `ds.cursed.solutions`. That name resolves to Cloudflare, and
   `netpol-authentik.yaml`'s `allow-egress-internet` already allows it. Check
   that no Cloudflare WAF, bot-fight or Access rule challenges a server-to-server
   POST to `/api/auth/backchannel-logout`. The request carries no browser
   headers.
3. **Admin group (optional).** Create an Authentik group, e.g.
   `distant-signal-admins`, and add the operators. Make sure the provider's
   `groups` scope mapping emits it. Then set `api.adminGroup:
   distant-signal-admins` in the distant-signal HelmRelease values.
4. **Verify.**
   - Log in to Distant Signal in a private window, then log out of Authentik
     in that window.
   - The worker's task list (Admin > System Tasks, or the worker log) should
     show "Back-channel logout successful".
   - The api log should show `event=backchannel_logout`.
   - Reloading Distant Signal should show you logged out.
   - Then deactivate a test user who has a live Distant Signal session, and
     confirm the same.
