# Journey Ticket Tracking — Design Sketch

**Status: sketch/proposal only, not an approved design.** Written to the
same rigor as the existing specs in this directory (e.g.
`docs/superpowers/specs/2026-08-28-train-tracking-design.md` and
`docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md`) so it can
be reviewed and iterated on the same way, but it has not gone through
implementation planning and nothing here is committed. It does **not**
contain a task-by-task implementation plan. This document is the output of
a research pass whose brief was explicitly to assess plausibility first —
see "Verdict" immediately below — and only sketch a design if that
assessment came out positive.

## Verdict

**Plausible, but only in a specific, narrower shape than "ticket
tracking" first suggests.** Full ticket-barcode decoding (RSP-6/Aztec) and
ITSO smartcard data are **not realistically accessible** to a hobby-scale
open-source project (see Research summary below) and this design
explicitly excludes both. What **is** plausible, and is the substance of
this design, is three complementary, ascending-effort ways for a user to
tell this app "I have a ticket for this journey" — manual entry (always
available, zero new data-access relationship), and two forms of
best-effort auto-fill from ticket files the user already possesses:
Apple Wallet `.pkpass` boarding passes (an openly-documented container
format) and PDF e-tickets (native embedded text, not OCR). None of the
three ever decodes a barcode or touches ITSO smartcard data. The payoff —
the actual reason to build this at all — is surfacing likely **Delay
Repay** eligibility against the real delay data this app's in-progress
train-tracking feature already collects, and linking the user to their
operator's own claim form. Nothing here submits a claim, asserts proof of
travel, or touches payment data.

## Problem

The in-progress train-tracking feature
(`docs/superpowers/specs/2026-08-28-train-tracking-design.md`) will let a
user pin a specific `(train_uid, service_date)` and get back TRUST-sourced
real delay/position data for it. That data answers "how did this train
actually run" but has no concept of *why a passenger cares* — specifically,
UK rail's single most concrete, money-on-the-table use of "my train was
late" is **Delay Repay** compensation, and today a user tracking a train
in this app still has to separately remember they had a ticket for it,
work out which operator's claim form applies, and manually check the
percentage/threshold rules themselves. This document researches whether
this app can plausibly close that gap — let a user say "I was travelling
on this journey" and have the app tell them "you were delayed 22 minutes
against a service with a 15-minute Delay Repay threshold — you're likely
owed 25% back, claim here" — without requiring this app to acquire any
new TOC/RDG/ITSO commercial data relationship, and without materially
changing what kind of personal data this app now holds now that it has
real user accounts (`docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md`).

## Goals

1. Let a user record "I have/had a ticket for this journey" against a
   tracked train, via manual entry as the durable, always-available
   baseline — operator, origin/destination, scheduled departure, ticket
   type (single/return/season/etc.), with **no barcode/ITSO/payment data
   of any kind**.
2. Offer two **best-effort, review-before-save** auto-fill paths on top of
   manual entry, for the subset of tickets in formats this app can
   honestly parse without any barcode/ITSO decoding: `.pkpass` files
   (Apple Wallet boarding passes) and PDF e-tickets (native text layer).
   Both only ever pre-populate the same manual-entry form fields — the
   user reviews and confirms before anything is saved, exactly matching
   this app's existing posture of never trusting an inferred field
   silently (DESIGN.md's `dataQuality` philosophy, §5.5).
3. Once a ticket record exists against a *resolved* tracked train (i.e.
   train-tracking's TRUST-sourced `train_current_state` has real delay
   data for it), derive a plain-language Delay Repay eligibility estimate
   — threshold band, rough compensation percentage, a link to the
   relevant operator's own claim page — using a small, explicit,
   maintained-in-this-repo ruleset, honestly labelled as
   community-sourced rather than fed by any official API (none exists —
   see Research summary).
4. Keep the legal/privacy footprint of this feature honestly assessed and
   deliberately minimized: no payment data, no barcode/ITSO data, no
   proof-of-travel assertions made to any operator, no automated claim
   submission.

## Non-goals (this pass)

- **RSP-6/Aztec barcode decoding.** Investigated in depth (see Research
  summary) and explicitly rejected for this app: no official public spec
  exists, and the only working decoder found in this research pass is
  built on reverse-engineered RSA keys obtained by decompiling ticket
  inspector apps — a materially different, and much riskier, legal
  posture than reading an openly-documented file format. Not part of this
  design, not a "v2" either, unless that access picture changes.
- **ITSO smartcard data.** Gated behind ITSO Ltd's own accreditation/
  membership process, not a public spec or open API — out of scope for
  the same "no accessible route for a hobby project" reason as barcodes.
- **Automated Delay Repay claim submission.** This app will surface
  eligibility and link out; it will never submit a claim to an operator
  on the user's behalf, or assert to any third party that the user
  actually travelled. See Legal/privacy section for why this line is
  drawn deliberately, not just for scope-control reasons.
- **Payment/price/refund data of any kind.** No card numbers, no prices
  paid, no refund processing. A ticket record in this design is journey
  metadata only.
- **OCR of scanned/photographed tickets.** PDF parsing here means the
  native embedded text layer only, per Goal 2 — a photo of a paper ticket
  is out of scope (falls back to manual entry).
- **Frontend UI design.** Sketched only, deferred to its own follow-up
  doc per this repo's established convention (train-tracking's design did
  the same).
- **An implementation plan.** See the status note at the top.

## Research summary

### 1. Barcode and smartcard data: not accessible

**RSP-6 (Aztec) ticket barcodes.** RSP (Rail Settlement Plan, part of
ATOC/Rail Delivery Group) approved the mobile/self-print barcode standard
in 2008, and it is genuinely not publicly published — a Freedom of
Information request for the spec went unanswered, and RDG, being a
private company, isn't obliged to respond to FOI requests at all
([WhatDoTheyKnow: RSP-6 Specification for Barcodes on UK Rail
Tickets](https://www.whatdotheyknow.com/request/rsp_6_specification_for_barcodes_2)).
The only detailed technical account found in this research pass is an
independent reverse-engineering effort
([eta.st: "Reversing UK mobile rail tickets"](https://eta.st/2023/01/31/rail-tickets.html),
covered by [Hackaday](https://hackaday.com/2023/02/09/reverse-engineering-british-rail-tickets/)):
the payload is RSA-signed (PKCS#1), base26-encoded, and contains origin/
destination, journey/date/departure-time, ticket type, railcard status,
and a 2-character issuer ID. **Decoding it requires the issuer's RSA
public key**, which the author obtained by decompiling ticket-inspector
apps (Masabi, ttkMobile) — not from any published source. Building a
production feature on top of reverse-engineered keys extracted from
decompiled third-party apps is a fundamentally different legal posture
than reading an openly-documented format (see Non-goals) — this alone is
enough to rule out barcode decoding for this app, independent of the
technical difficulty.

**ITSO smartcard data.** ITSO Ltd (a non-profit standards body, DfT-backed
for rail franchise requirements) maintains ITSO Technical Specification
1000, but real access "generally involves engaging with ITSO's
accreditation and certification process rather than a fully open public
download" ([ITSO: Technical
Specification](https://www.itso.org.uk/itso-specification/itso-technical-specification);
[ITSO Ltd — Wikipedia](https://en.wikipedia.org/wiki/ITSO_Ltd)). Same
conclusion as barcodes: no route accessible to a hobby project.

### 2. `.pkpass` (Apple Wallet PassKit): openly documented, real UK usage, one open question

Unlike RSP-6, the `.pkpass` container format is **published directly by
Apple**: a pass is a signed ZIP bundle containing `pass.json` (plain
JSON, not barcode-encoded), a `manifest.json` + signature, and images
([Apple: Wallet Developer Guide — Pass Design and
Creation](https://developer.apple.com/library/archive/documentation/UserExperience/Conceptual/PassKit_PG/Creating.html)).
A boarding-pass-style pass sets `"boardingPass"` at the top level with a
required `transitType` field — `PKTransitTypeTrain` is one of five defined
values (`Air`/`Boat`/`Bus`/`Generic`/`Train`) — which alone confirms
"this is a rail ticket" without touching any barcode. Fields are laid out
in `headerFields`/`primaryFields`/`secondaryFields`/`auxiliaryFields`/
`backFields` arrays of arbitrary issuer-chosen label/value pairs (e.g.
"FROM"/"Kings Cross"). Apple additionally defines a `semantics` dictionary
of *standardised* machine-readable keys for exactly this use case —
`departureStationName`/`destinationStationName`,
`departureLocation`/`destinationLocation`, `currentDepartureDate`/
`currentArrivalDate`, `transitStatus`/`transitStatusReason`, `carNumber`,
`vehicleNumber` — confirmed present in Apple's semantic-tags schema as
implemented by third-party pass libraries
([Passcreator: Apple Wallet Semantic
Tags](https://developer.passcreator.com/en/apple-wallet/semantic-tags)).
Real UK rail Wallet usage is confirmed, if uneven across operators/
retailers: LNER and Trainline both support adding tickets to Apple
Wallet today; Avanti's support has fluctuated (suspended for a period);
other operators (GWR and others) lean on in-app mobile tickets or PDF
rather than true Wallet passes
([RailUK Forums: multiple threads, e.g. "Apple wallet
e-tickets"](https://www.railforums.co.uk/threads/apple-wallet-e-tickets.222524/);
[RailUK Forums: "LNER Apple Wallet
issue"](https://www.railforums.co.uk/threads/lner-apple-wallet-issue.265072/)).
One of those threads independently confirms the format is exactly what
Apple documents: a user who unzipped an LNER `.pkpass` found a readable
`pass.json` with a `relevantDate` field, and diagnosed a real display bug
from its value — i.e. this is not a theoretical parseability claim, a
member of the public did exactly this and read plain JSON out of a real
UK rail ticket.

**The one open question this research pass could not close**: no example
`pass.json` from an actual UK train operator/retailer (Trainline, LNER)
was found to confirm whether they populate the standardised `semantics`
dictionary, or only the freeform label/value field arrays. If `semantics`
is populated, extraction is close to trivial and issuer-agnostic. If not,
extraction still works but needs a small per-issuer heuristic (matching
known label strings like "FROM"/"TO" to their values) — meaningfully
easier and more stable than PDF parsing below, since it's still
structured JSON rather than freeform text/layout, but not the
zero-maintenance ideal. **This needs to be resolved against one or two
real sample passes before implementation**, not assumed either way — see
Open Questions.

Signature/manifest verification is not needed to *read* the data: the
signature exists so a Wallet app can trust a pass came from its claimed
issuer before displaying/updating it, which is irrelevant when the user
is handing the app a pass they already possess and trust as their own —
reading `pass.json` directly out of the ZIP, unverified, is sufficient
for this feature's purpose (pre-filling a form the user then reviews).

**Rust tooling**: `.pkpass` is a ZIP + JSON, parseable with the standard
`zip` + `serde_json` crates directly, no dedicated dependency strictly
required. Existing pure-Rust crates also exist if a higher-level API is
preferred — `passes`/`passes-rs` (MIT,
[github.com/mvodya/passes-rs](https://github.com/mvodya/passes-rs))
explicitly supports "reading & parsing .pkpass files," alongside its
generation-focused sibling `pkpass` on crates.io — though these lean
toward pass *generation* as their primary use case, so a hand-rolled
`zip`+`serde_json` reader may be the better fit for a read-only need,
consistent with this app's existing preference for narrow, self-owned
code over a framework-shaped dependency (the same reasoning the
account-system design gives for hand-rolling session storage instead of
`tower-sessions`/`axum-login`).

### 3. PDF e-tickets: real, but structurally fragile

Many UK e-tickets (Trainline, LNER, others) are PDFs containing an Aztec
barcode *plus* human-readable text — route, date, departure time, ticket
type, price — as a native, selectable text layer rather than a scanned
image, which is exactly what makes text extraction (not OCR) a real
option. The Rust PDF-extraction landscape is active and maintained as of
this research pass: `pdf-extract` and `lopdf` remain the two foundational
crates and both show recent activity; newer higher-level crates
(`pdfsink-rs`, `pdf-inspector`, `pdfluent-extract`) are built on top of
them for text/layout/table extraction, with `pdfsink-rs` specifically
noting it detects text-based-vs-scanned PDFs and skips OCR for the
majority that don't need it. None of this requires touching the
embedded barcode.

**The honest risk, not found to have a clean answer**: this research pass
found no evidence of a standardised UK rail ticket PDF layout across
retailers. Unlike `.pkpass`'s Apple-defined `semantics` schema, PDF text
extraction gets back an unstructured stream of strings with no field
labels at all — parsing "18:32 London Waterloo to Woking, Off-Peak Day
Single" back into structured fields is a per-retailer-template regex/
heuristic problem, more fragile than the `.pkpass` case even in its
worse-case (`semantics`-absent) scenario, and one that will silently
break whenever a retailer changes its PDF template. **Recommendation:
treat PDF parsing as a genuinely best-effort, lower-confidence tier**
below `.pkpass` — attempt a small number of known-retailer templates,
and fall through to the manual-entry form (pre-filled with whatever
partial match succeeded, or empty) rather than ever blocking on a
confident parse. This is not a reason to exclude PDF parsing (see Goal 2
and the Verdict above), but it is a reason its confidence/maintenance
cost is real and should be sized honestly, not glossed over.

### 4. Delay Repay: real rules, no public API, and a platform in flux

**The rules are real, genuinely useful, and genuinely TOC-siloed.** Most
operators use "Delay Repay 15" (DR15): 25% of the affected fare for a
15–29 minute delay, 50% for 30–59, 100%+ beyond that. A minority — LNER,
CrossCountry, ScotRail among them — still run the older "Delay Repay 30"
(DR30) scheme, with no 15–29 minute band at all, so a 25-minute delay on
those operators pays nothing (multiple secondary sources, e.g.
[Railed: "Train Delay Compensation by Operator: 15 vs 30 Minute Delay
Repay"](https://gotrailed.co.uk/blog/train-delay-compensation-by-operator/)).
Delay Repay is calculated on arrival delay at the passenger's final
destination, and a delay/cancellation *published before* the ticket was
bought is not claimable. National Rail's own compensation page confirms
the shape ("each train company has its own compensation threshold") but,
notably, **provides no threshold table, no API, and no third-party
submission path** — it directs passengers to claim "directly from your
train company," online or by post, and only mentions that "some train
companies allow customers to register certain kinds of tickets online to
make future claims quicker" as their only nod toward automation
([National Rail: Compensation and
refunds](https://www.nationalrail.co.uk/help-and-assistance/compensation-and-refunds/)).
No RDG or NRE API for eligibility checking or claim submission was found
anywhere in this research pass.

**A real, in-flight industry change is worth flagging, not ignoring.**
The Rail Delivery Group has a live procurement (2026) for a consolidated
Delay Repay platform — a five-year, £25.4M contract intended to unify
claims across operators ahead of Great British Railways, whose Phase 1
explicitly includes "electronic ticket validation database capability,"
and whose stated goal is letting passengers claim "more directly through
third-party retailers like Trainline for the first time"
([Rail Magazine: "Notice issued for consolidated Delay Repay
platform"](https://www.railmagazine.com/news/notice-issued-for-consolidated-delay-repay-platform);
[GOV.UK: "Delay Repay changes will make rail travel easier under Great
British Railways"](https://www.gov.uk/government/news/delay-repay-changes-will-make-rail-travel-easier-under-great-british-railways)).
This has real implications for this design: (a) it confirms there is
currently no such integration to build against, so this design's
"eligibility estimate + link out" scope is the honest ceiling today, and
(b) the threshold table this design proposes maintaining by hand (see
Data model) may become obsolete or supersede-able by an official source
within the lifetime of this feature — worth a note to re-check, not a
reason to wait indefinitely.

### 5. Prior art: proven at exactly this scale, with one real cautionary tale

**Trainline** ships an in-app Delay Repay *notification* feature: it
tracks a booked journey, and once complete, alerts the user by push/email
with a direct link to the responsible operator's claim form — it never
submits a claim itself, and responsibility for paying stays with the
operator, not the retailer
([Railed: "Trainline Delay Repay: Who Do You Claim
From?"](https://gotrailed.co.uk/blog/trainline-delay-repay-who-to-claim-from/)).
This is effectively the same "cross-reference tracked delay data with a
known journey, then link out" shape this design proposes, at commercial
scale, and never automates the claim itself either.

**TrainPal** similarly stops at "check live status, identify the
responsible operator, direct you to their claim form" — it doesn't submit
claims or pay compensation directly
([TrainPal: "Delay Repay
explained"](https://www.mytrainpal.com/guide/delay-repay-explained)).

**Delay Repay Sniper is the important cautionary tale, not a template to
copy.** It was a subscription web portal that collated publicly available
delay/cancellation data and let users submit their own journey data to
match against it, similar in spirit to this design's manual-entry path —
but it went further and used that matching to *submit claims on the
user's behalf*. That crossed into real trouble: GTR (Govia Thameslink
Railway) investigated users who admitted using it, demanded repayment of
compensation (in one documented case, 100% of everything a user had ever
received), and multiple other users reported similar demands, with the
underlying concern being that self-reported journey data, submitted as a
claim without independent proof of travel, can't be distinguished from a
journey the user never actually took
([abcommuters: "Commuters Beware — delay repay could get you fined for
doing absolutely nothing
wrong!"](https://abcommuters.com/2018/04/26/commuters-beware-delay-repay-could-get-you-fined-for-doing-absolutely-nothing-wrong/);
[RailUK Forums: "Delay compensation & Delay Repay
Sniper"](https://www.railforums.co.uk/threads/delay-compensation-delay-repay-sniper.115097/)).
The service appears defunct as of this research pass. **The direct
design lesson: never submit a claim on the user's behalf, and never
assert proof of travel to an operator** — this app has no way to verify a
user actually boarded a specific train (this is exactly the same honest
limit the train-tracking design already accepts for TRUST/Darwin
correlation), and claiming otherwise to a TOC is the specific behaviour
that got Sniper's users in trouble. Stopping at "here's your likely
eligibility and a link to claim it yourself" — Trainline's and TrainPal's
posture, not Sniper's — is both the safer and the already-market-proven
shape.

## Data model

New table, hanging off the existing `users` table
(`docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 1) and
the in-progress `tracked_trains` table
(`docs/superpowers/plans/2026-08-28-train-tracking.md`'s Task 1) — this
feature has no reason to exist independent of a tracked train, so it is
naturally a child of that table, not a parallel concept:

```sql
CREATE TABLE tracked_train_tickets (
    id BIGSERIAL PRIMARY KEY,
    tracked_train_id BIGINT NOT NULL REFERENCES tracked_trains(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),  -- redundant with
                                                   -- tracked_trains.user_id
                                                   -- by construction, kept
                                                   -- explicit so ownership
                                                   -- checks on this table
                                                   -- never require a join.

    operator TEXT,           -- free text or a known operator code; not
                              -- validated against a hard catalogue in v1.
    ticket_type TEXT,        -- e.g. "single", "return", "season",
                              -- "advance" -- user-entered or auto-filled,
                              -- never parsed from a barcode.
    origin_crs TEXT,
    destination_crs TEXT,

    -- Provenance -- extending DESIGN.md §5.5's existing dataQuality
    -- philosophy of never collapsing inferred data into an unlabelled
    -- value. "manual" is the only trustworthy-by-construction source;
    -- "pkpass-semantics" / "pkpass-heuristic" / "pdf-heuristic" are all
    -- pre-fills the user reviewed and explicitly confirmed before this
    -- row was created -- confirmation, not the parse itself, is what
    -- makes the row trustworthy.
    source TEXT NOT NULL DEFAULT 'manual'
        CHECK (source IN ('manual', 'pkpass-semantics', 'pkpass-heuristic', 'pdf-heuristic')),

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX tracked_train_tickets_tracked_train ON tracked_train_tickets (tracked_train_id);
```

Deliberately **not** stored: passenger name (present on many pkpass/PDF
tickets — must be discarded during extraction, never persisted, matching
DESIGN.md's restraint of not storing a field this app has no feature need
for), price/payment data, any barcode payload (raw or decoded), any ITSO
data, any file upload itself past the point of extraction (the uploaded
`.pkpass`/PDF is processed transiently and discarded, not retained as a
blob — reduces both storage and the sensitivity of what's at rest).

## Delay Repay eligibility derivation

A pure function, in the same style as train-tracking's proposed
position-in-journey/ETA-propagation functions
(`crates/aggregator/src/matcher.rs`'s existing purity precedent):

```
fn estimate_delay_repay(
    operator: &str,
    delay_minutes: i32,
) -> Option<DelayRepayEstimate>
```

backed by a small, explicitly-maintained-in-this-repo static table (e.g.
`crates/api/src/data/delay_repay_rules.rs`) of `(operator, scheme)` pairs
— DR15 vs DR30, per operator — plus the fixed percentage bands each
scheme defines. **This table has no official source to sync against** (see
Research summary §4) — it is compiled from secondary sources and must be
labelled as such wherever it's surfaced (`"estimate, not a guarantee —
verify with [operator]'s own Delay Repay page"`, with a direct link),
mirroring this app's existing comfort with explainable heuristics over
false authority (DESIGN.md §6.1's severity classifier takes the same
posture). The estimate is: given a resolved tracked train's
`train_current_state.delay_minutes` (already computed by train-tracking)
and a linked ticket's `operator`, look up the scheme, find the matching
band, and return a percentage + a link to that operator's own Delay Repay
page (a second small static table, since claim-page URLs are also not
available from any API). No claim is ever constructed or submitted by
this app.

## Ingestion: three tiers, same destination

All three tiers write to the same `tracked_train_tickets` row shape via
the same review step — nothing skips user confirmation, including
`.pkpass`/PDF pre-fills:

1. **Manual entry** (always available): a form matching the table's
   columns above, presented when linking a ticket to an already-tracked
   train. No new backend capability beyond a straightforward insert —
   this is the actual v1 backbone; everything else is acceleration on
   top of it, not a replacement for it.
2. **`.pkpass` upload** (best-effort auto-fill): user uploads a `.pkpass`
   file (e.g. exported from their Wallet app, or the retailer's original
   email attachment); the app reads `pass.json` directly (no signature
   verification needed, per Research summary §2), checks
   `boardingPass.transitType == "PKTransitTypeTrain"` as a sanity gate,
   attempts the `semantics` dictionary first, falls back to a small
   per-known-issuer label/value heuristic, and pre-fills the manual-entry
   form with whatever it found (never all fields with equal confidence —
   surface which fields were auto-filled vs. left blank). Processed
   transiently; the uploaded file itself is not persisted (see Data
   model).
3. **PDF upload** (best-effort auto-fill, lower confidence): same
   review-before-save shape, using native PDF text extraction
   (`pdf-extract`/`lopdf`) against a small set of known-retailer
   templates; anything unmatched is left blank for manual completion
   rather than guessed at.

## Legal/privacy assessment

**This is meaningfully lower-risk than "ticket tracking" sounds at face
value, provided the design stays inside the lines drawn above** — the
load-bearing constraints are (a) no barcode/ITSO data ever touches this
app, (b) no payment data, (c) no file retention past transient parsing,
(d) no passenger-name persistence, and (e) no claim submission or
proof-of-travel assertion to any third party. Given all five hold:

- **Personal data classification.** `tracked_train_tickets` rows are
  ordinary personal data (linked to an authenticated `user_id`, same as
  every other owned table this app now has) — journey metadata
  (operator, route, ticket type, date) is not, on its own, UK GDPR
  Article 9 "special category" data. The ICO's own guidance is clear that
  ordinary data can *become* special-category if it lets a controller
  infer a protected characteristic with reasonable confidence (e.g. a
  destination that's a religious site) — a real, if narrow, edge case
  worth being aware of, but not one this feature's schema deliberately
  courts (it stores CRS codes and operator names, not destination
  *purpose*), and not materially different from the risk the train-
  tracking feature itself already carries by storing origin/destination
  station pins.
- **Marginal risk over train-tracking, not a step-change.** Train-
  tracking already established the precedent of a `user_id`-owned table
  storing "this specific journey, on this specific day" — this feature
  adds "...and I had a ticket for it," which is a smaller marginal
  disclosure than the journey-identifying data already being stored, not
  a new category of sensitivity. The genuinely higher-risk version of
  this feature — actual barcode/payment/ITSO data — is exactly what's
  excluded by this design's Non-goals, and that exclusion is doing real
  work: it's the difference between "the app knows what train you were
  interested in" (already true) and "the app holds something that could
  double as proof of purchase/travel" (deliberately avoided).
- **The Delay Repay Sniper precedent is the sharpest concrete risk, and
  it's a product-behaviour risk, not a data-storage one.** The GTR
  fallout wasn't about data protection law — it was about a tool
  functionally vouching for a user's claim without any way to verify it
  actually happened. This design's hard line against ever submitting a
  claim or asserting proof of travel (see Non-goals, and the eligibility
  derivation section's "no claim is ever constructed") is the direct,
  deliberate mitigation, not an incidental scope cut.
- **File upload handling needs its own care at implementation time,
  independent of the above.** Accepting user-uploaded `.pkpass`/PDF files
  server-side means the usual file-upload hygiene applies (size limits,
  content-type/structure validation before parsing, no execution of
  anything embedded) — not researched to implementation depth here, but
  flagged as a real, ordinary engineering requirement, not a novel one
  for this app.

**Bottom line: recording "user X had a ticket for journey Y" without
barcode/payment/ITSO data is a small, well-bounded increment over what
train-tracking + user-accounts already committed this app to, not a
step-change in what kind of service this becomes** — but that conclusion
depends entirely on this design's exclusions holding at implementation
time, not on the feature's name alone.

## Architecture sketch

No new crate, no new persistent-connection service — unlike train-
tracking, this feature is pure request/response CRUD plus a couple of
pure functions, fitting entirely inside `crates/api`:

```
frontend (upload .pkpass/PDF or fill a form)
        │  POST /Train/{trackingId}/tickets  (session-gated, per Task 3's
        │                                      AuthenticatedUser pattern)
        ▼
crates/api
  - routes/train.rs: new sub-routes under the existing /Train/... family
  - data/ticket_extraction.rs: pure fn's, unit-tested --
      parse_pkpass(bytes) -> PartialTicket
      parse_pdf(bytes) -> PartialTicket           (best-effort, may be
                                                     entirely empty)
  - data/delay_repay_rules.rs: static table + estimate_delay_repay()
  - data/train_tracking.rs (extended): insert/read tracked_train_tickets,
      scoped by user_id like every other owned table (per the
      account-system design's ownership pattern)
  - new migration, timestamp-sorted after train-tracking's own
      (crates/api/migrations/20260828120000_train_tracking.sql)
```

`crates/aggregator` is untouched, for the same reason train-tracking's
design keeps it untouched — this is per-user, per-journey data, not
line-level aggregation input.

## Open questions / risks

1. **Whether real UK retailers populate `.pkpass`'s `semantics`
   dictionary is unconfirmed.** This research pass found Apple's schema
   supports exactly the fields needed (`departureStationName` etc.) and
   independent evidence that real UK rail `.pkpass` files are plain,
   readable JSON, but no confirmed real example showing `semantics`
   populated specifically by a UK operator/retailer. Obtain one or two
   real sample passes (e.g. a maintainer's own LNER/Trainline booking)
   before committing to the `semantics`-first extraction strategy — the
   per-issuer label/value fallback should be designed regardless, not as
   a contingency.
2. **PDF layout variability across retailers is a real, unresolved
   maintenance cost**, not just a hypothetical risk — no standardised UK
   rail ticket PDF template was found. Start with the smallest possible
   set of known templates (e.g. just LNER and Trainline) and expect
   silent breakage on retailer template changes; this tier's value
   proposition is weaker than `.pkpass`'s and should be sized/prioritized
   accordingly, not treated as equally reliable.
3. **The Delay Repay threshold/scheme table has no official source and
   will drift.** Needs a periodic manual re-verification process (not
   designed here) against operator Passenger's Charters, and should be
   revisited entirely if RDG's consolidated Delay Repay platform (in
   procurement as of this research pass) ships anything resembling a
   public ruleset or API within this feature's lifetime.
4. **File upload hygiene** (size limits, content validation, no execution
   of embedded content) is flagged but not designed to implementation
   depth in this pass.
5. **Whether to support non-Apple wallet formats (Google Wallet passes)
   is unresearched** — this pass focused on `.pkpass` specifically per
   the brief; Google Wallet uses a different (JWT-based) format that
   would need its own research pass if pursued.
6. **Operator catalogue validation for the `operator`/claim-link static
   table** — this design proposes hand-maintaining operator → scheme →
   claim-URL mappings; whether that should reuse this app's existing TOC
   data source (`poller-tocs`) for operator identity, or stay a fully
   separate small table, is an implementation-level question not
   resolved here.
