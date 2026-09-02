# Ticket File Support Improvements — Research

**Status: research only, not a plan.** No code was changed to produce this
document. Written to the same rigor as
`docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
(verified-against-real-artifacts findings, file:line citations, a
prioritized recommendation list, not a pure survey) — the difference here
is the "real artifact" is two real ticket files the repo owner provided at
the repo root rather than repo files alone.

## Legal/privacy boundary this document operates inside

`crates/api/migrations/20260829090000_journey_ticket_tracking.sql`'s header
(lines 1–16) is a hard, audited constraint: `tracked_train_tickets`
"deliberately stores ONLY operator, ticket_type, origin_crs, destination_crs,
source, and timestamps/ownership. It must NEVER gain a column for
payment/price data, any barcode payload (raw or decoded), any ITSO data,
passenger name, or the uploaded .pkpass/PDF file itself." `ticket_extraction.rs`'s
own module doc (lines 1–7) independently repeats the barcode/ITSO half of
that constraint.

Every recommendation below stays inside that approved five-field set
(`operator`, `ticket_type`, `origin_crs`, `destination_crs`, plus the
already-existing `source` provenance tag). None of them requires a schema
change or a new column. Where a real file contains data outside that set —
price, ticket number, order id, the Aztec barcode payload — this document
says so and explicitly does not propose extracting it. **No barcode payload
from either example file is decoded, logged, reproduced, or quoted anywhere
in this document**; where the barcode is discussed below, it is described
structurally (format, encoding, rough length) only.

## The two example files

Provided by the repo owner at the repo root (not committed to git, and this
document does not commit them either — referenced by filename only, per the
task instructions):

- `LON-MKC-20260830-TRB326R68RB.pdf` — a PDF e-ticket, London Terminals →
  Milton Keynes Central, dated 30 Aug 2026.
- `MKC-LON-20260830-TRB326R68RB.pkpass` — an Apple Wallet pass, Milton
  Keynes Central → London Terminals, same date.

Both carry the same ticket number (`TRB326R68RB`) and the same order id
(`83631061`), and both describe the ticket type as "Super Off-Peak Return" —
these are the outbound and return legs of one round-trip purchase, issued
simultaneously in both formats. That matches the design doc's own research
finding that "an eTicket is delivered simultaneously and identically in
both pdf and pkpass form" (see References).

## Goal

Determine, against real files rather than the synthetic examples the
existing code's own tests and doc comments were written against, whether
today's `.pkpass`/PDF extraction actually works — and if it doesn't, what
concrete, boundary-respecting changes would close the gap, sized and
prioritized rather than listed generically.

## Current relevant state

### Extraction logic — `crates/api/src/data/ticket_extraction.rs`

- `PartialTicket` (lines 19–34) mirrors the approved column set exactly:
  `operator`, `ticket_type`, `origin_crs`, `destination_crs`, `source`. Its
  own doc comment (lines 24–30) asserts `origin_crs`/`destination_crs` are
  "almost never a real CRS code in practice... both give station NAMES,
  e.g. 'Kings Cross'" — a claim this document tests below and finds only
  partially true.
- `.pkpass` parsing (`parse_pass_json`, lines 49–83): requires
  `boardingPass.transitType == "PKTransitTypeTrain"` (line 57–60);
  `operator` comes straight from top-level `organizationName` (lines 62–65);
  origin/destination prefer Apple's structured `semantics` dictionary
  (`departureStationName`/`destinationStationName`, `semantics_origin_destination`,
  lines 85–93) and fall back to a strict positional read of `primaryFields`
  — accepted only if it is *exactly* a two-element array, taken as
  `[origin, destination]` in that order (`primary_fields_origin_destination`,
  lines 95–122). **`ticket_type` is hardcoded to `None` on every `.pkpass`
  parse** (line 78) — there is no code path that reads it from anywhere in
  `pass.json`, confirmed by the module's own test
  `ticket_type_is_never_guessed_at` (lines 224–228). `parse_pkpass` (lines
  139–153) is the thin unzip-and-deserialize wrapper, bounding the
  `pass.json` read at 1 MiB (line 130) as zip-bomb hygiene.
- PDF parsing (`parse_pdf_text`, lines 298–327): `operator` is a literal
  substring match against `KNOWN_RETAILER_MARKERS = ["LNER", "Trainline"]`
  (lines 329–332, explicitly "the smallest possible set of known
  templates" per the design doc's Open Question 2, not meant to be
  exhaustive); `ticket_type` is a literal-substring match (case-insensitive)
  against `TICKET_TYPE_KEYWORDS = ["Anytime Day Single", "Off-Peak Day
  Single", "Off-Peak Day Return", "Advance Single", "Season", "Open
  Return"]` (lines 334–341); origin/destination come from one unanchored
  regex, `ROUTE_PATTERN` (lines 343–363), matching the literal shape
  `<words> to <words>` (`\s+to\s+`) anywhere in the extracted text, with the
  first match in document order winning — explicitly flagged in its own doc
  comment as an approximation "not verified against real tickets," pending
  exactly the kind of real-sample check this document performs.
  `parse_pdf` (lines 413–432) validates the `%PDF-` magic header, runs
  `pdf_extract::extract_text_from_mem` inside `catch_unwind` (crash
  containment for untrusted input), and hands the resulting text to
  `parse_pdf_text`.
- Both paths are read-only preview generators — "this module and every
  function in it NEVER writes to the database" (module doc, lines 3–5) —
  and both are followed by `train_tracking::validate_ticket_entry`
  (`crates/api/src/data/train_tracking.rs:100–115`), which only checks that
  a supplied `origin_crs`/`destination_crs` is exactly 3 letters (lines
  104–113) — **format-only, not checked against any real station list**.
  Its own doc comment (lines 92–99) frames this length check as the actual
  safety net that "guarantees" an unedited auto-fill preview fails
  validation and forces human correction, because extraction "can never
  recover a real CRS code" — a premise this document's pkpass finding below
  directly contradicts for at least one real ticket family.

### Upload routes — `crates/api/src/routes/train.rs`

Four upload endpoints share two handlers: `POST /Train/tickets/pkpass` and
`POST /Train/{trackingId}/tickets/pkpass` both call `handle_pkpass_upload`
(lines 580–593, calling `ticket_extraction::parse_pkpass` at line 584);
`POST /Train/tickets/pdf` and `POST /Train/{trackingId}/tickets/pdf` both
call `handle_pdf_upload` (lines 625ff, running `parse_pdf` inside
`spawn_blocking` per its own doc comment at lines 599–606, since
`pdf_extract` is CPU-bound and untrusted). Both read a single `"file"`
multipart field via the shared `read_single_file_field` helper (line 663+).
A parse failure returns `422` with the extraction error's own message
surfaced verbatim (`.map_err(...)`); this is what
`TicketEntryForm.tsx`'s `response.status === 400`/`422` branches show the
user.

### Frontend — `frontend/components/TicketEntryForm.tsx`

Three tabs — manual, `.pkpass` upload, PDF upload — all converge on the
same four-field form (lines 301–397). `applyPreview` (lines 122–143) copies
whatever the upload response set to the corresponding form field and marks
it `autoFilled`, which renders "Auto-filled — please check this value" (or,
for the CRS fields specifically, "...please check this is a real 3-letter
CRS code," lines 349, 362) — every uploaded field is client-editable and
nothing is auto-submitted. `CRS_PATTERN = /^[A-Za-z]{3}$/` (line 12) is the
client-side mirror of the backend's format-only check — same limitation:
any 3-letter string passes. `handleUpload` (lines 145–195) maps `422`
straight to the backend's own message, `400` to a generic "doesn't look
like a valid upload" message, and every other failure (network error, `504`
timeout, `413` too-large) to a manual-entry fallback prompt via
`UploadPanel`'s `onFallback` button (lines 442–454) — the existing
fallback UX this document's recommendations are meant to feed into, not
replace.

## Findings — real-file inspection

### `.pkpass`: `MKC-LON-20260830-TRB326R68RB.pkpass`

Unzipped and read `pass.json` directly (plain JSON, no barcode payload
touched):

- **`organizationName: "Thameslink Railway"`.** This is a real UK rail
  brand, but it is the *retailer* brand, not necessarily the *operating*
  company for this specific leg — the pass's own `backFields.itinerary`
  text (free-form prose, not a structured field) reads "17:49 WEST MIDLANDS
  TRAINS / From Milton Keynes Central / To London Euston." `organizationName`
  and the operator that actually runs the train can legitimately differ.
  Not a bug in the current code (it never claims to resolve this), but a
  real ambiguity worth naming for anyone tightening `operator` extraction
  later: "whichever brand issued the pass" and "the train operating
  company" are two different things, and this real pass demonstrates the
  gap concretely.
- **`passTypeIdentifier: "pass.4.io.otrl.eticket"`.** "OTRL" is On Track
  Retail — a joint venture between Assertis and the Go-Ahead Group that
  provides the online ticketing platform for all four GTR brands (Southern,
  Gatwick Express, Great Northern, Thameslink) plus Southeastern (see
  References). That means this exact `pass.json` shape is very likely
  shared, verbatim in structure, across at least five separate TOC brands —
  a single template-specific parsing improvement here has multi-operator
  reach, not just single-brand reach. This is inferred from the pass type
  identifier and public reporting on OTRL's client list, not independently
  confirmed against a second OTRL-issued sample in this research pass — see
  Open Questions.
- **No `semantics` dictionary present anywhere in `boardingPass`.** This
  real pass answers the design doc's Open Question 1 (line 183–194 of that
  doc) for at least this one OTRL-issued sample: it does **not** populate
  Apple's standardised `semantics` keys, so `parse_pass_json` always falls
  through to the `primaryFields`-positional heuristic (`source:
  "pkpass-heuristic"`) for this ticket family, never the
  `"pkpass-semantics"` tier.
- **`primaryFields` is exactly the two-entry shape the heuristic expects,
  and it already extracts correctly** — `depart`/`value: "MKC"` and
  `arrive`/`value: "LON"`. This is the first concrete finding that
  contradicts the module doc's "almost never a real CRS code... both give
  station NAMES" framing (`ticket_extraction.rs:24–30`): this real pass's
  `primaryFields.value` entries are already 3-letter, upper-case,
  CRS-shaped strings, not full station names (the full names — "MILTON
  KEYNES CENTRAL", "LONDON TERMINALS" — are in the adjacent `label` field,
  which the code correctly ignores). For this real ticket, `parse_pass_json`
  today produces `origin_crs: "MKC"` and `destination_crs: "LON"` with zero
  changes needed.
- **But `"LON"` is a real gap, just not a parsing gap: it is CRS code
  "London Terminals," a fare/ticketing *umbrella* code covering multiple
  physical stations, not a single station with its own live departure
  board.** The pass's own itinerary text names the actual arrival station
  as "London Euston" (CRS `EUS`). `"LON"` is 3 letters, passes both the
  backend's and frontend's format-only CRS check unedited, and would save
  successfully as a tracked ticket's destination — but a live-train lookup
  keyed on `"LON"` cannot correspond to one real station the way `"EUS"`
  can. This directly undercuts `validate_ticket_entry`'s own doc comment
  (`train_tracking.rs:92–99`), which frames the 3-letter check as
  *guaranteeing* human correction because extraction "can never recover a
  real CRS code" — for this real ticket, extraction recovers a
  format-valid-but-wrong-granularity code that the existing safety net does
  not catch.
- **`ticket_type` is available, structured, and currently never read.**
  `boardingPass.auxiliaryFields` contains `{"key": "ticketType", "label":
  "TICKET TYPE", "value": "Super Off-Peak Return"}` — a clean, directly
  keyed field, sitting in the same JSON the code already parses for
  `primaryFields`. `parse_pass_json` never looks at `auxiliaryFields` at
  all; `ticket_type` is hardcoded `None` (line 78). This is the single
  cleanest, lowest-risk finding in this document: the data is present,
  structured, keyed, already inside the approved field set, and simply not
  read.
- **Compliance check, confirmed correct**: the pass's `barcode.message` (an
  Aztec-format, ISO-8859-1-encoded string) and `backFields` prose (which
  contains the price, "£13.10," the ticket number, and the order id, all
  in one free-text block alongside the itinerary) are never touched by
  `parse_pass_json` — correctly, since none of those are in the approved
  column set. Worth flagging for any future contributor: `backFields` is a
  real temptation trap, since itinerary text (arguably useful) sits in the
  exact same free-text field as price and order id (definitely
  off-limits) — any future code reading `backFields` at all needs to be
  reviewed against the legal boundary specifically, not just "does it
  parse."

### PDF: `LON-MKC-20260830-TRB326R68RB.pdf`

Read via the sandbox's native PDF text-layer extraction (equivalent in kind
to what `pdf_extract` recovers — a text-layer read, not OCR, not a barcode
decode). The full extracted text block, in document order:

```
TRB326R68RB
= 30 Aug 2026 Ret: LON - MKC
LONDON TERMINALS MILTON KEYNES CENTRAL
LON ß MKC
TICKET TYPE ROUTE
Super Off-Peak Return LNR ONLY
ADULT VALID UNTIL
16-25 Railcard 29 Sep 2026
Ticket Details:
...
Ticket Number TRB326R68RB
Price £13.10
Purchased on 30 August 2026
Contact us by phone 0345 026 4700
...
```

- **The visible "ThamesLink" wordmark never appears in the extracted text
  layer at all.** It is rendered purely as a vector/image logo graphic in
  the PDF, not as selectable text. This means `KNOWN_RETAILER_MARKERS`
  cannot match this ticket today, and **adding "ThamesLink" (or any other
  string) to that list would not fix it either** — the string genuinely
  isn't in the text stream `pdf_extract` returns. Recovering an
  operator/retailer name from this specific template requires either OCR
  over the logo image or accepting that `operator` stays unfilled for this
  whole ticket family. This is a structurally different, and materially
  bigger, problem than "the marker list is too short."
- **The route line does not contain the word "to" at all — `ROUTE_PATTERN`
  never matches this real PDF.** The large on-ticket arrow glyph between
  the two station codes extracts as the character `ß`, not `→`, `to`, or
  any word — a font/glyph-encoding artifact of how this template's icon
  font maps through `pdf_extract`'s text-layer reader. `ROUTE_PATTERN`'s
  `\s+to\s+` literal never appears anywhere in this document's text, so
  `origin_crs`/`destination_crs` both come back `None` for this real
  ticket, confirming the `ROUTE_PATTERN` doc comment's own caveat
  ("Confirm this against 1-2 real e-ticket PDFs at implementation time...
  this is a starting point, not a pattern verified against real tickets,"
  lines 356–359) was accurate to flag — for this real sample, unverified
  turned out to mean **non-functional**, not just imprecise.
- **A much better-anchored route signal exists two lines above the broken
  arrow, in plain ASCII: `Ret: LON - MKC`.** This is the same
  `<3-letter-code> - <3-letter-code>` shape as the pkpass's own
  `description` field (`"Out: MKC - LON"`) — a hyphen, not the icon glyph,
  and already CRS-shaped with no station-name normalization required at
  all. A template-specific pattern targeting an `Out:`/`Ret:`-prefixed line
  would extract this real ticket's route correctly where the generic
  "X to Y" prose pattern fails outright.
- **`ticket_type` fails for a different, simpler reason: the exact string
  present ("Super Off-Peak Return") is not in `TICKET_TYPE_KEYWORDS`, and
  no listed keyword is a substring of it.** `"Off-Peak Day Return"` does
  not match "Super Off-Peak Return" (no "Day" in the real string). This is
  the cheapest possible gap to close — one more literal string in an
  existing list — but as currently sized, this ticket's `ticket_type` also
  comes back `None`.
- **Net result for this real PDF: `operator`, `ticket_type`, `origin_crs`,
  and `destination_crs` all come back `None` today.** `parse_pdf` still
  returns `Ok` (a `200`, not a `422`) with a fully-empty `PartialTicket` —
  not a crash, not an error message, just a silent no-op that looks
  identical in the UI to "nothing extractable was found," which is exactly
  what happened, just not for the reason a user would guess (the text is
  all there; none of the three heuristics happens to match this template's
  actual layout).
- **Compliance check, confirmed correct**: the visible Aztec barcode image
  and the `Ticket Number`/`Price`/`Purchased on`/order-id lines in "Ticket
  Details:" are present in the extracted text but never referenced by
  `parse_pdf_text`'s regex/keyword logic — correctly out of scope already.

## Findings — broader format landscape (scoping, not a build list)

Per the existing design doc's own Research Summary (§2–§3, cited in
References), the barcode standard (RSP-6/Aztec) and ITSO smartcard data are
already, correctly, permanently out of reach for legal reasons independent
of this document — nothing here changes that. Within the text-layer-only
space this app already operates in:

- **`.pkpass` issuers are known to be uneven**: LNER and Trainline confirmed
  to support Apple Wallet; Avanti's support has fluctuated; GWR and others
  lean on in-app tickets or PDF instead (design doc, §2). This document adds
  one concrete new data point: **OTRL** (GTR's four brands + Southeastern)
  is a fifth, previously-unconfirmed real issuer, and evidently does not
  populate Apple's `semantics` dictionary — the `"pkpass-heuristic"` tier,
  not `"pkpass-semantics"`, is what actually fires for it.
- **PDF retail platforms are plural and not brand-aligned with the
  operating TOC**: this document's PDF sample is issued by a GTR/OTRL brand
  for a West Midlands Trains service — retailer and operator diverge on
  PDFs the same way they can on `.pkpass`. Beyond OTRL and the design doc's
  already-known Trainline/LNER pair, Assertis separately operates a
  distinct platform ("WebTIS") for Arriva Group, Caledonian Sleeper,
  Heathrow Express, Omio, and Uber — a different template family again,
  unexamined in this research pass (no sample obtained). The realistic
  population of "distinct PDF template families in real UK rail retail" is
  therefore at least three known ballparks (Trainline, LNER-direct,
  OTRL) plus at least one more (Assertis WebTIS) not yet sampled, before
  even counting each TOC's own direct-sale website — sizing "full PDF
  coverage" honestly puts it well beyond a short marker-list expansion.
- **A relevant industry signal, already flagged in the design doc (§4) and
  worth repeating here**: RDG's 2026 procurement for a consolidated
  Delay Repay platform explicitly includes "electronic ticket validation
  database capability" as a Great British Railways Phase 1 goal. If that
  ever ships as a real, accessible API, it would be a fundamentally
  different (and likely much better) way to resolve operator/route/ticket-type
  than text-layer heuristics — worth watching, nothing to act on today.

## Recommendations

Ordered by (confirmed value against a real file) ÷ (implementation risk and
size). Every item below stays inside the approved `operator`/`ticket_type`/
`origin_crs`/`destination_crs`/`source` field set; none requires touching
`backFields` prose, the barcode, or any new column.

1. **Read `.pkpass` `ticket_type` from `boardingPass.auxiliaryFields`
   (or `secondaryFields`) by key, the same way `primaryFields` is already
   read.** Highest-priority item in this document: the data is present in
   this real pass, structurally clean (a keyed `{key, label, value}`
   object, not free text), already inside the approved field set, and the
   current code's only reason for not reading it is that it was never
   written to, not that it's hard or risky. `ticket_type` is currently
   hardcoded `None` for every `.pkpass` (`ticket_extraction.rs:78`); this
   closes that gap without touching anything the legal boundary restricts.
2. **Add an OTRL-template-specific PDF heuristic**, extending the design
   doc's own already-endorsed "small number of known-retailer templates"
   approach (Research Summary §3) rather than trying to generalize
   `ROUTE_PATTERN`: detect this template (e.g. presence of a "Ticket
   Details:" section header alongside "TICKET TYPE"/"ROUTE" column labels)
   and, when detected, extract origin/destination from the `Out:`/`Ret:`
   line's `<CODE> - <CODE>` shape instead of the generic "X to Y" pattern
   this real PDF never matches. Because OTRL is confirmed to serve at least
   five TOC brands' online retail, one template match plausibly unlocks a
   meaningfully larger real-world slice than the current two-brand
   (`LNER`/`Trainline`) marker list, for a bounded, template-scoped amount
   of new regex/matching code — the same "genuinely best-effort,
   lower-confidence tier" framing the design doc already set for PDF
   parsing applies unchanged.
3. **Add "Super Off-Peak Return" (and likely sibling variants — "Super
   Off-Peak Single," "Super Off-Peak Day Return" — unconfirmed without more
   samples) to `TICKET_TYPE_KEYWORDS`.** Cheapest possible fix in this
   document, one literal string in an existing list, directly justified by
   this real PDF's exact ticket-type text failing to match today.
4. **Distinguish "format-valid but not a trackable single station" CRS
   codes in the existing auto-fill review copy**, not by rejecting them:
   `TicketEntryForm.tsx`'s existing "Auto-filled — please check this is a
   real 3-letter CRS code" strings (lines 349, 362) already exist for
   exactly this kind of "don't trust the auto-fill blindly" moment; a small
   known-umbrella-code list (`LON` = "London Terminals" is the one
   confirmed here; there are a handful of others in UK rail fare data, not
   catalogued in this pass) could swap in a more specific warning when one
   of these is auto-filled, e.g. naming that it covers multiple stations
   and asking the user to pick the specific one. This is a UX/copy change
   inside the existing reviewed-before-save flow, not a parsing change, and
   stays inside the approved field set (no new column, no change to what
   `validate_ticket_entry` accepts) — scoped separately from items 1–3
   because it touches the frontend, not `ticket_extraction.rs`, and needs
   its own small reference list rather than piggybacking on either upload
   parser.
5. **Explicitly not recommended for now: OCR over PDF logo/branding
   images to recover `operator` for logo-only-branded templates** (this
   PDF's ThamesLink wordmark is exactly this case). Nothing about OCR
   itself crosses the legal boundary — `operator` stays an approved,
   optional, free-text field either way — but it's a materially larger
   engineering lift (an image-analysis pipeline, not a text/regex
   extension) for a field that's already optional and already has a
   working manual-entry fallback. Flagging as a real, scoped-out idea
   rather than silently omitting it, per this document's brief — someone
   should make a deliberate call on this, not inherit it by default.

None of the above requires expanding `tracked_train_tickets`' approved
column set — no item here needs a separate legal/product decision before
it could be implemented.

## Explicitly out of scope

- Barcode decoding (Aztec/RSP-6) or ITSO smartcard data, in any form — see
  the design doc's Research Summary §1 for why this is a legal dead end
  independent of this document, not merely undesired.
- Persisting, logging, or displaying anything from either example file's
  `barcode` field, `backFields` price/ticket-number/order-id text, or the
  file itself.
- A full per-TOC/per-retailer PDF template library. This document confirms
  at least four distinct real-world template families exist (Trainline,
  LNER-direct, OTRL, Assertis WebTIS) with only one (OTRL) actually sampled
  here — building out full coverage is a much larger effort than this
  document's recommendations, sized only as "the landscape is at least this
  big," not planned task-by-task.
- Google Wallet or other non-Apple pass formats — not investigated in this
  pass; no example provided.
- Changing what `validate_ticket_entry` accepts (e.g. checking CRS codes
  against a real station list). That function's job and the umbrella-code
  problem it doesn't catch are both real, but changing its validation logic
  is a `train_tracking.rs` change with its own blast radius, not a ticket
  *extraction* change — out of this document's scope, noted only as
  context for Recommendation 4's UI-copy alternative.
- Any change to what counts as a "known retailer" for `operator` beyond the
  specific, confirmed gap in Finding "PDF" above (the OTRL/ThamesLink logo
  case) — a general survey of every UK TOC's own PDF branding was not
  performed.

## Open questions/risks

- **Only one OTRL-issued sample was inspected.** Whether Southern, Gatwick
  Express, Great Northern, and Southeastern's own OTRL-issued `.pkpass`/PDF
  files share this exact field layout (not just the same backend platform)
  is inferred from public reporting on OTRL's client list, not confirmed
  against a second sample — worth confirming with one more real file from a
  different OTRL brand before Recommendation 2 is implemented, the same
  "confirm against 1-2 real samples before relying on it" discipline the
  design doc already applied to `.pkpass` `semantics` and PDF `ROUTE_PATTERN`.
- **Is the `ß`-for-arrow-glyph mangling stable?** This document found it in
  one PDF, generated once, on one date. Whether `pdf_extract` maps this
  template's icon font to `ß` consistently, or whether it varies by PDF
  generator version/font-subsetting, is unknown — this is exactly why
  Recommendation 2 targets the plain-ASCII `Out:`/`Ret:` line instead of
  trying to pattern-match the glyph itself.
- **No catalogue of umbrella/group CRS codes (like `LON`) exists in this
  repo today.** `docs/superpowers/specs/2026-09-01-stanox-crs-live-reference-data-research.md`
  does not mention group codes at all (checked directly for this document);
  Recommendation 4 would need its own small reference list, sourced
  separately — sizing that list (how many umbrella codes exist in UK rail
  fare data, e.g. `LON`, possibly Manchester/Birmingham-area equivalents)
  was not attempted here.
- **The RDG electronic-ticket-validation procurement** (design doc §4) is a
  real but not-yet-live industry change that could eventually obsolete some
  or all of this document's heuristic-based recommendations — worth a
  periodic check, not a blocker on anything here.

## References

- `crates/api/migrations/20260829090000_journey_ticket_tracking.sql:1–16` —
  the legal/privacy boundary this document operates inside.
- `crates/api/src/data/ticket_extraction.rs` — full file read; specific
  lines cited inline above.
- `crates/api/src/data/train_tracking.rs:81–115` — `TICKET_SOURCES`,
  `validate_ticket_entry`.
- `crates/api/src/routes/train.rs:30–90, 555–670` — upload route
  definitions and handlers.
- `frontend/components/TicketEntryForm.tsx` — full file read; specific
  lines cited inline above.
- `docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md` —
  Research Summary §1–§4 (barcode/ITSO inaccessibility, `.pkpass` Open
  Question 1, PDF fragility and Open Question 2, Delay Repay landscape).
- `docs/superpowers/specs/2026-09-01-stanox-crs-live-reference-data-research.md` —
  checked for umbrella/group CRS coverage; confirmed silent on the topic
  (see Open Questions).
- The two real example files at the repo root:
  `LON-MKC-20260830-TRB326R68RB.pdf`, `MKC-LON-20260830-TRB326R68RB.pkpass`
  (not committed to git; inspected in place for this document only).
- Web research performed for this document: On Track Retail (OTRL)
  ownership/client-brand confirmation (Assertis/Go-Ahead Group joint
  venture serving GTR's four brands plus Southeastern) and Assertis's
  separate WebTIS platform (Arriva Group, Caledonian Sleeper, Heathrow
  Express, Omio, Uber) — via Assertis's own public site (assertis.co.uk)
  and RailUK Forums discussion of operator booking-system assignments.
