# Ticket File Processing Improvements — Design

**Status: design proposal, not approved.** No code was edited to produce
this document. This spec turns
`docs/superpowers/specs/2026-09-02-ticket-file-support-improvements-research.md`
(real-file gap analysis) and
`docs/superpowers/specs/2026-09-02-rail-ticket-barcode-format-research.md`
(barcode-format reverse-engineering, legal/provenance findings) into a
concrete design for three in-bounds extraction improvements plus a
detection signal, and gives the barcode-decode question research doc #2
surfaced its own explicit, prominent non-decision. Written to the same
citation discipline as
`docs/superpowers/specs/2026-09-01-enricher-period-cap-remediation-design.md`
(re-verified file:line citations, Decisions-with-real-alternatives-weighed
structure) and to the same "name the boundary, don't quietly cross it"
discipline as
`docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md`'s
Decision 4, which already declined to build barcode decoding/rendering
for an independent reason (file retention) and is directly reinforced by
this spec's own Explicitly out of scope section below.

## Goal

1. Design exactly how `parse_pass_json` should read `ticket_type` from
   `boardingPass.auxiliaryFields` instead of hardcoding `None`.
2. Design a new, anchored PDF route-extraction pattern targeting the
   `Out:`/`Ret:` line format research doc #1 found on a real OTRL-platform
   PDF, added to — not replacing — the existing `\s+to\s+` pattern, and
   design the smallest structural change needed to make this a genuine
   ordered chain rather than a single regex.
3. Design a barcode-presence/format **detection** signal (not decode) for
   uploaded files, concrete about what it is actually used for.
4. Give the RSP-6 decode/render direction research doc #2 investigated an
   explicit, prominent "shelved pending a legal decision" treatment — not
   a silent omission, not a technical blocker dressed up as a legal one.
5. Design how each improvement is tested, using synthetic fixtures that
   match the two real (gitignored, uncommitted) example files' *structure*
   without reproducing their real personal data — following this module's
   own established inline-fixture convention, not a new file-based one.

## Relationship to prior research

- **`2026-09-02-ticket-file-support-improvements-research.md`** inspected
  the two real example files (`LON-MKC-20260830-TRB326R68RB.pdf`,
  `MKC-LON-20260830-TRB326R68RB.pkpass`, both at the repo root,
  gitignored, not referenced further here beyond their filenames) against
  today's `ticket_extraction.rs` and found: (a) `.pkpass`'s
  `boardingPass.auxiliaryFields` already contains a clean, keyed
  `{"key": "ticketType", "value": "Super Off-Peak Return"}` entry that
  `parse_pass_json` never reads; (b) the PDF's route arrow renders as a
  mangled glyph (`ß`), not the literal word "to", so `ROUTE_PATTERN` never
  matches this real ticket, while a better-anchored `Ret: LON - MKC` line
  sits two lines above it in the same extracted text; (c) the ticket's
  issuing platform, On Track Retail (OTRL), serves at least five TOC
  brands (Southern, Gatwick Express, Great Northern, Thameslink,
  Southeastern), so a template-specific fix here has multi-brand reach.
  This spec's Decisions 1 and 2 implement findings (a) and (b) directly;
  Decision 3 (barcode detection) is new work this spec adds on top,
  requested separately from that research doc.
- **`2026-09-02-rail-ticket-barcode-format-research.md`** independently
  confirmed the barcode format both example files use — RSP-6, RDG/Rail
  Settlement Plan's proprietary Aztec-encoded UK rail ticket barcode
  standard — reverse-engineered and cross-validated with high confidence
  against public prior art (eta's 2023 write-up and the `rsp6-decoder`
  crate) and against the two real tickets' own printed text. Critically,
  that document's own Open questions/risks §1 states plainly: RDG's
  on-record 2018 FOI response says access to the RSP-6 specification "is
  deliberately gated behind" being "a TOC, TIS supplier, or accredited
  third-party retailer," and the verification keys used in that research
  "were obtained by reverse-engineering inspector apps and an
  unauthenticated third-party endpoint — never officially published or
  licensed for third-party use," such that "any *product* feature built
  on this would rest on unlicensed key material of awkward provenance."
  This spec's Explicitly out of scope section below is this design's
  answer to that finding: decode/render is shelved, not designed, pending
  a legal decision this spec does not make.

## Current relevant state (re-verified this session)

### `.pkpass` parsing — `crates/api/src/data/ticket_extraction.rs`

- `PartialTicket` (lines 19-34) mirrors the approved
  `tracked_train_tickets` column set exactly: `operator`, `ticket_type`,
  `origin_crs`, `destination_crs`, `source`
  (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:9-13`,
  the migration's own audited header: this table "deliberately stores
  ONLY operator, ticket_type, origin_crs, destination_crs, source, and
  timestamps/ownership. It must NEVER gain a column for payment/price
  data, any barcode payload (raw or decoded), any ITSO data, passenger
  name, or the uploaded .pkpass/PDF file itself"). This spec's every
  addition stays inside that set; nothing here needs a migration.
- `parse_pass_json` (lines 49-83): reads `organizationName` for
  `operator` (lines 62-65); tries `boardingPass.semantics` first via
  `semantics_origin_destination` (lines 85-93), falling back to the
  strict two-element positional read `primary_fields_origin_destination`
  (lines 95-122); **hardcodes `ticket_type: None` unconditionally** (line
  78) — confirmed by the module's own test,
  `ticket_type_is_never_guessed_at` (lines 224-228), which only proves
  the field stays `None` on an *empty* `boardingPass`, not that a real,
  present `ticketType` value would ever be read (it wouldn't — there is
  no code path that touches `auxiliaryFields` or `secondaryFields` at
  all). Never reads `boardingPass.auxiliaryFields`,
  `boardingPass.secondaryFields`, or `barcode`/`barcodes` anywhere in the
  function.
- `parse_pkpass` (lines 139-153) unzips the archive, reads `pass.json`
  bounded at `MAX_ENTRY_BYTES = 1_000_000` (line 130, zip-bomb hygiene),
  deserializes it to a `serde_json::Value`, and hands the whole value to
  `parse_pass_json`. The full parsed JSON — including `barcode`/
  `barcodes` and `auxiliaryFields`/`secondaryFields` — is already in
  memory as this `Value` by the time `parse_pass_json` runs; nothing
  proposed below needs a new read path into the archive, only reading
  more of the `Value` already held.

### PDF parsing — `crates/api/src/data/ticket_extraction.rs`

- `parse_pdf_text` (lines 298-327): `operator` via literal substring
  match against `KNOWN_RETAILER_MARKERS = ["LNER", "Trainline"]` (line
  332); `ticket_type` via case-insensitive substring match against
  `TICKET_TYPE_KEYWORDS` (lines 334-341, six literal strings, none of
  which is or contains "Super Off-Peak Return"); origin/destination via
  exactly one regex, `ROUTE_PATTERN` (lines 343-363,
  `r"([A-Za-z][A-Za-z '\-]+?)\s+to\s+([A-Za-z][A-Za-z '\-]+?)(?:[,\.\n]|$)"`),
  `captures()`'d once against the whole document with no anchoring to a
  specific line — first match anywhere wins. There is currently no chain,
  list, or fallback structure for route extraction: one static regex,
  one `.captures(text)` call (lines 304-312). `parse_pdf` (lines 421-432)
  validates the `%PDF-` magic header, runs `pdf_extract::extract_text_from_mem`
  inside `catch_unwind` (crash containment for untrusted input), and hands
  the resulting text straight to `parse_pdf_text`.
- Real-file result (research doc #1): for the OTRL-issued PDF, `operator`,
  `ticket_type`, `origin_crs`, and `destination_crs` all come back `None`
  today — a silent, 200-status empty preview, not an error.

### Upload routes — `crates/api/src/routes/train.rs`

- `handle_pkpass_upload` (lines 580-592) and `handle_pdf_upload` (lines
  625-654, wrapping `parse_pdf` in `spawn_blocking` + a wall-clock
  `timeout` per its own doc comment at lines 599-606) both return
  `Json<ticket_extraction::PartialTicket>` directly as the HTTP response
  body — `PartialTicket` **is** the wire format, not an internal type
  with a separate serialization step. Any field added to what these
  handlers return is a frontend-visible API change; anything that must
  stay purely server-side (this spec's barcode-detection signal, Decision
  3) cannot be added to `PartialTicket` and must live outside it (e.g. a
  log line), not as a "hidden" extra JSON field.
- Both handlers structurally cannot write to the database — no
  `sqlx::query` call anywhere in this file's upload path (their own doc
  comments, lines 576-579, 596-598, restate this as a hard
  review-before-save property, not incidental).

### Frontend — `frontend/components/TicketEntryForm.tsx`

- `applyPreview` (lines 122-143) copies whatever `PartialTicket` fields
  the upload response set into the form and marks them `autoFilled`
  (line 82), which renders "Auto-filled — please check this value" (lines
  329, 338) or, for the two CRS fields specifically, "...please check
  this is a real 3-letter CRS code" (lines 349, 361-362). `CRS_PATTERN =
  /^[A-Za-z]{3}$/` (line 12) is the client-side mirror of the backend's
  format-only check. Nothing here is auto-submitted; every field stays
  editable. This spec's Decisions 1 and 2 feed more/better values into
  this existing, unchanged review flow — no frontend change is proposed
  or needed for either.

### Validation — `crates/api/src/data/train_tracking.rs`

- `validate_ticket_entry` (doc comment 92-99, function body 100-115 in
  this checkout) checks `source` against the fixed `TICKET_SOURCES` list
  (lines 81-86: `"manual"`, `"pkpass-semantics"`, `"pkpass-heuristic"`,
  `"pdf-heuristic"`) and that `origin_crs`/`destination_crs`, if present,
  are exactly 3 characters (lines 104-111) — format-only, not checked
  against a real station list. This function is unchanged by this spec;
  cited here because Decisions 1 and 2 both produce values that flow
  straight into it unedited if a user doesn't correct the auto-fill.

## Decisions

### Decision 1 — Read `.pkpass` `ticket_type` from `auxiliaryFields`, matched by `key`, not by `label` text

**Chosen approach.** Add a small helper that searches
`boardingPass.auxiliaryFields` (a JSON array of `{key, label, value}`
objects — the same shape `primaryFields` already uses, just a different
top-level array) for the first entry whose `"key"` field is exactly
`"ticketType"`, and returns that entry's `"value"` as a string:

```
fn keyed_field_value(fields: &serde_json::Value, key: &str) -> Option<String> {
    fields
        .as_array()?
        .iter()
        .find(|f| f.get("key").and_then(|v| v.as_str()) == Some(key))?
        .get("value")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}
```

called as `keyed_field_value(boarding_pass.get("auxiliaryFields")?,
"ticketType")` inside `parse_pass_json`, replacing the hardcoded
`ticket_type: None` at line 78. If `auxiliaryFields` is absent, not an
array, or has no `"ticketType"`-keyed entry, this returns `None` — the
same "never guess" behavior the module already guarantees everywhere
else, not a new failure mode.

**Alternatives weighed:**

- **Match by `label` text (e.g. `"TICKET TYPE"`) instead of `key`.**
  Rejected. `label` is a display string — Apple's own PassKit format
  allows issuers to localize or reword it freely, whereas `key` is the
  machine-readable field identifier the pass author chose specifically so
  code (not a human) could find the field reliably. This also matches the
  precedent already in this file: `semantics_origin_destination` (lines
  85-93) reads `departureStationName`/`destinationStationName` by exact
  key, never by any adjacent label text. Matching by key keeps this
  addition consistent with the one dictionary-lookup pattern the module
  already has, rather than inventing a second, weaker one.
- **Search `secondaryFields` too, unconditionally, in addition to
  `auxiliaryFields`.** Considered, not adopted for the initial
  implementation. Research doc #1's Recommendation 1 explicitly floats
  "`auxiliaryFields` (or `secondaryFields`)" as both plausible, but only
  `auxiliaryFields` was confirmed against a real file — no second,
  independently-issued `.pkpass` sample was inspected to test whether any
  real issuer places `ticketType` in `secondaryFields` instead. Given the
  module's stated "never guess" discipline, extending `keyed_field_value`
  to also try `secondaryFields` should wait for that second real sample
  (Open questions, below) rather than be built against zero evidence —
  though the helper above is written key-generically enough that adding
  a second call site (`keyed_field_value(boarding_pass.get("secondaryFields")?,
  "ticketType")` as a fallback if the first returns `None`) is a one-line
  addition whenever that evidence exists, not a redesign.
  Same reasoning as `primary_fields_origin_destination`'s test 
  `a_primary_fields_array_of_the_wrong_length_yields_none_not_a_guess`
  (lines 199-210): an unconfirmed guess is worse than a confirmed gap.
- **Try several plausible key spellings (`"ticketType"`, `"ticket_type"`,
  `"type"`, …) defensively.** Rejected outright. This is the fuzzy-match
  failure mode the module's own doc comments repeatedly warn against —
  `primary_fields_origin_destination`'s doc comment (lines 95-99)
  specifically frames its strict two-element check as "rather than
  guessing at which field is which." One confirmed literal key
  (`"ticketType"`), from one real file, is what's confirmed; adding
  unconfirmed synonyms trades a known gap for an unverified guess, which
  is a worse trade under this module's own stated philosophy.
- **Also read the free-text `backFields.itinerary` prose to extract
  ticket type or operator as a fallback when structured fields are
  missing.** Rejected, out of scope — research doc #1 explicitly flags
  `backFields` as "a real temptation trap, since itinerary text
  (arguably useful) sits in the exact same free-text field as price and
  order id (definitely off-limits)." Any code path that reads
  `backFields` at all needs its own dedicated legal review, independent
  of this decision — not something to fold in as a fallback here. See
  Explicitly out of scope.

**Doc-comment update.** `PartialTicket.ticket_type`'s own doc comment
currently only describes `origin_crs`/`destination_crs`'s
"almost never a real CRS code" caveat (lines 24-30); no change needed
there since `ticket_type` isn't mentioned. `parse_pass_json`'s own doc
comment (lines 36-48) should gain one sentence noting `ticket_type` is
read from `auxiliaryFields` by key, so a future reader doesn't have to
re-derive this from the diff.

### Decision 2 — An ordered route-extraction chain, OTRL's anchored `Out:`/`Ret:` pattern tried before the existing generic pattern

**Chosen approach.** Replace `parse_pdf_text`'s single inline
`ROUTE_PATTERN.captures(text)` call (lines 304-312) with a small ordered
function, the minimal structural change needed to have a real chain
rather than one regex:

```
fn extract_route(text: &str) -> (Option<String>, Option<String>) {
    for pattern in ROUTE_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(text) {
            return (Some(caps[1].trim().to_string()), Some(caps[2].trim().to_string()));
        }
    }
    (None, None)
}

static ROUTE_PATTERNS: std::sync::LazyLock<[regex::Regex; 2]> = std::sync::LazyLock::new(|| {
    [
        // Out:/Ret: line -- anchored, already CRS-shaped, tried first.
        regex::Regex::new(r"(?:Out|Ret):\s*([A-Z]{3})\s*[-\u{2010}-\u{2015}]\s*([A-Z]{3})").unwrap(),
        // Existing generic "<name> to <name>" prose match, unchanged.
        regex::Regex::new(r"([A-Za-z][A-Za-z '\-]+?)\s+to\s+([A-Za-z][A-Za-z '\-]+?)(?:[,\.\n]|$)").unwrap(),
    ]
});
```

`parse_pdf_text` then calls `extract_route(text)` in place of the current
two-line block. Both patterns capture `(origin, destination)` in the same
group order, so the call site's downstream handling
(`Some(caps[1]...)`/`Some(caps[2]...)`) does not change shape — only
where the capture comes from changes. The new pattern requires the
literal word `Out:` or `Ret:`, immediately followed by two 3-uppercase-letter
codes separated by a hyphen (a small Unicode hyphen/dash range is included
defensively, since it is unconfirmed whether every OTRL PDF generation
renders a plain ASCII `-`; see Open questions).

**Ordering rationale.** The OTRL pattern is tried first because it is
strictly higher-confidence when it matches: it requires an explicit
`Out:`/`Ret:` label and already-CRS-shaped codes, versus the generic
pattern's unanchored prose match against arbitrary station-name text
(which the pattern's own doc comment, lines 343-359, already documents
can latch onto unrelated boilerplate containing the word "to"). This
mirrors the ordering precedent already established one function away:
`parse_pass_json` tries the higher-confidence `semantics` dictionary
before falling back to the positional `primaryFields` heuristic (lines
68-74) — "most specific/structured signal first, generic fallback
second" is now the same shape in both parsers, not a one-off for PDFs.

**Alternatives weighed:**

- **Gate the new pattern behind explicit template detection** (e.g.
  requiring "Ticket Details:" plus "TICKET TYPE"/"ROUTE" column headers
  to be present before even trying the `Out:`/`Ret:` regex), as research
  doc #1's Recommendation 2 phrasing literally suggests ("detect this
  template... and, when detected, extract..."). Considered, not adopted.
  The `Out:`/`Ret:` regex is already narrow — it requires a literal label
  word, a colon, and an exact `<3 letters> - <3 letters>` shape
  immediately following it, which is a materially smaller false-positive
  surface than the existing generic pattern's bare `\s+to\s+` (there is
  no comparably common English phrase that reads "Out: XXX - YYY" by
  accident the way "...to bring your ID..." can contain the word "to").
  Adding a separate template-detection function would be a second,
  independently-unverified heuristic (built from one real sample, same as
  the pattern itself) for marginal additional safety over a regex that is
  already this anchored — not proportionate. If a false-positive pattern
  emerges from more real samples (Open questions), gating can be added
  then, against real evidence, rather than speculatively now.
- **Try to generalize `ROUTE_PATTERN` itself to also accept the arrow
  glyph** (matching `ß`, or a class of similar glyph substitutions,
  instead of literal `to`). Rejected. Research doc #1's own Open
  questions flags this glyph mapping as possibly unstable ("whether
  `pdf_extract` maps this template's icon font to `ß` consistently... is
  unknown"); pattern-matching a specific mangled-glyph character is
  exactly the fragile approach the research doc says Recommendation 2
  deliberately avoids by targeting the plain-ASCII `Out:`/`Ret:` line
  instead. Building on an unstable glyph would trade one unverified regex
  for a strictly worse one.
- **Match order reversed (generic pattern first, OTRL pattern as
  fallback).** Rejected — this would only matter when both patterns
  match the same document, in which case a document containing genuine
  route-shaped prose (a real "to" match) alongside an anchored `Out:`/
  `Ret:` line should prefer the more specific, structurally-guaranteed
  signal, not whichever a document-order scan happens to hit first, which
  is the source of the generic pattern's own known imprecision.

**`TICKET_TYPE_KEYWORDS` addition, scoped in as a near-free companion
fix.** Not asked for explicitly in this spec's brief, but directly
completes the same real PDF's extraction and was research doc #1's
Recommendation 3 (its "cheapest possible fix"): add `"Super Off-Peak
Return"` to `TICKET_TYPE_KEYWORDS` (line 334-341). This is a one-line
addition to an existing literal list, not a new pattern or structural
change, and stays inside the approved field set exactly like the rest of
this section. Sibling variants ("Super Off-Peak Single," "Super Off-Peak
Day Return") are *not* added — research doc #1 flags them as "likely...
unconfirmed without more samples," and this list has the same
never-guess discipline as everything else in this module.

### Decision 3 — Detect and log `.pkpass` barcode *format* (never payload), as a diagnostic signal only, not returned to the frontend or persisted

**The concrete use case.** Purely diagnostic: a structured log field
attached to `.pkpass` parsing, intended to let a developer later query
production logs for the real-world distribution of barcode formats
across uploaded tickets (Aztec vs. QR vs. PDF417 vs. Code128, etc.) —
the same kind of "confirm against more real samples before relying on
this" evidence-gathering research doc #1's Open questions repeatedly call
for (e.g. "only one OTRL-issued sample was inspected... worth confirming
with one more real file"). It is **not** surfaced in the API response,
**not** shown in the upload UI, and **not** stored — see "Why not a
data-quality/source indicator in the UI" below for why the more visible
option from the task's own two suggested directions was not chosen.

**What is read, concretely.** `.pkpass`'s `pass.json` may declare its
barcode under a singular `"barcode"` object (the shape research doc #2
found on the real file: `{"format": "PKBarcodeFormatAztec",
"messageEncoding": "iso-8859-1", "message": "..."}`) or, per Apple's
newer, still-current PassKit convention, a plural `"barcodes"` array of
the same per-entry shape (for passes offering multiple scanner-compatible
formats). A new helper reads only the `"format"` string from whichever is
present — **never `"message"`, which is the barcode payload and is
categorically off the approved field set**:

```
fn barcode_format(pass: &serde_json::Value) -> Option<String> {
    pass.get("barcode")
        .or_else(|| pass.get("barcodes").and_then(|b| b.as_array()).and_then(|a| a.first()))
        .and_then(|b| b.get("format"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}
```

`parse_pass_json` calls this once and emits it via
`tracing::debug!(barcode_format = ?format, "parsed .pkpass")` alongside
the existing parse — a one-line addition next to code that already reads
the same `pass: &serde_json::Value`, no new archive read, no new
dependency.

**Why not a data-quality/source indicator surfaced in the UI (the other
direction the task named).** `PartialTicket` **is** the literal JSON
response body for both upload routes (Current relevant state, above) — a
new field on it is a real, frontend-visible API change, and the barcode
format itself is not in the approved DB column set, so it could never be
persisted even transiently in a way that survives past the parse request.
That leaves only an ephemeral, request-scoped value that
`TicketEntryForm.tsx` would have to thread through, display, and then
discard on every save — real frontend work (a new response field, new
UI copy, a decision about what "no barcode detected" should even mean to
a user reviewing an auto-filled form) for a signal whose only confirmed
value today is diagnostic. If the log-based signal above eventually shows
a real pattern worth surfacing to users (e.g. "PDFs with no detectable
barcode are disproportionately likely to also fail all three text-layer
heuristics," a genuine data-quality correlation), *that* would justify
the frontend work as a follow-up decision made with real log evidence
behind it — not built speculatively now.

**Why PDF-side barcode detection is not designed here.** Research doc #2
found the real PDF's barcode is drawn as vector graphics, not embedded as
a raster image — there is no metadata field or embedded-image marker to
read the way `.pkpass`'s JSON `barcode.format` string is a free, already-parsed
value. Detecting a vector-drawn barcode's *presence*, let alone its
*format*, would require rasterizing the PDF page and running an
image-based barcode-symbology scan — structurally the same pipeline
research doc #2 used (`pymupdf`-equivalent rendering + a `zxing`/`rxing`-class
scanning library) minus only its final RSA/base26 decode step. That is a
materially larger engineering lift than the `.pkpass` case (a new Rust
image-processing dependency, a rasterization step added to the existing
`spawn_blocking`-wrapped PDF path, and its own false-negative risk against
vector-drawn barcodes specifically) for a signal whose only designed use
is diagnostic logging — the same cost/benefit call research doc #1's
Recommendation 5 already made about OCR for logo-image operator
extraction ("nothing... crosses the legal boundary... but it's a
materially larger engineering lift... for a field that's already
optional"). Flagged here as a real, explicitly scoped-out idea per that
same discipline — see Explicitly out of scope — not silently omitted.

**Alternatives weighed:**

- **Use barcode presence/absence as an upload-rejection gate** (reject a
  PDF/`.pkpass` with no detectable barcode as "probably not a real
  ticket"). Rejected. This risks blocking legitimate uploads on a weak
  signal — a real ticket could plausibly have a barcode this detector
  fails to find (as the PDF vector-drawn case above already shows for
  PDFs specifically), and this module's established philosophy is to
  leave fields `None` for manual completion, never to guess *or* reject
  on an unconfirmed heuristic. A hard rejection here would be a strictly
  worse UX than today's silent-empty-preview fallback, for no confirmed
  benefit.
- **Read `barcode.message`'s length or first few characters (not the
  full payload) as an additional "looks real" signal, without full
  decoding.** Rejected as still crossing the spirit of the boundary: even
  a byte-length or magic-prefix read of `message` is reading barcode
  *payload* bytes, which research doc #2's own module-doc-quoted
  constraint (`ticket_extraction.rs:1-7`) frames as "NEVER... touches...
  a barcode," not "never touches it beyond N bytes." `format` is
  documented Apple PassKit container metadata, structurally no different
  from `organizationName` or `transitType`, both already read; `message`
  is the payload research doc #2's own findings say to leave alone
  entirely. Keeping the line at "container metadata only, never payload
  bytes" is a clean, easily-audited rule; "payload bytes, but only a
  few" is not.

## Explicitly out of scope

### RSP-6 barcode payload decoding, or rendering any barcode/QR code from a decoded payload, in any form — deliberately shelved pending a legal decision, not an oversight

**This is the most important section of this document.** Nothing in this
spec decodes, renders, stores, logs, or otherwise surfaces any barcode
*payload* or anything derived from one. This is a deliberate, considered
exclusion, made with full knowledge of what research doc #2 found
technically possible — not something left out because nobody thought of
it.

**What research doc #2 found, technically.** Both real example tickets'
barcodes are RSP-6 (Rail Settlement Plan's proprietary Aztec-encoded
format), and that document independently re-implemented and validated
the full decode path against both tickets: base-26 string decoding,
RSA-1024 signature verification with full message recovery (no
encryption — anyone holding the issuer's public key can read every
field), PKCS#1 unpadding, and a bit-packed record layout recovering
roughly fifteen fields with high confidence, including the passenger's
name (confirmed decodable at bit offset 255-326). Building an actual
decode-and-render feature on top of that research would require, at
minimum: (a) a maintained set of issuer RSA public keys (the community
dump research doc #2 used covers 37 issuer IDs, of the provenance
described next); (b) the base-26/RSA/PKCS#1/bit-unpack pipeline that
document validated; (c) for **PDF** tickets specifically — unlike
`.pkpass`, which already carries the payload as plain text in
`pass.json`'s `barcode.message` — rasterizing the page and running an
Aztec image decode, since the barcode is drawn as vector graphics, not
stored as accessible text; and (d) a QR/Aztec rendering step from the
decoded fields. None of this is designed here, at any level — no code
sketch, no schema, no API shape.

**Why it's blocked — quoted, not paraphrased.** Research doc #2's Open
questions/risks §1, in full:

> "The RSP-6 specification is proprietary to RDG/RSP, and RDG has stated
> on the record (the 2018 FOI thread, §5) that the spec is available
> only to TOCs, TIS suppliers, and accredited third-party retailers
> through its accreditation process — i.e. the closure is deliberate
> policy, not an oversight. The verification keys in public circulation
> were obtained by reverse-engineering inspector apps and an
> unauthenticated third-party endpoint — never officially published or
> licensed for third-party use. Decoding one's *own* tickets for personal
> understanding is the fact pattern of all the public research and has
> drawn no known enforcement; but any *product* feature built on this
> would rest on unlicensed key material of awkward provenance, process
> third parties' personal data (passenger names) from a format its owner
> deliberately keeps closed, and sit in exactly the territory this
> codebase's audited constraints exist to avoid."

In short: the blocker is **legal/provenance, not technical**. The
decode path is understood well enough to implement (research doc #2's
own re-implementation proves that); what's missing is a legitimate right
to use the key material a product feature would depend on, and a
considered answer to processing passenger names (categorically excluded
from `tracked_train_tickets` today, per this spec's Current relevant
state) that a decode feature would newly introduce.

**This sharpens, not merely repeats, an existing decision.** Decision 4
of `docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md`
already declined to build "render the QR code for scanning" for a
`.pkpass`, but for a *different*, independently-sufficient reason at the
time: no code path retains the uploaded file or its barcode payload past
the initial parse request, and building that retention would itself
require reversing an audited no-file-retention constraint. That spec's
own words: rendering a QR code "would require **both** forbidden things
at once: storing/retaining the file or its barcode payload... **and**
adding a barcode-decoding capability this codebase's own design research
explicitly investigated and declined to build, for reasons independent of
data retention." Research doc #2 has since supplied the second half of
that sentence with hard, on-record evidence (the FOI thread) rather than
the earlier design doc's more general "no official public spec exists"
framing. Even if the file-retention question above were separately,
deliberately revisited someday, the decode step itself remains blocked
on this now much better-documented legal question independently.

**What would need to change before this is revisited.** Not a schema
change, not a new dependency, not more real-file evidence — a **legal
decision by the repo owner** on whether to accept the key-material
provenance risk research doc #2 documents, made consciously, the same
way Decision 4 named its own blocker as "a product/legal decision, not a
design decision." Until that decision is made, this capability is
shelved, not planned, and no part of this spec's Decisions 1-3 depends on
it or lays groundwork toward it — Decision 3's barcode detection reads
only a `format` string (documented container metadata), never touches
`message` (the payload), and is designed explicitly to stay clear of this
exact line (see Decision 3's "Alternatives weighed" for why even reading
a few bytes of `message` was rejected).

### Other items explicitly not designed in this spec

- **`.pkpass` `ticket_type` fallback to `secondaryFields`.** Only
  `auxiliaryFields` is confirmed against a real file (Decision 1); adding
  a second, unconfirmed lookup location is deferred, not designed, until
  a second real sample exists.
- **Ticket-type keyword variants** ("Super Off-Peak Single," "Super
  Off-Peak Day Return," and any other sibling of "Super Off-Peak
  Return"). Research doc #1 itself flags these as unconfirmed without
  more samples; only the one confirmed literal string is added
  (Decision 2).
- **A "format-valid but not a trackable single station" CRS-code warning
  in `TicketEntryForm.tsx`** (research doc #1's Recommendation 4, e.g. a
  more specific warning when `"LON"`/"London Terminals"-style umbrella
  codes are auto-filled). This is a frontend UX/copy change to the
  existing review-before-save form, not a ticket *extraction* change —
  outside this spec's title and scope, exactly as research doc #1 itself
  scoped it separately ("scoped separately because it touches the
  frontend, not `ticket_extraction.rs`").
- **OCR over PDF logo/branding images to recover `operator`** for
  logo-only-branded templates (e.g. this real PDF's ThamesLink wordmark,
  which never appears in the extracted text layer). Research doc #1's own
  Recommendation 5 explicitly declines this for now — "a materially
  larger engineering lift... for a field that's already optional and
  already has a working manual-entry fallback" — and this spec does not
  revisit that call.
- **PDF-side barcode presence/format detection.** See Decision 3's own
  "Why PDF-side barcode detection is not designed here" — a real,
  considered exclusion for engineering-cost reasons, independent of the
  RSP-6 legal question above.
- **Changing `validate_ticket_entry`'s CRS-format check** to validate
  against a real station/CRS list. A `train_tracking.rs` change with its
  own blast radius, not an extraction change; research doc #1 scoped this
  out for the same reason.
- **A full per-TOC/per-retailer PDF template library**, or investigating
  Assertis's separate WebTIS platform (Arriva Group, Caledonian Sleeper,
  Heathrow Express, Omio, Uber) or Google Wallet passes. No samples exist
  for either; research doc #1 already scoped both out, unchanged here.
- **Any change to `backFields` handling.** Not read today, not proposed
  to be read here — see Decision 1's "Alternatives weighed" for why this
  is a real temptation trap (itinerary text sits alongside price/order-id
  in the same free-text field) requiring its own dedicated legal review
  if ever revisited.

## Architecture

No new modules, routes, or dependencies. All changes are internal to
`crates/api/src/data/ticket_extraction.rs`:

```
parse_pass_json(pass: &Value) -> PartialTicket
  operator            <- pass.organizationName                    (unchanged)
  ticket_type         <- keyed_field_value(boardingPass            (NEW: Decision 1)
                            .auxiliaryFields, "ticketType")
  origin/destination  <- semantics, else primaryFields             (unchanged)
  [diagnostic only]   <- barcode_format(pass) -> tracing::debug!() (NEW: Decision 3,
                                                                     not on PartialTicket)

parse_pdf_text(text: &str) -> PartialTicket
  operator            <- KNOWN_RETAILER_MARKERS substring match    (unchanged)
  ticket_type         <- TICKET_TYPE_KEYWORDS substring match      (+1 literal: Decision 2)
  origin/destination  <- extract_route(text):                      (NEW structure: Decision 2)
                            1. OTRL Out:/Ret: pattern (anchored)
                            2. existing "X to Y" pattern (fallback)
```

`parse_pkpass` and `parse_pdf` (the thin unzip/extract wrappers) are
unchanged — both already hand a fully-parsed value (`serde_json::Value`
or extracted `text: &str`) to the functions above, which is all Decisions
1-3 need.

## Error handling

- **Decision 1**: no new error paths. `keyed_field_value` returns `None`
  on any missing/malformed structure (absent `auxiliaryFields`, not an
  array, no matching `key`, a `value` that isn't a string) — identical
  "leave for manual entry" behavior to every other optional field in this
  module, not a new failure mode requiring a new test category.
- **Decision 2**: no new error paths either. `extract_route` returns
  `(None, None)` when neither pattern matches, same as today's single
  `.unwrap_or((None, None))`. A malformed or partially-matching
  `Out:`/`Ret:` line (e.g. only one 3-letter code present) simply doesn't
  match the regex and falls through to the second pattern, then to
  `(None, None)` — no panic path, since `regex::Regex::captures` never
  panics on non-matching input.
- **Decision 3**: `barcode_format` returns `None` on any missing/malformed
  `barcode`/`barcodes` structure. The `tracing::debug!` call is
  unconditional (logs `barcode_format = None` as much as a real value) —
  debug-level specifically so it costs nothing in default-configured
  production logging and cannot become a de facto data-collection channel
  by being promoted to `info!`/`warn!` without a deliberate decision to do
  so later.
- No change to either upload handler's existing `422`-on-parse-error
  behavior (`crates/api/src/routes/train.rs:587-591, 638-641`) — none of
  Decisions 1-3 can turn a previously-`Ok` parse into an `Err`, only fill
  in more of an already-`Ok` `PartialTicket`, or (Decision 3) add a log
  line beside it.

## Testing

All new tests follow this module's own established convention — inline
`serde_json::json!()` fixtures for `.pkpass` tests (`pass_json_tests`,
lines 156-229) and literal Rust string-literal fixtures for PDF tests
(`parse_pdf_text_tests`, lines 366-411) — hand-written, structurally
representative of the real files' confirmed shapes (real key names, real
label conventions, real line formats) without containing any of the real
files' actual personal data (no real ticket number, order id, price,
passenger name, or exact station-pair/date). This mirrors what the
existing tests already do: `semantics_present_is_preferred_and_labelled_accordingly`
(lines 161-178) uses `"LNER"` / `"Kings Cross"` / `"Edinburgh"` — a
plausible, real-shaped, but entirely invented pass, not a captured real
one. The two real example files stay exactly where they are today (repo
root, gitignored, referenced only by filename in research docs) and are
not read, copied, or referenced by path from any test.

- **Decision 1 (`.pkpass` `ticket_type`)**, new tests in
  `pass_json_tests`:
  - `ticket_type_is_read_from_auxiliary_fields_by_key`: a `boardingPass`
    with `auxiliaryFields: [{"key": "ticketType", "label": "TICKET TYPE",
    "value": "Super Off-Peak Return"}]` (the real field shape, an
    invented-but-plausible value already used nowhere else as personal
    data) — asserts `ticket_type == Some("Super Off-Peak Return")`.
  - `ticket_type_ignores_a_field_with_the_wrong_key`: an
    `auxiliaryFields` entry with a *different* key (e.g. `"railcard"`)
    whose `label` happens to contain "ticket type"-adjacent text —
    asserts `ticket_type == None`, proving the match is genuinely by
    `key`, not accidentally by label-text substring.
  - `ticket_type_is_never_guessed_at` (existing, lines 224-228):
    unchanged, still asserts `None` on a `boardingPass` with no
    `auxiliaryFields` at all — still a correct, load-bearing regression
    test after this change.
- **Decision 2 (OTRL PDF route pattern)**, new tests in
  `parse_pdf_text_tests`:
  - `otrl_out_ret_line_is_matched_when_the_generic_to_pattern_fails`: a
    synthetic text block modeled on the real extracted layout (a mangled
    glyph line with no "to", plus an `Out:`/`Ret:` line above it, e.g.
    `"= 1 Sep 2026 Ret: ABC - XYZ\nSTATION A STATION B\nABC ß XYZ"`) —
    asserts `origin_crs == Some("ABC")`, `destination_crs ==
    Some("XYZ")`.
  - `otrl_pattern_is_preferred_over_a_coincidental_to_match_earlier_in_the_text`:
    a synthetic text containing both an unrelated "...remember to bring
    ID..." phrase *and* an `Out:`/`Ret:` line — asserts the anchored
    pattern's result wins, directly exercising Decision 2's chosen
    ordering rather than just documenting it in prose.
  - Existing `matches_the_design_docs_own_worked_example` (lines
    369-379) and the other three existing `parse_pdf_text_tests`:
    unchanged, still pass — confirms the generic pattern's existing
    behavior is preserved as a fallback, not replaced.
  - `ticket_type_matches_the_super_off_peak_return_keyword`: extends an
    existing-style text fixture with `"Super Off-Peak Return"` present,
    asserting the Decision 2 keyword-list addition works, mirroring
    `no_ticket_type_keyword_present_yields_none_not_a_guess`'s existing
    structure (lines 396-400).
- **Decision 3 (barcode format detection)**: `barcode_format` is
  designed as a small pure function specifically so it's directly
  unit-testable without needing to assert on log output (mirroring how
  `parse_pkpass` itself is "not unit-tested beyond the round-trip smoke
  test" per its own doc comment, lines 132-138, because the thin wrapper
  around it carries no logic of its own worth separately testing) — new
  tests in a `barcode_format_tests` module:
  - `reads_format_from_the_singular_barcode_object`: a `pass.json`-shaped
    fixture with `"barcode": {"format": "PKBarcodeFormatAztec",
    "message": "PLACEHOLDER-NOT-A-REAL-PAYLOAD"}` — asserts
    `barcode_format(&pass) == Some("PKBarcodeFormatAztec".to_string())`.
    The `message` value is deliberately an obvious non-payload placeholder
    string, never anything resembling the real 233-character RSP-6
    payload shape research doc #2 describes structurally — a discipline
    worth stating explicitly so a future contributor extending this test
    doesn't reach for something more "realistic."
  - `falls_back_to_the_plural_barcodes_array`: a fixture with only
    `"barcodes": [{"format": "PKBarcodeFormatQR"}]` (no singular
    `"barcode"` key) — asserts the fallback path is exercised.
  - `returns_none_when_neither_field_is_present`: an empty `pass.json` —
    asserts `None`, the same "leave it blank, don't guess" contract as
    every other optional read in this module.
  - No test asserts anything about the `tracing::debug!` call site itself
    (log-line content isn't unit-tested elsewhere in this crate either);
    the pure function above carries all the real logic and all the real
    test coverage.

## Open questions/risks

- **`.pkpass` `ticket_type` in `secondaryFields`, and whether OTRL's
  exact `pass.json` shape (Decision 1's confirmed evidence) generalizes
  to Southern, Gatwick Express, Great Northern, and Southeastern's own
  OTRL-issued passes** — both inherited unresolved from research doc #1;
  a second real OTRL-issued sample (any of those four brands) would
  answer both at once.
- **Whether the `Out:`/`Ret:` line's separator is always a plain ASCII
  hyphen.** Decision 2's regex defensively accepts a small Unicode
  dash range, but this is an unverified guess in the opposite direction
  from the module's usual discipline — worth confirming (or narrowing
  back to a plain `-`) against a second real OTRL PDF rather than left
  speculative indefinitely.
- **Whether the `ß`-glyph mangling (and therefore the generic pattern's
  failure on this template) is stable across PDF-generator versions** —
  unresolved from research doc #1; irrelevant to Decision 2's own
  correctness (which doesn't depend on the glyph at all) but relevant to
  whether the *generic* pattern might start working again for a future
  OTRL PDF generation, at which point both patterns matching the same
  document becomes a real (not just theoretical) case the ordering
  decision above needs to keep handling correctly.
- **Real-world barcode-format distribution across other issuers' `.pkpass`
  files** — the entire point of Decision 3's diagnostic logging; this is
  explicitly an open question the design defers to log evidence
  gathered after shipping, not something resolved here.
- **The RSP-6 legal/provenance question itself** (Explicitly out of
  scope, above) is the largest open item in this document by far, and is
  deliberately left to the repo owner, not estimated or nudged toward a
  particular answer here.

## References

- `docs/superpowers/specs/2026-09-02-ticket-file-support-improvements-research.md`
  — full document, cited throughout above.
- `docs/superpowers/specs/2026-09-02-rail-ticket-barcode-format-research.md`
  — full document, cited throughout above, especially Open questions/risks
  §1 (quoted in full in Explicitly out of scope).
- `docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md`
  Decision 4 — the earlier, independently-reasoned decline of
  barcode-decode/render work, reinforced rather than repeated by this
  spec's Explicitly out of scope section.
- `crates/api/src/data/ticket_extraction.rs` — full file re-read for this
  document; specific lines cited inline above.
- `crates/api/src/data/train_tracking.rs` — `TICKET_SOURCES`,
  `validate_ticket_entry`, re-verified this session.
- `crates/api/src/routes/train.rs` — upload handlers, re-verified this
  session (`handle_pkpass_upload` 580-592, `handle_pdf_upload` 625-654).
- `crates/api/migrations/20260829090000_journey_ticket_tracking.sql:1-16`
  — the audited legal/privacy column-set boundary every decision above
  stays inside.
- `frontend/components/TicketEntryForm.tsx` — re-verified this session;
  unchanged by this spec.
