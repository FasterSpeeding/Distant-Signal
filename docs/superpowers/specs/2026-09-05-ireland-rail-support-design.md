# Ireland Rail Support (Both Jurisdictions) — Design Spec

**Status: design spec, not an approved plan.** No implementation, no code
in this pass.

## This document supersedes an existing spec — read this first

`docs/superpowers/specs/2026-09-05-northern-ireland-rail-support-design.md`
("the NI spec") already exists, merged to `main`. It scoped Northern
Ireland Railways (NIR/Translink) coverage in isolation and concluded:
Tier A (static station/line reference data) is a confirmed, low-cost,
recommendable first step; Tier B (live departures) and Tier C
(line-status parity) are a no-go for now, blocked on an RTPI API whose
field-level schema could not be read (every fetch attempt 500'd/503'd,
across two research sessions) and an access-gated incident-data API this
app never registered for.

Since that document was written, a follow-up research pass —
`docs/superpowers/specs/2026-09-05-ireland-vs-northern-ireland-friction-research.md`
("the friction doc") — was run at the repo owner's request to answer a
narrower question the NI spec never asked: *if this app also covered the
Republic of Ireland (Iarnród Éireann), how much marginal friction would
that add on top of NI alone?* Its headline finding changes the shape of
the answer, not just the scope: Iarnród Éireann's own data situation is
**not** an incremental cost on top of NIR's — for the static/reference
tier it is confirmed *cheaper and more immediately available* than NIR's
own equivalent (a real, key-free, standard GTFS feed, downloaded and
parsed directly this session, versus NIR's still-unread CSV schema), and
for the live tier it is **currently reachable** where NIR's is not (a
legacy XML API answered every query on the first attempt, across
sessions where NIR's modern-looking API answered none). The one real cost
unique to covering both is new and unaddressed by either prior document:
the Belfast–Dublin Enterprise service and several border-area stations
are covered by *both* networks' feeds independently, under unrelated
identifier schemes.

**What this document supersedes, precisely:** the NI spec's §4/§5 —
its Tier A/B/C framing and its "Tier A only, no-go on B/C" recommendation
— are superseded in full by §5/§6 below, which re-derive scope for
*combined* coverage rather than NI alone. The NI spec's §2 data-model
proposal (parallel `common::NirStation`/`NirLineDefinition` types) is
superseded by §3 below's generalized type, made possible only because a
second non-GB network is now actually in scope. The NI spec's §1
(Translink/NIR data-source findings) and §3 (reusable-infrastructure
analysis: TfL-shaped frontend reuse, `poller-ldbws`-shaped backend
analogue) are **not** superseded — they're accurate, cited by reference
below, and remain the NI half of this document's combined findings. The
friction doc is not superseded either; it remains the primary source for
every Iarnród Éireann-side factual claim below and is cited throughout
rather than restated in full. A future reader should treat the NI spec as
a historical record of a scoping decision made before Ireland was
considered jointly, and this document as the current answer for combined
scope.

## What was asked for

Consolidate the NI spec and the friction doc into one coherent design
covering both Northern Ireland (Translink/NIR) and the Republic of
Ireland (Iarnród Éireann), re-deriving — not just restating — the
data-model shape, the cross-border overlap handling, the tiered scope
recommendation, and an overall go/no-go, now that the friction doc has
shown combined scope isn't simply double the cost of NI alone.

Per this repo's citation discipline, carried through both source
documents: every claim below is cited to one of the two source documents
(themselves cited to `file:line` or a directly-fetched source) or to
`crates/common/src/lib.rs`/other in-repo files directly. Nothing here is
a new, unverified factual claim about either network's data — where
something would need actual further verification, it's flagged as an
open question (§8), not guessed at.

---

## 1. What's actually available — both networks, one comparison

### 1.1 Static/reference-data tier

| | Northern Ireland (NIR) | Republic of Ireland (Iarnród Éireann) |
|---|---|---|
| Format | CSV + GeoJSON (stations), SHP + GeoJSON (network lines) — OpenDataNI | Standard GTFS zip — Transport for Ireland |
| Access | No key; dataset pages found via search, but the stations dataset's own page 403'd on direct fetch, so its **column schema is still unread** (NI spec §1.4) | No key, no registration; a plain anonymous HTTPS GET of a 9.6 MB zip, downloaded and unzipped directly this session (friction doc §1) |
| Confirmed scale | ~50 stations / 5-6 lines, ~220 route miles — a **secondary-source cross-check** (fan wiki, Wikipedia), not a verified dataset schema (NI spec §1.4) | **152 stations** (`stops.txt` row count), **18 route corridors** (`routes.txt`) — this session's own primary count from the downloaded feed (friction doc §1, §3) |
| Freshness | Not stated; static CSV/GeoJSON, no cadence documented | `feed_start_date`/`feed_end_date` in `feed_info.txt` show a live, rolling one-year window, current as of the download (friction doc §1) |
| Parsing effort | Hand-curate structs from a CSV whose schema isn't yet readable | A mature, versioned public spec (gtfs.org) plus an off-the-shelf, actively-maintained Rust crate (`gtfs-structures`, 195k downloads, latest release days before the friction doc's research session) — "pick a parser, map its structs onto `common::` types," not "reverse-engineer a schema" (friction doc §2) |

**Bottom line for this tier, stated once rather than left implicit in
either source document: Iarnród Éireann's static tier is unambiguously
cheaper to stand up than NIR's, despite covering a ~3x larger network.**
GTFS parsing cost is close to flat with respect to feed size — the same
`gtfs-structures` call reads 15 stations or 1,500 (friction doc §6.4) —
so the larger Ireland catalogue does not carry a proportionally larger
integration cost. NIR's own tier remains real and accessible, just
gated on one remaining step neither research session performed: actually
reading the OpenDataNI stations CSV's column schema (§8).

### 1.2 Live/real-time tier

| | Northern Ireland (NIR) | Republic of Ireland (Iarnród Éireann) |
|---|---|---|
| Candidate API | `apis.opendatani.gov.uk/translink/` (RTPI arrivals/departures) | `api.irishrail.ie/realtime/realtime.asmx` (legacy ASMX/SOAP, callable as REST-style GET) |
| Documentation | Modern-looking, well-documented dataset page; formats and licensing confirmed | Undocumented on any current NTA developer portal; per a third-party developer comment, apparently unchanged since ~2012 |
| Reachability | **Unreachable across two independent research sessions** — every direct fetch 500'd, 503'd, or (for the Worldline fallback mirror) returned only a client-rendered loading shell with no data | **Reachable on the first attempt**, this session — `getStationDataByCodeXML`, `getAllStationsXML`, and `getCurrentTrainsXML` all returned well-formed, fully populated XML with no key |
| Schema | **Still unread** — the single biggest open fact from the NI spec, unchanged by this document | Confirmed directly: per-service `Origin`/`Destination`/`Scharrival`/`Schdepart`/`Exparrival`/`Expdepart`/`Late`(minutes)/`Status`/`Duein` fields — structurally close to what `poller-ldbws` already ingests for GB stations |
| Pattern-A (pre-computed status) alternative | Translink Transport API's "incident information" type — access-gated behind an email registration this app never pursued; NI-rail-vs-bus scope unconfirmed | None — GTFS-Realtime for Ireland (`gtfsr.transportforireland.ie/v1`) is confirmed bus-only (Dublin Bus, Bus Éireann, Go-Ahead Ireland), no rail, no DART, per data.gov.ie's own announcement and independent corroboration |

**Bottom line for this tier: both networks need the same shape of
bespoke work (LDBWS-style delay-threshold inference over raw per-service
data — neither has a confirmed Pattern-A feed), but only one of the two
is actually reachable today.** The friction doc's framing is worth
carrying forward directly: "if NIR's API remains unreachable, 'add
Iarnród Éireann's live tier too' is not an incremental cost on top of a
working NI live tier — it may end up being the *only one of the two
that's actually buildable* in the near term" (friction doc §6.2).

### 1.3 What neither network offers

Neither NIR nor Iarnród Éireann has a confirmed, reachable, pre-computed
line-status/incident feed (the TfL-shaped Pattern-A this app's
`poller-tfl` already relays cheaply). NIR's one candidate is access-gated
and unconfirmed for rail scope (NI spec §1.3); Iarnród Éireann has none
at all — its only GTFS-RT product is bus-only (friction doc §1). For both
networks, any line-status feature beyond raw departure data means this
app building its own severity inference, mirroring the cost this app
already paid once for GB LDBWS sampling (DESIGN.md §6.2).

---

## 2. Reusable infrastructure (carried forward from the NI spec, applies to both networks)

The NI spec's §3 analysis holds unchanged and applies equally to an
Iarnród Éireann integration, since the same two structural facts drive
it for both networks:

- **Frontend line-status rendering is reusable as-is**, if either
  network's poller produces `LineStatus`/`LineStatusReport` values keyed
  by a line id — the existing TfL-shaped JSON path
  (`to_tfl_shape`/`to_tfl_shape_with_overlay`, `crates/api/src/render.rs:14-44`)
  already renders five non-national-rail modes generically today, and
  needs no new rendering path for a sixth or seventh.
- **Frontend station pages need new rendering, not just new data**, for
  the same reason in both cases: the existing route
  (`frontend/app/stations/[crs]/page.tsx`) and search flow
  (`StationSearchForm.tsx`) are keyed on CRS codes, and neither NIR nor
  Iarnród Éireann stations have one.
- **A backend poller for either network would resemble `poller-ldbws`
  more than `poller-tfl`**, per §1.2 above: both are raw per-service
  arrivals data needing this app's own inference, not an already-computed
  status feed.

None of this changes by adding a second non-GB network — it's the same
architectural fact applying twice, which is itself part of why §3 below
concludes a shared data-model abstraction is now warranted where it
wasn't for NI alone.

---

## 3. Data-model decision, re-derived for combined scope

The NI spec proposed NIR-specific types: `common::NirStation { id, name,
latitude, longitude }` and a parallel `common::NirLineDefinition`,
deliberately not reusing `common::Station`/`LineDefinition` because
`Station.crs` is a required, non-`Option` field with no NIR-shaped value,
and NIR has no ATOC operator code either (NI spec §2). The friction doc
flagged, but did not decide, that this NIR-specific shape stops being the
obviously-right choice once Iarnród Éireann is also in scope: both
networks share the identical structural gap from `common::Station` (no
CRS, no ATOC), so a single generic type could serve both — but that
generalization is "only worth doing if both are actually in scope"
(friction doc §5), a condition that was left open there and is resolved
here: **this document is about combined scope, so that condition is now
met.**

**Decision: adopt one generic, network-tagged type — not two
NIR-specific types, and not a naive shared type with no network
discriminant.**

```rust
pub enum IslandOfIrelandNetwork {
    NorthernIreland,   // NIR/Translink-sourced
    RepublicOfIreland, // Iarnród Éireann-sourced
}

pub struct IslandOfIrelandStation {
    pub id: String,             // the *sourcing* network's own station code/slug
    pub name: String,
    pub network: IslandOfIrelandNetwork,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

pub struct IslandOfIrelandLineDefinition {
    pub id: String,
    pub name: String,
    pub network: IslandOfIrelandNetwork,
    pub stations: Vec<String>,  // IslandOfIrelandStation.id, same-network only
}
```

Reasoning, weighed directly against the alternative (keep NIR-specific
types, add a separate parallel `IarnrodEireannStation`/
`IarnrodEireannLineDefinition` pair):

- **For:** the two networks' domain shape is identical field-for-field —
  `id`/`name`/optional lat-lon, no CRS, no ATOC, no segment — for the
  same underlying reason in both cases (neither is in the GB TOC-code/CRS
  ecosystem). Two structurally-identical struct definitions differing
  only in name would be pure duplication, the same anti-pattern this
  repo already avoids elsewhere (there is one `Station`/`LineDefinition`
  pair for every GB operator, not one per TOC). The friction doc's own
  reasoning (§5) — a single type "would serve both without duplicating
  the struct definition twice" — is correct and this document adopts it
  rather than re-deriving a different answer.
- **Against, considered and rejected:** the friction doc also flagged
  that a clean per-station `network` tag "stops being clean exactly at
  Belfast/Lisburn/Portadown/Lurgan" (§4/§5) — i.e. the border overlap
  makes "which network is this station in" an ambiguous question for a
  handful of stations. This is real, but it is resolved by §4 below
  reframing `network` as **"which feed is authoritative for this row,"
  not "which jurisdiction is this station physically in."** Once the
  Enterprise-overlap decision (§4) designates a single authoritative
  source per border-area station, every `IslandOfIrelandStation` row has
  exactly one unambiguous `network` value by construction — the
  ambiguity lives in the §4 sourcing decision, not in the type itself.
- **Ingestion stays separate, deliberately.** A shared `common::` type
  does not imply a shared poller. Iarnród Éireann's loader is
  GTFS-parser-backed (`gtfs-structures`, reading `stops.txt`/`routes.txt`
  once and mapping onto `IslandOfIrelandStation`/`LineDefinition`); NIR's
  loader remains a hand-curated/CSV-backed pipeline (pending §8's schema
  read). This mirrors how this app already has multiple distinct
  pollers (`poller-ldbws`, `poller-tfl`, `schedule-ingest`) feeding a
  shared `common::` model today — the friction doc's own analogy (§5),
  reused here rather than re-argued.
- **`LineDefinition.mode`/TfL-shape integration is unaffected.** Per §2
  above, whichever network's poller emits `LineStatus` rows would still
  route through the existing TfL-shaped JSON path with a new `modeName`
  value per network (e.g. `"nir-railways"`, `"iarnrod-eireann"|"irish-rail"`)
  — the same mechanism the NI spec already designed (NI spec §2, final
  paragraph), unaffected by whether the *station/line catalogue* is one
  generic type or two specific ones.

**What this does not decide:** the exact serialization/API-shape
consumers of `IslandOfIrelandStation` would use, or whether the frontend
needs one shared non-CRS station route or two (`/ni-stations/[id]` vs.
`/irish-rail-stations/[id]`) — that's an implementation-time UI decision,
out of this document's scope per its own framing (documentation
consolidation, not implementation).

---

## 4. The Enterprise/border-overlap problem — a concrete resolution

Neither source document fully designed a solution to this; the friction
doc identified and scoped it (§4 there) but stopped short of a decision.
This section makes the call.

**The problem, restated precisely:** Iarnród Éireann's own static (GTFS)
and live (`api.irishrail.ie`) data both extend past the border and
already model the full Belfast–Dublin Enterprise route, including
Belfast, Lisburn, Portadown, Lurgan, and Newry as first-class stops, plus
NI-side signalling/junction points (CITY JUNCTION, CENTRAL JUNCTION,
DUNMURRAY, MOIRA), all under Iarnród Éireann's own station-code scheme
(`BFSTC`, `LBURN`, `PDOWN`, `LURGN`, `NEWRY`). NIR's own data (per the NI
spec) independently covers the same physical stations and the same
Enterprise service under its own, unrelated scheme, once its own
Belfast/Enterprise coverage is built out. If both feeds were ingested
naively, the same physical train and the same physical stations would
appear twice, under two unrelated identifiers, with two independently
computed delay/status readings that could disagree — a real
double-counting risk, not a hypothetical one (friction doc §4).

**Decision: for the border-area stations and the Enterprise service
itself, source from Iarnród Éireann only — do not ingest NIR's data for
these specific stations at all, for now. No dedup/correlation layer is
built in this pass.**

Concretely, the following stations/lines are designated
`network: RepublicOfIreland` (i.e. sourced from Iarnród Éireann's feeds),
even though Belfast, Lisburn, Portadown, and Lurgan are physically in
Northern Ireland: Belfast (Grand Central/whatever terminus GTFS's `BFSTC`
maps to), Lisburn, Portadown, Lurgan, Newry, and the Enterprise line
itself (`DUB-BFT-I`/`DUB-BFT-O`). NIR's own station/line catalogue (§5
below) is scoped to NIR's *remaining* stations only — it does not need to
independently curate these five names from OpenDataNI's dataset once
that dataset's schema is read, because they're already covered under the
Iarnród Éireann-sourced rows.

**Reasoning:**

1. **Iarnród Éireann's feed already models the entire physical route**,
   not just the ROI-side segment — it includes NI-side signalling
   junctions with no passenger role, evidence that this is a complete,
   intentional model of the corridor, not an accidental border-crossing
   artifact of a feed that happens to include a few extra rows.
2. **It's the only one of the two that's actually reachable and
   schema-confirmed today** (§1.2). Choosing NIR as the authoritative
   source for these stations would mean gating the *entire* Enterprise
   corridor — the busiest, most cross-border-visible line either network
   operates — on the one API that has failed on every fetch attempt
   across two independent research sessions. Choosing the confirmed,
   working source for exactly the stations where a choice must be made
   is the lower-risk call.
3. **A single-authoritative-source policy needs no dedup/correlation
   machinery at all**, which a "ingest both, then reconcile" design
   would. That correlation would be non-trivial to build correctly: the
   two feeds' identifier schemes differ completely (Iarnród Éireann's
   own `Traincode`, e.g. `A101`, is unrelated to whatever headcode/
   service-id scheme NIR's RTPI feed would use), so any dedup mechanism
   would have to key on **scheduled time + station-pair** (e.g. "a
   service scheduled to depart Belfast for Dublin Connolly at 06:00" as
   the join key across both feeds, since neither feed's own train
   identifier is comparable to the other's) — a real, buildable
   mechanism in principle, but genuinely new engineering work with
   failure modes of its own (schedule changes, short-notice
   cancellations, and clock-skew between two independently-run systems
   could all break a naive time-based match). Building that now, for a
   feed on the other side of the join that isn't even confirmed
   reachable, is not justified.
4. **This is a scoping decision for this app's own data model, not a
   claim about which operator "really" runs the border-area stations.**
   Both operators jointly run the Enterprise service in reality; this
   app picking one feed as its own internal source of truth for a subset
   of stations doesn't change or need to represent that operational
   joint-running fact.

**When to revisit:** if NIR's RTPI schema is eventually read (§8) and
turns out to carry meaningfully better data for the NI-side stations
than Iarnród Éireann's feed does (e.g. more accurate real-time delay data
specific to the NI signalling territory), the single-source-of-truth
choice above should be re-evaluated — possibly by switching the
authoritative source for just those stations, not by building the
dedup/correlation layer preemptively. This is flagged as an open
question (§8), not resolved here, because it depends on a fact (NIR's
actual schema/data quality) neither source document has.

---

## 5. Scope tiers, re-derived for combined coverage

The NI spec's Tier A/B/C were scoped to NIR alone. Re-deriving them for
combined coverage, given §1's comparison and §4's overlap resolution:

**Tier A — static reference data, both networks, sequenced by
availability, not by which network was spec'd first.**

- *Iarnród Éireann half — ship first.* GTFS-parse `GTFS_Irish_Rail.zip`
  via `gtfs-structures` into `IslandOfIrelandStation`/
  `IslandOfIrelandLineDefinition` rows tagged `RepublicOfIreland`,
  including the five border-area stations/the Enterprise line per §4.
  Fully unblocked today — no unread schema, no access gate, no unresolved
  fact standing between this document and a concrete build. This is a
  **stronger** Tier-A candidate than the NI spec's own NIR-only Tier A
  was, because it has zero remaining open questions (contrast NIR's
  CSV schema, still unread).
- *NIR half — ship once one prerequisite is met.* Hand-curate NIR's
  *remaining* stations (excluding the border-area set now sourced from
  Iarnród Éireann, per §4) from OpenDataNI's CSV/GeoJSON, tagged
  `NorthernIreland`. Unchanged from the NI spec's own Tier A in kind, but
  its one remaining blocker — actually reading the stations dataset's
  column schema, which 403'd on direct fetch in both research sessions
  — still needs a human with a browser or an authenticated fetch (§8),
  exactly as the NI spec already concluded.

**Tier B — live departures, both networks, no longer symmetric.**

- *Iarnród Éireann — viable now, not gated.* `api.irishrail.ie`'s legacy
  XML API is confirmed reachable and schema-readable today
  (`Origin`/`Destination`/`Scharrival`/`Schdepart`/`Exparrival`/
  `Expdepart`/`Late`/`Status`/`Duein` — §1.2). A `poller-ldbws`-shaped
  service per station, feeding the same delay-threshold inference this
  app already runs for GB LDBWS, is buildable now on the evidence in
  hand. This is a genuinely new finding versus the NI spec's own
  framing, where the equivalent tier for NIR was a flat no-go: **for
  Iarnród Éireann specifically, Tier B is not blocked.**
- *NIR — still blocked, unchanged from the NI spec.* The RTPI feed's
  field-level schema remains unread after two independent research
  sessions (§1.2); this document does not resolve that, and does not
  recommend building Tier B for NIR until it is.
- *Border-area stations/Enterprise service — sourced from Iarnród
  Éireann's live API per §4*, so this piece of "NIR's live tier" is
  actually already covered once Iarnród Éireann's Tier B ships, without
  waiting on NIR's own API at all.

**Tier C — line-status/incident parity, unchanged for both networks.**
Neither network has a confirmed Pattern-A pre-computed status feed
(§1.3): NIR's Transport API incident-data lead remains access-gated and
unconfirmed for rail scope; Iarnród Éireann's only GTFS-RT product is
confirmed bus-only. For both, Tier C means the same bespoke
severity-inference investment the NI spec already sized against this
app's own LDBWS-sampling cost (DESIGN.md §6.2) — the friction doc's
scale finding (§3, Ireland ~3x NIR by station/line count) doesn't change
this specific cost, since inference-pipeline cost tracks *feed
complexity*, not just row count, and both feeds are equally raw
per-service data. Tier C for either or both networks remains a real,
substantial, and currently unjustified investment absent a Pattern-A
discovery for one of them.

**Recommended minimum-viable first step, combined:** ship Iarnród
Éireann Tier A now, and treat Iarnród Éireann Tier B as a fast-follow
worth attempting in the same pass, given it is *also* unblocked on the
evidence gathered — the friction doc's own framing applies directly
here: "doing both together may be a more efficient first target than NI
alone, precisely because Iarnród Éireann's reference-data tier is real,
standard, and immediately usable in a way NIR's own isn't yet" (friction
doc §6). This is a genuine change from the NI spec's own recommendation
("Tier A [NIR], nothing else, for now") — not because NI's case
weakened, but because Ireland's case turned out stronger and more
immediately actionable than either prior document, read alone, would
have suggested. NIR's own Tier A remains worth shipping too — it's cheap
and still delivers real value (`/stations` stops returning zero NI
results) — but it should be sequenced behind, or run in parallel with,
Ireland's work rather than treated as the sole first deliverable, and it
still needs its one prerequisite (§8's CSV-schema read) regardless of
sequencing.

---

## 6. Reusable-infrastructure note specific to combined scope

One point neither source document had reason to raise alone: with two
non-GB networks now real candidates, the case for the generic
`IslandOfIrelandStation`/`LineDefinition` type (§3) also strengthens the
case for a shared, generic ingestion-adjacent concern — a single
`infer_from_samples`-equivalent parameterized over "how do I identify a
station/how do I read a delay-minutes field," rather than one bespoke
copy per network. The NI spec flagged this as a maybe ("optimistically, a
shared one — but that would first need to work generically over an id
scheme that isn't CRS," NI spec §3). This document does not resolve that
question either way — it depends on actually building at least one of
the two Tier-B pipelines first and seeing how much of `infer_from_samples`
genuinely generalizes versus how much is GB-LDBWS-specific incidentally.
Flagged as a design question for whichever Tier-B implementation lands
first, not decided here.

---

## 7. Go/no-go recommendation

**Go on Iarnród Éireann Tier A and Tier B. Conditional go on NIR Tier A,
pending the CSV-schema read. No-go on NIR Tier B/C and on Tier C for
either network, unchanged from the NI spec's own conclusion on this
point.**

This is a genuine synthesis, not a restatement of either source
document's separate conclusion. The NI spec's own go/no-go ("Tier A
only, no-go on B/C") was correct for NI evaluated alone, given what was
knowable about NIR alone — but the friction doc's evidence (a real,
directly-downloaded, standard GTFS feed; a legacy but directly-queryable
live XML API; an actively-maintained parsing crate; all confirmed this
research pass, none of it available to the NI spec's own author) shows
that evaluating NIR in isolation understated how much of "Ireland rail
coverage" as a combined goal is *already* buildable today. The overall
picture across both documents: **the honestly cheapest and most
immediately shippable next step in this entire area is not NIR's Tier A
— it's Iarnród Éireann's Tier A, with Tier B realistically in the same
pass.** NIR's own Tier A remains a real, worthwhile, low-cost win in its
own right and is not downgraded by this finding — it just isn't the
strongest candidate for "what ships first" anymore, and its live tiers
remain correctly gated on facts (§1.2, §8) neither research pass could
establish.

The one qualifier that applies to the whole combined scope, not just one
tier: this recommendation assumes the border-overlap resolution in §4
(single authoritative source, no dedup) is acceptable as a v1 policy. If
a future requirement demands both operators' independent views of the
Enterprise service be shown (e.g. for cross-checking delay data), that
requires the dedup/correlation mechanism §4 explicitly deferred, which
is real, unbudgeted, additional work beyond everything scoped as "go"
above.

---

## Non-goals

- **Implementation of any kind.** This is a documentation consolidation
  and design-synthesis pass; no code, no `Cargo.toml` additions
  (`gtfs-structures` or otherwise), no migrations.
- **Reading NIR's OpenDataNI stations-CSV column schema, or its RTPI
  API's field-level response schema.** Both remain open (§8); this
  document reasons around their absence, it doesn't resolve them.
- **Registering for the Translink Transport API key**
  (`servicedata@translink.co.uk`) or otherwise resolving whether its
  incident-information type covers NI rail. Still an open lead, not
  pursued in this pass, same as both source documents.
- **Designing the Enterprise dedup/correlation mechanism in full**,
  beyond naming its likely join key (scheduled time + station pair) and
  the reasons it isn't being built now. If §4's policy is revisited, that
  mechanism needs its own design pass.
- **Frontend route/UI design** for either network's station pages
  (`/ni-stations/[id]`-shaped vs. a shared route) — flagged in §3 as a
  later, implementation-adjacent decision.
- **Confirming `api.irishrail.ie`'s usage terms, rate limits, or
  long-term stability.** It is undocumented on any current NTA developer
  portal and, per a third-party comment cited in the friction doc,
  apparently unchanged since roughly 2012 — reachable and schema-clean
  today, but its operational durability as a dependency is not verified
  here (§8).
- **An implementation plan.** A separate, later step, matching this
  repo's own process, and matching the posture both source documents
  already took.

---

## Open Questions

1. **NIR's OpenDataNI stations-CSV column schema is still unread** — the
   dataset page 403's on direct fetch in every attempt across both
   research sessions. This is the single blocking fact for NIR's own
   Tier A (§5) and needs a human with a browser, or an authenticated
   fetch, to resolve — the same conclusion the NI spec already reached,
   unchanged here.
2. **NIR's RTPI live-API field-level schema is still unread** — every
   direct fetch attempt has 500'd, 503'd, or returned a client-render-
   only shell, across two independent research sessions. Blocks NIR
   Tier B entirely (§5); does not block anything on the Iarnród Éireann
   side.
3. **The Translink Transport API's "incident information" scope
   (rail vs. bus-only) remains unconfirmed** — requires actually
   registering for a key (`servicedata@translink.co.uk`), which neither
   research pass pursued. This is the one lead that could make NIR Tier
   C cheap (Pattern-A) if it resolves favorably.
4. **`api.irishrail.ie`'s operational durability is unverified.** It
   answered every query this session sent it, but it is legacy,
   undocumented on any current official portal, and per third-party
   comment apparently unmaintained since ~2012. Whether it has any
   informal rate limits, whether NTA has plans to deprecate it, and
   whether its uptime is reliable over time are all unconfirmed — worth
   checking before committing to it as this app's sole live-data source
   for the Enterprise corridor (§4).
5. **Whether NIR's data (once §8.2 is resolved) is meaningfully better
   for the border-area stations than Iarnród Éireann's** is unknown and
   directly gates whether §4's single-authoritative-source decision
   should be revisited. Not answerable until NIR's schema is actually
   read.
6. **Whether a shared, generic delay-inference pipeline (§6) is
   realistic across both networks**, versus two bespoke copies, is
   speculative until at least one of the two Tier-B pipelines is
   actually built and its data shape is compared against
   `infer_from_samples`'s existing GB-LDBWS-specific assumptions.
7. **Exact frontend route/naming for the generic station type** (§3's
   closing paragraph) — a shared non-CRS station route versus two
   parallel per-network routes — is left to implementation time.

---

## References

- `docs/superpowers/specs/2026-09-05-northern-ireland-rail-support-design.md`
  — superseded in part per this document's opening section; source for
  every NIR-specific factual claim above (§1.1-1.5 there cover NIR's
  data-source findings in full; §2 covers the original NIR-specific
  data-model proposal; §3 covers the reusable-infrastructure analysis
  reused in §2 above).
- `docs/superpowers/specs/2026-09-05-ireland-vs-northern-ireland-friction-research.md`
  — not superseded; source for every Iarnród Éireann-specific factual
  claim above, and for the friction/scale/overlap findings in §1, §3,
  §4, and §5 that this document synthesizes rather than restates.
- `docs/superpowers/specs/2026-09-05-status-observability-grafana-design.md`
  — structural/tone precedent for this document's own supersession
  section.
- `crates/common/src/lib.rs:443-451` (`Station`), `:461-500`
  (`LineDefinition`) — cited via the NI spec, load-bearing for §3's
  data-model reasoning (required `crs: String` field with no NIR/Iarnród
  Éireann-shaped value).
- `crates/api/src/render.rs:14-44` (`to_tfl_shape`/
  `to_tfl_shape_with_overlay`) and `crates/api/src/data/queries.rs:460-494`
  (`TflLineSummaryRow`) — cited via the NI spec, the shared JSON path §2
  above says both networks' line-status data could reuse.
- `frontend/app/stations/[crs]/page.tsx`,
  `frontend/app/stations/StationSearchForm.tsx:11` — cited via the NI
  spec, the CRS-keyed frontend route/search flow neither network's
  stations fit.
- DESIGN.md §5.1 (ATOC operators), §6.2 (LDBWS inference cost precedent),
  §10 (line-catalogue size targets) — cited via the NI spec, reused in
  §5's Tier C sizing.
