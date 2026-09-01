# Dynamic Post-Login Redirect — Design Sketch

**Status: sketch/proposal only, not an approved design.** Written to the
same rigor as the existing specs in this directory (e.g.
`docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md` and
`docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md`) so it can
be reviewed and iterated on the same way, but it has not gone through
implementation planning and nothing here is committed. No code was
written or changed for this pass — this document only.

## Problem

Today, `GET /auth/login` and `GET /auth/callback` (`crates/api/src/routes/auth.rs`)
take no return-path information at all. Every successful sign-in ends the
same way, unconditionally:

```rust
let mut response = Redirect::temporary(&app.config.sso_post_login_redirect_url).into_response();
```

`sso_post_login_redirect_url` (`crates/api/src/data/config.rs:100-106`) is a
single operator-configured URL — "the frontend's own root URL" per its own
doc comment, which also states plainly: *"One fixed target, not a
round-tripped 'return to this page' value — a v1 scope simplification."*
That simplification was made deliberately, not by oversight — the original
plan's Global Constraints
(`docs/superpowers/plans/2026-08-28-user-accounts-sso.md:140-144`) call it
out explicitly: *"No 'return to originating page' round-trip... A v1 scope
simplification, consistent with the design doc's own Frontend section
already being sketch-only."* This design is the deferred follow-up work
that closes that gap.

The practical symptom: a visitor reading `/lines/some-custom-line`, or
mid-way through attaching a ticket to a tracked train
(`frontend/components/TicketPanel.tsx`), who clicks one of this app's
several inline "Log in" nudges, is dumped back at the site root after a
successful sign-in — losing whatever page, and whatever in-progress task,
they were on.

## Goals

- After a successful sign-in, land the browser back on the page the user
  was actually on when they clicked "Log in" (path + query string), not
  always `sso_post_login_redirect_url`.
- Absolutely no open-redirect regression: the value that ends up in
  `Redirect::temporary(...)` after login must be provably same-origin and
  relative, validated **server-side**, never trusted from client input
  as-is.
- Reuse this app's existing DB-backed, single-use `oidc_login_state` row
  (already tied 1:1 to one in-flight login attempt) as the vehicle that
  carries the return path through the round trip to the IdP and back,
  rather than inventing a second mechanism alongside it.
- Leave the OAuth `state` parameter's own semantics untouched — still a
  bare `CsrfToken::new_random()` value, compared byte-for-byte on
  callback, exactly as today.
- No behavior change from today when no return path was captured, or the
  one that was fails validation: fall back to the existing
  `sso_post_login_redirect_url`, not an error page.

## Non-goals

- **`/auth/logout`'s redirect behaviour.** It doesn't have one today —
  `routes::auth::logout` returns `StatusCode::NO_CONTENT`, not a
  `Redirect`; every call site (`LogoutButton.tsx`) is a client-side
  `fetch(..., { method: 'POST' })` followed by `router.refresh()`, no
  navigation at all. (Note: this quietly diverges from the original plan's
  Global Constraints, which described `/auth/logout` as also redirecting
  to a fixed URL — that part of the plan was apparently narrowed or
  changed before shipping; not this design's concern to reconcile.) Out of
  scope.
- **Referer-header-based capture**, considered and rejected — see Design →
  Where the return path is captured.
- **Any change to `sso_redirect_url`** (the OIDC callback URI registered
  with the IdP) or the cookie-`Secure` derivation that depends on it
  (`routes::auth::cookie_secure`). Untouched by this design; see Research
  below for why this design is specifically shaped to avoid reintroducing
  that class of bug.
- **RP-Initiated Logout, multi-provider chooser, rate limiting on auth
  routes** — pre-existing open items from the original design/plan,
  unrelated to and unaffected by this change.
- **Implementation.** Per the brief, this is a design only.

## Research

### The existing login-state mechanism (recap, with citations)

`GET /auth/login` (`crates/api/src/routes/auth.rs:39-69`) already:

1. Calls `app.oidc.authorize_url()` (`crates/api/src/auth/oidc.rs:156-166`),
   which generates a PKCE verifier/challenge pair, a random CSRF `state`
   token, and a nonce.
2. Persists the verifier, nonce, and CSRF state server-side via
   `data::users::insert_login_state` (`crates/api/src/data/users.rs:147-170`),
   keyed by a freshly generated opaque `login_state_id`.
3. Sets `login_state_id` as a short-lived (`Max-Age=900`), `HttpOnly`
   cookie (`LOGIN_STATE_COOKIE_NAME`, `crates/api/src/auth.rs:64`) — this
   is how the id survives the round trip in the *browser*, while the
   secrets it keys survive it in the *database*.
4. Redirects the browser to the IdP.

On the way back, `GET /auth/callback` (`crates/api/src/routes/auth.rs:78-158`)
reads `login_state_id` back off the cookie, calls
`data::users::consume_login_state` (`crates/api/src/data/users.rs:176-186`)
— a `DELETE ... RETURNING`, so the row is fetched and invalidated in one
atomic step, enforcing single-use — and compares the stored `csrf_state`
against the `state` query param the IdP echoed back.
`oidc_login_state` (`crates/api/migrations/20260828090000_user_accounts.sql:61-67`)
is a small, short-lived (15-minute sweep) table with exactly this job:
bridging one login attempt across an untrusted third-party hop. It is
already, structurally, the "per-in-flight-login-attempt" row the brief
points at as the natural place to also carry a return path — nothing new
needs to be invented to get a value from `/auth/login` to `/auth/callback`
intact.

### The historical `sso_redirect_url` / `Secure`-cookie bug, and what it implies here

`routes::auth::cookie_secure` (`crates/api/src/routes/auth.rs:26-37`) and
`auth::set_cookie_header`'s doc comment (`crates/api/src/auth.rs:80-88`)
document a real, previously-live bug: every `Set-Cookie` this app issued
used to hardcode the `Secure` attribute, which a browser unconditionally
rejects over plain HTTP — so login could never actually set a cookie over
`http://localhost:3000`. The fix was deriving `secure` from
`sso_redirect_url`'s own scheme (the one config value that's already the
real, operator-set, browser-facing origin) rather than a second,
independently-set flag that could drift out of sync with it.

The lesson that generalizes: **two config/data values that are each
independently supposed to represent "the app's own origin" are a latent
drift hazard** — nothing enforces they stay in sync, and the failure mode
(a `Secure` cookie the browser silently drops) is exactly the kind of bug
that only shows up live, not in a type-checker or a unit test. This
design is shaped specifically to not reintroduce that hazard: the
validator below (Design → Validation) accepts only a *relative* return
path — no scheme, no host, no port — so it never needs to compare against
`sso_redirect_url`, `sso_post_login_redirect_url`, or any other
"known-good origin" value at all. There is nothing here that could drift
out of sync with those, because nothing here duplicates what they encode.

### OWASP Unvalidated Redirects and Forwards Cheat Sheet

Fetched 2026-09-01. OWASP's stated preference is to avoid trusting raw
client-supplied redirect input entirely: *"Where possible, have the user
provide short name, ID or token which is mapped server-side to a full
target URL."* Where that's not practical, its fallback guidance is
allow-listing (*"Sanitize input by creating a list of trusted URLs (lists
of hosts or a regex)"*) and confirming any accepted value is *"valid,
appropriate for the application, and is **authorized** for the user."*

This app's design already leans toward the *preferred* option, not the
fallback: the browser never gets to hand a URL straight to
`Redirect::temporary` at the moment of redirect. The client-supplied value
is validated once at `/auth/login` time and stored server-side, keyed by
`login_state_id` — structurally the same "indirect mapping" shape OWASP
recommends, just keyed by this app's existing per-attempt row instead of a
newly invented short id.

Source: [Unvalidated Redirects and Forwards Cheat Sheet | OWASP](https://cheatsheetseries.owasp.org/cheatsheets/Unvalidated_Redirects_and_Forwards_Cheat_Sheet.html)
(fetched 2026-09-01).

### RFC 6749 §4.1.1 and RFC 9700 (OAuth 2.0 Security Best Current Practice) on `state`

Fetched 2026-09-01. RFC 6749 §4.1.1 defines `state` narrowly:
*"state: RECOMMENDED. An opaque value used by the client to maintain state
between the request and callback,"* and that it *"SHOULD be used for
preventing cross-site request forgery."* The parameter's own name
(`state`, not `csrf_token`) reflects that the spec always contemplated it
carrying more than a bare anti-forgery nonce.

RFC 9700 — the IETF's 2025 OAuth 2.0 Security BCP, which updates and
tightens 6749's original guidance — is the more directly relevant, more
recent source, and it is explicit about the risk of doing that carelessly:

> "If `state` is used for carrying application state, and the integrity
> of its contents is a concern, clients MUST protect `state` against
> tampering and swapping. This can be achieved by binding the contents of
> state to the browser session and/or by signing/encrypting state
> values."

And, separately, directly on point for this feature's core risk:

> "Clients and authorization servers MUST NOT expose URLs that forward the
> user's browser to arbitrary URIs obtained from a query parameter."

That second sentence is precisely the failure mode this design's
validation strategy exists to prevent — cited here as the concrete,
current IETF authority behind the "must validate server-side, never trust
raw" requirement in this design's Goals, not just this document's own
assertion of good practice.

Source: [RFC 6749 §4.1.1](https://www.rfc-editor.org/rfc/rfc6749.html)
and [RFC 9700, OAuth 2.0 Security Best Current Practice](https://www.rfc-editor.org/rfc/rfc9700.html)
(both fetched 2026-09-01).

### Auth0's documented pattern for `state` + return-URL

Fetched 2026-09-01, from Auth0's own attack-protection documentation on
state parameters. Auth0's concrete recommendation for carrying a
post-login return URL alongside CSRF protection is a **split**, not a
single combined value:

> "Store the nonce locally, using it as the key to store all the other
> application state information such as the URL where the user intended
> to go."

I.e.: keep the random `state`/nonce value opaque and CSRF-only; store
everything else (including the return URL) server-side (or in a signed
cookie, for a client that has no server-side store), keyed by that nonce.
This is, structurally, **exactly** the shape `oidc_login_state` already
has today — `login_state_id` is the key, the row holds the round-tripped
secrets. Auth0's own stated best practice and this app's existing
architecture converge on the same answer independently.

Source: [State Parameters | Auth0](https://auth0.com/docs/secure/attack-protection/state-parameters)
(fetched 2026-09-01).

### Recommendation: extend `oidc_login_state`, do not encode the return path into `state`

Given the above, this design recommends storing the validated return path
as a new column on `oidc_login_state`, keyed by the existing
`login_state_id`, and leaving the OAuth `state`/`CsrfToken` value exactly
as it is today (opaque, `CsrfToken::new_random()`, compared verbatim).
Reasoning:

1. **Matches the documented industry pattern** (Auth0's nonce-as-key
   approach) rather than inventing something novel.
2. **Avoids RFC 9700's explicit tampering warning.** Packing the return
   path into `state` itself would mean `state` now carries
   security-relevant application data, which the BCP says MUST then be
   protected against tampering — via signing/encryption or session
   binding. That's real additional complexity (a signing key, a MAC
   scheme, a versioning story if the encoding ever changes) this design
   does not need to take on, because the alternative avoids the problem
   entirely: `oidc_login_state` rows are server-side-only and never leave
   this app's own database, so there's nothing for a client to tamper
   with in the first place.
3. **This app already has the mechanism.** `oidc_login_state` exists,
   is already single-use, already swept on a TTL, and is already exactly
   "the row for this one login attempt." Extending it is strictly less
   new surface area than a second, parallel round-trip mechanism.
4. **Avoids a real interop risk with an unstructured `state` string.**
   Different OIDC providers (this app's own dev-OIDC-server design
   targets Authentik; a production deployment could point at anything
   conformant) aren't guaranteed to round-trip an arbitrarily long or
   structured `state` value identically — URL-encoding differences, or a
   provider-side length cap, are the kind of thing that would only surface
   as a live bug against a specific IdP, not in a unit test. Keeping
   `state` exactly what `openidconnect`'s `CsrfToken::new_random()`
   already produces sidesteps that risk completely.

## Design

### Where the return path is captured: client-side, explicit, at render time

**Recommendation: client-side**, via a new query parameter,
`return_to`, appended to `/api/auth/login` by the frontend at the moment
each "Log in" link is rendered — computed from `usePathname()` +
`useSearchParams()` (Next.js's client-side router hooks), i.e. the path
and query string of the page the link is sitting on.

**Server-side capture (a `Referer` header) was considered and rejected.**
Reasoning:

- It is not reliably present. Browser privacy settings, a
  `Referrer-Policy` the frontend doesn't fully control end-to-end through
  the proxy, browser extensions, and some corporate/privacy proxies can
  all strip or truncate it — silently degrading to "always redirect to
  the default," with no way for this app to tell "no Referer was sent"
  apart from "the visitor really did navigate here directly."
- It would still need every bit of the same server-side validation this
  design already specifies for a query parameter, since a `Referer` header
  is exactly as attacker-influenceable in principle (a crafted link can't
  forge it directly, but nothing about trusting it removes the need for
  the same same-origin check) — so using it buys no simplification, only
  added unreliability.
- It's an implicit, "invisible" data path — harder to test (no explicit
  value to assert on in a component test, unlike an `href` attribute) and
  harder to reason about than an explicit, greppable `return_to` query
  parameter.

No hybrid is proposed either: a pure client-side, explicit parameter is
simpler to specify, test, and validate than "prefer the parameter, fall
back to Referer if absent" — and the fallback-to-static-default behaviour
already covers the "no parameter present" case just as well as a Referer
fallback would, without the reliability cost above.

#### Frontend: one new shared component, `LoginLink`

Every current "Log in" call site renders a bare
`<TextLink href="/api/auth/login" ...>`. `TextLink`
(`frontend/components/TextLink.tsx`) is deliberately **not** a Client
Component — its own doc comment explains it must stay server-renderable
because most of its call sites are Server Components. `usePathname()`/
`useSearchParams()` are Client-Component-only hooks, so they can't be
added to `TextLink` itself without breaking that constraint.

The clean fix, consistent with an existing pattern already in this
codebase: a new small Client Component, `frontend/components/LoginLink.tsx`,
that wraps `TextLink` and computes the `return_to` value itself:

```tsx
'use client';

import { usePathname, useSearchParams } from 'next/navigation';
import { TextLink } from './TextLink';

export function LoginLink({
  children,
  underline,
}: {
  children: React.ReactNode;
  underline?: 'hover' | 'always';
}) {
  const pathname = usePathname();
  const search = useSearchParams().toString();
  const returnTo = search ? `${pathname}?${search}` : pathname;
  const href = `/api/auth/login?return_to=${encodeURIComponent(returnTo)}`;
  return (
    <TextLink href={href} underline={underline}>
      {children}
    </TextLink>
  );
}
```

(Sketch — not implemented or type-checked against this repo's actual
`next`/`react` versions as part of this design pass.)

This is not a new pattern for this codebase: `AuthStatus.tsx`, itself a
Server Component, already embeds `LogoutButton` — a Client Component — as
a direct child, precisely because a small interactive leaf can live inside
a server-rendered tree. `LoginLink` follows the same shape, which is why
it works for the two Server Component call sites below (`AuthStatus.tsx`,
`TicketPanel.tsx`) exactly as well as the five Client Component ones.

Deliberately excluded: the URL **fragment** (`#...`). A fragment is never
sent to the server on any HTTP request, by design of the URL/HTTP specs —
`window.location.hash` isn't visible server-side at all, so there is no
mechanism (this design or any other) that could round-trip it through a
full-page OIDC redirect. No page in this app was identified as relying on
fragment-only state during this pass, but this wasn't exhaustively
audited — flagged under Open Questions.

#### The seven call sites

All read `href="/api/auth/login"` today (confirmed via
`grep -rn "auth/login" frontend/`) and would swap to `<LoginLink>`,
keeping each site's own wording unchanged:

| File | Context |
|---|---|
| `frontend/components/AuthStatus.tsx` | Nav-bar "Log in" link (Server Component) |
| `frontend/components/TicketPanel.tsx` | "Log in to attach a ticket to this journey" (Server Component) |
| `frontend/app/lines/CustomLineForm.tsx` | "Log in to create/edit a line" nudge |
| `frontend/components/DeleteLineButton.tsx` | "Log in to delete a line" nudge (inside a confirm modal) |
| `frontend/components/TicketEntryForm.tsx` | "Log in to save this ticket" nudge |
| `frontend/components/TrackTrainForm.tsx` | "Log in to track this train" nudge |
| `frontend/components/PinToggle.tsx` | "Log in to pin" nudge |

Representative diff, `AuthStatus.tsx`:

```diff
+import { LoginLink } from './LoginLink';
 import { Group, Text } from '@mantine/core';
-import { TextLink } from './TextLink';
 import { LogoutButton } from './LogoutButton';
 import type { SessionInfo } from '@/lib/types';

 export function AuthStatus({ session }: { session: SessionInfo }) {
   if (!session.authenticated) {
-    return <TextLink href="/api/auth/login">Log in</TextLink>;
+    return <LoginLink>Log in</LoginLink>;
   }
   ...
```

The other six call sites already import `TextLink` for other purposes too
(error text, cancel links), so they'd add a `LoginLink` import alongside,
not replace it wholesale — only the specific `href="/api/auth/login"`
`TextLink` usage at each site changes.

#### No proxy changes needed

`frontend/app/api/[...path]/route.ts`'s `proxy()` already builds its
backend request target as
`` `${API_BASE_URL}${resolveTargetPath(path)}${req.nextUrl.search}` `` —
the incoming query string, `return_to` included, is already forwarded
verbatim today. `resolveTargetPath` maps `/api/auth/login` to
`/public/auth/login`, so the backend receives
`GET /public/auth/login?return_to=%2Flines%2Fsome-line` unmodified.
Nothing in this proxy needs to change for this design.

### Surviving the OIDC round trip: a new column on `oidc_login_state`

New migration, timestamped after the most recent existing one
(`20260829090000_journey_ticket_tracking.sql`):

`crates/api/migrations/20260831090000_login_state_return_to.sql`:

```sql
-- Adds the (nullable, already-validated-before-write) return path a user
-- was on when they clicked "Log in", so /auth/callback can send them back
-- there instead of always to SSO_POST_LOGIN_REDIRECT_URL. See
-- docs/superpowers/specs/2026-08-31-dynamic-post-login-redirect-design.md.
-- NULL means "no return path captured, or the client-supplied one failed
-- validation at insert time" -- both fall back to the existing static
-- default, unchanged from before this column existed.
ALTER TABLE oidc_login_state ADD COLUMN return_to TEXT;
```

`crates/api/src/data/users.rs` changes:

- `LoginState` gains `pub return_to: Option<String>`.
- `insert_login_state` gains a `return_to: Option<&str>` parameter and
  binds it in the `INSERT`.
- `consume_login_state`'s `RETURNING` clause gains `return_to`.

`crates/api/src/routes/auth.rs` changes:

- `login()`: add
  ```rust
  #[derive(Deserialize)]
  struct LoginParams {
      return_to: Option<String>,
  }
  ```
  extracted via `Query<LoginParams>`, matching this file's existing
  `CallbackParams` shape/convention. Validate `params.return_to` through
  the shared validator (below) **before** calling `insert_login_state` —
  an invalid or malicious value is discarded at the door and never
  persisted at all; `insert_login_state` receives `None` for it, exactly
  as if the parameter had never been sent. A bad `return_to` must never
  fail the login attempt itself, only silently lose the "return to this
  page" behavior for that one attempt.
- `callback()`: after `consume_login_state` succeeds, re-validate
  `stored.return_to` through the **same** validator function again
  (defense in depth — cheap, and guards against any future code path that
  might write to that column without going through `login()`'s own
  validation) and compute the redirect target:
  ```rust
  let target = stored
      .return_to
      .as_deref()
      .and_then(auth::validate_return_to)
      .unwrap_or_else(|| app.config.sso_post_login_redirect_url.clone());
  let mut response = Redirect::temporary(&target).into_response();
  ```
  replacing today's unconditional
  `Redirect::temporary(&app.config.sso_post_login_redirect_url)`.

### Validation: a concrete same-origin/relative-path check

Proposed home: `crates/api/src/auth.rs`, beside `parse_cookie`/
`set_cookie_header` — general request/response helpers, not
OIDC-protocol-specific, matching that file's existing scope (`pub mod
oidc;` is the OIDC-specific submodule; this file is everything else).

```rust
/// Accepts only a same-origin, absolute-path, relative URL reference --
/// rejects anything that could make `Redirect::temporary` send a
/// just-authenticated, trusting browser somewhere off-site (open
/// redirect / post-login phishing). Called twice per login: once in
/// `routes::auth::login` (validate before persisting to
/// `oidc_login_state`) and once in `routes::auth::callback` (validate
/// again before using the persisted value) -- see that module for both
/// call sites.
pub fn validate_return_to(raw: &str) -> Option<String> {
    const MAX_LEN: usize = 2048;
    if raw.is_empty() || raw.len() > MAX_LEN {
        return None;
    }
    // Header-injection guard, and a defense against browsers that strip
    // or reinterpret stray control characters (tabs, NULs) during URL
    // normalization in ways this function shouldn't have to model.
    if raw.chars().any(|c| c.is_control()) {
        return None;
    }
    // Some browsers normalize a leading `/\` (or backslashes generally)
    // into `//` during navigation -- i.e. into a protocol-relative URL.
    // Rejecting `\` anywhere sidesteps needing to reason about exactly
    // which browsers do this and how.
    if raw.contains('\\') {
        return None;
    }
    // Must be an absolute-path reference: exactly one leading '/', not
    // '//...' (protocol-relative -- a browser resolves this to
    // `https://<attacker-controlled-host>/...`) and not a scheme
    // (`javascript:`, `https:`, etc., which `starts_with('/')` already
    // excludes on its own, but is worth stating as intent).
    if !raw.starts_with('/') || raw.starts_with("//") {
        return None;
    }
    // Authoritative check, not just belt-and-braces: resolve `raw`
    // against a fixed, arbitrary dummy origin using the same URL parser
    // this crate already depends on (`openidconnect::url`, i.e. the
    // `url` crate -- a WHATWG URL Standard implementation, the same
    // parsing algorithm real browsers use). If the parsed result's
    // scheme/host ever differ from the dummy origin, `raw` smuggled a
    // scheme or host past the prefix checks above through some
    // normalization quirk those checks didn't anticipate -- reject
    // rather than trust the prefix checks alone.
    let base = openidconnect::url::Url::parse("http://return-to.invalid").ok()?;
    let joined = base.join(raw).ok()?;
    if joined.scheme() != "http" || joined.host_str() != Some("return-to.invalid") {
        return None;
    }
    Some(raw.to_string())
}
```

Notes:

- Returns the **original** string, not a re-serialization of the parsed
  `Url` — `Url`'s own serialization can reorder or percent-re-encode a
  query string in ways a caller wouldn't expect from what they passed in.
  The parse-and-compare step exists purely to *authorize* `raw`, not to
  normalize it.
- Deliberately validates a *relative* reference rather than an absolute
  URL checked against an allow-listed host — see Research above (the
  `sso_redirect_url` history) for why this specific shape was chosen: it
  never needs a second "this app's real origin" value to compare against,
  so there's nothing here that can drift out of sync with
  `sso_redirect_url`/`sso_post_login_redirect_url`.
- `2048` bytes is an arbitrary, conservative ceiling, not derived from
  this app's actual longest real route — flagged under Open Questions.

### Fallback behavior

- **No `return_to` sent at all** (a bookmarked or hand-typed
  `GET /api/auth/login`): `params.return_to` is `None`,
  `insert_login_state` stores `NULL`, `callback`'s `unwrap_or_else` falls
  through to `sso_post_login_redirect_url` — byte-for-byte today's
  existing behavior, unchanged.
- **`return_to` present but fails validation** (a crafted link, or some
  future bug that lets a non-relative value through unexpectedly): same
  fallback, silently. A bad return path degrades to a known-safe
  destination; it must never surface as a user-visible error on an
  otherwise-successful sign-in.
- **Login-state row expired or already consumed** — an existing,
  unrelated failure mode (`BAD_REQUEST`, "login state expired or already
  used") — unaffected by this design either way.

### `sso_post_login_redirect_url`'s doc comment needs updating at implementation time

`crates/api/src/data/config.rs`'s current comment — *"One fixed target,
not a round-tripped 'return to this page' value — a v1 scope
simplification"* — becomes inaccurate the moment this ships. An
implementer should update it to describe the field's new role precisely:
the fallback destination when no return path was captured, or the one
captured failed validation — not, as today, the sole destination for
every successful login.

## Open questions / risks

- **Redirect-loop guard.** `validate_return_to` as specified would accept
  `/api/auth/login` itself, or `/api/auth/callback`, as a "valid" same-origin
  relative path — they are absolute-path references on this app's own
  origin, which is all the check above verifies. A `return_to` pointing
  back into the auth flow itself isn't a security hole (it can't escape
  this app's own origin), but it is a plausible dead-end/confusing-loop
  edge case (e.g. a user who somehow bookmarks a mid-flow URL). Recommend
  an explicit additional rule — reject any `return_to` whose path starts
  with `/api/auth/` — either inside `validate_return_to` itself or at its
  call site; not resolved further in this pass.
- **Fragment (`#...`) loss**, noted above under Design — not resolved, no
  exhaustive audit of whether any current page relies on hash-only state.
- **Not smoke-tested end-to-end.** No code was written for this design
  pass. Before treating this as final: confirm `return_to` actually
  survives real-world proxying (this app's own Next.js catch-all proxy,
  plus whatever reverse proxy an operator puts in front of the whole
  stack) end-to-end against a live IdP — mirroring the same "proposed, not
  verified" honesty
  `docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md` applies to
  its own unverified claims.
- **The 2048-byte cap on `return_to`** is an arbitrary, not
  research-derived, ceiling — untuned against this app's actual longest
  real route (a `/lines/[id]` page with several query filters, or
  `/train/[uid]/[date]`, are the plausible longest candidates, not
  measured here).
- **Whether to also cap or reject `return_to` values containing a query
  string wildly different in shape from what this app's own pages ever
  produce** (e.g. a very large number of repeated query keys) — not
  investigated; the length cap is the only bound proposed.
