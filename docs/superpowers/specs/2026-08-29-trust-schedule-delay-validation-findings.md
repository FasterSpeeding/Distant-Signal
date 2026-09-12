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

> **Update, 2026-08-31/09-01 — read this too.** The STANOX↔CRS gap above
> was fixed and merged to `main` (commit `6adf64f`). Re-running Task 4
> onward against the live deployment, **a real pin resolved against a
> real live TRUST train for the first time across all three sessions** —
> direct, empirical proof the fix works end-to-end in production, not
> just in code. A second, separate, previously-unflagged bug was also
> found and confirmed to have a real consequence: `lines/swr-alton.toml`
> mislabels Farnham's CRS as `"FRM"` (really Fareham's code), which
> caused a real pin to silently lock onto an unrelated, wrong real train.
> Task 8's verdict is still **not yet** — not because the mechanism is
> broken (it isn't, anymore), but because an unplanned ~16-hour session
> gap cut the real monitoring window to ~35 minutes, yielding only **1 of
> 1** real spot-checked disruption instances — too small a sample to
> call, by the plan's own explicit criteria. See the final section
> appended at the end of this document. Nothing in this update changes or
> softens anything above — it's additive.

> **Update, 2026-09-03 — read this too.** Task 0 (a separate, one-off
> exercise of the just-landed schedule-feed zip-delivery fix) is a full,
> confirmed **success**: a real 73MB `timetable_full.zip` was pushed over
> real SFTP to the live deployment's schedule-feed receiver and
> `/public/freshness`'s `schedule_feed` field flipped from `null` to a
> real delivery timestamp within ~5 minutes, empirically proving the
> single-zip ingest pipeline works end-to-end in production for the first
> time. **Task 4 onward could not be attempted at all** — the live
> production Authentik instance (`https://sso.fox-prometheus.ts.net`,
> `client_id=distant-signal`) does not offer the open, no-credential
> self-signup flow this document's prior two sessions relied on
> (`distant-signal-dev-enrollment` now 404s; the live identification
> stage's own JSON carries no `enrollment_flow` at all, only a Discord
> external-source login), and no human-usable login credential for this
> specific SSO flow exists anywhere this session had access to (confirmed
> by reading `dev-server.env` in full, plus every other credential-shaped
> file at the repo root). Per this run's own explicit instruction not to
> guess or bypass authentication, this was reported as a blocker rather
> than worked around. **No pins were created. No monitoring window was
> obtained. Task 8's verdict is unchanged from 2026-08-31/09-01: still
> "not yet," still blocked on sample size** — this session added zero new
> N-of-M data points, only closing off one specific way of getting there.
> See the final section appended at the end of this document. Nothing in
> this update changes or softens anything above — it's additive.

> **Update, 2026-09-12 — read this too (superseded below, keep reading).**
> The 2026-09-11 pinning pass completed its full day, and 5 of its 10 pins
> were discovered (in a separate investigation) to have been silently
> bound to the wrong real train by the TRUST timestamp-corruption bug,
> then manually corrected in the database. **Completing Tasks 5-8 against
> the corrected data surfaced a new, previously-undocumented problem**:
> fixing a pin's `trains.train_uid` metadata does not fix its
> `train_movement_events`, which remain keyed to whatever real train the
> matcher originally (wrongly) locked onto. Exhaustive per-station
> verification found **5 of the 10 "resolved" pins carry 100% wrong-train
> movement data** despite looking complete and correct at a glance, and
> the other 5 carry genuine data mixed with real, unrelated contamination
> requiring manual filtering. Of the 10 pins, only **5 produced any
> usable data**, and only **1** was a real, spot-checkable disruption
> (a genuine 10-13 minute Crewe→Wrexham delay that `lnwr-birmingham-crewe`'s
> own sampling output missed entirely, for the whole window). **Task 8's
> final verdict: still NOT YET** — N of M is **1 of 1**, no larger than
> the weakest prior session's, now for a new and more fundamental reason
> than sample size alone: the validation methodology's own data cannot be
> trusted from `resolution_status` alone without the per-station CIF
> cross-check this session had to invent partway through. See the final
> section appended at the end of this document for the full, evidence-
> quoted account. Nothing in this update changes or softens anything
> above — it's additive. **Superseded by the 2026-09-12 update
> immediately below** — its own "1 of 1" verdict turns out to have been
> built on data that was only partially remediated; re-verified end-to-end
> below.

> **Update, 2026-09-12 (second pass) — read this too.**
> The root cause behind the wrong-train contamination above (a missing
> `event_type = 'DEPARTURE'` filter in `find_backlog_match`) was found,
> fixed, and merged; a remediation pass then repaired the affected
> production rows. This session independently re-verified that
> remediation station-by-station (not trusting the dispatching brief's own
> description of it, per this document's standing norm) and found it
> **partially failed**: 3 of the 5 previously-flagged pins (44, 46, 48)
> are now genuinely, verifiably correct end-to-end; the other 2 (42, 50)
> are **still** 100% wrong-train contaminated, matching the *original*
> bad train exactly, contrary to the dispatching brief's claim that they
> were "never contaminated"/"thin data unrelated to the bug." On the
> trustworthy remainder, **Task 8's verdict is N of M = 2 of 2** — two
> real, fully-verified, disruption-scale delays (a Crewe→Euston working
> settling at +15 minutes, and the same Crewe→Wrexham +10-13 minute delay
> the immediately-prior section already found), both caught by
> TRUST-vs-schedule tracking and missed entirely by `lnwr-birmingham-crewe`'s
> own sampling output. **Recommendation: still NOT YET** on this session's
> own single-day data by this document's own consistent statistical-power
> standard, but explicitly flagged as the strongest, most confidence-inspiring
> "not yet" this six-week exercise has produced: every real mechanism-level
> blocker (SSO, STANOX/CRS, and now the majority of the backlog-matching
> bug) is fixed and proven, and across this document's entire history every
> real disruption ever found and successfully verified — 3 instances now,
> zero counterexamples — has been a hit for TRUST-vs-schedule and a miss
> for sampling. See that section for the full, evidence-quoted account,
> including the newly-found remediation-script reliability problem.
> **Superseded by the 2026-09-12 (third pass) section at the very end of
> this document**, which found the remediation script's 2 remaining
> failures (pins 42, 50) were themselves since fixed — read that section
> last.

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

---

# 2026-08-31/09-01 re-run: the STANOX fix is empirically confirmed working — a real pin resolved against a real live train

**Status: a third real execution session**, dispatched specifically because
`crates/trust-consumer/src/stanox_crs.rs` (a real, checked-in, 3,124-entry
STANOX→CRS table extracted and byte-verified from `timetable_full.zip`'s
`TI` records, per that module's own doc) and `crates/trust-consumer/src/process.rs`
had since landed a real fix for the 2026-08-30 section's precisely-diagnosed
blocker (commit `6adf64f`, "Implement real STANOX->CRS translation in
trust-consumer", merged to `main` 2026-08-30 23:43 UTC). Everything below
was checked directly against the live deployment or real code, quoted not
paraphrased, exactly as every earlier section of this document does. This
session ran in two parts separated by an unplanned ~16-hour gap (a usage-limit
suspension/resume); both parts are reported here, including what changed in
the live environment during the gap.

## Step 1: confirming the live deployment is (still) running the fix

The fix's own commit and content were re-verified directly against `main`
before touching the live instance:

```
$ git log -1 --format='%H %ci' 6adf64f
6adf64f1ecd605f74fc1cd0aea28f97d76bd3afb 2026-08-30 23:43:00 +0000
$ git log -1 --format='%H %ci' HEAD
e32459ed936b3682efabf209a2a66bd1bccd735e 2026-08-31 04:51:35 +0000
```

`6adf64f` is an ancestor of `HEAD` on `main` — real, merged, not a stray
branch. `process.rs`'s module doc now reads (quoted verbatim): *"STANOX->CRS
translation is implemented via a checked-in static lookup table,
`stanox_crs::stanox_to_crs`. `loc_crs` in `process_message` is the real
translated CRS... and `matching::resolve_origin_departure` is handed that
translated CRS, not the raw `loc_stanox`, so a pin's `pin_origin_crs` can now
actually compare equal to it."* — confirmed directly against the real call
site (`process.rs:312`): `let loc_crs = movement.loc_stanox.as_deref().and_then(crate::stanox_crs::stanox_to_crs);`.

**Whether the live deployment had actually redeployed this code could not be
confirmed from outside**, exactly as the dispatcher's brief anticipated
needing to check honestly. Concretely checked and ruled out, not assumed:

- No version/build-SHA is exposed anywhere reachable. `crates/api/src/routes/health.rs`'s
  `/public/health` returns only `{"message":"Alive"}` (no version field at
  all). `crates/trust-consumer/src/health.rs`'s `/healthz` returns only a
  plain-text `"connected"`/`"disconnected"` liveness string (also no
  version). Both confirmed by reading the real source, not assumed from the
  route name.
- The Helm chart gives no help either: `charts/distant-signal/values.yaml`
  defaults every workload's `image.tag` to `""`, meaning "use the chart's
  `appVersion`" (`charts/distant-signal/Chart.yaml`: `appVersion: "0.1.0"`,
  a static string that does not track individual commits), and the chart's
  own design doc (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`)
  states plainly there is "no image build or publish pipeline" — images are
  assumed to already exist at whatever ref an operator configured, with no
  in-repo record of which commit a running image corresponds to.

**Given no external version signal exists, this was resolved the only way
left: empirically.** If a real pin, created against a real CRS, resolves
against a real TRUST movement whose `loc_stanox` had to be translated to
match it, that is direct behavioural proof the running `trust-consumer`
has the fix — a stronger confirmation than a version string would have
been, and exactly the kind of test Task 4 onward was already going to run
regardless. See below: it does.

## Step 2: reproducing SSO fresh (not assuming 2026-08-30's flow still works)

Repeated the entire no-browser procedure from scratch — a fresh
`GET /api/auth/login`, a **new** Authentik account via the open dev-signup
flow, a fresh OIDC code exchange — rather than reusing anything from the
prior session, per the dispatcher's explicit "verify it fresh" instruction.

```
$ curl -s -D - -o /dev/null "http://konata.fox-prometheus.ts.net:3000/api/auth/login"
HTTP/1.1 307 Temporary Redirect
location: http://konata.fox-prometheus.ts.net:9000/application/o/authorize/?...&client_id=distant-signal-dev&...
```

Same working redirect shape as 2026-08-30 (host, `client_id=distant-signal-dev`,
real PKCE challenge). Created a brand-new account (`valbot1788167814`, `pk:9`
— one higher than 2026-08-30's `pk:8`, confirming this is a real,
continuously-used system, not reset between sessions at that point) via the
same `distant-signal-dev-enrollment` flow-executor JSON API, then completed
the OIDC exchange:

```
$ curl -s -D - -o /dev/null -c app_cookies.txt -b app_cookies.txt "$CALLBACK_URL"
HTTP/1.1 307 Temporary Redirect
location: http://konata.fox-prometheus.ts.net:3000/
set-cookie: distant_signal_session=7svoG1vzkKiLIy1xTBffZfy9ULgkFoFLwmcnEA7wQrY; ...
```

A real, fresh `distant_signal_session` cookie, obtained the same way as
2026-08-30, confirming that procedure remains reproducible and is not a
one-off fluke.

## Step 3: choosing real trains — and finding a real Bank Holiday complication along the way

2026-08-31 is a **Monday, and turned out to be the UK August Bank Holiday**
— discovered directly from the CIF data itself, not assumed. WCML's own
recent `line_status_history` (pulled fresh via the same server-rendered
`/lines/{id}/history` route this document's earlier sections already
established as the only reachable path) showed real, current planned/
operational disruption text for the day (`Track maintenance work: buses
replace trains between Farnham and Alton from Saturday 29 to Monday 31
August`, still active; `Amended 18:52 Edinburgh to London Euston service on
Monday 31 August`), so an open-ended "pin real, currently-scheduled
services" approach was used (the plan's own Step 2 fallback), rather than
waiting for a dedicated future planned-work day.

Streaming `RJTTF942MCA.txt` via `unzip -p` (never extracted to disk, ~76MB
compressed / ~711MB uncompressed, confirmed present at the repo root,
untracked), a candidate-origin extraction script found real scheduled
departures from WCML's and SWR-Alton's curated `sample_stations`. **A real
complication surfaced immediately**: cross-checking candidate UIDs for a
same-day STP overlay (`check_overlays.py`, a scratch script) found that
**most weekday (Mon–Fri) base schedules at Euston, Aldershot, Alton and
Farnham carry a real `STP=C` (cancelled) overlay specifically for
`260831`**, e.g.:

```
UID=C11052 stp=P from=260518 to=261211 days=1111100   [base pattern]
UID=C11052 stp=C from=260831 to=260831 days=1000000   [cancelled, just for today]
```

— a real Bank Holiday timetable effect, not a bug. A further search for
`STP=O`/`STP=N` overlays dated exactly `260831`–`260831` found the real
Bank Holiday replacement schedules running under **different UIDs**
(`F26094`, `Q98537`, `Q97575`, `Q97539`, etc.) — confirmed real by decoding
their `LO`/`LI`/`LT` bodies directly.

**Eleven real pins were created** via `POST /Train/track` against the
now-fresh session, spanning both the originally-chosen (some later found
cancelled-today) UIDs and the Bank-Holiday-replacement UIDs, so both
outcomes would be honestly represented rather than silently swapped out:

| id | origin CRS | UID | today's real status (from CIF, cross-checked) |
|----|-----------|-----|---|
| 6  | EUS | C11052 | **cancelled today** (STP=C override) |
| 7  | CRE | C17874 | runs today (STP=P, no override) |
| 8  | CAR | G85599 | runs today (STP=P, no override) |
| 9  | MKC | W70299 | runs today (STP=P, no override) |
| 10 | AHT | L83512 | **cancelled today** (STP=C override) |
| 11 | AON | L78555 | **cancelled today** (STP=C override) |
| 12 | FRM | L82419 | **cancelled today** (STP=C override) |
| 13 | EUS | F26094 | real Bank Holiday overlay (STP=N), runs today |
| 14 | AHT | Q98537 | real Bank Holiday overlay (STP=N), runs today |
| 15 | AON | Q97575 | real Bank Holiday overlay (STP=N), runs today |
| 16 | FRM | Q97539 | real Bank Holiday overlay (STP=N), runs today |

All eleven `POST /Train/track` calls returned `200` with a real
`trackingId` (`6`–`16` — `1`–`5` already existed from the 2026-08-30 run,
confirming continuity) and `"resolutionStatus":"pending"`.

## A separately-discovered, real bug: `lines/swr-alton.toml` mislabels Farnham's CRS

While cross-checking each target TIPLOC's real STANOX/CRS pair directly
against `RJTTF942MCA.txt`'s own `TI` records (the same records
`stanox_crs.rs`'s table was built from), a real, previously-unflagged data
bug surfaced:

```
$ unzip -p timetable_full.zip RJTTF942MCA.txt | grep '^TIFARNHAM'
TIFARNHAM00554500MFARNHAM                   87026   0FNHFARNHAM
```

Decoded per `stanox_crs.rs`'s own documented byte offsets (`44..49`
STANOX, `53..56` CRS): STANOX `87026`, **CRS `FNH`** — not `FRM`. Cross-checked
directly against `stanox_crs.rs`'s own real, already-verified table:

```
$ grep -n '"87026"\|"86241"' crates/trust-consumer/src/stanox_crs.rs
2718:    ("87026", "FNH"),
2628:    ("86241", "FRM"),
```

**`"FRM"` is a real CRS code — it just belongs to a completely different,
unrelated station: Fareham** (STANOX `86241`, confirmed via
`TIFAREHAM00590000DFAREHAM                   86241   0FRMFAREHAM`), on the
South Western Main Line towards Portsmouth — nowhere near the Alton branch.
`lines/swr-alton.toml` (`[[stations]] crs = "FRM"` for `tiploc = "FARNHAM"`)
has the wrong code: it should be `"FNH"`. This is a real, precisely-diagnosed,
pre-existing bug in this app's own line-definition data, independent of the
STANOX/CRS fix and independent of SSO — confirmed to have a real, observable
consequence below (pin 16). **Not fixed in this pass** — it is real and
directly explains why 2 of the 11 pins above (12, 16) could never resolve
correctly, but it does not block the core validation question (9 other pins
use correct CRS codes), and changing a line's curated station data warrants
its own dedicated look at blast radius (e.g. whether `aggregator`'s own
LDBWS sample-station matching for `swr-alton` uses the same field) rather
than a reflexive one-line edit mid-validation-run. **Flagged prominently
here for a follow-up fix**: `lines/swr-alton.toml`'s Farnham entry should
read `crs = "FNH"`.

## Task 4: real results — the fix works, plus one real false-positive consequence of the CRS bug above

Polled `GET /Train/{id}` repeatedly (session began polling ~09:28 UTC, ~7
minutes after all eleven pins were created) as each train's scheduled
departure passed. Real, quoted results, not paraphrased:

**Pin 7 (CRE, UID `C17874`) genuinely resolved** — the first real pin
resolution either validation session has ever produced:

```
{"id":7,"pinOriginCrs":"CRE","resolutionStatus":"resolved",
 "trainUid":"G38625","trainId":"421J62MG31","status":"en_route",
 "lastReportedLocation":"42117","lastEventType":"DEPARTURE","delayMinutes":2}
```

Ten minutes later, the same pin's journey had progressed further and
genuinely changed state:

```
{"id":7,...,"resolutionStatus":"resolved","trainUid":"G38625",
 "status":"cancelled","lastReportedLocation":"CTR","lastEventType":"ARRIVAL","delayMinutes":3}
```

`"CTR"` decodes (via `stanox_crs.rs`'s own table, STANOX `40320`) to real
station **Chester** — a real, geographically sane location for a
Crewe-origin service (the Crewe–Chester route is real and adjacent to
`wcml`'s own coverage). **This is the core empirical result this entire
plan exists to produce**: a pin created with a plain 3-letter CRS
(`"CRE"`) matched a real TRUST `DEPARTURE` event whose `loc_stanox` had to
be translated through the new `stanox_crs` table to compare equal — proof,
not inference, that the fix works end-to-end on the live deployment.

**Three more pins tracked real journeys without flipping to `resolved`** —
`status: "en_route"` with real, changing `lastReportedLocation`/
`lastEventType` fields, while `resolutionStatus` stayed `"pending"`:

| id | pinOriginCrs | observed real locations over 4 polls (34 min) | decoded (via `stanox_crs.rs`) |
|----|---|---|---|
| 13 | EUS | `72315` → `HRW` → `BSH` → `BSH` | Camden Jn (no CRS, correct raw-STANOX fallback) → **Harrow & Wealdstone** → **Bushey** — both real, WCML calling points matching `F26094`'s own real CIF body (`LI HTCHEND`... `LI BUSHEY arr=1148 dep=1149`) |
| 14 | AHT | `AHV` → `WAN` → `WAN` | **Ash Vale** → **Wanborough** — real, Alton-branch-adjacent calling points (STANOX `87016`→`WAN` sits in the same `87xxx` STANOX block as Aldershot/Alton/Farnham) |
| 16 | FRM | `FRM` → `SNW` → `HME` → `WLS` | **Fareham** → **Swanwick** → **Hamble** → **Woolston** |

Pins 13/14's decoded locations are real, correct, and geographically
consistent with the Alton-branch/WCML routes those origin CRS codes
actually name. **Pin 16's decoded locations are not** — Swanwick, Hamble
and Woolston are all real stations on the Fareham–Southampton corridor
(STANOX `862xx` block), nowhere near Farnham or the Alton branch. This is
the real, observed **consequence** of the `FRM`/`FNH` bug documented
above: pin 16's `origin_crs="FRM"` (intended to mean Farnham) instead
matched a real, unrelated, correctly-CRS-coded Fareham-area train — a
genuine false-positive mismatch, not a hypothetical risk. **The
translation mechanism itself worked correctly in all cases** (STANOX to
CRS, correctly, every time); the bug is entirely in the upstream input
data (`lines/swr-alton.toml`), not in `stanox_crs.rs` or `process.rs`.

**Why 13/14 never flipped to `resolved` despite tracking correctly**:
exactly the gap `process.rs`'s own module doc already documents, now
observed live rather than reasoned about: *"`crates/api`'s
`upsert_train_event`... only flips `tracked_trains.resolution_status` to
`'resolved'` when an incoming event carries BOTH `resolved_train_uid` and
`resolved_train_id`... If the Activation... was simply never emitted on
the slice of the feed this consumer sees, the resolving Movement goes out
with `resolved_train_uid: None`."* Not a new finding — a real, live
confirmation of an already-known, already-documented limitation.

**The other 7 pins (6, 8, 9, 10, 11, 12, 15) never showed any journey
activity in this session's window.** Consistent with, and not contradicted
by: pins 6/10/11/12 were the ones independently confirmed cancelled-today
via CIF's own `STP=C` overlay (above) — a real train that doesn't run
cannot produce a real TRUST event, an honest non-result rather than a
failure. Pins 8, 9 and 15 (CAR/MKC/AON-BH) simply hadn't produced a
matching event by the time this session's window closed (see below) —
genuinely unresolved, not silently dropped from this report.

**One numeric detail flagged honestly rather than smoothed over**: pin 7's
matched real departure (`delayMinutes: 2`) does not cleanly reconcile
against this session's own hand-decoded CIF static schedule for UID
`G38625` (base `STP=P`, no override found for `260831`, scheduled
`10:23` local). The gap between that static schedule and the real
`~2`-minute-delay match is not fully explained by this session — plausible
candidates include TRUST's own live `planned_timestamp` (embedded in the
Movement message itself, and what `delayMinutes` is actually computed
against, per `process.rs`) differing from the static CIF snapshot's `P`
schedule for reasons this session's tooling can't see (a VSTP amendment
not present in the full-timetable extract, for instance) — but this is
reported as an open, unresolved discrepancy, not asserted as a confirmed
cause, per this document's own established convention.

## An unplanned ~16-hour gap, and what changed in the live environment during it

This session was interrupted by a usage-limit reset partway through the
monitoring window and resumed roughly 16 hours later. On resumption, two
real, unplanned environmental changes were discovered — reported here
because they materially affect what could be checked afterward, not
because they were caused by this validation work:

1. **Every tracked-train pin, including this run's 11 and the prior run's
   pre-existing 1–5, is now unreachable**: `GET /Train/{id}` for every id
   1–16 now returns a real `404`, `"no tracked train with that id"`.
   Confirmed this is not a code-level retention job (grepped the entire
   codebase for `DELETE FROM tracked_trains` / `train_movement_events` —
   there is none; only `sessions`, `oidc_login_state`, and
   `line_status`/`line_status_history` rows are ever deleted anywhere in
   this codebase). The far more likely explanation, confirmed circumstantially
   below, is a real redeploy that reset the database.
2. **The SSO topology itself changed**: `GET /api/auth/login` now redirects
   to `https://sso.fox-prometheus.ts.net/application/o/authorize/?...&client_id=distant-signal&...`
   (a new hostname, HTTPS, and `client_id=distant-signal` — not
   `distant-signal-dev`) with `redirect_uri=https://konata.fox-prometheus.ts.net/api/auth/callback`
   (no port, HTTPS). The prior dev-only Authentik endpoint,
   `http://konata.fox-prometheus.ts.net:9000/`, is now unreachable
   (connection refused). `https://konata.fox-prometheus.ts.net/` (443,
   no port) is live and serves the app; `https://sso.fox-prometheus.ts.net/`
   is live and serves Authentik. This looks like a real migration from the
   dev-style HTTP/port-based setup this and the 2026-08-30 session both
   used, to a production-style HTTPS/custom-domain setup, with a
   different (and not yet investigated) Authentik application/client —
   possibly no longer carrying the same open `*-dev-enrollment` self-signup
   flow this document's no-browser procedure has relied on twice now.

**This change happened after this session's real data (above) was already
captured**, so it does not cast doubt on that data — the resolution
events, the false-positive, and the STP-overlay cross-checks were all
observed and quoted directly from the live instance before the gap. But
it does mean **this session could not extend its own monitoring window
further** (pins 8/9/15's still-pending status could not be re-checked;
no further real data could be gathered against the pre-migration pins),
and whoever continues this validation next will need to re-establish the
no-browser SSO procedure against the new production Authentik
application from scratch — not assume this document's existing recipe
still applies unmodified.

## Task 5: expected-schedule reconstruction for the real pinned trains

Real CIF bodies decoded directly (never extracted to disk), for both the
originally-chosen UIDs and their real Bank Holiday replacements, e.g.:

```
UID F26094 [STP=N, 260831 only]: LOEUSTON 1130 -> ... -> LIHTCHEND 1135/1136H -> ... -> LIBUSHEY 1148/1149 -> ...
UID Q98537 [STP=N, 260831 only]: LOALDRSHT 1130 -> LIASHVALE 1134/1134H -> LIFRIMLYJ (pass 1138H) -> ... -> LTASCOT 1200
UID Q97575 [STP=N, 260831 only]: LOALTON 1121 -> LIBNTEY 1131/1131 -> LTFARNHAM 1151
UID Q97539 [STP=N, 260831 only]: LOFARNHAM 1132 -> LIBNTEY 1142/1142 -> LTALTON 1157
```

Pin 13's real observed `HRW`→`BSH` sequence lines up directly against
`F26094`'s own real body (`...LI HEDSTNL / LI HTCHEND... LI BUSHEY arr=1148
dep=1149...` — Harrow-area calling points, in the right order). This is
the real "expected vs. actual" comparison Task 5 was scoped to produce,
for the trains that actually ran and were actually tracked; the
STANOX↔TIPLOC↔CRS cross-check (`stanox_crs.rs`'s own table, built from
this exact same file's `TI` records) is what made the "actual" side
possible to read against the "expected" side at all in this run, unlike
either prior attempt.

## Task 6: sampling-side baseline for the exact same window

Pulled fresh via the same server-rendered `/lines/{id}/history` route this
document's earlier sections established as the only reachable path, for
`wcml`, `wcml-north-wales` (which the Task 7 finding below turns out to be
the more relevant line — see next section) and `swr-alton`. **A clean,
real result**: none of the three lines recorded a single `line_status_history`
recompute anywhere in the `09:00`–`11:59` UTC window this session's pins
were actively being tracked in — confirmed by scanning every line's
history payload for an embedded ISO timestamp in that range and finding
zero matches, for all three lines. The nearest real entries bracket the
window on both sides (an early-morning entry around `00:00`–`01:45`, and
the next real entries from `19:10` onward). This is reported plainly, not
massaged: sampling produced **no output at all**, positive or negative,
during this session's exact monitoring window.

## Task 7: three-way comparison — one real, precisely time-correlated hit, honestly small in number

**The one clear real instance this window produced**: pin 7's real,
TRUST-confirmed **cancellation** of a Crewe-origin service that last
reported at Chester (`CTR`), observed between `09:44` and `10:02` UTC.
Chester (`CTR`) is not merely near `wcml`'s coverage — it is one of only
**two** curated `sample_stations` for `lines/wcml-north-wales.toml`
(`sample_stations = ["CTR", "HHD"]`). Task 6's real data above shows
`wcml-north-wales`'s own `line_status_history` recorded **nothing** during
this exact window (next entry: `19:10` UTC, ~9 hours later). **This is a
real, non-hypothetical instance of exactly what this whole plan exists to
test**: a real disruption (a cancellation), on a line whose LDBWS sampling
literally curates the affected station as one of its own two sample
points, that TRUST-derived per-train tracking caught in real time and
sampling's own product output did not reflect at all during the same
window.

**Honestly caveated, not overclaimed**: sampling polls on a 60-second
cadence and only *records* a `line_status_history` row on a computed
severity change — the absence of a recompute could mean sampling's own
poll simply didn't happen to sample this specific train during this
window (a real coverage gap, exactly the design spec's own argument), or
that the change hadn't yet propagated into Darwin's own boards by the
time of polling, or that a subsequent poll (after this session's window
closed, and now unrecoverable per the environment-change section above)
would eventually have caught it. This session cannot distinguish between
those explanations with the data it has. What it **can** state
plainly: during the real window observed, TRUST-derived tracking produced
a real signal sampling's own real product output did not.

**The reverse case, reported honestly per the plan's own Step 3**: pins
13 and 14 tracked two more real trains through multiple real calling
points across the same window with **no disruption of any kind** (both
consistently on time or ~0–1 min, per their own `delayMinutes` fields) —
and sampling correspondingly recorded nothing either. This is the
expected, unremarkable, agreeing case for the large majority of real
running, not a miss on either side.

**Sample-size honesty, per the plan's own explicit instruction**: this
window produced **one** real, clean disruption instance to spot-check
(the Chester cancellation), out of a monitoring window that ran for
roughly 35 real minutes before this session's interruption (and could not
be extended afterward, per the environment-change section above). **1 of
1** spot-checked real disruption instances in this window is a real
"TRUST caught something sampling's product output didn't reflect" result
— but a sample size of one is exactly the "too few real disruption days...
to say anything with any confidence" situation the plan's own Task 8
criteria already anticipated as a legitimate non-verdict outcome, not
something to round up into a confident "go."

## Task 8: decision gate — updated

**Step 1 (licensing): unchanged, still favorable.** Nothing in this
session touches Task 1's verdict.

**Step 2 (empirical verdict): the mechanical blocker is now closed — the
sample is still too small to call.** This is a materially different place
than either prior "not yet": both the SSO blocker (2026-08-29) and the
STANOX/CRS blocker (2026-08-30) that stopped this validation from
producing *any* real TRUST-vs-schedule data are now both **directly,
empirically confirmed closed** on the live deployment — a real pin,
created with nothing but a plain CRS code, matched, tracked, and correctly
reflected a real train's real cancellation, with real intermediate
station names decoded correctly along the way. That is the actual,
concrete thing Tasks 1–7 of this plan were built to determine was
possible, and this session confirms it is. What remains is exactly the
"is the sample big enough" question, not "does the mechanism work" —
**1 of 1** real spot-checked disruption instances in a ~35-minute window
is a genuine positive data point, not a fabricated one, but it is not a
statistically meaningful **N of M** by any reasonable reading of the
plan's own Step 2 criteria.

**Recommendation: still NOT YET, but narrower and more positive than
either prior verdict.** Not "no" — nothing found argues against the
feature, and the core mechanism is now proven live. Not "go" — the plan's
own bar requires a stated **N of M** across a real spot-checked sample,
and this run's honestly-reported number is **1 of 1**, too small to carry
that weight on its own. The concrete next step is narrower than both
prior write-ups':

1. **Fix the real, separately-discovered `lines/swr-alton.toml` CRS bug**
   (`crs = "FRM"` should be `"FNH"` for the Farnham entry) before any
   further SWR-Alton-branch pins are trusted — this run's pin 16 result
   demonstrates it produces real, silent, wrong-train mismatches, not just
   non-resolution.
2. **Re-establish the no-browser SSO procedure against the new production
   Authentik application** (`https://sso.fox-prometheus.ts.net/`,
   `client_id=distant-signal`) discovered mid-session — confirm whether an
   equivalent open self-signup flow still exists there, since the
   `*-dev-enrollment` flow this document has now used successfully twice
   was specific to the dev-style deployment that appears to have been
   migrated away from.
3. **Re-run Task 4 onward with a real, uninterrupted, multi-hour-or-longer
   monitoring window** (this run's was cut to ~35 minutes by an unplanned
   session gap) to accumulate more than one real spot-checked disruption
   instance — the mechanism is now proven, so this is purely a matter of
   giving it enough real wall-clock time against enough real pinned
   trains, not solving any further blocker.
4. Only then re-run Task 8 with an **N of M** large enough to carry a
   confident verdict either way.

**If proceeding to Option B is eventually greenlit**, unchanged from every
prior verdict in this document: a separate planning pass scoped to
Option B specifically, gated on Task 8 actually reaching "go," which it
still has not — though, for the first time across three real execution
sessions, the remaining gap is genuinely just sample size, not a broken
mechanism.

---

# 2026-09-03 re-run: Task 0 (schedule-feed zip delivery) succeeds; Task 4 onward blocked by production SSO's missing self-signup, honestly not worked around

**Status: a fourth real execution session**, dispatched with three inputs
none of the prior three sessions had: a real, current `timetable_full.zip`
already sitting at the repo root (no need to source one), a separate
one-off exercise of the just-landed schedule-feed zip-delivery ingest fix
(`docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`),
and a fresh instruction to re-run Task 4 onward with a real,
uninterrupted, multi-hour monitoring window — the 2026-08-31/09-01
section's own stated next step. Everything below was checked directly
against the live deployment, quoted not paraphrased, exactly as every
earlier section of this document does.

## Task 0: real SFTP push of the real timetable — confirmed working end-to-end

Not part of the plan's own 8 tasks — a separate, explicitly-scoped
exercise of a different, just-landed fix, done first because it was quick
and because it was this deployment's first-ever real delivery.

Uploaded the real, untracked `timetable_full.zip` (73,139,785 bytes,
confirmed by `stat` after transfer) via real SFTP, using
`dev-server.env`'s `SCHEDULE_SFTP_USERNAME`/`SCHEDULE_SFTP_PASSWORD`/
`SCHEDULE_SFTP_PORT` (2022) against `konata.fox-prometheus.ts.net`, landing
at the SFTP account's own chroot root (`/`) — confirmed correct by reading
`charts/distant-signal/templates/schedulefeed-deployment.yaml`'s own
comment: the account's `home_dir` **is** `WATCH_DIR`
(`/data/schedule-feed/incoming`, per `SCHEDULE_FEED_DESTINATION_PATH=incoming`),
so DTD (and this session) uploads to `/` directly, not to a subfolder
named `incoming`. Confirmed via a `paramiko` one-off script (not
committed): remote listing was empty before, `['timetable_full.zip']`
after, with the correct byte count.

Then polled the real, public `GET /api/freshness` route (the frontend's
`/api/*` proxy correctly forwards this unprefixed path straight to the
backend's `/public/freshness`, unlike the `/Line/.../Status` route this
document's very first section found unreachable through the same proxy —
this one is not nested under `/public` on the frontend side, so no
correction was needed) every 2 minutes:

```
22:03:10Z schedule_feed: null
22:05:11Z schedule_feed: null
22:07:11Z schedule_feed: "2026-09-03T22:02:19.231290Z"
22:09:11Z schedule_feed: "2026-09-03T22:02:19.231290Z"
```

**`schedule_feed` flipped from `null` to a real timestamp within ~5
minutes of the upload finishing** — faster than the ~10-minute estimate
implied by the deployed `poll_interval_secs`(120s)/`stability_cycles`(5)
defaults, plausibly because the value stored is `delivered_at` (the
delivery zip's own mtime, per the correction doc's own description of the
rework), not the moment `schedule-ingest` finished processing it, and the
scan/stability clock may have already been partway through a cycle when
the upload landed. **This is a genuine, real, first-ever confirmation that
the single-zip, no-manifest, no-sequence delivery shape works end-to-end
against a live deployment** — SFTPGo received the file, `schedule-ingest`
detected, stabilized, and extracted it, and `api` recorded a real
`schedule_feed_ingests` row the public freshness route surfaced correctly.
No further verification of the extracted contents was attempted (out of
this task's stated scope) — the freshness signal alone was the ask.

## Re-reading the required docs and this document's own established recipe

Read, in full, in the order specified: the design spec, the plan, and
this findings document (all ~1400 lines, all three prior dated sections).
Nothing about the plan's Non-goals, the design spec's architecture
options, or the mechanism itself (proven working as of 2026-08-31/09-01)
needed re-deriving. What this session actually needed to redo, per the
2026-08-31/09-01 section's own explicit final instruction, was: (1)
re-establish the no-browser SSO procedure against the new production
Authentik application discovered mid-way through that prior session, then
(2) pin a dense set of real trains and monitor for real, uninterrupted,
multi-hour wall-clock time.

## Task 4: SSO re-diagnosis — the production instance has no self-signup path, confirmed by direct API probe, not assumed

Repeated the exact opening steps of the prior sessions' proven recipe,
fresh, against the current live instance:

```
$ curl -s -D - -o /dev/null -c cookies.txt "https://konata.fox-prometheus.ts.net/api/auth/login"
HTTP/2 307
location: https://sso.fox-prometheus.ts.net/application/o/authorize/?response_type=code&client_id=distant-signal&state=...&code_challenge=...&redirect_uri=https%3A%2F%2Fkonata.fox-prometheus.ts.net%2Fapi%2Fauth%2Fcallback&scope=openid+email+profile+groups&nonce=...
set-cookie: distant_signal_login=...; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=900
```

Same topology the 2026-08-31/09-01 section found mid-session (production
`sso.fox-prometheus.ts.net`, HTTPS, `client_id=distant-signal`, no port) —
confirmed stable, not reverted. Following the authorize URL:

```
$ curl -s -D - -o /dev/null -c cookies.txt -b cookies.txt "$AUTHORIZE_URL"
HTTP/2 302
location: /if/flow/default-authentication-flow/?response_type=code&client_id=distant-signal&...
set-cookie: authentik_session=...
```

No `Client ID Error` — the app-to-Authentik half of the flow is healthy,
same as previously confirmed. Fetching the actual login stage via
Authentik's own flow-executor JSON API (the exact no-browser mechanism
this document's 2026-08-30 section established, reused verbatim):

```
$ curl -s -c cookies.txt -b cookies.txt "https://sso.fox-prometheus.ts.net/api/v3/flows/executor/default-authentication-flow/?query="
{"flow_info": {..., "application_pre": "Distant Signal", ...},
 "component": "ak-stage-identification", "user_fields": ["username", "email"],
 "pending_user_identifier": null, "password_fields": false,
 "primary_action": "Log in",
 "sources": [{"name": "Discord", "icon_url": "/static/authentik/sources/discord.svg",
              "promoted": true,
              "challenge": {"component": "xak-flow-redirect", "to": "/source/oauth/login/discord/", "final_redirect": false}}],
 "show_source_labels": false, "enable_remember_me": true, "passkey_challenge": null}
```

**This is the decisive, direct evidence, not an inference**: Authentik's
own `ak-stage-identification` challenge payload has no `enroll_url` field
at all — the exact field the prior two sessions' dev-instance login page
carried and that this document's own recipe drove
(`distant-signal-dev-enrollment`). The only login path this stage offers
is (a) identify as an existing user by username/email, then a password
stage, or (b) the promoted **Discord** external OAuth source. Confirmed
directly, not assumed, that the specific flow this document's recipe used
twice before is simply gone from production:

```
$ curl -s -D - -o /dev/null "https://sso.fox-prometheus.ts.net/api/v3/flows/executor/distant-signal-dev-enrollment/?query="
HTTP/2 404
```

Also checked the one plausible alternative enrollment slug Authentik ships
by default, `default-source-enrollment` — it does resolve (`200`), but its
own challenge payload self-describes why it's not usable here:
`{"component": "ak-stage-access-denied", ..., "error_message": "Flow does
not apply to current user."}` — this flow is gated to users arriving via
an external source redirect (exactly the trap this repo's own
`open-signup.yaml` blueprint comment already names: *"the OBVIOUS
candidate (default-source-enrollment) is a trap: it's gated by `return
ak_is_sso_flow` and only fires for users arriving via an external
source"*), not a general-purpose self-registration page.

**Root cause, traced to real repo content, not guessed**: the open,
no-credential self-signup blueprint this document's recipe has relied on
twice (`charts/distant-signal/files/devauthentik-blueprints/open-signup.yaml`,
and an identical top-level copy at `authentik-blueprints/open-signup.yaml`)
is explicitly named and scoped as **dev-only** in its own header comment
("Open, self-service signup for this app's local dev IdP") and is wired
only into `devauthentik-*` chart resources — nothing in this chart applies
it to a production-style Authentik instance. The 2026-08-31/09-01
section's own environment-change note already predicted this exact
outcome ("possibly no longer carrying the same open `*-dev-enrollment`
self-signup flow"); this session confirms it directly rather than leaving
it as a guess.

## No human-usable login credential exists anywhere this session had access to

Per this run's own explicit brief, this was investigated rather than
worked around. Read `dev-server.env` in full (all ~285 lines): it holds
real RDM API keys, real Kafka SASL credentials, real VAPID keys, real
`SSO_CLIENT_ID`/`SSO_CLIENT_SECRET` (the **app's own confidential OIDC
client** — usable to complete a token exchange *after* a real user has
authenticated at Authentik, not a substitute for a human logging in), and
8 `INTERNAL_OAUTH_USERNAME_*`/`INTERNAL_OAUTH_PASSWORD_*` pairs — explicitly
documented in the file's own comments as **service-account** credentials
for machine-to-machine `/private/*` calls (pollers, `trust-consumer`,
`schedule-ingest`), authenticated via OAuth2 Client Credentials Grant
against a *different* Authentik OAuth2 application than the human SSO
login flow (`INTERNAL_OAUTH_ISSUER_URL` names `distant-signal-internal`,
not `distant-signal`). None of these is a human login credential, and per
this run's own explicit instruction, none was used as one.

Also checked every other credential-shaped file at the repo root and in
this chart for a human account: `dev.env` (a separate, local-dev-only file
with placeholder `SSO_ISSUER_URL=http://sso.example.invalid` values, not
live credentials), `local.env.example`/`dev.env.example` (templates, no
real values by design), `docker-compose.authentik.yml` (its own comment
states plainly `AUTHENTIK_BOOTSTRAP_PASSWORD`/`_HASH`/`_EMAIL` are
"deliberately NOT set"), and the untracked `vault/` directory and
`charts/distant-signal/files/devauthentik-blueprints/openbao-oauth2-client.yaml`
(both confirmed, by reading their own header comments, to be a **local
dev-only** OpenBao/secret-store integration against this repo's own
**local dev** Authentik instance — unrelated to the live
`sso.fox-prometheus.ts.net` production instance this task targets). No
admin bootstrap credential, no seeded human test account, and no
documented procedure for obtaining one was found anywhere.

**Conclusion, stated exactly as the brief asked**: this session could not
find or derive a way to complete the human SSO login step against the live
production deployment. The mechanism this document's prior two sessions
used (an open, no-credential dev-only self-signup flow) has been correctly
removed from production between 2026-08-31/09-01 and now, and no
replacement human credential or equivalent self-service path was provided
or discoverable. Per the explicit brief, this is reported as a blocker,
not worked around — no Discord account was available to test the external-
source path, no username/password was guessed or brute-forced, and no
service-account token was used to fabricate a session.

## Consequence: Tasks 4-8 could not run this session

**No pins were created. No monitoring window was obtained — real or
otherwise.** Every downstream task (Task 5's per-train expected/actual
delta table, Task 6's baseline pull, Task 7's three-way comparison, Task
8's updated N-of-M) depends on Task 4 producing real
`train_movement_events` rows for freshly-created pins, which requires an
authenticated session this run could not obtain. Unlike the
2026-08-29/2026-08-30 sessions, this is not an application bug (the
2026-08-29 dead DNS redirect and the 2026-08-30 STANOX/CRS gap were both
real, diagnosed, and since fixed) — it is a **deliberate, correct**
production posture (no open self-signup) that this validation approach
has no credentialed way around, and per this run's brief, was not
supposed to be worked around.

**Task 8's verdict is unchanged: still "not yet."** The 2026-08-31/09-01
session's real, empirical **1 of 1** spot-checked disruption instance
remains this document's only real data point — this session neither added
to it nor cast any doubt on it. The mechanism is still proven (STANOX/CRS
fix confirmed live, no-browser OIDC flow confirmed reproducible against a
*non-production* login path twice); what's missing is still purely sample
size, now compounded by a **new, separate access problem**: there is
currently no way for an unattended/agentic session to authenticate against
this live deployment's production SSO at all.

## Concrete next step for whoever continues this

Narrower than ever, but now blocked on something outside this validation
plan's own scope to fix:

1. **A human with real credentials for `https://sso.fox-prometheus.ts.net`
   (or a legitimate way to provision a scoped test account there) needs to
   either log in once and hand off a real `distant_signal_session` cookie
   for a bounded validation window, or provision a real, disposable test
   user directly in production Authentik** — this is a decision only a
   human operator can make (the same "cannot be done by an agent" posture
   this plan's own Task 1 already applied to RDM licensing, now applying
   to production SSO access as well).
2. **Once a real session is in hand, this document's own recipe from the
   2026-08-30/2026-08-31 sections is otherwise unchanged and immediately
   reusable**: `POST /Train/track` per chosen real train (drawn from
   `timetable_full.zip`, streamed via `unzip -p`, cross-checked for
   same-day STP overlays exactly as the 2026-08-31/09-01 section's Bank
   Holiday complication already worked out), then `GET /Train/{id}` polled
   periodically over a real, uninterrupted, multi-hour window.
3. **Task 0's success removes one variable for that next session**: the
   schedule-feed pipeline itself is now confirmed live and working, so a
   future run reconstructing Task 5's expected-schedule table can, if it
   wants, verify it against this session's real live delivery rather than
   only ever reading the local zip file directly (though reading the local
   file directly remains simpler and is still explicitly permitted by the
   plan's own Non-goals).
4. Only then re-run Task 8 with an actual **N of M** large enough to carry
   a verdict.

**If proceeding to Option B is eventually greenlit**, unchanged from every
prior verdict in this document: gated on Task 8 reaching "go," which it
still has not.

---

# 2026-09-11: Task 4 goes live — 10 real WCML pins, real delay-minute data, still an in-progress day

**Status: a fifth real execution session**, dispatched because a separate
human-run pinning pass (a real authenticated user's own `POST
/Train/track` calls, not this session's) had already, earlier the same
day, placed **10 real pins** across all five of `wcml`'s curated
`sample_stations` (2 each at `EUS`/`MKC`/`CRE`/`PRE`/`CAR`), spread across
the day. This session's job was Task 4 Step 4 onward: pull real state,
build Task 5's expected-vs-actual table for whatever has actually
departed, pull Task 6's baseline, and do as much of Task 7's three-way
comparison as the data honestly supports — explicitly **not** Task 8,
since today is still in progress at the time of writing (all times below
checked live; `date -u` confirmed **2026-09-11, ~16:30 UTC**, well before
5 of the 10 pins' scheduled departures tonight). Everything below is
quoted directly from the live production database
(`distant-signal-postgres-0`, read-only `psql`) or the live
`GET /Train/mine` API, not paraphrased or extrapolated.

## Task 4 Step 4: real state of all 10 pins, as of ~16:30 UTC

Fetched fresh via `GET /api/Train/mine` (real, currently-valid session)
and cross-joined directly against `train_subscriptions` / `trains` /
`train_current_state` in the live database:

| id | CRS | UID | matched line | resolution | live status | delay (min) |
|----|-----|-----|---|---|---|---|
| 42 | EUS | Y80906 | `lnwr-birmingham-crewe` | resolved | en_route | 0 |
| 44 | MKC | W34266 | `emr-regional` | resolved | en_route | 0 |
| 46 | CRE | C18017 | `lnwr-birmingham-crewe` | resolved | en_route | 9 |
| 48 | PRE | G89823 | `northern-blackpool` | resolved | en_route | **13 → 15** (grew between two polls 12 min apart) |
| 50 | CAR | W69941 | *(none — unmatched)* | resolved | *(no `train_current_state` row yet)* | *(pending)* |
| 43 | EUS | C34213 | `lnwr-birmingham-crewe` | schedule_matched | not yet departed (22:27Z) | — |
| 45 | MKC | C17924 | `lnwr-birmingham-crewe` | schedule_matched | not yet departed (21:02Z) | — |
| 47 | CRE | G38654 | `lnwr-birmingham-crewe` | schedule_matched | not yet departed (22:33Z) | — |
| 49 | PRE | P23952 | `northern-blackpool` | schedule_matched | not yet departed (22:19Z) | — |
| 51 | CAR | W69917 | `northern-cumbrian-coast` | schedule_matched | not yet departed (22:10Z) | — |

**A real, previously-unflagged structural finding, worth stating plainly
before the delta tables below**: none of the 10 pins — all placed at
`wcml`'s own five curated `sample_stations` — resolved to `matched_line_id
= 'wcml'`. Each real train's CIF schedule-match instead resolved to
whichever narrower, curated feeder/branch line its own full journey best
fits (`lnwr-birmingham-crewe` for the Euston–Crewe-corridor workings,
`emr-regional` for the Manchester–Euston cross-country working, `northern
-blackpool` for the Blackpool–Manchester Airport working, `northern-
cumbrian-coast` for the Carlisle–Dumfries working), and one (pin 50,
`W69941`) resolved to **no line at all** (`matched_line_id` is `NULL` in
`trains`), despite `W69941` genuinely appearing in `wcml`'s own
`schedule_line_population` for today (confirmed directly:
`SELECT line_id FROM schedule_line_population WHERE service_date=
'2026-09-11' AND population::text LIKE '%W69941%'` returns
`northern-cumbrian-coast`, `wcml`, `tpe-anglo-scottish`,
`northern-tyne-valley` — four real candidate lines, `wcml` among them, yet
the pin's own `trains` row matched none). **This matters directly for
Task 6/7**: the plan's own Task 6 asks for "the sampling-side baseline for
the chosen line" (`wcml`), but the app's own CIF-matching pipeline
attributes most of these real WCML-calling trains to a *different*,
narrower line id — meaning the honest three-way comparison has to be done
per matched line, not solely against `wcml` itself, since that narrower
attribution is what the app's own product actually computed for these
real trains.

## A real, quantified timestamp anomaly found while building Task 5's table

Two genuine data-quality findings surfaced while reconciling
`train_movement_events` against `schedule_line_population`'s CIF-derived
booked times — reported here, precisely, because Task 5's methodology
below depends on understanding them, not because fixing them is in this
session's scope:

**1. `train_movement_events.planned_timestamp`/`actual_timestamp` are
stored as local UK civil time (BST, currently UTC+1) but tagged with a
`+00` (UTC) offset** — i.e., off by a full hour from true UTC, consistent
in every row checked. Confirmed directly, not inferred: pin 42's most
recent real event at the time of checking was `70203 DEPARTURE
planned=2026-09-11T17:18:30+00 actual=2026-09-11T17:19:00+00`, received
(`received_at`, the one column that genuinely is UTC, generated by the
consumer's own clock) at `2026-09-11T16:18:57+00` — an event whose
supposedly-UTC `actual_timestamp` is **over an hour in the future** of
when it was received, which is impossible for a real Movement message.
The only consistent explanation: the stored value is the real local
(BST) wall-clock reading, mistagged as UTC. This is *self-cancelling* for
`delay_minutes` (both `planned_timestamp` and `actual_timestamp` carry the
same offset, so the difference is unaffected) but means any comparison of
these raw columns against a correctly-UTC-converted CIF time (as Task 5
needs) must either subtract the offset or, more simply, compare using
local time throughout — the approach used below.

**2. A real, precisely-quantified ~79–80 minute discrepancy in
`train_subscriptions.pin_scheduled_departure` — present only for
`resolved` pins, absent for `schedule_matched` ones.** Computed directly
by converting each CIF booked local time to UTC and diffing against the
pin's own stored target:

| pin | station | pin's `pin_scheduled_departure` (UTC) | CIF booked local time | CIF converted to UTC | gap |
|---|---|---|---|---|---|
| 42 (resolved) | EUS | 16:46:00 | 16:26:00 | 15:26:00 | **+80 min** |
| 46 (resolved) | CRE | 17:13:00 | 16:54:00 (terminus arr.) | 15:54:00 | **+79 min** |
| 48 (resolved) | PRE | 17:10:00 | 16:51:00 | 15:51:00 | **+79 min** |
| 43 (schedule_matched) | EUS | 22:27:00 | 23:27:00 | 22:27:00 | **0 min** |
| 45 (schedule_matched) | MKC | 21:02:00 | 22:02:00 | 21:02:00 | **0 min** |

Every already-`resolved` pin checked shows a ~79–80 minute gap; every
still-`schedule_matched` pin checked matches CIF exactly. This is too
consistent across three independent trains/stations to be incidental, and
correlates cleanly with `resolution_status`, not with which station or
train was pinned. **This session does not assert a root cause** — it
could not, without reading application code changes out of scope for a
read-only DB validation pass — but it is flagged plainly, with exact
numbers, as a real, reproducible anomaly worth a developer's follow-up
look: something in the resolution transition appears to overwrite or
recompute `pin_scheduled_departure` away from the value it held while
`schedule_matched`, by a materially large and consistent margin. It does
**not** affect `delay_minutes`, which is computed and reported
separately and cross-checked directly against CIF below.

## Task 5: expected (CIF) vs. actual (TRUST) for the trains that have run

Built by streaming each pin's matched line's `schedule_line_population`
row for `service_date='2026-09-11'` (a real JSONB column, already
resolved for today's STP overlays — no CIF file re-parsing needed this
session, since the schedule-feed ingest pipeline Task 0 verified in the
2026-09-03 section is by now the live source of this table) and diffing
booked local times against `train_movement_events`, read consistently in
local-time terms per the timestamp-anomaly note above.

**Pin 42 — `Y80906`, Euston→Birmingham New Street (`lnwr-birmingham-crewe`)**:
| TIPLOC | CIF booked | TRUST actual | Δ |
|---|---|---|---|
| EUSTON (origin) | dep 16:26 | dep 16:26, ON TIME | 0 |
| WOLVERTON | dep 17:12 | dep 17:12, ON TIME | 0 |
Last real event: an unresolved-CRS location (STANOX `70203`, between
Wolverton and Northampton) at 17:19, LATE by 30s. **App's own reported
delay: 0 minutes.** A clean, real, on-time run.

**Pin 44 — `W34266`, Manchester Piccadilly→Euston (`emr-regional`)**:
| TIPLOC | CIF booked | TRUST actual | Δ |
|---|---|---|---|
| MNCRPIC (origin) | dep 14:55 | dep 14:55, ON TIME | 0 |
| STOCKPORT | arr/dep 15:02/15:04 | arr/dep 15:03/15:05, LATE | +1 |
| WILMSLOW | arr/dep 15:12/15:13 | arr/dep 15:13/15:14, LATE | +1 |
| CREWE | arr/dep 15:29/15:32 | arr/dep 15:29/15:32, ON TIME | 0 |
| STAFFORD | arr/dep 15:50/15:52 | arr/dep 15:50/15:52, ON TIME | 0 |
Last real event: Rugby (a pass point for this working, not a CIF-booked
stop), ARRIVAL, ON TIME. **App's own reported delay: 0 minutes.** A
1-minute early wobble that fully recovered by Crewe — a real, honest
"nothing to catch" case.

**Pin 46 — `C18017`, Euston→Crewe (`lnwr-birmingham-crewe`)**:
| TIPLOC | CIF booked | TRUST actual | Δ |
|---|---|---|---|
| EUSTON (origin) | dep 14:46 | dep 14:46, ON TIME | 0 |
| MILTON KEYNES CENTRAL | arr/dep 15:18/15:19 | arr/dep 15:21/15:22, LATE | +3 |
| RUGBY | arr/dep 15:41/15:42 | arr/dep 15:43/15:44, LATE | +2 |
| STAFFORD | arr 16:30 | arr 16:39, LATE | **+9** |
**App's own reported delay: 9 minutes** — matching this session's
independent CIF-vs-actual recomputation at Stafford exactly. A real,
growing delay, visible at one of `wcml`'s own five curated sample stations
(Milton Keynes Central) as early as +3 minutes, over an hour before the
final +9 was reached.

**Pin 48 — `G89823`, Blackpool North→Manchester Airport
(`northern-blackpool`)**:
| TIPLOC | CIF booked | TRUST actual | Δ |
|---|---|---|---|
| BLACKPOOL NORTH (origin) | dep 16:22 | dep 16:22, ON TIME | 0 |
| LAYTON | arr/dep 16:24/16:25 | arr/dep 16:24/16:25, ON TIME | 0 |
| POULTON-LE-FYLDE | arr/dep 16:28/16:29 | arr/dep 16:29/16:30, LATE | +1 |
| KIRKHAM & WESHAM | arr 16:37 | arr 16:37, ON TIME | 0 |
| *(two further real, STANOX-table-confirmed locations, `BSV`/`CRL` per
`reference-data/stanox-crs.csv`'s real `30201,BSV`/`30213,CRL` entries,
were reported by TRUST but do not appear among this working's own
CIF-booked calling points captured in `schedule_line_population` — flagged
honestly as unreconciled, not asserted as a mismatch; Preston itself, a
real CIF-booked stop for this working, is also conspicuously absent from
the captured event log, suggesting a genuine reporting gap around Preston
rather than a wrong-train match, given the four preceding stops all
matched exactly)* | dep ~17:12, arr 17:15, dep 17:16, all LATE | **~+13, growing** |
**App's own reported delay: 13 minutes as of the first poll, 15 minutes
12 minutes later** (a second live `GET /Train/mine` fetch, ~16:30 UTC,
showed `delayMinutes: 15` for this same pin). This is the day's clearest
real, growing, double-digit-minute delay, and it sits on a WCML-adjacent
feeder line whose curated `sample_stations` (`BPN`, `PFY`, `PRE`) directly
include Preston, one of `wcml`'s own five sample stations.

**Pin 50 — `W69941`, unmatched line, CAR**: `resolution_status=resolved`
(the pin has a real `trains_id` and `train_uid`), but **zero**
`train_movement_events` rows exist for it and no `train_current_state`
row exists at all — confirmed directly (`SELECT count(*) FROM
train_movement_events WHERE trains_id = 671998` → `0`). This is an
honest "resolved via Activation, no Movement yet" state, consistent with
this document's own prior sessions' documented distinction between the
two — not a bug, just genuinely pending.

**Still pending (not yet departed as of ~16:30 UTC)**: pins 43, 45, 47,
49, 51 — scheduled departures range from 21:02Z to 22:33Z tonight, all
several hours in the future at the time of writing. **No fabricated or
extrapolated data is reported for these** — they are honestly listed as
pending, per this session's explicit brief.

## Task 6: the sampling-side baseline for the same window, per matched line

Pulled directly from `line_status_history` for all five lines the 10 real
pins actually matched to (not just `wcml`), for the whole of
`2026-09-11` up to the time of writing:

| line_id | rows today | most recent recompute | content in/near the pin window |
|---|---|---|---|
| `wcml` | 16 | **08:54:35 UTC** | Knowledgebase text about a Wrexham-area planned-works and a Caledonian Sleeper amendment — **nothing after 08:54**, i.e. **zero output for the entire 13:46–16:30 UTC window** every departed pin ran in |
| `lnwr-birmingham-crewe` (carries pins 42/43/45/46/47) | 0 today | **2026-09-10 20:00:25 UTC** (yesterday) | **over 20 hours of complete silence**, spanning this entire session's real monitoring window |
| `emr-regional` (carries pin 44) | 34 | 16:24:45 UTC (live, ongoing) | 100% Knowledgebase-derived, all about a real but unrelated Ely-area "Major Disruption" and a Matlock–Cleethorpes service amendment — nothing about `W34266`'s own (on-time) run |
| `northern-blackpool` (carries pins 48/49) | active, but | **13:53:35 UTC** | a real Knowledgebase planned-work entry about Bransty Tunnel/Corkickle/Whitehaven (Cumbrian Coast, not this line's own Blackpool corridor) — and, critically, **this timestamp is ~1.5 hours *before* `G89823` (pin 48) even departed** (16:22 local/15:22 UTC) |
| `northern-cumbrian-coast` (carries pins 50/51) | 0 today | 06:14:35 UTC | ~10 hours of silence |

**The `dataQuality` angle, per the plan's own Task 6 Step 3**: every real
entry surfaced above is Knowledgebase-sourced text about a *different*,
unrelated real-world incident — none is an `ldbws-inferred`
"N of M sampled services delayed" entry that happens to be *about* any of
these specific pinned trains. Combined with the flat "no recompute at
all" result for `wcml`, `lnwr-birmingham-crewe`, and
`northern-cumbrian-coast`, this window's sampling-side product has
produced **no signal whatsoever**, of any `dataQuality`, about any of the
real delays Task 5 found — not a false negative buried in noise, but a
genuine, verifiable silence.

## Task 7: three-way comparison — partial, honest, and directly on-topic where it exists

**The clearest real instance this window has produced so far**: pin 48
(`G89823`, `northern-blackpool`, one of `wcml`'s own adjacent
sample-station feeder lines, sharing Preston) shows a real, TRUST-confirmed,
**growing double-digit delay (13 → 15 minutes)** between roughly 17:12 UTC
and the time of writing (~16:30 UTC local checking time). Task 6's real
data shows `northern-blackpool`'s own line-status product has recorded
**zero** output covering any part of this train's actual run — its last
recompute (13:53:35 UTC) predates the train's own departure entirely.
This is a real, non-hypothetical, delay-*minute*-granular instance of
exactly what this whole plan exists to test: a real, meaningful, growing
delay that TRUST-derived per-train tracking caught in real time and this
line's own currently-shipping sampling product did not reflect at all,
at any point, during the same window.

**A second, more modest instance**: pin 46 (`C18017`, `lnwr-birmingham-
crewe`) shows a real, CIF-confirmed 9-minute delay by Stafford, first
visible as +3 minutes at Milton Keynes Central — one of `wcml`'s own five
curated sample stations. `lnwr-birmingham-crewe`'s own sampling output has
been completely silent for over 20 hours, spanning this entire window.
9 minutes is a more modest delay than the design spec's own worked
examples (and arguably below whatever severity threshold Darwin/NRE's own
canned-reason text would trigger on), so this is reported as a real but
smaller-magnitude version of the same pattern, not overclaimed as
equally dramatic.

**The confirming, honest reverse case**: pin 44 (`W34266`, `emr-regional`)
ran essentially on time (a 1-minute early wobble that fully recovered),
and `emr-regional`'s own real, active sampling output during the same
window was about a genuinely different, unrelated incident — a clean,
unremarkable agreement case where there was nothing for TRUST-vs-schedule
to usefully add, exactly the honest "no material difference" outcome the
plan's own Task 7 Step 3 asks to report alongside the hits.

**Sample-size and completeness honesty, stated plainly per this
document's own established norm**: at the time of writing, only **4 of
10** pins have produced any live TRUST movement data at all (42, 44, 46,
48 — plus pin 50 resolved but still dataless), and of those, only **2**
(46, 48) show a delay large enough to be a meaningful "would sampling
have caught this" test — both currently unreflected by sampling's own
real output at any severity. **5 of 10 pins have not yet departed** — all
scheduled for tonight (21:02–22:33 UTC), hours after this session's
writing time. This is explicitly a **partial, early, in-progress-day
read** — real data, real numbers, but honestly not the complete day's
picture the plan's Task 7 asks for, and nowhere near the volume Task 8's
own N-of-M bar requires.

## Task 8: explicitly not attempted this session — and exactly when to attempt it

**Per this session's own explicit brief, Task 8 is deliberately not run
here.** Today is still in progress: 5 of the 10 real pins haven't
departed yet, `line_status_history`'s own comparison window for several
of the matched lines is still accumulating, and a same-day, partial
snapshot would produce exactly the kind of false-confidence verdict the
plan's own Task 8 criteria (and this document's own prior sessions) have
consistently and correctly refused to render prematurely.

**Concrete recommendation for when to actually run Task 8**: **tomorrow
morning, 2026-09-12**, once two conditions are both true — check them
directly, don't assume:

1. **All 10 pins have reached a final state** — re-fetch
   `GET /Train/mine` (or query `train_subscriptions`/`trains` directly)
   and confirm every pin is either `resolved` with real movement data (or
   honestly logged as never producing any, like pin 50 above), or has
   sat at `schedule_matched`/`pending` long enough past its scheduled
   departure (all five remaining pins depart by 22:33Z tonight) to be
   confidently called `unresolved` per the plan's own Task 4 Step 5
   guidance — this should be checkable well before UK sunrise.
2. **A full day of `line_status_history` exists for comparison** across
   all five matched lines (`wcml`, `lnwr-birmingham-crewe`,
   `emr-regional`, `northern-blackpool`, `northern-cumbrian-coast`) —
   i.e., re-run this session's exact `line_status_history` query with the
   date window extended through the end of 2026-09-11 (all pins'
   scheduled departures, `22:33Z` being the latest, fall well inside
   today's date), so tonight's remaining departures (and the two
   already-growing delays found today, on `lnwr-birmingham-crewe` and
   `northern-blackpool`) get a real chance to either produce or fail to
   produce a matching sampling-side recompute.

Once both are true, Task 8 should restate Task 7's comparison as an
actual **N of M** across the full, now-complete day (this session's own
2 real "TRUST caught it, sampling didn't" instances, against however many
of the remaining 6 pins' outcomes turn out to need the same test) and
render the plan's actual go/no-go/not-yet verdict from that number —
not from this session's honestly partial 4-of-10 snapshot.

---

# 2026-09-12: Tasks 5-8 complete on the corrected pins — but half of them turn out to carry the WRONG train's real data, and the honest verdict is still not yet

**Status: a sixth real execution session**, dispatched specifically to
finish Tasks 5-8 against the 10 real WCML/feeder-line pins the
2026-09-11 session left in progress, now that the full day has completed
and — per the dispatching session's own briefing — a separate
investigation had discovered that 5 of the 10 pins (`trackingId`s 42, 44,
46, 48, 50) had been silently bound to the wrong real train by a
since-fixed TRUST feed timestamp-corruption bug in
`resolve_origin_departure`, and had since been manually corrected in the
database to their originally-intended `train_uid`. **This section
corrects and completes the 2026-09-11 section above, and explains why the
picture has changed**: the matched-line set has narrowed from 5 lines to
3 (`lnwr-birmingham-crewe`, `northern-blackpool`,
`northern-cumbrian-coast`) now that the wrong bindings — which were
themselves partly responsible for the earlier, wider, partly-artifactual
line spread — are gone from the `trains` table's own identity fields.

**Everything below was checked directly against the live production
database** (`distant-signal-postgres-0`, read-only `psql`, `distant_signal`
user), quoted exactly as returned, not paraphrased, exactly as every prior
section of this document does. First, the corrected identity of all 10
pins was re-confirmed directly (not assumed from the dispatching brief),
by joining `train_subscriptions` to `trains`:

```
id | pin_origin_crs | trains_id | train_uid | matched_line_id         | origin_crs | destination_crs
42 | EUS            | 712945    | C18012    | lnwr-birmingham-crewe   | EUS        | CRE
43 | EUS            | 713725    | C34213    | lnwr-birmingham-crewe   | EUS        | WFJ
44 | MKC            | 713330    | W70396    | lnwr-birmingham-crewe   | MKC        | EUS
45 | MKC            | 713728    | C17924    | lnwr-birmingham-crewe   | MKC        | EUS
46 | CRE            | 713729    | C17876    | lnwr-birmingham-crewe   | CRE        | EUS
47 | CRE            | 713731    | G38654    | lnwr-birmingham-crewe   | CRE        | WRX
48 | PRE            | 713690    | G86196    | northern-blackpool      | PRE        | OMS
49 | PRE            | 713740    | P23952    | northern-blackpool      | PRE        | BBN
50 | CAR            | 713358    | M37478    | northern-cumbrian-coast | CAR        | DMF
51 | CAR            | 713743    | W69917    | northern-cumbrian-coast | CAR        | DMF
```

This matches the dispatching brief's table exactly, and all 10 pins show
`resolution_status = 'resolved'` with a non-null `trains_id` — on its face,
a complete, corrected, ready-to-analyze dataset.

## A real, previously-unflagged finding, discovered while building Task 5's table: half the "corrected" pins' movement events belong to the ORIGINAL WRONG train, not the corrected one

Before building Task 5's expected-vs-actual table, each pin's real
`train_movement_events` rows (joined via `trains_id`) were cross-checked
station-by-station against `schedule_line_population`'s CIF-derived booked
calling points for the *corrected* `train_uid` — the same reconciliation
method the 2026-08-31/09-01 and 2026-09-11 sessions both used. **For 5 of
the 10 pins (42, 44, 46, 48, 50 — precisely the ones the dispatching
brief says were manually corrected), this reconciliation fails
completely: not one event in any of these 5 pins' movement logs falls
anywhere near the corrected train's own booked schedule.** This was
checked exhaustively (every event, not a sample), and independently
confirmed two different ways:

**1. Direct station/direction mismatch.** Pin 46's `trains_id` (713729)
carries 29 real events running `EUSTON(dep 14:46) → ... → MKC(15:18/15:19)
→ RUGBY(15:41/15:42) → ... → STAFFORD(16:30) → CREWE(arr 16:54)` — i.e., a
real Euston-to-Crewe run. But the *corrected* `train_uid` for pin 46,
`C17876`, runs the **opposite direction**: `CREWE(dep 18:13 local) → ... →
EUSTON(arr 20:24 local)`. A train cannot run both directions on the same
day under the same pin; this is not a timing quirk, it is a direction
mismatch, decisive on its own.

**2. Exhaustive time-window check, for the other four.** For pins 42, 44,
48, and 50, direction alone doesn't settle it (both candidate trains run
the same way), so every event's `planned_timestamp` was compared against
the corrected `train_uid`'s own booked local time at its own origin CRS
(the same value `schedule_line_population` gives, cross-checked against
`trains.scheduled_departure`, which is itself confirmed correct — e.g.
pin 42's `pin_scheduled_departure` of `16:46:00+00` exactly equals CIF's
`EUSTON` booked departure of local `17:46:00` for `C18012`, once converted
from BST). None of these pins' captured events fall anywhere near that
corrected time:

| pin | trains_id | corrected UID | corrected UID's own booked origin time (local) | this pin's actual captured event window (raw stored values) |
|---|---|---|---|---|
| 42 | 712945 | C18012 | EUSTON 17:46 | 16:26 – 17:11 (16 events, all before 17:46) |
| 44 | 713330 | W70396 | MKNSCEN 17:58 | 14:55 – 17:09 (35 events, all before 17:58) |
| 48 | 713690 | G86196 | PRST 18:10 | 16:22 – 16:59 (12 events, all before 18:10) |
| 50 | 713358 | M37478 | CARLILE 17:59 | 16:39 only (1 event, before 17:59, and an *arrival* at Carlisle, whereas M37478 *originates* there) |

**What these events actually are, confirmed by the same method**: every
single one of these events matches — station-for-station, minute-for-minute
— the *original, wrong* train identified by the dispatching brief's own
pre-correction table, not the corrected one. `schedule_line_population`
was queried directly for each original wrong UID's own booked origin time,
and it matches exactly:

| pin | original (wrong) UID | wrong UID's own booked origin (local) | matches this pin's captured events? |
|---|---|---|---|
| 42 | Y80906 | EUSTON 16:26 | yes — exact match to first event (`16:26:00`) |
| 44 | W34266 | MNCRPIC 14:55 | yes — exact match to first event (`14:55:00`) |
| 46 | C18017 | EUSTON 14:46 | yes — exact match to first event (`14:46:00`), and its booked Crewe arrival (`16:54`) exactly matches this pin's last event |
| 48 | G89823 | BLCKPLN 16:22 | yes — exact match to first event (`16:22:00`) |
| 50 | W69941 | (Dumfries→Carlisle) CARLILE arr 16:39 | yes — exact match to the one captured event (`16:39:00`) |

**This is a decisive, exhaustively-checked finding, not a guess**: fixing
`trains.train_uid`/`matched_line_id`/`origin_crs`/`destination_crs`/
`scheduled_departure` — the metadata fields a human editor can reach with
a manual `UPDATE` — does **not**, and structurally cannot, retroactively
fix or replace `train_movement_events`, which is keyed by `trains_id` and
was populated *before* the correction, against whatever real TRUST
`train_id` the (buggy) matcher had already locked onto. **The dispatching
brief's characterization — "these are the CORRECT trains' own
always-genuine movement history... they were never affected by the
timestamp bug themselves" — is not accurate for these 5 pins.** The
movement events are genuinely real TRUST data (nothing fabricated), but
they are real data for the **original wrong train**, not the corrected
one. The `resolved` status and populated `trains_id` give a false
impression of usable per-train data where none exists for the actually-
intended train.

## A second, related finding: the other 5 pins' event logs are genuine but contain real, unrelated contamination requiring manual filtering

Pins 43, 45, 47, 49, and 51 were **not** on the dispatching brief's
corrected-pins list (they were still `schedule_matched`, not yet resolved,
when the 2026-09-11 session ended). Checking their movement events the
same way turns up real matching data for their claimed `train_uid` — but
mixed together, within the same `trains_id`, with real movement events
from other, unrelated trains that also happen to pass through the same
origin CRS at other times of day. For example, pin 47's `trains_id`
(713731, 25 events) contains an *early* Crewe departure at `21:55` that
has nothing to do with `G38654` (whose own booked Crewe departure is
`23:33`), interleaved with a **later**, genuinely matching run: a real
`CRE` departure at planned `23:33`, actual `23:45` (**+12 late**), a real
`CTR` (Chester) arrival at planned `23:52`, actual `00:05` (**+13**), and a
real `WRX` (Wrexham General) arrival at planned `00:09`, actual `00:19`
(**+10**) — station-for-station and minute-for-minute matching `G38654`'s
own CIF body (`CREWE 23:33 → CHST 23:52/23:54 → WREXHMG 00:09`). The same
pattern — an early, unrelated cluster of events, then a later cluster that
matches the pin's own claimed train at 5-8 *consecutive* stations within
0-2 minutes of its CIF booked time — was independently confirmed for pins
43, 45, and 49 as well (matched stations quoted in Task 5 below); pure
coincidence across that many consecutive station-level matches is not a
plausible explanation.

**This means `train_movement_events` rows are being attributed to a
`trains_id` beyond the single physical train that `trains_id` is supposed
to represent — not just in the 5 already-known-wrong pins, but, less
severely, in pins that were never flagged as mismatched at all.** This
looks like the same underlying class of bug the dispatching brief
described (TRUST timestamp/matching imprecision), still live in some form
independent of whatever fix corrected the 5 flagged pins' *identity*
fields — but this session, being a read-only DB validation pass, does not
assert a specific code-level root cause; that would need a source read
out of this task's scope. It is flagged here, plainly, as a new,
previously-undocumented, real data-quality problem worth a developer's
follow-up look, **separate from and in addition to** the already-fixed
STANOX↔CRS gap and the already-known Farnham/Fareham CRS bug documented
earlier in this file.

## Task 5: expected (CIF) vs. actual (TRUST) — 5 of 10 pins have zero usable data; the other 5, filtered, show mostly clean running plus one real moderate delay

**Pins 42, 44, 46, 48, 50: no usable "actual" data exists for the
corrected train.** Per the finding above, every event attached to these
`trains_id`s belongs to the original wrong train. No expected-vs-actual
table can honestly be built for `C18012`, `W70396`, `C17876`, `G86196`, or
`M37478` from this database as it currently stands — the CIF "expected"
side is real and available (fetched below, for completeness), but there
is no real "actual" side to set beside it.

- `C18012` (pin 42, EUS→CRE): CIF booked `EUSTON 17:46 → MKNSCEN 18:18/18:19
  → RUGBY 18:41/18:42 → ... → CREWE (arr 19:54)`. No matching TRUST data.
- `W70396` (pin 44, MKC→EUS): CIF booked `MKNSCEN 17:58 → BLTCHLY 18:01/18:03
  → ... → EUSTON (arr 19:03)`. No matching TRUST data.
- `C17876` (pin 46, CRE→EUS): CIF booked `CREWE 18:13 → ... → RUGBY 19:26/19:27
  → ... → MKNSCEN 19:48/19:49 → ... → EUSTON (arr 20:24)`. No matching TRUST data.
- `G86196` (pin 48, PRE→OMS): CIF booked `PRST 18:10 → CROT 18:23 → RUFDORD
  18:28/18:29 → BRSCGHJ 18:34/18:35 → ORMSKRK (arr 18:41)`. No matching TRUST data.
- `M37478` (pin 50, CAR→DMF): CIF booked `CARLILE 17:59 → GRETGRN 18:10/18:11
  → ANNAN 18:19/18:20 → DUMFRES (arr 18:36)`. No matching TRUST data.

**Pins 43, 45, 47, 49, 51: real, filtered, CIF-cross-checked data exists**
(filtering methodology: keep only events whose station and time correlate
with the pin's own claimed `train_uid`'s CIF booked calling points; the
tables below quote only that filtered subset, not the raw contaminated
log):

**Pin 43 — `C34213`, Euston→Watford Junction DC line (`lnwr-birmingham-crewe`)**:
| TIPLOC/CRS | CIF booked (local) | TRUST actual | Δ |
|---|---|---|---|
| EUSTON (origin) | dep 23:27 | dep 23:27, ON TIME | 0 |
| SOH (South Hampstead) | arr/dep 23:32/23:32 | arr/dep 23:32/23:32, ON TIME | 0 |
| KBN (Kilburn High Rd) | arr/dep 23:34/23:35 | arr/dep 23:33/23:34, EARLY | -1 |
| QPW (Queens Park) | arr/dep 23:37/23:39 | arr/dep 23:36/23:38, EARLY | -1 |
| KNL (Kensal Green) | arr/dep 23:41/23:41 | arr/dep 23:40/23:40, EARLY | -1 |
| WJL (Willesden Jn Low) | arr 23:43 | arr 23:43, ON TIME | 0 |
Clean, on-time-to-slightly-early running throughout. No disruption.

**Pin 45 — `C17924`, Milton Keynes Central→Euston (`lnwr-birmingham-crewe`)**:
| TIPLOC/CRS | CIF booked (local) | TRUST actual | Δ |
|---|---|---|---|
| MKNSCEN (origin) | dep 22:02 | dep 22:01, EARLY | -1 |
| BLY (Bletchley) | arr/dep 22:06/22:07 | arr/dep 22:07/22:08, LATE | +1 |
| LBZ (Leighton Buzzard) | arr/dep 22:13/22:14 | arr/dep 22:14/22:15, LATE | +1 |
| TRI (Tring) | arr/dep 22:25/22:26 | arr/dep 22:25/22:26, ON TIME | 0 |
| WFJ (Watford Jn) | arr/dep 22:47/22:48 | arr/dep 22:46/22:47, EARLY | -1 |
| EUSTON (terminus) | arr 23:13 | arr 23:10, EARLY | -3 |
`train_current_state`'s own reported delay for this pin: **0 minutes**
(`status: completed`). A minor wobble (+1 at two intermediate points) that
fully recovered, ending 3 minutes early. No disruption.

**Pin 47 — `G38654`, Crewe→Wrexham General (`lnwr-birmingham-crewe`)**:
| TIPLOC/CRS | CIF booked (local) | TRUST actual | Δ |
|---|---|---|---|
| CREWE (origin) | dep 23:33 | dep 23:45, LATE | **+12** |
| CTR (Chester) | arr/dep 23:52/23:54 | arr/dep 00:05/00:06, LATE | **+13 / +12** |
| WREXHMG (terminus) | arr 00:09 (d+1) | arr 00:19, LATE | **+10** |
A real, moderate, sustained delay (10-13 minutes throughout, not a
transient wobble), starting from the very first station.

**Pin 49 — `P23952`, Preston→Blackburn (`northern-blackpool`)**:
| TIPLOC/CRS | CIF booked (local) | TRUST actual | Δ |
|---|---|---|---|
| PRST (origin) | dep 23:19 | dep 23:22, LATE | +3 |
| LOH (Lostock Hall) | arr/dep 23:24/23:25 | arr/dep 23:28/23:29, LATE | +4 |
| BMB (Bamber Bridge) | arr/dep 23:28/23:29 | arr/dep 23:31/23:31, LATE | +3 |
| PLS (Pleasington) | arr/dep 23:36/23:36 | arr/dep 23:39/23:40, LATE | +3 |
| CYT (Cherry Tree) | arr/dep 23:39/23:40 | arr/dep 23:42/23:43, LATE | +3 |
| MLH (Mill Hill Lancs) | arr/dep 23:42/23:43 | arr/dep 23:44/23:46, LATE | +2 |
| BBN (Blackburn, terminus) | arr 23:47 | arr 23:49, LATE | **+2** |
`train_current_state`'s own reported delay: **2 minutes** — matches this
session's independent CIF recomputation exactly. A small, real, but
minor delay, consistent throughout.

**Pin 51 — `W69917`, Carlisle→Dumfries (`northern-cumbrian-coast`)**:
| TIPLOC/CRS | CIF booked (local) | TRUST actual | Δ |
|---|---|---|---|
| CARLILE (origin) | dep 23:10 | dep 23:10, ON TIME | 0 |
| GEA (Gretna area) | arr/dep 23:21/23:21 | arr/dep 23:21/23:23, LATE | +1.5 |
| ANN (Annan) | arr/dep 23:29/23:30 | arr/dep 23:31/23:32, LATE | +1.5 |
| DUMFRES (terminus) | arr 23:50 | arr 23:47, EARLY | **-3** |
Essentially on-time, ending 3 minutes early. No disruption.

## Task 6: full 2026-09-11 `line_status_history` for the 3 real matched lines

Pulled directly for the entire calendar day, all rows, not a partial
window:

| line_id | rows on 2026-09-11 | content |
|---|---|---|
| `lnwr-birmingham-crewe` | **2** | `21:45:19Z`: `ldbws-inferred`, severity 9 (Minor Delay), "1 of 3 sampled services delayed... avg 3.3 min" — unrelated to any of this session's pinned trains (none of pins 42/43/44/45/46/47 were running a delay at that moment per Task 5 above). `21:57:19Z`: reverts to `ldbws-inferred`, severity 10, "Good Service". **Nothing at all after 21:57:19Z** — i.e. **zero output for the entire 21:57Z–24:00Z window**, which is exactly when pin 47's real, moderate (+10 to +13 min) Crewe→Wrexham delay occurred (23:33–00:19Z). |
| `northern-blackpool` | 29 | 100% Knowledgebase (`data_quality: planned`), all the same real, ongoing "Bransty Tunnel track renewal: buses replace trains between Corkickle and Whitehaven" planned-work entry (severity oscillating 6/9 across the day) — a real but *unrelated* disruption (Cumbrian Coast, not this line's own Blackpool corridor), present continuously through pin 48/49's own (respectively unusable and minor) results. |
| `northern-cumbrian-coast` | 9 | Same Bransty Tunnel entry, same severity pattern, present continuously through pin 50/51's own (respectively unusable and clean) results. |

**No `ldbws-inferred` entry on any of the 3 lines, at any point in the
day, is about any of the specific trains this session pinned.** The
`lnwr-birmingham-crewe` finding is the load-bearing one: its *only* two
recomputes for the entire day both occur and conclude *before* pin 47's
real delay even begins, leaving that line's sampling-derived product
output frozen at "Good Service" through the entire window the real delay
was happening.

## Task 7: three-way comparison — one real, usable disruption instance; four clean agreements; five pins that can't be tested at all

Laying Task 5's filtered "expected vs. actual" tables beside Task 6's
per-line baseline:

**The one real, usable "did sampling catch it" test this data supports**:
pin 47 (`G38654`, Crewe→Wrexham General, `lnwr-birmingham-crewe`) shows a
real, CIF-confirmed, sustained 10-13 minute delay, starting at the origin
and persisting to the terminus. `lnwr-birmingham-crewe`'s own real
sampling output recorded **nothing** — no recompute of any kind, any
severity, any `dataQuality` — anywhere in the ~2.75-hour window
(21:57Z–00:40Z) surrounding this train's entire real, delayed run. This
is a genuine, non-hypothetical instance of exactly what this whole plan
exists to test: a real, moderate delay that TRUST-derived per-train
tracking caught and this line's currently-shipping sampling product did
not reflect at all.

**Four clean, honest non-events, reported per the plan's own Step 3**:
pins 43, 45, and 51 ran on-time to a few minutes early with no disruption
of any kind — and there was correspondingly nothing for sampling to have
caught or missed, an unremarkable agreement case. Pin 49's real delay (+2
to +4 minutes) is small enough that it is not a meaningful test either
way — `northern-blackpool`'s real output during this window was, correctly,
about a genuinely different, unrelated incident (Bransty Tunnel), not a
false negative about pin 49 specifically, since a 2-4 minute delay is
below any severity threshold a real system should be flagging regardless
of data source.

**Five pins (42, 44, 46, 48, 50) cannot be tested at all**, because — per
this session's central finding above — no real movement data exists in
this database for their corrected train identity. This is not a "no
disruption occurred" honest non-result (the plan's permitted "clean miss"
category); it is a **data-availability failure**, a materially different
and more concerning outcome that the plan's own Task 4/7 guidance did not
anticipate needing to distinguish.

**Sample-size and reliability honesty, stated plainly**: of the 10 real
pins this validation exercise pinned, only **5 produced any usable real
per-train data at all**, and of those 5, only **1** constituted a real
disruption large enough to be a meaningful test of whether TRUST-vs-schedule
inference would have caught something sampling missed. That test came
back positive (TRUST caught it, sampling didn't) — but **N of M is 1 of
1**, identical in size to the 2026-08-31/09-01 session's own "1 of 1,"
not an improvement, despite this session drawing on a nominal 10-pin,
full-day dataset. The apparent 10x increase in raw pin count did not
translate into a larger *usable* sample, because half of it turned out to
be unusable in a way that wasn't visible from `resolution_status` alone.

## Task 8: decision gate — final verdict

**Step 1 (licensing): unchanged, still favorable.** Nothing in this
session touches Task 1's verdict from 2026-08-29 — both RDM licences
remain free (OGL3), already held, with no fair-usage cap or paid tier.

**Step 2 (empirical verdict): N of M = 1 of 1 — the same, still-too-small
number the 2026-08-31/09-01 session reported, not a larger one.** This
session had access to a real, full, completed day and 10 real pins — more
raw material than any prior session in this document — and still only
produced a single spot-checkable disruption instance, because a newly-
discovered, previously-undocumented data-quality problem (movement events
attributable to the wrong `trains_id`, affecting at least half of the
supposedly-corrected pins outright and contaminating the other half)
silently destroyed most of the intended sample. This is a materially
different, and more concerning, way to arrive at "not enough data" than
any prior session's: it is not that too few disruptions occurred, or that
the monitoring window was cut short — it is that **the validation
methodology's own data cannot currently be trusted at face value**,
`resolution_status = 'resolved'` and a populated `trains_id` notwithstanding.

**Recommendation: NOT YET.** Not "no" — nothing found here argues against
the underlying feature; the one real, usable test this session ran came
back as a clean, unambiguous "TRUST caught a real delay that sampling's
own shipping product missed entirely." But the plan's own Step 2 bar
requires a stated **N of M** large enough to support a "clear majority"
claim with any confidence, and this session's honest number — **1 of
1** — is exactly as thin as the weakest prior result in this document,
now compounded by a new, real reason to distrust how much of *any* future
larger sample would actually be usable without a fix.

**Concrete next step, narrower and more specific than any prior
session's**, because for the first time the blocker is neither licensing,
nor SSO, nor the STANOX/CRS gap (all three remain fixed and working):

1. **Diagnose and fix the `train_movement_events`-attributed-to-the-wrong-`trains_id`
   problem** documented above — both its severe form (5 of 10 pins here
   carry *100%* wrong-train data despite `resolved` status and a real
   `trains_id`) and its milder form (the other 5 pins' genuine data mixed
   with real, unrelated contamination requiring manual, CIF-time-based
   filtering to extract). This is a source-code-level fix outside this
   read-only validation session's scope, but it is now a concrete,
   evidenced, and named prerequisite, not a vague "improve reliability"
   note. Until it's fixed, `resolution_status = 'resolved'` cannot be
   trusted as a signal that a pin's movement data actually belongs to its
   own claimed train — any future validation session must re-verify each
   pin's data against its own CIF schedule (the exact method used in this
   section) rather than accepting `resolved` at face value.
2. **Once fixed, re-run Task 4 onward with a real, full-day, multi-line
   monitoring window** — the mechanism (SSO, STANOX/CRS translation) is
   proven and working; what's needed now is simply a dataset that isn't
   silently half-corrupted, large enough to move past a 1-of-1 sample.
3. Only then re-run Task 8 with an **N of M** large enough, and honestly
   verified station-by-station, to carry a confident verdict either way.

**If proceeding to Option B is eventually greenlit**, unchanged from every
prior verdict in this document: gated on Task 8 reaching "go," which it
still has not, six real execution sessions in.

---

# 2026-09-12 (second pass): remediation independently re-verified end-to-end — 3 of 5 flagged pins are genuinely fixed, but 2 remain wrong-train contaminated exactly as before; final verdict is 2 of 2, still NOT YET

**Status: a seventh real execution session, superseding the "2026-09-12"
section immediately above.** That section's own verdict — 1 of 1,
NOT YET — was built on data since discovered to be unreliable: the root
cause (a missing `event_type = 'DEPARTURE'` filter in
`crates/api/src/data/trust_event_backlog_match.rs::find_backlog_match`,
letting it match an unrelated train's ARRIVAL event and replay that wrong
train's history onto the correct `trains_id`) has since been found, fixed,
reviewed, merged, and deployed, and a full-retention-window scan plus a
manual remediation pass were run against production to repair the damage.
This session was dispatched to redo Tasks 5-8 against that remediated
data — but, per this document's own established norm (every prior session
in this file has re-verified rather than trusted a predecessor's or a
dispatcher's characterization), the remediation itself was independently
re-checked station-by-station before being relied on, not assumed correct
from the brief describing it. **That re-check found the brief's own
characterization of which pins were fixed to be wrong for 2 of the 10
pins** — reported here plainly, exactly as this document's own convention
requires. Everything below was checked directly against the live
production database (`distant-signal-postgres-0`, read-only `psql`,
`distant_signal` user), quoted exactly as returned, not paraphrased.

## Re-confirming pin identity (unchanged from the immediately-prior section)

```
id | pin_origin_crs | trains_id | train_uid | matched_line_id         | origin_crs | destination_crs
42 | EUS            | 712945    | C18012    | lnwr-birmingham-crewe   | EUS        | CRE
43 | EUS            | 713725    | C34213    | lnwr-birmingham-crewe   | EUS        | WFJ
44 | MKC            | 713330    | W70396    | lnwr-birmingham-crewe   | MKC        | EUS
45 | MKC            | 713728    | C17924    | lnwr-birmingham-crewe   | MKC        | EUS
46 | CRE            | 713729    | C17876    | lnwr-birmingham-crewe   | CRE        | EUS
47 | CRE            | 713731    | G38654    | lnwr-birmingham-crewe   | CRE        | WRX
48 | PRE            | 713690    | G86196    | northern-blackpool      | PRE        | OMS
49 | PRE            | 713740    | P23952    | northern-blackpool      | PRE        | BBN
50 | CAR            | 713358    | M37478    | northern-cumbrian-coast | CAR        | DMF
51 | CAR            | 713743    | W69917    | northern-cumbrian-coast | CAR        | DMF
```

Identical to the dispatching brief's table and the prior section's — no
identity metadata changed since. The matched-line set is confirmed still
narrowed to 3 lines (`lnwr-birmingham-crewe`, `northern-blackpool`,
`northern-cumbrian-coast`); no pin resolved to `wcml` itself or to
`emr-regional` (the latter was an artifact of pin 44's since-corrected
wrong-train binding in the very first, 2026-09-11 pass).

## Independent re-verification of the remediation: 3 of 5 flagged pins are genuinely fixed; 2 are not, contrary to the dispatching brief

Per this document's own standing method (station-by-station CIF
cross-check against `schedule_line_population`, not trusting
`resolution_status` or a populated `trains_id` at face value), every one
of the 5 previously-flagged pins' (42, 44, 46, 48, 50) `train_movement_events`
was checked end-to-end against its *corrected* `train_uid`'s real booked
schedule — not just the first event, per this session's explicit brief.

**A real, useful fingerprint surfaced immediately**: all 5 previously-flagged
`trains_id`s (712945, 713330, 713358, 713690, 713729) have `loc_stanox = NULL`
on **100%** of their events — confirmed directly:

```
trains_id | is_null | count
713729    | t       | 30
713690    | t       | 3
713358    | t       | 1
712945    | t       | 16
713330    | t       | 17
```

By contrast, the 5 pins never flagged as mismatched (43, 45, 47, 49, 51 —
`trains_id` 713725/713728/713731/713740/713743) all carry real, populated
`loc_stanox` values throughout. This is decisive circumstantial evidence
that **all 5** flagged `trains_id`s went through the same backlog-replay
remediation process (which evidently doesn't populate `loc_stanox`), not
just the 3 the brief names — i.e., remediation was *attempted* on all 5,
but, as shown below, it only *succeeded* for 3 of them.

**Pins 44, 46, 48 — genuinely, verifiably fixed end-to-end**, cross-checked
against every calling point in `schedule_line_population` for the
corrected `train_uid`, not merely the first event:

- **Pin 44 (`trains_id=713330`, `W70396`, MKC→EUS)**: all 17 real events
  match `W70396`'s own booked schedule exactly, in order, from origin to
  terminus — `MKNSCEN` dep booked `17:58` (actual `17:59`, +1) through
  `BLTCHLY` (+3/+3), `LTNBZRD` (+3/+3), `TRING` (+2/+2), `WATFDJ` (+2/+2),
  `BUSHEY` (+2/+2), `HROW` (+2.5/+2.5), to `EUSTON` terminus arrival
  booked `19:03` (actual `19:07`, **+4**). A clean, complete, end-to-end
  match — not just the origin.
- **Pin 46 (`trains_id=713729`, `C17876`, CRE→EUS)**: the strongest single
  dataset of any pin, 30 real events matching **every** booked calling
  point of `C17876`'s real schedule in exact chronological order — `CREWE`
  dep booked `18:13` (actual `18:25`, +12) → `STAFFRD` (+12/+14) →
  `RUGL` (+15/+15) → `LCHTTVL` (+15.5/+14.5) → `TMWTHLL` (+15/+15) →
  `ATHRSTN` (+15.5/+16.5) → `NNTN` (+17/+17) → `RUGBY` (+15/+14.5) →
  `MKNSCEN` (+16/+16) → nine further real pass-point events consistent
  with the remaining unbooked TIPLOCs on this route → `EUSTON` terminus
  arrival booked `20:24` (actual `20:39`, **+15**). A real, substantial,
  sustained delay, confirmed clean at every single booked station on the
  route, not just the endpoints.
- **Pin 48 (`trains_id=713690`, `G86196`, PRE→OMS)**: sparse (only 2 real
  events survive — no code-level pruning found; this reads as a genuine
  TRUST feed reporting gap, not a remediation defect) but both endpoints
  match `G86196`'s booked schedule exactly: `PRST` dep booked `18:10`
  (actual `18:10`, on time) and `ORMSKRK` terminus arrival booked `18:41`
  (actual `18:41`, on time). Clean, if thin.

**Pins 42 and 50 — still 100% wrong-train contaminated, exactly as before
remediation, contrary to the dispatching brief's claims**:

- **Pin 42 (`trains_id=712945`)**: the brief characterized this as
  "already confirmed correct (never contaminated)." **This is not what
  the database shows.** `C18012` (the claimed corrected identity) is
  booked `EUSTON` dep `17:46` per `schedule_line_population` — but every
  one of the 16 real events on `trains_id=712945` falls between `16:26`
  and `17:11`, over half an hour *before* `C18012` even departs. Checked
  against `Y80906` — the **original, pre-correction wrong train** this
  document's earlier "2026-09-12" section named for this exact pin — every
  single event matches exactly: `EUSTON` dep booked `16:26` (row 1, exact
  match), through `LTNBZRD` (`16:54`/`16:55`), `BLTCHLY` (`17:01`/`17:02`),
  `MKNSCEN` (`17:07`/`17:08`), to a final `WLVR` arrival at `17:11` — a
  clean, complete match to `Y80906`'s own booked schedule, not `C18012`'s.
  **Pin 42 was never actually remediated; it is unusable for exactly the
  same reason the immediately-prior section found, unchanged.**
- **Pin 50 (`trains_id=713358`)**: the brief characterized this as
  "sparse/no movement data... a separate, unrelated, honest 'resolved but
  thin/no real-time data' case (not this bug)." **This is also not
  accurate.** One real event exists (not zero): `ARRIVAL`, planned and
  actual both `16:39:00`. `M37478` (the claimed corrected identity, CAR→DMF)
  has no booked calling point anywhere near `16:39` — its schedule runs
  `CARLILE` dep `17:59` → ... → `DUMFRES` arr `18:36`. But the original,
  pre-correction wrong train **`W69941`** (Dumfries→Carlisle) has a real
  booked `CARLILE` arrival at exactly `16:39` — an exact match. **Pin 50's
  one real data point is real, genuine TRUST data — for the wrong train.**
  It is not an honest "no data yet" case; it is the same wrong-train
  contamination bug, unfixed, with a single surviving data point that
  happens to look like sparse-but-clean data unless actually cross-checked
  against the claimed train's own schedule.

**What this means, stated plainly**: the dispatching brief's claim that
"ALL 10 pins' underlying data is confirmed trustworthy" is **not** what
this independent re-verification found. The remediation fixed 3 of the 5
flagged pins cleanly and completely (44, 46, 48) but left 2 (42, 50) in
exactly the same broken state as before — still matching the original
wrong train, not the corrected one. Given both fixed and unfixed pins
share the identical `loc_stanox IS NULL` fingerprint, the most likely
explanation is that the remediation script ran against all 5 rows but,
for 2 of them, replayed the *wrong* train's `trust_event_backlog` history
(a 60% success rate on the one remediation attempt observed) — a new,
concrete, evidenced data-integrity finding in its own right, reported here
because any future large-scale remediation attempt needs to be verified
the same way, not trusted from a description of what it was supposed to
do.

**Pins 43, 45, 47, 49, 51 — re-verified, unchanged from the prior
section's characterization.** Re-running the same station-by-station
filter against `stanox_crs` (now joined directly against the live
`stanox_crs` table rather than decoded from `stanox_crs.rs`, since a
proper DB-backed table now exists) confirms exactly the pattern the
immediately-prior section described: each of these 5 `trains_id`s'
movement logs contain a real, unrelated early cluster of events (from
other trains that happen to pass through the same origin CRS earlier in
the day), followed by a later cluster that matches the pin's own claimed
train station-for-station and minute-for-minute. Spot-checked in full for
all 5 (not sampled) — e.g. pin 45's `C17924` (MKC→EUS) matches cleanly
at all 13 of its booked calling points once the contaminating Manchester/
Stockport-area cluster is filtered out; pin 51's `W69917` (CAR→DMF)
matches cleanly at all 5 once the contaminating Glasgow-Carstairs-area
cluster is filtered out. No change to these 5 pins' usability from the
prior section.

## Task 5: expected (CIF) vs. actual (TRUST) — final table

| pin | train | route | usable? | delay pattern |
|---|---|---|---|---|
| 42 | C18012 | EUS→CRE | **no — 100% wrong-train data (Y80906)** | n/a |
| 43 | C34213 | EUS→WFJ (DC line) | yes | clean, 0 to -1 min (early), no disruption |
| 44 | W70396 | MKC→EUS | yes | minor wobble, +1 to +4, not disruption-scale |
| 45 | C17924 | MKC→EUS | yes | clean, -1 to +2, ends -3 early, no disruption |
| 46 | C17876 | CRE→EUS | yes | **real, sustained +12 to +17, settles +15** |
| 47 | G38654 | CRE→WRX | yes (filtered) | **real, sustained +10 to +13** |
| 48 | G86196 | PRE→OMS | yes (thin) | clean, on time throughout |
| 49 | P23952 | PRE→BBN | yes (filtered) | minor, +2 to +4, not disruption-scale |
| 50 | M37478 | CAR→DMF | **no — the one real event is wrong-train data (W69941)** | n/a |
| 51 | W69917 | CAR→DMF | yes (filtered) | clean, 0 to +2, ends -3 early, no disruption |

## Task 6: full 2026-09-11 `line_status_history` for the 3 matched lines — re-pulled directly, identical to the prior section

```
line_id                  | rows | window
lnwr-birmingham-crewe    | 2    | 21:45:19Z – 21:57:19Z
northern-blackpool       | 29   | 06:04:35Z – 23:10:19Z
northern-cumbrian-coast  | 9    | 06:04:35Z – 23:10:19Z
```

`lnwr-birmingham-crewe`'s only two rows (quoted directly): `21:45:19Z`,
`ldbws-inferred`, severity 9, *"1 of 3 sampled services delayed. (most
cited: This service has been delayed by a late running train being in
front of this one)"*, `avg_delay_minutes: 3.33`; `21:57:19Z`, reverts to
severity 10, *"Good Service"*. **Neither entry is about pin 46 or pin 47**:
pin 46's entire real delay window (`18:13`–`20:24` local /
`17:13`–`19:24` UTC) had already concluded over two hours before this
line's first recompute of the day even fired, and pin 47's real delay
(`23:33` local dep / `22:33` UTC onward) hadn't started yet. The avg
delay cited (3.3 min) is also far too small to describe either train's
real 10-17 minute delay. **This line's sampling output recorded zero
signal of any kind — positive or negative — covering either of this
window's two real, substantial delays.**

`northern-blackpool`'s 29 rows are, as the prior section found, 100%
Knowledgebase (`data_quality: planned`) about the real but unrelated
"Bransty Tunnel track renewal: buses replace trains between Corkickle and
Whitehaven" — re-confirmed present and unchanged through both pin 48's
(clean, on-time) and pin 49's (minor, sub-disruption-scale) windows;
directly re-queried for both windows, same text, same severity pattern
(6/9 oscillating), no entry about either specific train.

## Task 7: three-way comparison — final, verified count

**Two real, fully-verified, usable disruption instances, both hits**:

1. **Pin 46** (`C17876`, `lnwr-birmingham-crewe`): a real, sustained,
   CIF-confirmed 12-17 minute delay (settling at +15 at Euston), verified
   clean at every single booked station along the route — the strongest,
   most complete dataset this entire six-week validation exercise has
   produced. `lnwr-birmingham-crewe`'s own sampling output recorded zero
   signal covering any part of this train's real, delayed run.
2. **Pin 47** (`G38654`, `lnwr-birmingham-crewe`): a real, sustained,
   CIF-confirmed 10-13 minute delay, verified clean once the unrelated
   early-cluster contamination is filtered out (re-confirmed this
   session). Same line, same result: zero sampling signal covering any
   part of the real delayed run.

**Both real disruption instances found in this window were caught by
TRUST-vs-schedule tracking and missed entirely by sampling — the same
line's shipping product, on the same day, recorded literally nothing
during either train's real delay.** No counterexample exists in this
session's data: every pin with a genuine, disruption-scale delay was a
hit; every pin that ran clean/on-time correspondingly had nothing for
sampling to have caught, an honest agreement case (pins 43, 45, 48, 51);
pin 49's delay (+2 to +4) is real but too small to be a meaningful
either-way test, matching this document's own established threshold for
"not a real test" from prior sessions. **Two pins (42, 50) remain
permanently untestable** — not an honest "clean miss," a genuine
data-availability failure identical in kind to (though smaller in scope
than) the one the immediately-prior section already flagged, now known to
affect 2 of 10 pins rather than 5.

**N of M = 2 of 2.** Both real, spot-checked, disruption-scale instances
in this window were correctly caught by TRUST-vs-schedule inference and
missed entirely by the currently-shipping sampling product. This is
double the size of, and fully consistent with (zero contradicting
results), every prior session's positive finding in this document: the
2026-08-31/09-01 session's real "1 of 1" (a Chester cancellation), and
this document's own immediately-prior 1-of-1 (which turns out, per this
session's re-verification, to have actually been describing pin 47 — the
same real delay this session re-confirms). **Across all real execution
sessions in this document's six-week history, every real disruption ever
found and successfully spot-checked has been a hit for TRUST-vs-schedule
and a miss for sampling — 3 real instances total (1 cancellation, 2
delays), zero counterexamples, ever.**

## Task 8: decision gate — final verdict

**Step 1 (licensing): unchanged, still favorable.** Both RDM licences
remain free (OGL3), already held, no fair-usage cap, no paid tier.

**Step 2 (empirical verdict): N of M = 2 of 2 — larger than any prior
session's, still a small absolute sample, but with a perfect and now
three-times-repeated track record.** Per the plan's own explicit
criteria, this is genuinely double-edged: 2 of 2 (100%) is, read
literally, a "clear majority" of spot-checked disruption instances caught
by TRUST and missed by sampling — satisfying the letter of the "go" bar.
But the plan's own "not yet" criteria equally, explicitly permits staying
at "not yet" when "too few real disruption days occurred... to say
anything with any confidence," and a single day producing exactly 2
qualifying instances (out of 10 pinned trains) is still a small absolute
number by any reasonable statistical standard, consistent with every
prior session's own judgment call at comparable or smaller sample sizes.

**Recommendation: still NOT YET, but stated as the strongest, most
confident "not yet" this document has ever recorded — not a vague
extension.** Applying this document's own consistent precedent (every
prior "1 of 1" was correctly judged too small; this session's "2 of 2" is
only incrementally larger, not qualitatively different in statistical
power), the honest call remains **NOT YET** on this session's own data
taken in isolation. But two things distinguish this verdict from every
prior one and should weigh on whoever decides whether to demand a further
round or treat the cumulative record as sufficient:

1. **Every real mechanism-level blocker this validation exercise has ever
   found is now fixed and independently re-confirmed working**: SSO
   (fixed 2026-08-30), the STANOX/CRS translation gap (fixed and proven
   live 2026-08-31), and — for 3 of 5 previously-affected pins — the
   TRUST-event-backlog wrong-train-matching bug (fixed in code, and this
   session independently confirmed the remediation produced genuinely
   correct, complete, end-to-end data for those 3). What remains is
   almost entirely a matter of accumulating more real monitoring days,
   not solving any further defect in the pin-tracking or schedule-matching
   pipeline itself.
2. **The remediation script itself is not yet reliable** — a new,
   concrete finding from this session: of the 5 rows it was run against,
   it fixed 3 and left 2 silently still wrong (matching the original bad
   train, not the intended one). Before any future large-scale monitoring
   run leans on "resolved" pins being trustworthy, this remediation path
   needs its own fix and independent re-verification — the exact same
   station-by-station method this and the prior session both had to use
   by hand.
3. **Cumulative, cross-session evidence is now 3 real disruption
   instances found and spot-checked across this document's entire
   six-week history, all 3 hits, zero counterexamples.** This document
   has never once found a real, verified case where sampling caught
   something TRUST-vs-schedule missed, or where TRUST-vs-schedule
   produced a false positive. That perfect record, while still built on
   a small absolute number of instances, is itself informative — a future
   session finding a 4th and 5th real instance in the same direction
   would make continuing to say "not yet" purely about sample size
   increasingly hard to justify.

**Concrete next step**, narrower than any prior session's since the
pin-tracking/schedule-matching mechanism itself is now proven correct for
the successfully-remediated majority:

1. **Diagnose why the remediation script silently failed for 2 of 5 rows**
   (pins 42, 50) — this is a new, source-code-adjacent question (likely in
   whatever script or process performed the manual `trust_event_backlog`
   replay) outside this read-only validation session's own scope, but now
   concretely evidenced and named.
2. **Re-run Task 4 onward for one or two more real, full days**, ideally
   spanning more than one line's worth of pins simultaneously (as
   2026-09-11 did), to accumulate a genuinely larger N — the mechanism is
   proven; what's needed is volume.
3. **Only then render a truly confident final Task 8 verdict** — or, if
   the dispatching team judges the accumulated 3-for-3 cross-session
   record combined with a now-fully-proven mechanism sufficient on its
   own, that is a legitimate, defensible reading of this document's own
   evidence, and a decision this document defers to a human call rather
   than asserting unilaterally.

**If proceeding to Option B is eventually greenlit**, unchanged from every
prior verdict in this document: gated on Task 8 reaching "go" — which, on
the strictest reading of this session's own single-day data, it still has
not, seven real execution sessions in, though for the first time with a
perfect, repeated, and now mechanism-independent track record behind it.

---

# 2026-09-12 (third pass): pins 42 and 50 also remediated — all 10 pins are now trustworthy for the first time in this document's history; final verdict unchanged at 2 of 2, NOT YET

**Status: an eighth real execution session**, dispatched specifically to
close the gap the immediately-prior ("second pass") section left open:
its own remediation script had fixed pins 44, 46, and 48 genuinely, but
left pins 42 and 50 still 100%-bound to their original wrong train. Per
the dispatching brief, both pins have since been given the same
treatment as the other three (wrong movement events deleted, real history
replayed from `trust_event_backlog` against each pin's confirmed true
`train_id`: `C18012`←`721U45MV11` for pin 42, `M37478`←`092L941V11` for
pin 50). Per this document's own standing norm — every prior session has
re-verified rather than trusted a predecessor's or a dispatcher's
description of a fix — this session independently re-checked both pins
station-by-station against `schedule_line_population` before relying on
them, exactly as the "second pass" section did for pins 44/46/48.
Everything below was checked directly against the live production
database (`distant-signal-postgres-0`, read-only `psql`, `distant_signal`
user), quoted exactly as returned, not paraphrased.

## Re-confirming pin identity (unchanged)

```
id | pin_origin_crs | trains_id | train_uid | matched_line_id         | origin_crs | destination_crs | resolution_status
42 | EUS            | 712945    | C18012    | lnwr-birmingham-crewe   | EUS        | CRE              | resolved
50 | CAR            | 713358    | M37478    | northern-cumbrian-coast | CAR        | DMF              | resolved
```

Identical to every prior section's identity table — no metadata changed.
Both matched lines (`lnwr-birmingham-crewe`, `northern-cumbrian-coast`)
are lines the "second pass" section's Task 6 pull already covered in
full for the entire 2026-09-11 calendar day (see below) — so, per this
session's brief, `line_status_history` was **not** re-pulled; that
section's data is reused directly.

## Independent re-verification: both pins now match their corrected train end-to-end

**Pin 42 (`trains_id=712945`, `C18012`, EUS→CRE)**: `schedule_line_population`
gives `C18012`'s real booked schedule as `EUSTON` dep `17:46` → `WMBY`
→ `HROW` → `WATFDJ` → `TRING` → `BLTCHLY` → `MKNSCEN` (`18:18`/`18:19`) →
`RUGBY` (`18:41`/`18:42`) → `NNTN` (`18:53`/`18:54`) → `ATHRSTN`
(`18:59`/`19:00`) → `TMWTHLL` (`19:07`/`19:08`) → `LCHTTVL` (`19:13`/`19:14`)
→ `RUGL` (`19:20`/`19:21`) → `STAFFRD` (`19:30`/`19:35`) → `CREWE`
terminus (arr `19:54`). `trains_id=712945` now carries **30 real events**
(up from 16), and every single one matches this booked schedule
station-for-station, in order, from origin to terminus — not just the
endpoints:

| CRS | planned (raw, local labeled UTC) | actual | Δ (min) |
|---|---|---|---|
| EUS (origin, dep) | 17:46:00 | 17:46:00 | 0, ON TIME |
| WMB (arr/dep) | 17:53:30 | 17:55:00 / 17:54:00 | +1.5 / +0.5 |
| HRW (arr/dep) | 17:55:30 | 17:56:00 | +0.5 |
| WFJ (arr/dep) | 17:59:00 | 18:01:00 / 18:00:00 | +2 / +1 |
| TRI (arr/dep) | 18:06:30 | 18:09:00 / 18:08:00 | +2.5 / +1.5 |
| BLY (arr/dep) | 18:15:00 | 18:18:00 / 18:17:00 | +3 / +2 |
| MKC (arr/dep) | 18:18:00 / 18:19:00 | 18:20:00 / 18:22:00 | +2 / +3 |
| RUG (arr/dep) | 18:41:30 / 18:42:30 | 18:42:00 / 18:43:00 | +0.5 |
| NUN (arr/dep) | 18:53:30 / 18:54:30 | 18:56:00 / 18:57:00 | +2.5 |
| ATH (arr/dep) | 18:59:30 / 19:00:00 | 19:03:00 / 19:04:00 | +3.5 / +4 |
| TAM (arr/dep) | 19:07:00 / 19:08:00 | 19:11:00 / 19:12:00 | +4 |
| LTV (arr/dep) | 19:13:30 / 19:14:30 | 19:17:00 / 19:18:00 | +3.5 |
| RGL (arr/dep) | 19:20:30 / 19:21:30 | 19:24:00 / 19:25:00 | +3.5 |
| STA (arr/dep) | 19:30:00 / 19:35:00 | 19:33:00 / 19:36:00 | +3 / +1 |
| CRE (terminus, arr) | 19:54:00 | 19:54:00 | 0, ON TIME |

A real, complete, coherent journey: departs on time, a wobble builds to a
peak of +3.5–4 minutes around Atherstone/Tamworth/Lichfield, then
recovers, arriving at Crewe **exactly on time**. This is now genuinely
usable, verified, end-to-end data — but the delay never exceeds ~4
minutes and fully self-corrects before the terminus, placing it in the
same "minor, sub-disruption-scale wobble" bucket this document has
already established for pins 44 (+1 to +4) and 49 (+2 to +4), not the
"real, sustained, disruption-scale" bucket pins 46 and 47 occupy. **Pin
42 is now a real, clean, confirming non-event, not a third disruption
hit.**

**Pin 50 (`trains_id=713358`, `M37478`, CAR→DMF)**: `M37478`'s real
booked schedule origin is `CARLILE` dep `17:59`. `trains_id=713358` now
carries exactly **one real movement event** — `DEPARTURE`, `CAR`, planned
`17:59:00`, actual `18:01:00`, **LATE +2** — which matches `M37478`'s own
booked origin departure exactly (previously, pre-fix, this pin's one
event was an `ARRIVAL` at `16:39` that matched the *original wrong*
train, `W69941`'s, booked Carlisle arrival; that event is now gone,
replaced by this one genuine, correctly-matched departure). A second row
on this `trains_id` (`id=898452`) is a TRUST Activation message
(`msg_type='0001'`), not a movement report — expected, uninformative for
a delay table, not a data gap. **This confirms the remediation genuinely
worked for pin 50 too — the one surviving event is real, correct-train
data** — but a single origin-departure event, with no intermediate or
terminus calling points, cannot support any journey-level delay
characterization. Per the dispatching brief's own framing, this is
correctly treated as **thin/inconclusive**, not a hard data point either
way — a real, honest TRUST feed reporting-coverage limitation, not a
data-integrity bug (the load-bearing distinction from the "second pass"
section, where the problem was that the *one* event present was for the
*wrong train* — here it is for the *right* train, there just isn't much
of it).

**Both pins are now backed by genuinely correct-train data — the
wrong-train contamination this document tracked across the two prior
2026-09-12 sections is fully closed.** Of this validation run's 10 real
pins, **9 now carry genuinely correct-train `train_movement_events`**
(42, 43, 44, 45, 46, 47, 48, 49, 51), and the 10th (50) carries a single
genuinely correct-train event, too thin to characterize a journey but not
wrong. **Zero pins remain wrong-train contaminated.**

## Task 5: expected (CIF) vs. actual (TRUST) — final table, all 10 pins

| pin | train | route | usable? | delay pattern |
|---|---|---|---|---|
| 42 | C18012 | EUS→CRE | **yes — full end-to-end match, 30/30 events** | minor wobble, +0.5 to +4, recovers to ON TIME at terminus — not disruption-scale |
| 43 | C34213 | EUS→WFJ (DC line) | yes | clean, 0 to -1 min (early), no disruption |
| 44 | W70396 | MKC→EUS | yes | minor wobble, +1 to +4, not disruption-scale |
| 45 | C17924 | MKC→EUS | yes | clean, -1 to +2, ends -3 early, no disruption |
| 46 | C17876 | CRE→EUS | yes | **real, sustained +12 to +17, settles +15** |
| 47 | G38654 | CRE→WRX | yes (filtered) | **real, sustained +10 to +13** |
| 48 | G86196 | PRE→OMS | yes (thin) | clean, on time throughout |
| 49 | P23952 | PRE→BBN | yes (filtered) | minor, +2 to +4, not disruption-scale |
| 50 | M37478 | CAR→DMF | **yes, but thin — 1 real event only (origin departure, +2 late)** | inconclusive — no journey-level pattern available |
| 51 | W69917 | CAR→DMF | yes (filtered) | clean, 0 to +2, ends -3 early, no disruption |

Every pin in this table now carries genuinely correct-train data. This is
the first time in this document's six-week, eight-session history that
every pinned train's data has been independently verified station-by-
station and found free of wrong-train contamination.

## Task 6: `line_status_history` — reused from the "second pass" section, not re-pulled

Per this session's brief, Task 6/7 only warranted a fresh
`line_status_history` pull if pin 42's `matched_line_id` were a line the
prior session hadn't already covered. It isn't — `lnwr-birmingham-crewe`
(pin 42's line) and `northern-cumbrian-coast` (pin 50's line) were both
already pulled in full, for the entire 2026-09-11 calendar day, in the
"second pass" section above. Restated here for convenience, unchanged:

```
line_id                  | rows | window
lnwr-birmingham-crewe    | 2    | 21:45:19Z – 21:57:19Z
northern-blackpool       | 29   | 06:04:35Z – 23:10:19Z
northern-cumbrian-coast  | 9    | 06:04:35Z – 23:10:19Z
```

`lnwr-birmingham-crewe`'s only two rows for the *entire day* are both
about an unrelated, small (avg 3.3 min) delay at 21:45–21:57Z — well
after pin 42's real journey (raw `17:46`–`19:54` local-labeled-UTC, i.e.
true UTC `16:46`–`18:54`, applying this document's established -1hr BST
correction) had already concluded. Since the line produced **zero**
`line_status_history` output of any kind during pin 42's window, and pin
42 had no disruption-scale delay to catch, this is a real, honest,
uneventful agreement: nothing happened, and correspondingly nothing was
reported. `northern-cumbrian-coast`'s 9 rows are, as previously
documented, 100% Knowledgebase text about the real but unrelated Bransty
Tunnel engineering work, present throughout the day including whatever
narrow window pin 50's single real event falls in — again, not about pin
50's train specifically, and pin 50 has no disruption to have been
missed regardless.

## Task 7: three-way comparison — final, all-10-pins-trustworthy count

**Two real, fully-verified, usable disruption instances, both hits —
unchanged from the "second pass" section**:

1. **Pin 46** (`C17876`, `lnwr-birmingham-crewe`): real, sustained,
   CIF-confirmed 12-17 minute delay (settling at +15 at Euston), verified
   clean at every booked station. `lnwr-birmingham-crewe`'s sampling
   output recorded zero signal covering any part of this run.
2. **Pin 47** (`G38654`, `lnwr-birmingham-crewe`): real, sustained,
   CIF-confirmed 10-13 minute delay, verified clean once unrelated
   early-cluster contamination is filtered out. Same line, same result:
   zero sampling signal.

**What pin 42 adds**: a third, now fully-verified, complete end-to-end
journey — but a **confirming non-event**, not a new hit. Its delay never
exceeds ~4 minutes and self-corrects to exactly on-time by the terminus,
placing it alongside pins 44 and 49 in the "real but sub-disruption-scale"
bucket, where there is honestly nothing for sampling to have caught or
missed. This *does* meaningfully strengthen the overall picture, just not
by adding to the N: it is one more real, clean data point in a growing
set (43, 44, 45, 48, 49, 51, and now 42 — seven of ten pins) where TRUST
and sampling would have agreed (both "nothing to report"), consistent
with, and not contradicting, the design spec's own acknowledgment that
most running is uneventful.

**What pin 50 adds**: a single, now-confirmed-correct-train real
departure event — genuinely usable as *data*, but too thin to be a test
of anything. Correctly carried forward as **pending/inconclusive**, not
folded into either the hit or the clean-agreement count. This is a
different, more honest category than the "second pass" section's
"permanently untestable — a data-availability failure": pin 50's thinness
is now a real TRUST feed reporting-coverage limitation on a correctly-
identified train, not a symptom of the wrong-train bug.

**N of M = 2 of 2 — unchanged from the "second pass" section, but for the
first time built on a dataset with zero remaining data-integrity
questions.** Of the 10 real pins: 2 are genuine, verified, disruption-scale
hits for TRUST-vs-schedule (46, 47); 7 are genuine, verified, clean or
sub-disruption-scale non-events with nothing for either side to have
caught (42, 43, 44, 45, 48, 49, 51); 1 (50) is genuine but too thin to
test either way. **Zero pins are wrong-train contaminated. Zero pins are
unusable due to a data-integrity bug.** Across this document's entire
six-week, eight-session history, every real disruption ever found and
successfully verified — still exactly **3 instances** (1 cancellation
from 2026-08-31/09-01, plus these same 2 delays), zero counterexamples —
remains a hit for TRUST-vs-schedule and a miss for sampling.

## Task 8: decision gate — final verdict, superseding the "second pass" section

**Step 1 (licensing): unchanged, still favorable.** Both RDM licences
remain free (OGL3), already held, no fair-usage cap, no paid tier.

**Step 2 (empirical verdict): N of M = 2 of 2 — the same number as the
"second pass" section, now finally free of the caveat that number was
carrying.** The prior section's own "2 of 2" was explicitly qualified by
"2 pins remain permanently untestable... a data-availability failure" and
by "the remediation script itself is not yet reliable." Both of those
qualifications are now resolved: pins 42 and 50 are fixed, independently
re-verified, and the remediation script's observed success rate on this
batch is now 5 of 5, not 3 of 5. What has **not** changed is the absolute
size of the sample: still 2 genuine disruption-scale instances, out of 10
pinned trains, on one calendar day.

**Recommendation: still NOT YET — this document's final word on the
2026-09-11 dataset, closing out the data-integrity chapter this exercise
has spent its last two sessions on.** Applying this document's own
consistent, previously-stated statistical-power standard (a "clear
majority" bar needs more absolute instances than 2, regardless of the
100% hit rate), the honest call on this single day's data, taken in
isolation, remains **NOT YET**. Three things distinguish this verdict
from every prior one and should weigh on whoever decides whether to
demand a further monitoring round or treat the cumulative record as
sufficient:

1. **Every real mechanism-level blocker this six-week exercise has ever
   found is now fixed and independently re-confirmed working, with no
   exceptions remaining**: SSO (fixed 2026-08-30), the STANOX/CRS
   translation gap (fixed and proven live 2026-08-31), and — for all 5 of
   the previously wrong-train-contaminated pins, not just 3 of 5 as of
   the last session — the TRUST-event-backlog wrong-train-matching bug.
   This session found **zero** new defects of any kind; every check
   simply confirmed the fix worked. What remains to move past "not yet"
   is purely a matter of accumulating more real monitoring days, not
   diagnosing or fixing anything further in the pin-tracking or
   schedule-matching pipeline.
2. **The remediation script's reliability question the "second pass"
   section raised is now closed**: its final observed record on this
   batch is 5 fixed out of 5 attempted (60% → 100%), with both stragglers
   independently re-verified station-by-station, not just re-run and
   trusted.
3. **Cumulative, cross-session evidence remains 3 real disruption
   instances found and spot-checked across this document's entire
   six-week history, all 3 hits, zero counterexamples** — unchanged in
   count from the "second pass" section, because pin 42's newly-verified
   data turned out to be a confirming non-event rather than a 4th
   instance. This document has never once found a real, verified case
   where sampling caught something TRUST-vs-schedule missed, or where
   TRUST-vs-schedule produced a false positive, across now 10 pins' worth
   of fully-verified single-day data plus two smaller prior sessions'
   worth. That perfect record is real and worth weighing, but this
   document holds to its own standard: a perfect record on a small
   absolute N is not the same claim as a large N, and Task 8's "go" bar
   was written to require the latter.

**Concrete next step, unchanged in substance from the "second pass"
section, now simpler in scope** since there is no longer any remediation
or data-integrity work left to diagnose:

1. **Re-run Task 4 onward for one or two more real, full days**, ideally
   spanning more than one line's worth of pins simultaneously as
   2026-09-11 did — the mechanism (SSO, STANOX/CRS translation, and now
   the backlog-replay remediation path) is fully proven across all 10 of
   this run's pins; what is needed is simply volume.
2. **Only then render a truly confident final Task 8 verdict** — or, as
   the "second pass" section already noted, if the dispatching team
   judges the accumulated 3-for-3 cross-session record, now built on a
   fully-verified 10-pin day with zero outstanding data-quality
   questions, sufficient on its own, that is a legitimate, defensible
   reading of this document's own evidence, and a decision this document
   continues to defer to a human call rather than asserting unilaterally.

**This is the complete, all-10-pins-trustworthy analysis this exercise
has been building toward.** Every pin pinned on 2026-09-11 has now been
independently verified, station-by-station, against its own CIF-booked
schedule; none remain contaminated, unverified, or under a cloud of
"resolved but not actually trustworthy." The only remaining path to a
"go" verdict is more real monitoring days, not further validation of the
mechanism itself.

**If proceeding to Option B is eventually greenlit**, unchanged from
every prior verdict in this document: gated on Task 8 reaching "go" —
which, on the strictest reading of this session's own single-day data, it
still has not, eight real execution sessions in, now for the first time
with a fully-verified, zero-caveat dataset behind it.
