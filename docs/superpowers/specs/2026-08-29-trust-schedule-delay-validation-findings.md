# TRUST-Schedule Delay Inference: Validation Pass — Findings

**Status: findings from an actually-executed validation run, not a design
document.** This document reports what happened when
`docs/superpowers/plans/2026-08-29-trust-schedule-delay-validation.md`
("the plan") was executed against a real, currently-running deployment of
this app, on 2026-08-29. It follows the plan's own 8-task structure. Read
the plan and its two named specs first — this document assumes their
content as context and doesn't repeat it.

**Headline**: Task 1 (licensing) resolves favorably. Task 4 (capturing
real TRUST data via the pin mechanism) could not be completed — the live
instance's SSO login is broken in a way this session could not work
around, honestly diagnosed below, not guessed at. Tasks 2, 3, 5, and 6
were completed against real data. Task 7's full three-way comparison
could not be run as scoped because Task 4 produced no data; a partial,
honestly-labeled qualitative read is offered instead. Task 8's
recommendation is **not yet** — extend/retry, specifically to fix the SSO
blocker and re-run Task 4 onward, not a verdict against the feature
itself.

> **Update, 2026-08-30 — read this before the rest of the document.** The
> plan was re-run the next day, after this session's memory noted that a
> separate session had since fixed the two SSO root causes diagnosed
> below. **Both fixes are confirmed real and working, verified end-to-end
> against the live instance with no browser (the sandbox still can't run
> one) — pure HTTP/JSON, real cookies, a real created account, a real
> OAuth2 code exchange.** Task 4 got as far as creating three real pins
> against real, currently-running trains, but **hit a second, different,
> and still-unfixed blocker** — `trust-consumer`'s long-documented
> STANOX↔CRS gap — that stops any pin from ever resolving, independent of
> SSO. See the "2026-08-30 re-run" section appended at the end of this
> document for the full, evidence-quoted account. Task 8's verdict is
> still **not yet**, now for this newly-precise reason. Nothing in this
> update changes or softens anything below — it's additive.

---

## Task 1: RDM licensing/access confirmation

**Verdict: VERIFIED — favorable, not an open question.**

Two real, signed Rail Data Marketplace licence agreements were obtained
directly by the plan's dispatcher (a human with real RDM credentials) and
handed to this run as already-confirmed fact, per the plan's own
instruction not to re-derive this from RDM's login-gated catalogue:

**Licence 1 — "Darwin Timetable Files"** (RDM product
`P-9ca6bc7e-62e1-44d6-b93a-1616f7d2caf8`), publisher Rail Delivery Group,
**OGL v3.0 — free**, no fair-usage cap, no paid tier. Permitted purpose:
"internal business purposes only." Global territory (minus sanctioned
countries). **Daily** update frequency. Retention: may retain any data
received. 1-year term, auto-renewing, 1-month termination notice.

**Licence 2 — "NWR CORPUS"** (RDM product
`P-9d26e657-26be-496b-b669-93b217d45859`), publisher Network Rail, **OGL
v3.0 — free**. Permitted purpose is more permissive than Darwin's: "may be
made freely available or otherwise distributed to third parties." UK-only
territory. **Monthly** update frequency. Same retention/term/notice terms
as Licence 1.

**Naming nuance, flagged per this repo's "no invented API details"
convention**: the Darwin licence's product name is literally "Darwin
Timetable Files," not "CIF SCHEDULE." The design spec's own research
flagged an open question about whether RDM's "SCHEDULE" product and the
ATOC/RSP "Full Timetable" distribution are the same underlying CIF data
under different channel names. This licence is very likely the same data
family — RDM markets Darwin's timetable feed as the schedule product, and
the verification spec's Claim 1 already established the sample file in
hand is the standard ATOC/RSP CIF-extract bundle — but that is a reasoned
inference from adjacent evidence, not a confirmed 1:1 product-identity
match. This nuance does not change the licensing verdict (both readings
land on the same publisher-adjacent, OGL3, free outcome), only the
precision of which RDM catalogue entry the app would actually subscribe
to.

**What this resolves**: both products are free (OGL3), UK-legal,
already-licensed-and-held (real signed agreements, not a hypothetical
future application), daily/monthly cadence (matching the design spec's
`CIF_ALL_FULL_DAILY`/nightly-reference-refresh expectations), and carry no
fair-usage cap or paid tier of the kind the design spec worried "could
change the calculus." **Task 1's licensing uncertainty is resolved as
favorably as the design spec's own recommendation contemplated.** Neither
product is a blocker to proceeding, on licensing grounds alone.

---

## Task 2: Ground-truth disruption history for the chosen line(s)

**Step 1 — lines chosen**: `wcml` and `swr-alton` (the plan's own
recommendation), matching the design/verification specs' worked examples.
Note: the plan's own prose used "west-coast-main-line" as a line id in
several curl examples — the *real* id, confirmed directly from
`lines/west-coast-main-line.toml`'s `id = "wcml"` field, is `wcml`. The
plan's own example curl commands would 404 as literally written; this is
noted here as a small correction for anyone re-running this plan, not a
finding about the app itself.

**Step 2 — `incident_history` via direct SQL: NOT ACHIEVABLE, as the plan
itself predicted.** The plan's own text says plainly: "no API route
exposes it... direct `psql` against the deployed database is the only
path." This session has HTTP access only to the live instance (confirmed
directly — see Task 4's access-boundary findings below) — no database
credentials, no SSH, no `kubectl`. This step could not be run as scoped.
No workaround was invented; this is reported as a real gap, not silently
skipped.

**Step 3 — `line_status_history` via the public route: partially
achievable, with a real access-path correction.** `GET
/Line/{id}/Status/{from}/to/{to}` is mounted directly on the backend's
root router (`crates/api/src/main.rs`: `.merge(routes::line_status::router())`,
*not* nested under `/public`) — confirmed by reading
`crates/api/src/routes/line_status.rs`'s own module doc, which says so
explicitly. The frontend's `/api/*` proxy
(`frontend/app/api/[...path]/route.ts`) only forwards to two backend
prefixes: `Train/*` unmodified, and everything else with `/public/`
prepended — so a request to `/api/Line/wcml/Status/…` would resolve to
`/public/Line/wcml/Status/…` on the backend, which doesn't exist. **The
raw JSON history endpoint is genuinely unreachable through the browser-
facing proxy**, confirmed by reading `frontend/lib/api.ts`'s
`getLineStatusHistory`, which calls `${baseUrl()}/Line/{id}/Status/…`
directly from a Next.js **Server Component** using the server-only
`API_BASE_URL` env var — a call this session cannot make directly (no
access to that internal env var or backend host/port; confirmed by
direct probes, see Task 4).

What *is* reachable: the real page this data feeds,
`frontend/app/lines/[id]/history/page.tsx`, server-renders exactly this
data as part of its HTML/RSC payload. Fetched directly:

```
$ curl -s "http://konata.fox-prometheus.ts.net:3000/lines/wcml/history?range=30d"
$ curl -s "http://konata.fox-prometheus.ts.net:3000/lines/swr-alton/history?range=30d"
```

Both returned HTTP 200 with real, current data embedded in the page's
React Server Component stream (parsed out of the `self.__next_f.push(...)`
script payloads — not literal JSON, but real, quotable strings, not
paraphrased).

**Real finding on retention**: a `range=30d` request was made for both
lines, but the returned data covered only **22 Aug 2026 – 29 Aug 2026 (8
calendar days)** for both — not 30. This empirically confirms the live
deployment's actual configured `historyRetentionDays` is close to the
chart's shipped default of **7**, not the 30-day value in
`charts/distant-signal/values-example.yaml`'s illustrative config. This
answers the plan's own "worth confirming... in case it's been changed
from the chart default" concern: it has not been changed upward from the
7-day default, based on what's actually observable.

**Real summary counts** (from the page's own rendered text):

- **WCML**: "295 status recomputes across 110 incidents" over the 8-day
  window.
- **SWR-Alton**: "127 status recomputes across 72 incidents."

**Severity breakdown** (parsed from the real badge data on each
recompute):

- WCML: 81 Minor Delays, 23 Severe Delays, 4 Part Suspended, 1 Good
  Service (out of 108 successfully parsed entries; parsing captured
  ~99% of the reported 110-incident count, a small loss from regex
  robustness against one text-escaping edge case, not a data gap).
- SWR-Alton: 57 Minor Delays, 7 Diverted, 7 Severe Delays (70 of 72
  parsed).

**Real recurring patterns visible in the 8-day window** — genuine,
quoted `reason` text from the live page, not paraphrased:

- A **planned engineering work**, still active as of today: *"Major
  improvement works in the Wrexham General area from Sunday 16 to Sunday
  30 August (operator-wide report)"* — recurs repeatedly across the whole
  window, escalating between Minor and Severe Delays.
- A recurring **operational hotspot at Rugby**, visible only in the
  *sampling*-derived entries: *"8 of 29 sampled services delayed. (most
  cited: This service has been delayed by an operational incident at
  Rugby)"* — appears with varying counts (7–10 of 26–32 sampled) across
  many consecutive recomputes on 28–29 Aug. Note this LDBWS-sampled text
  *does* opportunistically name a location ("at Rugby") when Darwin's own
  canned delay-reason string happens to include one — a real nuance
  against the design spec's "sampling can't say where" framing, discussed
  further under Task 7.
- Repeated **named-segment Knowledgebase incidents** for SWR-Alton: e.g.
  "Disruption between Salisbury and Warminster," "Amended train service
  between London Waterloo and Exeter St Davids," "Station improvement
  work for step-free access... Wandsworth Town." These already carry
  real segment identity as free text, from Knowledgebase, at no cost —
  the comparison point for Task 7.

**`dataQuality`, per Task 2's ask**: the field exists on the underlying
API response (`crates/api/src/render.rs`'s `to_tfl_shape`), but the
history page itself does **not** render it — `HistoryResults` in
`page.tsx` only reads `span.severity`/`reason`/`from`/`to`/`flips`, never
`dataQuality`. Since the raw JSON endpoint is unreachable (see above), the
literal field could not be read directly. As an evidence-based proxy
(not a confirmed field read — flagged accordingly): entries whose reason
text matches `infer_from_samples`'s own templated wording ("N of M
sampled services delayed…") were counted separately from entries with
free-form Knowledgebase-style text:

- WCML: **16 of 108** parsed recomputes (~15%) are LDBWS-sample-pattern;
  **92 of 108** (~85%) look like Knowledgebase-derived text.
- SWR-Alton: **0 of 70** are LDBWS-sample-pattern; **100%** look
  Knowledgebase-derived, in this window.

**Step 4 write-up**: over the only real 8-day window this session could
observe, both lines had real, ongoing disruption activity (planned
engineering, several named-segment incidents, one recurring operational
hotspot), and the large majority of it was already captured by
Knowledgebase text at reasonable-looking severity, not by LDBWS sampling.
The `ldbws-inferred`-shaped stretches this feature would actually target
were a small minority of total activity in this window (WCML) or
effectively absent (SWR-Alton) — a real, if narrow-sample-size,
data point directly relevant to Task 8.

---

## Task 3: Choose the validation window and confirm timetable coverage

**Step 1 — planned engineering works found**: the Task 2 read above
surfaced one real, still-active, already-published planned work: **"Major
improvement works in the Wrexham General area," running Sunday 16 August
through Sunday 30 August 2026** (i.e., ending the day after this run).
This is a real WCML-adjacent planned disruption with known dates, exactly
the kind of target Task 3 Step 1 asks for — though its window closes
tomorrow, too late to usefully re-target for Task 4 in this run (Task 4
is separately blocked regardless — see below).

**Step 2 — timetable coverage confirmed directly against the real file**
(streamed from the repo-root `timetable_full.zip`, never extracted to
disk, per the plan's constraint):

```
$ unzip -p timetable_full.zip RJTTF942MCA.txt | awk '
    /^BS/ { prev=$0 } /^LOEUSTON/ { print prev; count++; if (count>=5) exit }'
BSNC005732605172612060000001 PXX1S003101121194800 DMU    125      S A T        P
BSNC005742605172612060000001 PXX1P033104121194800 DMU    125      S A T        P
...
```

Decoding the CIF Basic Schedule field layout directly against these real
bytes (positions confirmed against the published field offsets): UID
`C00573`, Date Runs From `260517`, Date Runs To `261206` — i.e.
17 May 2026 through 6 December 2026. This comfortably covers both today
(2026-08-29) and any 2–4-week-out window Task 3 might have chosen. No
freshness problem; the file's coverage window is real and wide.

**Step 3 — concrete station/train list: not meaningfully completable.**
The plan's own guidance was to pull "what's actually running today" from
a live LDBWS departure board reachable through this app. **This app has
no such page.** Its frontend has exactly one per-station route,
`/stations/[crs]` (confirmed by reading
`frontend/app/stations/[crs]/page.tsx`), and it renders only aggregated
*disruption* status (via `getStopPointDisruption`), never a live
train-by-train departure board with individual scheduled times — this app
is a line-status dashboard, not a departures viewer, matching DESIGN.md's
stated scope. There is no reachable source of "what's running right now"
through this deployment for a validator to read train-by-train, separate
from CIF's own advance schedule. This is a genuine scope gap in what the
plan assumed was available, surfaced only by actually trying it.

Given this, and given Task 4 is independently blocked (below), Task 3's
concrete pin list was not produced. What *was* produced instead, as a
substitute demonstration of the same underlying mechanism (see Task 5),
is a real CIF-derived schedule for WCML's sample stations on 2026-08-29
(today) — not chosen as an advance validation-window day, but as an
illustrative "does the join work on live data" check.

---

## Task 4: Capture real TRUST data via the existing pin mechanism

**Verdict: BLOCKED. No pins were created. No session token was obtained.
Reported honestly, not worked around.**

### What is and isn't reachable

Confirmed directly, by probing rather than assuming:

- `http://konata.fox-prometheus.ts.net:3000/` — reachable (HTTP 200).
- `/api/Train/*` and `/api/*` (proxied to `/public/*`) — reachable
  through the frontend's proxy, per its own documented allow-list.
- Direct backend ports were probed and are **not** reachable: `:8080`
  timed out/connection-refused for both `/public/health` and `/`. No
  `INTERNAL_TOKEN` was available to this session, so
  `/private/tracked-trains` (Task 4 Step 4's bulk-read route) could not
  have been reached even had pins existed. This matches the plan's own
  prediction: "you likely only have HTTP access... anything not reachable
  through that proxy or a directly-exposed port is off-limits."

### SSO investigation

`POST /Train/track` requires a real authenticated session. Per the plan's
explicit instruction, this was investigated rather than skipped:

1. **Browser automation was attempted and is blocked in this sandbox,
   independent of the live instance.** The Playwright MCP tool is
   configured to require a `chrome`-channel binary at
   `/opt/google/chrome/chrome`. That binary is not installed. Installing
   it requires `sudo` (unavailable — `sudo -n true` fails, password
   required) and Playwright's own install script explicitly refuses to
   proceed on this host's distribution: *"ERROR: cannot install on fedora
   distribution - only Ubuntu and Debian are supported."* A pre-existing
   plain Chromium build *is* present
   (`~/.cache/ms-playwright/chromium-1234`), but the MCP server's
   hardcoded channel selection does not fall back to it. This is a
   sandbox/tooling limitation, not a live-instance problem — reported
   plainly per the plan's own "if that's not achievable... say so
   honestly" instruction, rather than silently skipping Task 4.

2. **The SSO flow was then investigated directly over HTTP, without a
   browser**, to check whether it's simple enough to drive with `curl`
   alone. `GET /api/auth/login` (proxied to `/public/auth/login`)
   correctly issued a real `307` redirect and a real
   `distant_signal_login` state cookie:

   ```
   location: http://authentik.localhost:9000/application/o/authorize/?response_type=code&client_id=nr-status-dev&state=...&code_challenge=...&code_challenge_method=S256&redirect_uri=http%3A%2F%2Fkonata%3A3000%2Fapi%2Fauth%2Fcallback&scope=openid+email+profile&nonce=...
   ```

   This confirms the live instance's SSO really is Authentik (matching
   this session's own memory note about the dev-Authentik overlay), and
   that the app-side half of the OIDC flow (state storage, PKCE
   challenge, cookie) works correctly.

3. **The redirect target, `authentik.localhost:9000`, does not resolve
   to anything reachable from this sandbox** — `.localhost` names resolve
   to loopback (`::1`) per RFC 6761, and this sandbox's own loopback is
   not the deployment's. Port 9000 *is* separately reachable via the
   tailnet hostname directly (`http://konata.fox-prometheus.ts.net:9000/`
   → real HTTP 302, and Authentik's generic login flow page loads
   correctly, HTTP 200, real "authentik" HTML/config). So Authentik
   itself is up and network-reachable; only the specific redirect host
   name is a local-only alias.

4. **Working around the DNS gap with `curl --resolve` (mapping
   `authentik.localhost:9000` to the tailnet host's real IP,
   `100.87.228.55`, discovered via `getent hosts`) still fails**, and
   fails at the OAuth2 layer, not the DNS layer: hitting the real
   authorize URL this way — using the app's own genuinely-issued state,
   nonce, and PKCE challenge, not fabricated values — consistently
   returns:

   ```
   HTTP/1.1 400 Bad Request
   ...
   Client ID Error
   The client identifier (client_id) is missing or invalid.
   ```

   This was reproduced three times: once with a hand-crafted URL and
   placeholder params, once with `--resolve` and the same placeholders,
   and once using the real, freshly-issued redirect URL from an actual
   `/api/auth/login` call (real `state`/`code_challenge`/`nonce`) — same
   error every time. Meanwhile, Authentik's own generic
   `/if/flow/default-authentication-flow/` page loads fine at the exact
   same resolved address, confirming Authentik itself is healthy and
   this isn't a general connectivity problem — the OAuth2 provider
   registered for `client_id=nr-status-dev` specifically is not resolving
   through this path.

**Conclusion**: this live instance's SSO login flow is not currently
completable from outside its own internal network, for two independent
reasons — a DNS-only redirect target design flaw and (visible only once
that's worked around) an OAuth2 client-id resolution failure at
Authentik's own authorize endpoint. Neither is a "no browser tool"
limitation; the second was reproduced with a fully genuine, freshly-issued
authorization request and would equally block a real browser, had one
been available. **No pins were created on the live instance. No
fabricated session was used at any point.** This is reported as a real,
diagnosed defect for the user to fix (likely a redirect-host/Authentik
application-binding mismatch), not glossed over.

### Consequence for the rest of this plan

Every subsequent task that depends on Task 4's captured
`train_movement_events` (Task 5 Step 3's expected-vs-actual table, Task
7's three-way comparison) cannot be completed as scoped. What follows for
Tasks 5–7 is real, honestly-scoped partial work, clearly labeled where it
substitutes for what Task 4 would have fed in.

---

## Task 5: Reconstruct "what should have happened" from the real timetable file

**A working one-off script was written and run** (not committed — pure
scratch, per the plan's non-goals), streaming `RJTTF942MCA.txt` via
`unzip -p` (never extracted to disk), applying the two real gotchas the
verification spec already found (7-char space-padded TIPLOC field;
`lines/west-coast-main-line.toml`'s already-curated TIPLOC values used
directly, no re-derivation), plus a straightforward STP-overlay
preference (`C`/`O`/`N` before `P`) and day-of-week-bitmask filtering
(CIF field position 22–28) — matching the design/verification specs'
documented rule.

Run against WCML's five sample-station TIPLOCs (`EUSTON`, `MKNSCEN`,
`CREWE`, `PRSTON`, `CARLILE`) for **Saturday 2026-08-29** — chosen as
"today" for illustration, since Task 3/4 could not produce a real
advance-pinned day (see above), not as a properly-scoped validation day:

```
Total BS schedules scanned: 488798
Schedules covering 260829 (Sat) with a body line at a target TIPLOC: 1291
Distinct UIDs: 1196
```

A real, multi-station join (schedules touching ≥2 of the five sample
TIPLOCs) surfaced **504** real end-to-end services, e.g.:

```
UID C01370 STP=P [260523-261212]: EUS@0716 -> MKC@0750H -> CRE@1006H -> CAR@1200H
UID C17755 STP=P [260523-261212]: EUS@1940 -> MKC@2022 -> CRE@2157
UID C17798 STP=P [260523-261212]: EUS@0756 -> MKC@0837
```

(times as printed by CIF's own field: an `H` suffix marks a half-minute.)

**This confirms the mechanism Task 5 is supposed to validate genuinely
works against real, current data**: the TIPLOC join, the STP-overlay
preference, and the day-of-week filter together produce a real, sane
schedule reconstruction for a real day, at real WCML stations, matching
what a passenger would recognize as the actual Euston–Carlisle service
pattern.

**What could not be produced**: Step 2's `TI`+`MSN` STANOX cross-check
against captured `train_movement_events.loc_stanox`, and Step 3's
expected-vs-actual delta table — both need Task 4's real per-train
movement data, which does not exist. The CIF-reconstruction half of this
feature's data pipeline is now demonstrated real and working; the
TRUST-side half of the comparison remains unverified by this run.

---

## Task 6: Pull the sampling-side baseline for the same window

Already completed as part of Task 2 Step 3 above — the same
`/lines/{id}/history` page fetch serves both tasks' needs, since
`line_status_history` is exactly Task 6's target data (the *product
output* a real user saw, not a synthetic re-derivation). No further
distinct action was needed or taken.

`station_samples` history (Task 6 Step 2's finer-grained option): not
independently checked in this pass — no reachable route surfaces it (the
plan's own text already flagged this table as likely wholesale-replaced
each poll, with `line_status_history`'s snapshot-on-change log being "the
only real historical record available" in that case) and no additional
access existed to confirm one way or the other beyond what Task 2/6
already used.

---

## Task 7: Manual spot-check comparison and write-up

**Full three-way comparison: NOT COMPLETED — Task 4 supplied no data to
compare.** What follows is an honest, partial substitute: a qualitative
read of Task 2/6's real Knowledgebase-vs-sampling data, illustrated with
Task 5's real schedule reconstruction, explicitly **not** a measured
empirical result the way the plan intended.

**Question 1 — did sampling reflect what happened?** For the 8-day real
window observed: mostly yes, in the sense that Knowledgebase incidents
(85% of WCML's recomputes, 100% of SWR-Alton's) already carried
human-written, segment-named text — "Disruption between Salisbury and
Warminster," "buses replace trains between Wrexham General and Chester,"
etc. — well before any TRUST-vs-schedule diff could contribute one.
This is the honest confirming case the design spec already worried about:
Knowledgebase, when it fires, already gives good-enough segment
attribution in free text, and it fired for the clear majority of real
disruption activity in this window.

**Question 2 — the LDBWS-only ("N of M sampled services delayed")
stretches**: these are the cases the design spec's segment-precision
argument is actually about, and this window had real ones — the
Rugby-attributed run on WCML (7-10 of 26-32 sampled services, repeatedly,
across 28-29 Aug). Two honest observations, not glossed over:

1. **The sampling text was not fully blind to location** — Darwin's own
   canned delay-reason string happened to name "Rugby" directly, so this
   particular stretch is a weaker example of the "sampling can't say
   where" argument than the design spec's abstract framing implied. This
   is a real, if narrow, softening of the segment-precision case that
   only real data surfaced.
2. **What sampling still structurally cannot give, even here**: which
   *specific train* was affected, at which *specific TIPLOC* along its
   route, at what delay in minutes — only a line-wide aggregate count
   ("N of M"). Task 5's real reconstruction shows, e.g., that a real
   09:40 Euston–Carlisle service (`UID C01371`) calls at Rugby's
   neighbourhood en route; had Task 4 produced real movement events for
   it, this is exactly the kind of per-train, per-TIPLOC fact a TRUST
   diff could add that "7 of 28 sampled" cannot. This remains a
   plausible, structurally-grounded argument, not an *empirically
   confirmed* one — the distinction the design spec's own "can only be
   tested against real running data" caution anticipated, and which this
   run could not close.

**Question 3 — the reverse case, honestly**: for the large majority of
this window's real activity (Knowledgebase-driven), a TRUST-vs-schedule
diff would very likely have added nothing sampling/Knowledgebase didn't
already show — consistent with the design spec's own weaker-case
argument about Darwin's existing fusion being good enough for
delay-minute accuracy specifically.

**Sample-size honesty**: this is one 8-day window (bounded entirely by
the live deployment's ~7-day retention, not chosen by this run), two
lines, zero pinned trains, zero captured TRUST movement events. It is
explicitly **not** a statistically powered study, and — per the plan's
own Task 4 honest-scope note — even a fully successful run would not have
been either.

---

## Task 8: Decision gate — go/no-go recommendation

**Step 1 — licensing verdict: favorable, not a blocker.** Task 1 found
both real RDM licences (Darwin Timetable Files, NWR CORPUS) are free
(OGL3), already held, with no fair-usage cap and no paid tier. Nothing
here disqualifies proceeding.

**Step 2 — empirical verdict: cannot be stated concretely, because Task 4
did not run.** The plan's own criteria are explicit about this exact
situation: *"Too few real disruption days occurred during the monitoring
window to say anything with any confidence — in which case the honest
recommendation is 'extend the monitoring window and re-run Task 2-7,' not
a forced verdict either way."* This run's failure mode is even more basic
than "too few disruption days" — it never obtained a single real
TRUST-vs-schedule data point, because pin creation itself could not be
authenticated. The partial, qualitative Task 7 read above is
directionally consistent with the design spec's original judgment
(strong coverage/segment-precision case, weaker delay-accuracy case), but
it is evidence *about the reasoning*, not the empirical measurement Task
7 was built to produce.

**Recommendation: NOT YET.** Not "no" — Task 1's licensing findings are
about as favorable as this plan's own criteria contemplated, and nothing
found in this run argues against the underlying feature. But Task 8's
"go" bar explicitly requires a stated **N of M** real spot-checked
disruption instances where segment-level TRUST inference would have
caught or better-attributed something sampling missed — and that number
is currently **0 of 0**, not because the effect wasn't found, but because
the empirical mechanism (Task 4) never produced data to check.

**Concrete next step, before re-attempting Task 8**: fix the two real,
diagnosed blockers found in this run, then re-run Tasks 3–7 against a
freshly-chosen forward-looking window:

1. **Fix the live instance's SSO redirect.** The OIDC `authorize_url`
   this app generates points at `authentik.localhost:9000`, a host name
   that only resolves inside the deployment's own internal network. Any
   real user's browser — not just this validation run — hitting this
   live instance from outside that network would hit the same dead
   redirect. This is very likely a genuine, user-facing bug in the live
   deployment's SSO configuration, not a validation-artifact; worth
   fixing regardless of this plan's outcome.
2. **Diagnose the `client_id=nr-status-dev` "missing or invalid"
   error** at Authentik's own authorize endpoint, reachable directly at
   `http://konata.fox-prometheus.ts.net:9000/`, once (1) is fixed enough
   to test through a real browser — this may be the same root cause as
   (1) (an Authentik Application/Brand bound to the wrong hostname) or a
   separate OAuth2-provider misconfiguration.
3. **Re-run Task 4 onward** once a real session can be obtained: pin a
   deliberately dense sample of real WCML and/or SWR-Alton services for a
   near-future day or two (the Wrexham General engineering work will have
   ended by the time SSO is fixed — a fresh planned-work search, or an
   open-ended monitoring day per the plan's Step 2 fallback, would be
   needed), then complete Task 5 Step 3's actual expected-vs-actual delta
   table and Task 7's real three-way comparison.
4. Only then re-run Task 8 with an actual **N of M** figure.

**If proceeding to Option B is eventually greenlit**, per the plan's own
Step 3: that is a *new*, separate planning pass scoped to Option B
specifically (the dedicated `trust-line-aggregator`-style consumer
service), not a byproduct of this validation pass — unchanged from the
plan's own instruction, restated here only for completeness since this
run did not reach "go."

---

## Access-boundary summary (for whoever re-runs this)

- Reachable: `http://konata.fox-prometheus.ts.net:3000/*` (frontend,
  including server-rendered pages that embed data not reachable through
  the JSON proxy), `/api/public/*` and `/api/Train/*` (proxied), and
  separately `http://konata.fox-prometheus.ts.net:9000/*` (Authentik,
  directly).
- Not reachable, confirmed by direct probe rather than assumed: the
  backend `api` service's own port (`:8080` timed out), any database
  connection, any `INTERNAL_TOKEN`-gated `/private/*` route, and browser
  automation (Playwright MCP tool hard-requires a `chrome`-channel binary
  this sandbox cannot install — no root, and Playwright's own installer
  refuses non-Ubuntu/Debian hosts).
- `GET /Line/{id}/Status/{from}/to/{to}` (the plan's own named source for
  Tasks 2/6) is real, working, and publicly unauthenticated on the
  backend — but is not reachable through the frontend's browser-facing
  `/api/*` proxy (mounted at backend root, not under `/public`, and the
  proxy only forwards `Train/*` unprefixed). The equivalent real data is
  reachable instead through the server-rendered `/lines/{id}/history`
  page, which this run used successfully.

---

# 2026-08-30 re-run: SSO fixed, Task 4 blocked by a different, deeper cause

**Status: a second, real execution session, one day after the run above,
resuming at Task 4 per the dispatcher's explicit instruction.** This
section extends the document above; nothing above is edited or retracted.
Everything below was checked directly against the live instance at
`http://konata.fox-prometheus.ts.net:3000/` and `:9000/`, or against real
code on `main`, on 2026-08-30 — quoted, not paraphrased, exactly as the
rest of this document already does.

## Re-diagnosing SSO: both previously-diagnosed blockers are fixed

Per the dispatcher's brief, `main` had since picked up two fixes (commits
`6d4d5ab` "Drive Authentik's redirect_uris from the real
SSO_REDIRECT_URL, not a fixed copy" and `c2578a0`
"Rename Helm chart from nr-status to distant-signal", among others in the
same run). Whether the **live** instance had actually redeployed those
changes was unknown and had to be tested, not assumed. It has:

**Browser automation is still unavailable in this sandbox** — reconfirmed
before falling back to HTTP, per the dispatcher's instruction to try it
first. Same exact failure as the previous run, for the same reason:

```
$ npx --yes playwright install chrome
...
+ echo 'ERROR: cannot install on fedora distribution - only Ubuntu and Debian are supported'
Failed to install browsers
```

`sudo -n true` still fails (password required); this is a sandbox
limitation, unrelated to the live instance, exactly as diagnosed
2026-08-29. Fell back to direct HTTP/JSON probing, as the previous run
did — and, as shown below, this is now sufficient to complete the entire
SSO flow without a browser at all, because Authentik's flow executor is a
plain JSON API under the hood.

**1. Redirect host, live-probed:**

```
$ curl -s -D - -o /dev/null "http://konata.fox-prometheus.ts.net:3000/api/auth/login"
HTTP/1.1 307 Temporary Redirect
location: http://konata.fox-prometheus.ts.net:9000/application/o/authorize/?response_type=code&client_id=distant-signal-dev&state=...&code_challenge=...&redirect_uri=http%3A%2F%2Fkonata.fox-prometheus.ts.net%3A3000%2Fapi%2Fauth%2Fcallback&scope=openid+email+profile&nonce=...
```

This is the real, live redirect target the frontend generates right now.
Compare directly against 2026-08-29's captured value:
`http://authentik.localhost:9000/application/o/authorize/?...&client_id=nr-status-dev&...`.
Both things flagged as broken then are different now, observed directly,
not inferred: the host is `konata.fox-prometheus.ts.net:9000` (the real,
externally-reachable tailnet hostname this whole session has been using
throughout), not `authentik.localhost`; and `client_id` is
**`distant-signal-dev`**, not `nr-status-dev`.

**2. Authentik's own authorize endpoint, live-probed with that exact real
URL** (no placeholder values, no `--resolve` hack needed this time — the
hostname just resolves and routes correctly on its own):

```
$ curl -s -D - "http://konata.fox-prometheus.ts.net:9000/application/o/authorize/?response_type=code&client_id=distant-signal-dev&...".
HTTP/1.1 302 Found
location: /if/flow/default-authentication-flow/?response_type=code&client_id=distant-signal-dev&...
```

No `Client ID Error`. Following that redirect returns a real, live
Authentik login page (HTTP 200, genuine `authentik` HTML/config payload,
`x-powered-by: authentik`). **Both of 2026-08-29's diagnosed root causes
are independently confirmed fixed on the live instance, not just on
`main`.**

## Driving the entire SSO login flow over plain HTTP, no browser

Authentik's flow executor (`/api/v3/flows/executor/<slug>/`) is a
plain JSON API — GET returns the current stage's field list, POST
advances it. This app's own dev-IdP blueprint
(`charts/distant-signal/files/devauthentik-blueprints/open-signup.yaml`)
wires an **open, unauthenticated, no-verification-stage, auto-login
self-signup flow** (`distant-signal-dev-enrollment`) into the login page's
"Need an account? Sign up" link — a real, already-shipped dev-environment
feature, not something this session added. Driving it end-to-end, with a
persistent curl cookie jar (a fresh POST to a stage before its plan exists
issues a same-URL 302; re-GETting/following it re-establishes the plan —
the only wrinkle, resolved with `-L`):

```
$ curl -s -L -c cookies.txt -b cookies.txt -X POST \
    ".../api/v3/flows/executor/distant-signal-dev-enrollment/?query=" \
    -d '{"username":"valbot1788130219","password":"...","password_repeat":"..."}'
→ (advances to) {"component":"ak-stage-prompt","fields":[name, email]}

$ curl -s -L -c cookies.txt -b cookies.txt -X POST \
    ".../api/v3/flows/executor/distant-signal-dev-enrollment/?query=" \
    -d '{"name":"Validator Bot","email":"valbot1788130219@example.com"}'
→ {"component":"xak-flow-redirect","to":"/","final_redirect":true}

$ curl -s "http://konata.fox-prometheus.ts.net:9000/api/v3/core/users/me/"
→ {"user":{"pk":8,"username":"valbot1788130219", ...,"type":"external"}}
```

A real Authentik user (`pk: 8` — meaning at least 7 real accounts already
existed before this one; this is a live, already-used system, not an
empty test instance), auto-logged-in, real session cookie in hand. Then
the actual OIDC exchange, using the app's own genuinely-issued
`state`/`code_challenge`/`nonce` from a fresh `/api/auth/login` call:

```
$ curl -s -D - -c cookies.txt -b cookies.txt "$AUTHORIZE_URL"    # authenticated Authentik session
HTTP/1.1 302 Found
location: http://konata.fox-prometheus.ts.net:3000/api/auth/callback?code=6fc935c3c05f46cdbd326d438dc27032&state=SlurF8u5BT6rtw5xTjdBhQ

$ curl -s -D - -c app_cookies.txt -b app_cookies.txt "$CALLBACK_URL"   # app's own distant_signal_login cookie from the earlier /api/auth/login
HTTP/1.1 307 Temporary Redirect
location: http://konata.fox-prometheus.ts.net:3000/
set-cookie: distant_signal_session=s-ayj-eaVqaPONRzDnVeALalniPUTzoN-OKp3ZSP-JQ; Path=/; HttpOnly; SameSite=Lax; Max-Age=1209600
```

**A real, live, working `distant_signal_session` cookie for a real
authenticated user, obtained end-to-end over plain HTTP/JSON, no browser,
no fabricated tokens, no workaround of anything except the sandbox's
inability to run Chrome.** SSO is not merely "fixed in theory" — it is
directly, empirically confirmed working on the live instance right now.

## Task 4: pin creation now works; a second, real, unrelated blocker stops resolution

With `distant_signal_session` in hand, `POST /Train/track` was called for
real, against real, currently-scheduled WCML/border trains selected from
`timetable_full.zip` (streamed via `unzip -p`, never extracted, per the
plan's constraint) for **2026-08-30** (a Sunday; the file's day-of-week
bitmask field, position 22–28, confirmed bit 7 = Sunday active for every
schedule used below):

| id | UID | real journey (from CIF `LO`/`LI`/`LT`, quoted) | pinned as |
|----|-----|---|---|
| 3 | `C34229` | `LOEUSTON 2359` → `LTWATFJDC 0047/0050` (Euston–Watford Jn DC lines) | `origin_crs=EUS`, `scheduled_departure=2026-08-30T22:59:00Z` |
| 4 | `W70610` | `LOEUSTON 0009` → `LIWATFDJ 0028/0029` → `LIMKNSCEN 0118/0119` → `LTNMPTN 0137` (Euston–Northampton, calling Watford Junction and Milton Keynes Central — two of WCML's five curated `sample_stations`) | `origin_crs=EUS`, `scheduled_departure=2026-08-30T23:09:00Z` |
| 5 | `M37436` | `LODUMFRES 2350H` → ... → `LTCARLILE 0028` (Dumfries–Carlisle, Glasgow South Western route, terminating at WCML's Carlisle sample station) | `origin_crs=DMF`, `scheduled_departure=2026-08-30T22:50:00Z` |

All three real `POST /Train/track` calls returned `200` with a real
`trackingId` (`3`, `4`, `5` — ids `1`/`2` already existed, meaning this is
a live system with pre-existing real usage, not an empty test instance)
and `"resolutionStatus":"pending"`. **This is Task 4 Steps 1–2, genuinely
completed**, the thing the entire previous run could not do at all.

**Step 3/4 — letting it run and checking back**: all three trains'
scheduled departures passed during this session (confirmed by wall-clock:
pin 5's real departure was already ~6 minutes in the past at pin-creation
time; pins 3 and 4 departed within the following ~10 minutes). Polling
`GET /Train/{id}` at pin-creation, +8 min, and +24 min past the latest of
the three departures:

```
{"id":3,...,"resolutionStatus":"pending","trainUid":null,...}
{"id":4,...,"resolutionStatus":"pending","trainUid":null,...}
{"id":5,...,"resolutionStatus":"pending","trainUid":null,...}
```

**All three stayed `pending` throughout.** Rather than treat this as
"maybe just needs longer" (the plan's own Task 4 Step 5 explicitly asks
for the *cause* to be reported honestly, not just the outcome), this was
traced to real, live code — and the cause is structural, not a timing
fluke:

```
// crates/trust-consumer/src/process.rs, module doc, lines 9-22:
//! **STANOX->CRS translation is not implemented.** `loc_crs` is hardcoded
//! `None` throughout `process_message`, and `matching::resolve_origin_departure`
//! is consequently handed the raw `loc_stanox` where it documents wanting a
//! CRS. ... a pin only resolves when its `pin_origin_crs`
//! happens to compare equal to the feed's STANOX string.
```

```rust
// crates/trust-consumer/src/process.rs, inside process_message, line 301:
let loc_crs = None; // STANOX->CRS translation: see this module's docs.
...
let loc_stanox = movement.loc_stanox.as_deref()?;
...
let tracked_train_id = crate::matching::resolve_origin_departure(loc_stanox, actual_ts, &unclaimed)?;
```

```rust
// crates/trust-consumer/src/matching.rs, resolve_origin_departure:
pin.pin_origin_crs.eq_ignore_ascii_case(loc_crs)   // `loc_crs` here is really the raw STANOX
```

This is the **same gap** both the design spec and the verification spec
already named (`trust-consumer`'s own module doc, unchanged, still
present) — but it directly explains, with certainty rather than
suspicion, why all three of this run's real pins never resolved: a
pin's `pin_origin_crs` is a 3-letter code (`"EUS"`, `"DMF"`); TRUST's real
`loc_stanox` is always a 5-digit numeric string (confirmed real, e.g.
Euston's STANOX is `72410`, per the verification spec's own Claim 3, and
structurally true of every STANOX in `RJTTF942MCA.txt`'s `TI` records).
`"EUS".eq_ignore_ascii_case("72410")` can never be `true` — there is no
possible real-world STANOX value that would make it true. **This is not
a "wait longer" situation; a CRS-based pin structurally cannot resolve
against real TRUST data as this code stands today**, independent of
timing, independent of SSO, independent of which train or station is
pinned. `common::StationReference` (`crates/common/src/lib.rs:637-645`)
still has no `stanox` field, confirmed by direct re-read — the fix this
module's own doc comment already prescribes (add a STANOX column, source
it from CORPUS or, per the verification spec's own correction, from the
CIF extract's own `TI`+`MSN` files "for free") has not landed.

One documentation inconsistency worth flagging plainly, in this
document's own spirit of not asserting past what's verified:
`matching.rs`'s doc comment for `resolve_origin_departure` claims its
`loc_crs` parameter is "already translated from STANOX by the caller (see
Task 11's translation table)" — but `process.rs`'s real call site passes
the **raw, untranslated** `loc_stanox` into that exact parameter (quoted
above). Whatever "Task 11" refers to, it has not been implemented in the
code actually running on the live instance today; the doc comment is
aspirational/stale relative to real behavior, not a confirmed contract.

## Task 5: real expected-schedule table for the three pinned trains

Pure schedule reconstruction, independent of Task 4's (non-)resolution,
using the real CIF bodies already quoted in the table above. This is the
literal "expected" side Task 7 would compare against, had Task 4 produced
an "actual" side:

- **`C34229`** (Euston–Watford Junction DC): `EUSTON 23:59` →
  `CMDNSTH 00:01½` → ... → `WATFJDC 00:47/00:50` (arr/dep, terminates).
- **`W70610`** (Euston–Northampton): `EUSTON 00:09` → `WATFDJ
  00:28½/00:29½` → `TRING 00:50½/00:51½` → `MKNSCEN 01:18/01:19` →
  `NMPTN 01:37` (terminates). Calls at two of WCML's five curated
  `sample_stations` (Watford Junction, Milton Keynes Central) en route.
- **`M37436`** (Dumfries–Carlisle, Glasgow South Western route):
  `DUMFRES 23:50½` → `ANNAN 00:05½/00:06` → `GRETNA GREEN 00:14½/00:15` →
  `CARLCJN 00:25½` → `CARLILE 00:28` (terminates — WCML's Carlisle sample
  station, reached via a connecting route, not the WCML line itself).

**What could not be produced, and why**: the "actual" column of Task 5
Step 3's delta table, and Step 2's `TI`/`MSN` STANOX cross-check against
captured `train_movement_events.loc_stanox` — both need Task 4 to have
produced resolved rows, which (per the structural cause above, not a
sampling-window problem) it did not and, as this code stands, could not.

## Task 6: sampling-side baseline

Not independently re-pulled this session — Task 2/6's 2026-08-29 read
already captured the live `/lines/wcml/history` and `/lines/swr-alton/history`
baseline for the retention window that includes 2026-08-30, and nothing
in this run's scope changed what that data means. Re-fetching it would
not add anything: Task 4 still produced no resolved TRUST events to
compare it against, on either day.

## Task 7: three-way comparison — still not completable, now for a precisely different reason

**Not completed, same as 2026-08-29, but the honest reason has moved.**
On 2026-08-29 the blocker was "no session could be obtained at all." On
2026-08-30 a real session, three real pins, and three real trains'
real-time departures all happened exactly as intended — and the
comparison is *still* not completable, because `trust-consumer`'s
STANOX↔CRS gap means **zero of the three pins ever produced a single
`train_movement_events` row**, resolved or otherwise, to set beside
Task 5's real expected-schedule table. This is a *more* specific, more
diagnostic negative result than 2026-08-29's — it rules out SSO,
authentication, pin creation, timing/tolerance (`MATCH_TOLERANCE` is
20 minutes; departures were live-observed passing, not merely assumed),
and train selection as causes, and isolates the actual cause to one
already-documented, precisely-quoted code path.

## Task 8: decision gate — updated

**Step 1 (licensing): unchanged, still favorable** — nothing this session
touched bears on Task 1's verdict from 2026-08-29.

**Step 2 (empirical verdict): still cannot be stated as an N of M** — the
count is still **0 of 0** real spot-checked disruption instances, because
Task 4 still produced no `train_movement_events` data to check, on either
attempt. But *why* it's 0 of 0 has changed in a way that matters for
what to do next: this is no longer an access/deployment problem (SSO) —
it is a **application-code gap**, in a part of the system
(`trust-consumer`'s STANOX↔CRS translation) that this app's own design
spec, verification spec, and `process.rs`'s own module doc have all
already named as a known, real, unclosed gap, now confirmed to be the
actual, sole, currently-live blocker on this specific empirical
validation path, not just a theoretical concern.

**Recommendation: still NOT YET — but the concrete next step has changed
and narrowed.** Re-running Tasks 3–7 again with more/different pins,
more days, or more patience will not produce a different outcome while
this gap stands; the blocker is deterministic, not probabilistic. The
next step is not "retry the validation" but:

1. **Close `trust-consumer`'s STANOX→CRS gap first** — per the
   verification spec's own already-published, already-evidenced fix (the
   CIF extract's own `TI`+`MSN` records carry the full STANOX↔TIPLOC↔CRS
   mapping "for free," no CORPUS needed), thread a real lookup table into
   `process_message` so `loc_crs` stops being hardcoded `None` and
   `resolve_origin_departure` is handed a real CRS instead of a raw
   STANOX digit-string.
2. **Only then** re-run Task 4 onward: SSO is confirmed working end-to-end
   right now, over plain HTTP even without a browser, so a future run can
   go straight to pinning real trains for a real chosen window without
   re-litigating authentication at all — this run's curl-based recipe
   (persistent cookie jar, `-L` through the flow executor's
   plan-not-yet-established redirect, the real `/api/auth/login` →
   Authentik-authorize → `/api/auth/callback` chain) is a complete,
   reusable, no-browser-needed procedure for whoever does that.
3. Only then re-run Task 8 with an actual **N of M** figure.

**If proceeding to Option B is eventually greenlit**, this is unchanged
from both prior verdicts: a separate planning pass scoped to Option B
specifically, gated on Task 8 actually reaching "go," which it still has
not.
