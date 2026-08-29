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
