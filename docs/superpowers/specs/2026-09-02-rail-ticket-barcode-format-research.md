# UK Rail E-Ticket Barcode Format (RSP-6) — Reverse-Engineering Research

**Status: research only, explicitly owner-requested.** The repo owner asked
for a bounded reverse-engineering investigation of the barcode format on
their own two real ticket files (a PDF e-ticket and its return leg's Apple
Wallet `.pkpass`), fully aware that this codebase deliberately never
decodes barcodes in production
(`crates/api/src/data/ticket_extraction.rs:6` — "NEVER decodes a barcode
or touches ITSO data"; the audited header of
`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:12`
forbidding "any barcode payload (raw or decoded)";
`docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md`
Decision 4). **Nothing in this document changes that stance** — see
"Explicitly out of scope" below. Written to the citation discipline of
`2026-09-01-pwa-support-research.md`: claims are split between what
existing public research documents, and what this session independently
verified against the two real tickets. No raw barcode payload, decoded
personal record, ticket number, order reference, or passenger name is
reproduced in this document; all extraction artifacts and decode output
lived only in the session scratchpad and were never staged for commit.

## Goal

Answer, for the two real tickets on hand: (1) which barcode
format/standard UK rail e-tickets of this kind actually use; (2) the
payload's structure — field list, offsets, encodings — with an honest
confidence rating per field; (3) how much of this is corroborated by
existing public research vs. inferred fresh from these two files; (4)
whether the understanding generalizes to arbitrary UK rail e-tickets or
is specific to this retailer; and (5) what legal/ToS considerations the
format carries — as input to any future deliberate decision about the
codebase's never-decode constraint, not as a step toward reversing it.

## Method

**Extraction.** The `.pkpass` is a zip archive; unzipping it (into the
session scratchpad) yields `pass.json`, whose `barcode` block declares
`"format": "PKBarcodeFormatAztec"`, `"messageEncoding": "iso-8859-1"`,
and — crucially — the full barcode payload as a plain-text `message`
string, so no image decoding was needed for that leg. The PDF contains no
embedded barcode raster (its only embedded image is a 400×55 logo; the
barcode is drawn as vector graphics), so the page was rendered at 4×
scale with `pymupdf` and decoded with `zxing-cpp` 3.1.1 (Python
bindings), which read one Aztec symbol (38% error-correction level)
containing a 233-character ISO-8859-1 text payload.

**Analysis.** The two payloads share an identical 15-character plaintext
header and differ only in their trailing 218-character uppercase-A–Z
blob. Web search for prior art surfaced eta's 2023 reverse-engineering
write-up and open-source decoder (see Findings §5). The decoder's source
(the `rsp6-decoder` 0.1.0 crate, downloaded from crates.io into the
scratchpad — the canonical git host is behind bot protection) documents
the exact bit-level record layout and ships a community-recovered issuer
public-key set. To validate that public spec against these 2026 tickets
rather than take it on faith, the decode path (base-26 → RSA →
PKCS#1-unpad → bit-unpack) was re-implemented independently in a
scratchpad Python script using that key set, and every recovered field
was cross-referenced against the tickets' own human-readable text (PDF
page text, `pass.json` display fields). Both barcodes verified and
decoded cleanly on the first matching key.

## Findings

### 1. Format identified: RSP-6 — conclusive

Both tickets are **RSP-6** ("Rail Settlement Plan" barcode format,
version/magic `06`), Aztec-encoded. Evidence: Apple Wallet renders
`PKBarcodeFormatAztec`; both payloads begin with the RSP-6 magic `06`;
and — decisively — both payloads' signature blocks verify under the
community-published RSA public key for their declared issuer ID, with
valid PKCS#1 padding, yielding a record whose every checkable field
matches the printed ticket. This is not UIC 918.3 (no `#UT` prefix), not
ITSO, and not a retailer-proprietary format. Both the PDF ticket and the
`.pkpass` carry the *same* RSP-6 payload format — the pkpass simply
stores it as text in `pass.json` while the PDF renders it as a vector
Aztec drawing.

### 2. Outer barcode-string layout — confirmed on both tickets

The Aztec symbol encodes plain ISO-8859-1 text:

| Chars | Content | Notes |
|---|---|---|
| 0–1 | Magic `06` | Format identifier ("RSP-6") |
| 2–10 | Ticket reference (9 chars) | Plaintext copy; the printed "Ticket Number" is the 2-char issuer ID + these 9 chars |
| 11–12 | Sub-UTN (2 digits) | `00` on both tickets; semantics unverified (eta's decoder carries it opaquely) |
| 13–14 | Issuer ID (2 chars) | Selects the RSA verification key; `TR` here |
| 15– | Base-26 blob (A–Z only) | The RSA signature; 218 chars on both tickets |

The base-26 decoding is quirky and was confirmed by independent
re-implementation: characters are read as base-26 digits (`A`=0…`Z`=25)
in *reverse* string order, and the resulting integer's big-endian byte
string is then reinterpreted little-endian before use as the RSA
signature integer. 218 letters ≈ 1025 bits of capacity; the observed
integers are ~1022 bits, sitting just under the 1024-bit modulus.

### 3. Cryptography — confirmed on both tickets

**RSA-1024 signature with full message recovery; there is no
encryption.** The signed record is not hashed-and-signed; the entire
record *is* the PKCS#1 v1.5 message: `sig^e mod N` (e = 65537 for this
issuer) recovers a type-`01` padded block (`00 01 FF… 00 || record` —
type-01 with a short 0xFF run observed here; eta's decoder also accepts
type-02), and stripping the padding yields a 116-byte record. Two
consequences worth stating plainly:

- **Anyone holding the issuer's *public* key can read every field**,
  including the passenger's name (§4). The format's confidentiality
  rests entirely on RDG/RSP not publishing the public keys — security
  through obscurity, since the keys are by construction non-secret
  material.
- **Forgery genuinely is prevented** — producing a valid barcode requires
  the issuer's private key, which no public research has surfaced.

The key that verified these tickets comes from the community key dump
bundled with eta's decoder (recovered in 2023 from ticket-inspector
apps — see §5), and carries a 2015→2040 validity window in that dump's
metadata — which is why a 2023-recovered key still verifies a
2026-issued ticket. The dump covers 37 issuer IDs.

### 4. Inner record structure — bit-packed, with per-field confidence

The recovered record is a **bit-packed structure, MSB-first**, using
three primitive encodings: (a) 6-bit characters, value + 32 → ASCII
(covers space, digits, uppercase); (b) unsigned big-endian bit integers;
(c) timestamps as a 14-bit day count plus an 11-bit minute-of-day count
**since a 1997-01-01 epoch**. Layout per eta's published spec, with this
session's validation result for each field. Confidence legend:
**Confirmed** = decoded value cross-checks against this ticket's own
printed/visible text; **Corroborated** = matches public research's spec
but not independently checkable from these tickets (typically
zero/blank here); **Speculative** = public research itself marks it
unknown/mystery.

| Bits | Field | Encoding | Confidence — evidence from these two tickets |
|---|---|---|---|
| 0 | "manually inspect" flag | bool | Corroborated (0 here) |
| 1–7 | Unknown header | 7 bits | Speculative (constant `0000001` on both) |
| 8–61 | Ticket reference | 9 × 6-bit chars | **Confirmed** — matches chars 2–10 of the plaintext header and the printed ticket number |
| 62–67 | Checksum character | 6-bit char | Corroborated; algorithm not publicly documented |
| 68–71 | Version | 4-bit int | Corroborated (0 on both) |
| 72 | Standard-class flag | bool | **Confirmed** (set; both tickets are standard class) — single-class observation only |
| 73–90 | Lennon ticket-type code | 3 × 6-bit chars | Corroborated — a settlement-system code, distinct from the fare code below; not independently mappable this session |
| 91–108 | Fare / ticket-type code | 3 × 6-bit chars | **Confirmed** — the decoded 3-letter code maps, via the decoder project's public fares table, to exactly the ticket-type name printed on both tickets ("Super Off-Peak Return") |
| 109–132 | Origin NLC | 4 × 6-bit chars | **Confirmed** — maps to the printed origin station, and the origin/destination pair swaps between the outbound pass and the return PDF exactly as the two legs' printed journeys swap |
| 133–156 | Destination NLC | 4 × 6-bit chars | **Confirmed** (same evidence) |
| 157–180 | Retailer ID (NLC) | 4 × 6-bit chars | **Confirmed** — maps to the issuing retailer's web-TIS NLC, matching the retailer named on the pass |
| 181 | Child-ticket flag | bool | Corroborated (0; adult ticket) |
| 182–183 | Coupon type | 2-bit enum: Single / Season / ReturnOutbound / ReturnInbound | **Confirmed** — decodes ReturnOutbound on the pass labeled "Out:" and ReturnInbound on the PDF labeled "Ret:" |
| 184–193 | Discount (railcard) code | 10-bit int | **Confirmed** — maps, via the decoder project's public discounts table, to exactly the railcard printed on both tickets ("16-25 Railcard") |
| 194–210 | Route code | 17-bit int | Plausible — non-zero on a ticket with a printed route restriction ("LNR ONLY"); the specific code→route mapping was not verified this session |
| 211–224 | Travel start date | 14-bit days since 1997-01-01 | **Confirmed** — decodes to the printed travel date |
| 225–235 | Departure time | 11-bit minutes | Corroborated (00:00 here — date-only validity) |
| 236–237 | Departure-time flag | 2-bit enum | Speculative (public research marks its semantics partly unknown) |
| 238–254 | Passenger ID document | 17-bit int (type prefix + number) | Corroborated (0 = none here) |
| 255–326 | Passenger name | 12 × 6-bit chars | **Confirmed** — decodes to the ticket holder's real surname (deliberately not reproduced here). Every RSP-6 barcode of this shape carries the passenger's name readable by anyone with the public key |
| 327–328 | Gender | 2-bit int | Corroborated (0 here) |
| 329–346 | Restriction code | 3 × 6-bit chars | Corroborated (`000` here; not independently mapped) |
| 347–370 | OSI NLC (cross-London transfer) | 4 × 6-bit chars | Corroborated (blank here) |
| 371, 372 | Unknown flag; bidirectional flag | bools | Speculative / Corroborated (both 0) |
| 379–382 | Limited-duration code | 4-bit enum (15 min … 18 h) | Corroborated (0 here) |
| 384 | "Full ticket" flag (purchase block present) | bool | **Confirmed** (set; purchase block decodes coherently) |
| 385, 383 | Free-text present / extended flags | bools | Corroborated (no free text here) |
| 386–389 | Reservation count | 4-bit int | **Confirmed** — 0, consistent with the pass's printed "No specific seat" |
| 390–435 | Purchase timestamp + price | 14+11-bit datetime; **21-bit price in pence** | **Confirmed** — purchase date matches the printed purchase date (with a plausible time-of-day), and the price decodes to the exact printed fare to the penny |
| 449–496 | Purchase/order reference | 8 × 6-bit chars | **Confirmed** — matches the order ID printed on both tickets |
| 497–505 | Days of validity | 9-bit int (0 → 1) | **Confirmed** — the strongest single cross-check: decodes to 1 on the outbound (printed VALID UNTIL = travel date) and 30 on the return (printed VALID UNTIL = one month later), from otherwise near-identical records |
| 512– | Reservations, 45 bits each | RSID (2 chars + 14-bit number), coach char, seat letter, 7-bit seat number | Corroborated only (none present on these tickets) |
| after reservations | Free text | 6-bit chars to bit 783/863 | Corroborated only (absent here) |

Net: roughly fifteen fields — including every personally- or
commercially-meaningful one — were independently confirmed against the
visible ticket text; the remainder match the public spec's layout but
were zero/blank on these tickets. Nothing decoded contradicted the
public spec anywhere, on a ticket issued three years after that spec was
published, which is strong evidence the format is stable.

### 5. Public research cross-reference

This investigation is **mostly corroboration, not first discovery** —
exactly as hoped. The definitive prior art:

- **eta, "Reversing UK mobile rail tickets" (Jan 2023)** —
  [eta.st/2023/01/31/rail-tickets.html](https://eta.st/2023/01/31/rail-tickets.html):
  the primary public reverse-engineering of RSP-6 — header layout,
  base-26/RSA/PKCS#1 scheme, and the key-recovery story (public keys
  extracted from Masabi's "JustRide Inspect" inspector-app APK, plus an
  unauthenticated key-download endpoint in The Ticket Keeper's app). Also
  documents a since-reported privacy hole in a third-party scan-history
  service.
- **The `rsp6-decoder` project (Rust + WASM web demo)** —
  [crates.io/crates/rsp6-decoder](https://crates.io/crates/rsp6-decoder),
  demo at [eta.st/tickets](https://eta.st/tickets/), canonical repo at
  `git.eta.st/eta/rsp6-decoder` (bot-walled; the crates.io tarball was
  used instead): the bit-level record layout in §4 comes from its
  `src/payload.rs`, the outer layout and crypto from `src/lib.rs`, and
  the issuer key set from its bundled `keys.json`; its `fares.json` /
  `discounts.json` / `stations.json` provided the code→name mappings
  used for cross-checking.
- **Hackaday, "Reverse Engineering British Rail Tickets" (Feb 2023)** —
  [hackaday.com/2023/02/09/reverse-engineering-british-rail-tickets](https://hackaday.com/2023/02/09/reverse-engineering-british-rail-tickets/):
  secondary coverage; corroborates the app-decompilation provenance of
  the keys and the "surprising amount of traveller data" finding.
- **FOI request for the RSP-6 spec (2018)** —
  [whatdotheyknow.com/request/rsp_6_specification_for_barcodes](https://www.whatdotheyknow.com/request/rsp_6_specification_for_barcodes)
  (read this session from the repo owner's PDF printout of the page,
  since the live site 403s automated fetches): Stuart Bain's May 2018
  request to Rail Delivery Group for the RSP-6 specification. RDG's Head
  of Service Assurance replied informally (June 2018) that FOI does not
  apply to RDG and that **access to rail industry standards requires
  being a TOC, TIS supplier, or accredited third-party retailer**, via
  RDG's accreditation and standards team; the thread died once Bain
  confirmed he was requesting as an individual, and the request is
  marked "long overdue" / closed. Bain's argument — that the format is
  secured by asymmetric cryptography, so publishing the *decoding* spec
  cannot weaken ticket security — is exactly the property §3 confirms.
  Net: there is **no official public specification**, and RDG has stated
  on the record that it is deliberately gated behind industry
  accreditation — consistent with (and sharpening) what Decision 4 of
  the ticket-display design already concluded. (RSP is Rail Settlement
  Plan Limited, one of the ATOC/RDG corporate entities, per the reply's
  own footer.)

**What this session added beyond the public record:** independent
re-implementation of the decode path (not a run of eta's code);
validation against tickets issued in 2026 by a retailer on the OTRL
platform under issuer ID `TR`, confirming the 2023 spec and 2023 key
dump still hold; the observation that a `.pkpass`'s `pass.json` carries
the raw RSP-6 payload as plain text (so pkpass-based tickets need no
image decoding at all); and the per-field confirmed/corroborated split
in §4, which the public spec itself doesn't provide.

### 6. Generalizability

This is a **genuine cross-industry format, not one retailer's scheme** —
the same spec decodes any issuer's tickets *given that issuer's public
key*. The practical gate for decoding an arbitrary UK rail e-ticket is
therefore not the format but **key coverage**: the community dump spans
37 issuer IDs with multi-decade validity windows, but a ticket from a
new issuer, a rotated key, or a barcode with a different magic prefix
(only `06` is publicly documented) would fail. Also explicitly *not*
covered by this research: ITSO smartcard-media tickets (an entirely
different, smartcard-oriented stack this codebase already refuses to
touch) and UIC 918.3 `#UT` barcodes (the continental-European format,
which has its own public decoders).

## Explicitly out of scope

- **This research does not change — and actively reaffirms — the
  codebase's "never decode a barcode in production" stance.** No
  production application code was modified (in particular not
  `crates/api/src/data/ticket_extraction.rs`), no database column,
  migration, or persistence path for barcode data was added or designed,
  and no decoding capability is proposed for the product. If anything,
  §4's confirmation that **every such barcode carries the passenger's
  name, readable with a non-secret key** strengthens the original
  audit's reasoning for refusing to store payloads.
- **No raw or decoded payload is committed.** The barcode strings, the
  decoded records, extracted images, downloaded decoder source, and the
  decode scripts all live only in the session scratchpad. This document
  intentionally describes structure in the abstract and omits the ticket
  number, order reference, passenger name, and payload text.
- **No decoder was built for reuse.** The scratchpad script existed
  solely to validate the public spec against these two tickets.
- **ITSO and UIC 918.x** formats — out of scope entirely (§6).

## Open questions / risks

1. **Legal/ToS posture is genuinely murky, and worth stating plainly.**
   The RSP-6 specification is proprietary to RDG/RSP, and RDG has stated
   on the record (the 2018 FOI thread, §5) that the spec is available
   only to TOCs, TIS suppliers, and accredited third-party retailers
   through its accreditation process — i.e. the closure is deliberate
   policy, not an oversight. The verification keys in public circulation
   were obtained by reverse-engineering inspector apps and an
   unauthenticated third-party endpoint — never officially published or
   licensed for third-party use. Decoding one's *own* tickets for
   personal understanding is the fact pattern of all the public research
   and has drawn no known enforcement; but any *product* feature built
   on this would rest on unlicensed key material of awkward provenance,
   process third parties' personal data (passenger names) from a format
   its owner deliberately keeps closed, and sit in exactly the territory
   this codebase's audited constraints exist to avoid. No published
   terms specifically restricting decoder implementations were found —
   but that's because there are no published terms at all, only the
   accreditation gate above, which is not the same comfort.
2. **Unknown fields remain unknown**: the 7-bit header (constant
   `0000001` here), the checksum algorithm, the bit-371 flag, the
   departure-time-flag semantics, and the sub-UTN's meaning (both legs
   read `00`) — none resolvable from two same-order tickets.
3. **Single-observation breadth**: both tickets are one order — same
   retailer, issuer, fare, railcard, day. Fields confirmed here are
   confirmed as *positioned and encoded* per the spec, but enum ranges
   (season coupons, reservations, free text, passenger-ID documents,
   limited-duration tickets) were not exercised.
4. **Key-set drift**: whether the community dump still covers all
   currently-active issuers/keys in 2026 was not tested beyond this one
   issuer; the endpoint eta identified for fresh keys was deliberately
   not probed as part of this research.

## References

- [eta — Reversing UK mobile rail tickets (2023-01-31)](https://eta.st/2023/01/31/rail-tickets.html)
- [rsp6-decoder crate (crates.io)](https://crates.io/crates/rsp6-decoder) /
  [lib.rs listing](https://lib.rs/crates/rsp6-decoder) /
  [browser demo](https://eta.st/tickets/)
- [Hackaday — Reverse Engineering British Rail Tickets (2023-02-09)](https://hackaday.com/2023/02/09/reverse-engineering-british-rail-tickets/)
- [WhatDoTheyKnow — RSP-6 Specification for Barcodes FOI request, 2018](https://www.whatdotheyknow.com/request/rsp_6_specification_for_barcodes)
  (read via the repo owner's PDF printout of the page; the live site
  blocks automated fetches)
- Tooling: `pymupdf` 1.28.2 (PDF render), `zxing-cpp` 3.1.1 Python
  bindings (Aztec decode), `unzip`/`jq` (pkpass inspection), scratchpad
  Python re-implementation of the decode path
- This repo: `crates/api/src/data/ticket_extraction.rs` (module-doc
  constraint),
  `crates/api/migrations/20260829090000_journey_ticket_tracking.sql`
  (audited no-barcode-payload header),
  `docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md`
  (Decision 4)
