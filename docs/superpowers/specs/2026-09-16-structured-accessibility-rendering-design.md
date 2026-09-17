# Design: Structured Rendering for Station Accessibility & Facilities

**Status: design proposal, not approved. No implementation, no code or
migration changes.** This document supersedes the rendering half of
`docs/superpowers/specs/2026-09-12-station-accessibility-design.md`
(Decision 6) on the strength of evidence that spec did not have. It changes
nothing else about that feature: the allowlist, the route, the three-state
handling and the section's placement all stand as designed and shipped.

## 0. The idea, as briefed

The shipped Accessibility & facilities section on `/stations/[crs]` renders
a 12-key allowlist through a generic, depth-limited renderer that falls back
to collapsed raw JSON for anything deeper than a flat object
(`frontend/lib/stationAccessibility.ts`,
`frontend/components/StationAccessibilitySection.tsx`). That fallback was a
deliberate hedge, not a preference. The original spec's Open question 1
(`2026-09-12-station-accessibility-design.md:607-613`) names the cost
plainly:

> **The exact real-world shape of every allowlisted key is unverified
> against a live RDM payload.** Correction 2 is the reason this design leans
> on a generic renderer rather than typed components; that renderer is a
> hedge against the unknown, not a substitute for eventually fetching one
> real station's full `Station` object and checking it by hand. Worth doing
> before or shortly after implementation, not resolved by this document.

This document does that survey against real production data, and proposes
what to render instead.

**Conclusion up front:** the hedge is no longer justified. The RDM payload
is not an unpredictable per-station blob — it is a stable, finite schema
with a small number of heavily-repeated shapes. The raw-JSON fallback fires
on **96.2%** of key-renders today, so the section is, in practice, a JSON
dump with headings. The proposal is a **frontend-only** change: render seven
confirmed structural patterns, keep a much narrower fallback, and leave the
backend a key-name-filtered passthrough exactly as it is.

## 1. The survey: method, sample, and its honest limits

### 1.1 Method

Cluster/database access was not usable from the authoring sandbox (SSH to
the cluster host is blocked by this environment's permission system, and no
RDM credentials are available). The data below was therefore obtained from
the **public production deployment** at `https://ds.cursed.solutions`, which
serves this exact feature.

`/stations/{crs}` is a server component
(`frontend/app/stations/[crs]/page.tsx`) that fetches
`/public/stations/{crs}/accessibility` server-side and passes the result to
the client component `StationAccessibilitySection`. React serializes those
props into the RSC flight payload embedded in the page HTML. Recovering that
payload therefore yields **the exact object the backend returned** — the
post-`filter_accessibility_fields` 12-key slice, not a re-derivation.

Two independent reads were used and cross-checked:

1. Parse `self.__next_f.push([1, "…"])` chunks out of the page HTML and
   brace-match the object after `"coverage":"present","data":`. This gives
   the JSON verbatim.
2. Load the same pages in headless Chromium, expand every
   `aria-expanded="false"` disclosure, and read the rendered section text.
   This confirms what a reader actually sees today, and independently
   corroborates (1) — the expanded "Raw data" blocks are
   `JSON.stringify(value, null, 2)` of the same values.

All access was read-only HTTP GET of public, unauthenticated pages. This is
public station facility data, not personal data.

### 1.2 Sample — 31 stations

Chosen before looking at any results, to span termini and request stops, all
four nations, and many operators. Every one returned `200` with
`coverage: 'present'`:

`ABD` `BAL` `BHM` `BRI` `BSK` `BTN` `CAR` `CBG` `CDF` `DNO` `EDB` `EUS`
`EXD` `GLQ` `HUL` `INV` `IPS` `KGX` `LDS` `LLE` `MAN` `NRW` `PMH` `PNZ`
`SHF` `SKG` `SOU` `STP` `TWY` `WVH` `YRK`

That deliberately includes the extremes: `MAN` (Manchester Piccadilly,
largest payload at 29,919 bytes), `DNO` (Dunrobin Castle, a seasonal
request stop) and `BAL` (Balham, smallest at 9,065 bytes).

### 1.3 Honest limits of this method

- **22 of 6,996 strings (0.31%) came back as RSC de-duplication references**
  (`"$2b"`, `"$35"`, …) rather than their literal text. These are an
  artifact of reading the flight payload, **not** real feed values. They
  affect a handful of quoted *values*; they do not affect any *shape* claim
  in this document, since a reference stands in for a string either way.
- 31 stations is a sample, not the ~2,600-station population. Every
  frequency below ("31/31", "396 instances") describes this sample. §2.6
  gives a concrete case where a 17-station sample was already misleading.
- The survey reads what the **API returns**, so it cannot distinguish "the
  feed omitted this key" from "the key was JSON `null` and
  `filter_accessibility_fields` dropped it". For `dropOffPickUp` (absent at
  6/31) either explanation is consistent with the data.

## 2. Findings

### 2.1 The current renderer degrades to raw JSON almost always

Applying `renderShallowObject`'s own bail condition
(`frontend/lib/stationAccessibility.ts:122-138` — the whole object degrades
to raw if *any* own value is a nested object or an array containing
non-primitives) to all 366 key-renders in the sample:

**352 / 366 = 96.2% of rendered keys are a collapsed "Raw data" block.**

Per key:

| Key | Raw-JSON fallback |
|---|---|
| `stationAccessibility` | 31/31 |
| `staffAssistance` | 31/31 |
| `toiletsAndChanging` | 31/31 |
| `transportLinks` | 31/31 |
| `cycling` | 31/31 |
| `platformFacilities` | 31/31 |
| `stationFacilities` | 31/31 |
| `helpAndSupport` | 31/31 |
| `loungesAndWaiting` | 31/31 |
| `dropOffPickUp` | 25/25 |
| `carParks` | 25/31 |
| `lifts` | 23/31 |

Ten of the twelve keys **never** render as anything but raw JSON. The two
exceptions are not a success case: `carParks` and `lifts` escape only at
stations whose `carParks`/`liftsInfo` array is empty, i.e. precisely where
there is nothing to show. Verified live at `DNO`, where the section renders
ten "Raw data" disclosures plus `Lifts` and `Car parks` as two-row key/value
lists.

This is the central finding. The section is not "structured with a fallback
for odd cases"; it is a JSON dump with a structured exception.

### 2.2 The schema is stable — 79/79 sub-keys present everywhere

Across the sample, for each of the 12 keys, **every** sub-key observed is
present in **100%** of the stations where the parent key is present:

| Key | Stable sub-keys | Stations |
|---|---|---|
| `stationAccessibility` | 9/9 | 31 |
| `stationFacilities` | 14/14 | 31 |
| `helpAndSupport` | 9/9 | 31 |
| `loungesAndWaiting` | 8/8 | 31 |
| `cycling` | 7/7 | 31 |
| `staffAssistance` | 7/7 | 31 |
| `transportLinks` | 7/7 | 31 |
| `carParks` | 5/5 | 31 |
| `platformFacilities` | 4/4 | 31 |
| `dropOffPickUp` | 4/4 | 25 |
| `lifts` | 3/3 | 31 |
| `toiletsAndChanging` | 2/2 | 31 |

There is **zero** key-set variation between a London terminus and a Highland
request stop. The whole sample contains **46 distinct object signatures**
and **480 distinct JSON paths** — a finite, enumerable schema, not an
open-ended blob. Variation between stations is entirely in *values* and in
`null`-vs-populated, never in which fields exist.

This directly contradicts the premise the hedge was built on.

### 2.3 Seven recurring shape patterns account for essentially all of it

Occurrence counts are objects matching that exact signature (or, for D,
objects with a `name` plus sibling fields) across the 31-station sample.

**Pattern A — Facility record (396 exact matches).**
`{available, location, notes, openingHoursNotes, openingTimes,
operatorContactDetails}`. The single most common object in the payload. It
appears under `stationFacilities.*` (`atm`, `wifi`, `shops`, `refreshments`,
`lostProperty`, `luggageStorage`, `postBox`, `trolleys`, `currencyExchange`,
`requestStop`, `defibrillator`), `toiletsAndChanging.{toilets,showers}`,
`loungesAndWaiting.{firstClass,seatingArea,waitingFacility}`,
`staffAssistance.{helpline,staffHelp}`, `helpAndSupport.staffHelp`, and
`stationAccessibility.{trainRamp,ticketBarriers}`. Several sites extend it
with extra scalars (`trainRamp` adds `storage`; `ticketBarriers` adds
`names`; `toilets` adds four booleans and `locations`).

A narrower two-field variant `{available, notes}` occurs 155 times — every
`transportLinks.*` sub-key.

**Pattern B — Opening-times entry (304 matches).**
`{daysOfTheWeek: string[], openPeriod: [{startTime, endTime}] | null,
openingStatus: string}`. Verbatim, from `ABD`:

```json
{
  "daysOfTheWeek": ["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday"],
  "openPeriod": [{ "endTime": "00:45:00.000", "startTime": "05:00:00.000" }],
  "openingStatus": "Specific Hours"
}
```

`startTime`/`endTime` are **always** `HH:MM:SS.mmm` (177 `{startTime,
endTime}` objects, no other format). `openingStatus` is a closed set of
three in this sample: `24 Hours`, `Specific Hours`, `Unavailable`.
`daysOfTheWeek` draws from **eight** values — the seven weekdays plus
`Public Holidays`; no entry repeats a day.

**Pattern C — Contact details (92 matches).**
`{emailAddress, name, note, operatorName, primaryTelephoneNumber}`,
optionally extended with `url` and a `postalAddress`
(`addressLine1`…`addressLine5`, `postcode`).

**Pattern D — Named-item collection (950 objects across 18 paths).** An
array of objects each carrying a `name` plus sibling descriptive fields.
Instances include `platformFacilities.platforms` (259),
`toiletsAndChanging.toilets.locations` (195), `lifts.liftsInfo` (111),
`transportLinks.taxi.taxiRanks`, `dropOffPickUp.points`,
`loungesAndWaiting.waitingRooms`, `stationAccessibility.passengerAssistance`
and `carParks.carParks`.

The important sub-finding: in most of these, **every non-`name` field is
already a complete English sentence**, so the item needs no field labels at
all. Verbatim, `BAL`:

```json
{
  "helpPointClose": "There is a Help Point close to this platform",
  "name": "Platform 3",
  "seatingAtIntervals": "Seating is limited on this platform",
  "waitingType": null
}
```

`carParks.carParks[]` is the exception — its items mix numbers, a `charges`
object of eleven rate strings, and an `operator` — and needs labelled
rendering.

**Pattern E — Sentence-valued scalar.** Of 372 non-HTML scalar strings at
depth 1, **340 (91%)** are full sentences beginning with a capital and
reading as display copy: `tactilePaving: "There are tactile warnings on all
platforms in use"`, `lifts.statement: "There are lifts"`,
`platformFacilities.entranceLevels: "The platforms are level with the Main
Entrance of the station"`, `announcements: "Announcements are made both
visually and audibly"`. The feed has already done the prose. The current
renderer's `humanizeKey` puts a redundant label in front of these
("Tactile paving: There are tactile warnings…").

**Pattern F — Token list.** Short `string[]` of labels or camelCase tokens:
`staffAssistance.informationAvailableFromStaff: ["Yes - from help point",
"Yes - from ticket office"]`, `helpAndSupport.customerInformationScreens`,
`cycling.typesOfStorage: ["Stands", "Racks"]`,
`staffAssistance.customerInformation: ["DepartureScreens", "Announcements",
"ArrivalScreens"]`, `ticketBarriers.names`.

**Pattern G — Rich text (HTML).** See §2.4.

### 2.4 10% of strings contain HTML — the finding that most changes the design

**708 of 6,996 strings (10.1%) carry HTML tags or entities.** They cluster
in the `notes`, `location`, `note` and `*Notes` fields of Patterns A, C and
D. Real values, verbatim:

```
"<p>Speak to on train staff.</p>"
"<p>Due to low platform constraints ramps can&#39;t be deployed.</p>"
"<p><a href=\"mailto:customer.relations@scotrail.co.uk\" title=\"\">customer.relations@scotrail.co.uk</a></p>"
"<p><strong>Passenger Assistance Meeting Points</strong>&#160;</p><p><em>For station accessibility, please refer to the &quot;Step Free Access&quot; section on the website.&#160;</em>&#160;</p><p>This is an unstaffed station. Please make yourself visible so that train staff can assist you and use the customer help point if you need to contact staff.&#160;</p><ul><li><strong>Platform 1</strong><em>By the pole on the pathway onto the platform, next to the request to stop box.</em></li></ul><p></p>"
```

Today these are printed as literal markup inside a `<Code block>`. Even if
every other pattern were rendered perfectly, leaving these as raw tag soup
would keep the section unreadable. Tags observed: `p`, `br`, `strong`, `em`,
`ul`, `ol`, `li`, `a[href]` (with `mailto:`, `tel:` and `https:` targets),
plus numeric and named entities (`&#160;`, `&#39;`, `&quot;`). Handling this
is a first-class requirement, not a polish item — see Decision 4.7.

### 2.5 `null` is pervasive, and "always null" is not a schema fact

Populated-ness varies enormously by field and station:
`stationAccessibility.escalatorInformation`,
`stationAccessibility.stepFreeCategory.levelAccess` and
`stationFacilities.limitedService` were `null` at all 31 stations, as were
`toilets.openingTimes`, `wifi.openingTimes` and several
`operatorContactDetails`.

**This must not be baked into the design.** A 17-station sample taken
earlier in this same survey showed `stationFacilities.defibrillator` as
`null` at 17/17; widening to 31 found it populated as a full Pattern A
object at 2 stations. Treat "never seen populated" as a display-time
decision (drop empties, as the section already does at
`StationAccessibilitySection.tsx:73-77`), never as a reason to omit a field
from the renderer.

### 2.6 Payload size — answering the original spec's Open question 2

`2026-09-12-station-accessibility-design.md:614-618` left per-station
payload size unmeasured. Measured over the sample, the filtered 12-key
object is **9,065 bytes (min, `BAL`), 20,667 bytes (median), 29,919 bytes
(max, `MAN`)**.

That is comfortably small for a synchronously-rendered server component, so
the risk that open question flagged does not materialise. It is *not*
negligible though: because the page is a server component, the entire blob
is serialized into the RSC flight payload and shipped to the browser whether
or not the reader opens a single disclosure. §5 returns to this.

## 3. Corrections to the original spec's assumptions

1. **"A payload whose exact field-level shape this codebase cannot
   currently cite with confidence"** (`:20-21`) — no longer true. §2.2
   documents 480 paths and 46 object signatures with 100% sub-key stability
   across 31 stations. The uncertainty was real when written; it has been
   discharged.
2. **Decision 6's framing of the data as "genuinely unknown shape"** is
   contradicted by the evidence. The generic renderer is well-built and
   crash-proof, and its authors were right to hedge without data — but it is
   now solving a problem the data does not have, at a cost (§2.1) of
   dumping JSON 96.2% of the time.
3. **`humanizeKey`'s "no hardcoded per-field dictionary"** rationale
   (`stationAccessibility.ts:25-30`, "since the field set is unverified
   against a real payload") no longer holds for the same reason. §4.6 argues
   most labels should be *dropped* rather than dictionary-mapped, which also
   sidesteps the maintenance cost a dictionary would add.
4. **The spec did not anticipate embedded HTML** (§2.4) anywhere in its
   Decision 6 rules. This is the one genuinely new requirement the survey
   surfaced, and the one with a security dimension.

## 4. Decisions (the proposed design)

### 4.1 Dispatch on shape, not on key name

Render by detecting the seven patterns structurally, not by switching on the
12 key names or on sub-field paths. Each pattern is recognised by a
predicate over the value's own shape (e.g. "an object with a boolean
`available`"), so a pattern is rendered correctly wherever it appears —
including at the 396 Pattern-A sites nested three levels deep, which a
key-name switch would miss.

This keeps the renderer's contract close to what it is today (a total
function from `unknown` to something displayable) while making its output
useful. It also means new RDM fields that reuse an existing shape render
correctly with no code change — the property that makes the passthrough
worth keeping (§5).

**Evidence status: confirmed.** Patterns A–G below are each backed by the
counts in §2.3.

### 4.2 Pattern A — Facility record

*Detect:* a plain object with a boolean `available`.

*Render:* a one-line availability statement led by a check/cross icon —
"Available" / "Not available" — with the field's humanized name as the
line's subject where a name is meaningful (`Wi-Fi — Available`). Then, in
order, and each omitted when null/empty:

- `location` and `notes` as rich text (§4.7).
- `openingTimes` via Pattern B, and `openingHoursNotes` as rich text
  beneath it.
- `operatorContactDetails` via Pattern C.
- Any *extra* scalar siblings (`storage`, the four `toilets` booleans) as
  Pattern E / boolean lines.
- Any extra array siblings (`names`, `locations`) via Pattern F / D.

The `{available, notes}` two-field variant (all of `transportLinks.*`) is
the same renderer with everything else absent, giving e.g. "Bus —
Available" plus a note.

The icon must not be the only carrier of meaning: the text label
("Available" / "Not available") is always present, so the section stays
legible to screen readers and in the axe-core sweep
`e2e/accessibility.spec.ts` already runs over this page.

### 4.3 Pattern B — Opening times

*Detect:* an array whose elements are objects with `daysOfTheWeek` and
`openingStatus`.

*Render:* one line per entry, days compacted into ranges:

- `Specific Hours` → `Mon–Sat, 05:00–00:45` (times trimmed from
  `HH:MM:SS.mmm` to `HH:MM`).
- `24 Hours` → `Mon–Sun, 24 hours` (ignore `openPeriod`, which is null or
  empty in these entries).
- `Unavailable` → `Mon–Sun, closed`.

Day compaction runs over the seven weekday tokens only; `Public Holidays`
(§2.3) is emitted as its own trailing token and never folded into a range.
An entry with an unrecognised `openingStatus` falls through to printing the
status string verbatim next to its days, which is still readable.

**Evidence status: `openingStatus`'s three values are confirmed for this
sample only.** The design must therefore not `switch` exhaustively without a
default branch — hence the fall-through above.

### 4.4 Pattern C — Contact details

*Detect:* a plain object with a `primaryTelephoneNumber` key.

*Render:* a small definition list. `primaryTelephoneNumber` as a `tel:`
link, `emailAddress` as `mailto:`, `url` as an external link,
`postalAddress` joined into one comma-separated line skipping null lines,
`operatorName` as plain text, `note` as rich text. Drop `name` — every
observed value is boilerplate that restates the context ("Basingstoke Help
Line Contact Details").

### 4.5 Pattern D — Named-item collection

*Detect:* an array of objects that all carry a string `name`.

*Render:* a list, one block per item, `name` as the block's label. Then:

- **If every other own value is a string or null** (the common case —
  platforms, lift info, toilet locations, taxi ranks, drop-off points,
  waiting rooms): render those values as a plain bullet list with **no field
  labels**, because they are already complete sentences (§2.3, Pattern E).
  "Platform 3" followed by "There is a Help Point close to this platform"
  and "Seating is limited on this platform" needs nothing else.
- **Otherwise** (`carParks.carParks[]`): recurse — scalars as labelled
  rows, `charges` as a rate table skipping null rates, `operator` and
  `openingHours` via Patterns C and B, `accessibleLocations` via Pattern D
  again.

Collections stay collapsed behind the existing `Disclosure` when long — the
sample has arrays up to 20 platforms and 18 lifts, so Decision 6's original
"must not produce a wall of text" concern remains valid and its `Accordion`
mechanism (`StationAccessibilitySection.tsx:46-58`) should be reused
unchanged. The control's item count must keep counting *visible* items, as
it does today.

### 4.6 Patterns E and F — sentences and token lists

**Pattern E (sentence-valued scalar):** render the value as a plain sentence
with **no humanized key label**, when the string is longer than ~20
characters, starts with a capital and contains a space. 91% of depth-1 plain
scalars qualify (§2.3). "Tactile paving: There are tactile warnings on all
platforms in use" becomes "There are tactile warnings on all platforms in
use". Strings that fail the test keep today's `humanizeKey` label — short
codes like `stepFreeCategory.category: "B1, (refer to quick reference
guide)"` genuinely need one.

**Evidence status: the 91% is measured; the specific heuristic threshold is
a judgement call** and should be tuned against the rendered page, not
treated as derived.

**Pattern F (token list):** render a `string[]` as chips rather than today's
comma-join. Values that are camelCase tokens
(`"DepartureScreens"`) get `humanizeKey` applied — which is exactly what
that function is good at, and a better use for it than labelling sentences.

### 4.7 Pattern G — rich text

Every `location`, `notes`, `note` and `*Notes` field must be treated as
HTML, not as text (§2.4). Two viable options:

**(a) Sanitize and render (recommended).** Strip to an allowlist —
`p`, `br`, `ul`, `ol`, `li`, `strong`, `em`, and `a` with `href` restricted
to `https:`, `mailto:` and `tel:` — then render inside Mantine's
`TypographyStylesProvider`. Preserves the structure and, importantly, the
hyperlinks: several `notes` values carry an operator's assistance phone
number or booking URL only as an `<a href>`.

**(b) Strip to text.** Decode entities, convert `</p>`, `<br>` and `</li>`
to line breaks, drop every other tag. No new dependency and no XSS surface,
but silently destroys every link's target.

Recommend (a), with two conditions: the allowlist must be enforced by a
vetted sanitizer rather than a hand-rolled regex, and sanitization must
happen **server-side in the RSC render**, so it cannot be bypassed by a
crafted payload reaching a client component. This content is third-party
data stored in our database; it is not trusted input merely because National
Rail is a reputable upstream.

**This adds a frontend dependency, which no other part of this app
currently needs.** That is a real cost and a genuine decision point, not an
implementation detail — flagged as Open question 1.

### 4.8 The 12 keys, mapped to patterns

Every key is an object; each row lists the patterns its contents draw on.

| Key | Patterns used | Evidence |
|---|---|---|
| `stationAccessibility` | A (`trainRamp`, `ticketBarriers`), D (`passengerAssistance`), E (`tactilePaving`), B, F (`names`), booleans | 31/31 |
| `staffAssistance` | A (`helpline`, `staffHelp`), B, C, E, F | 31/31 |
| `toiletsAndChanging` | A (`toilets`, `showers`) + four booleans, D (`locations`) | 31/31 |
| `lifts` | E (`statement`), boolean, D (`liftsInfo`) | 31/31 |
| `loungesAndWaiting` | A ×3, B, D (`waitingRooms`, `firstClassLounges`), E | 31/31 |
| `platformFacilities` | E (`entranceLevels`, `tactileWarnings`), number, D (`platforms`) | 31/31 |
| `stationFacilities` | A ×11, C, booleans | 31/31 |
| `helpAndSupport` | A (`staffHelp`), E ×5, F, nested help-points object | 31/31 |
| `transportLinks` | A-narrow `{available, notes}` ×6, D (`taxiRanks`) | 31/31 |
| `carParks` | D (structured branch), B, C, numbers, booleans | 31/31 |
| `dropOffPickUp` | A-ish (`available`/`location`/`notes`), D (`points`) | 25/25 |
| `cycling` | booleans, E, F (`typesOfStorage`), `spaces` object | 31/31 |

No key requires a bespoke component. `carParks` is the only one needing
Pattern D's structured branch.

### 4.9 The fallback stays — but narrower and better-dressed

Keep a terminal fallback. The patterns above are confirmed against 31 of
~2,600 stations and one point in time; the feed can add fields, and
`ACCESSIBILITY_KEYS` can gain keys. A renderer that throws or blanks on an
unmatched shape would be a worse regression than the status quo.

Two changes to it:

1. **It should almost never fire.** Under this design the raw fallback is
   reached only by a value matching none of A–G — in the sample, nothing.
2. **It should not be raw JSON.** An unmatched object should render as a
   labelled key/value list (today's `renderShallowObject` behaviour) with
   each value recursed through the same dispatcher, and only a value that
   defeats *that* — a non-plain object, or recursion past a depth bound —
   falls back to collapsed `JSON.stringify`. Raw JSON remains the last
   resort because for a genuinely unanticipated shape it is honest and
   lossless, which the original Decision 6 got right.

The existing depth limit should be raised, not removed: the real data
reaches five levels
(`carParks.carParks[].operator.contactDetails.postalAddress.postcode`), so a
bound of 6–8 preserves the "terminates by construction" guarantee while
covering everything observed.

### 4.10 Keep everything else

The allowlist (Decision 1), the route (Decision 4), the `unknown`-valued
wire type (Decision 5), category grouping and order (Decision 7), section
placement and naming (Decision 8), and the three honest states (Decision 9)
are all unaffected and should not be reopened. The empty-value skipping in
`renderableGroups` and `isEmptyRenderable` is load-bearing given §2.5 and
must be preserved.

## 5. Backend or frontend? — frontend only

**Recommendation: implement this entirely in the frontend. Leave
`crates/api` a key-name-filtered passthrough, and leave
`StationAccessibilityData`'s twelve `unknown`s alone.**

The tempting alternative is a typed intermediate representation: have the
API parse the JSONB into Rust structs and return an app-owned shape, so the
frontend stops re-parsing an opaque blob. Against it:

1. **Global Constraint 7 forbids it**
   (`docs/superpowers/plans/01-poller-microservices.md:40-42`): "Don't
   hand-model every Stations-JSON accessibility sub-field as a typed Rust
   struct field; store the sub-object as `serde_json::Value` / Postgres
   `JSONB`". The station-accessibility spec reaffirmed it as still binding
   (`2026-09-12-station-accessibility-design.md:538-539`). A backend IR
   would need that constraint explicitly amended — a bigger decision than
   this rendering change, and one this document does not ask for.
2. **This repo has already litigated exactly this trade-off and chosen
   passthrough.** `crates/api/src/routes/lines.rs:160-192` rejects
   hand-rendering a nested JSONB structure for two reasons that transfer
   verbatim: a recursive `serde_json::Value` transform for a nested shape is
   "real, untyped, error-prone code" with "no compiler checking the mapping
   stays exhaustive"; and a hand-rolled mapper "silently drops any field"
   the producer adds. With 480 paths and a feed we do not control, both
   risks are acute — and silent data loss in *accessibility* information is
   the worst possible failure mode for this feature.
3. **Shape-dispatch makes the IR redundant.** The value of a typed IR is
   knowing the shape. §4.1 recovers that at the render site from the data
   itself, and does so in a way that keeps working when the feed adds a
   field, which a fixed IR would not.
4. **It matches where this app already puts presentation.** `render.rs` owns
   the public wire shape for data the app *models*; the frontend is a thin
   typed consumer. But accessibility data is explicitly the un-modelled
   passthrough case, and prose-formatting decisions — dropping redundant
   labels, compacting day ranges, sanitizing HTML — are presentation, not
   wire shape. They belong next to the components that render them.

Two narrower backend changes *are* worth considering, and both stay within
Global Constraint 7 because neither needs a typed sub-field model:

- **Recursively strip `null`s in `filter_accessibility_fields`.** Purely
  key-name-agnostic, and given how pervasive nulls are (§2.5) it would
  meaningfully cut the 20.7 KB median payload (§2.6) that currently ships to
  every browser through the RSC flight payload. The frontend already drops
  nulls at render time, so this changes no output.
- **Nothing else.** In particular, do not sanitize HTML server-side in
  `crates/api`: that would bake a presentation decision into the wire
  format, and the sanitizer belongs where the markup is turned into DOM.

Both are optional and severable from the rendering work. The null-stripping
one is offered as a follow-up, not a prerequisite.

## 6. Alternatives considered and rejected

1. **Keep the raw-JSON hedge.** This was a legitimate possible conclusion
   and the survey was run willing to reach it. The evidence does not
   support it: §2.2's 100% sub-key stability removes the premise, and
   §2.1's 96.2% means the hedge is the primary experience rather than a
   safety net.
2. **One bespoke component per key.** Twelve components duplicating the same
   Facility/OpeningTimes/Contact rendering that recurs 396/304/92 times. More
   code, more drift, and blind to those shapes wherever they nest.
3. **A per-field label dictionary** (the option Decision 6 rejected for lack
   of data). Still the wrong answer, for a better reason: §2.3 shows 91% of
   scalars are self-describing sentences, so the correct move is removing
   labels, not curating ~480 of them.
4. **A typed backend IR.** §5.
5. **Runtime schema validation (zod or similar) on the frontend.** The app
   has no validation library and `fetchJson<T>` casts unvalidated
   everywhere; introducing one here alone would be inconsistent, and
   shape-dispatch already degrades safely without it.

## 7. Non-goals

- Changing the allowlist, the route, the URL, the wire type, the section's
  placement, headings or copy.
- Any migration, any change to `poller-stations`, any change to what is
  stored in `stations.accessibility`.
- Rendering RDM fields outside the twelve (`ticketBuying`, `stationMap`,
  `stationAlerts`, `address`, …) — still the separate product call the
  original spec's Open question 3 names.
- Resolving the `accessibility` naming collision with WCAG audits (that
  spec's Open question 4) or CRS case-normalization (Open question 5).
- Localization. Every string is English prose from the feed.

## 8. Testing approach

- **Unit tests on the pattern predicates and renderers**, using real
  payload fragments captured in this survey as fixtures — the first time
  this feature would have tests written against real RDM data rather than
  invented values. Fixtures should include `BAL` (smallest), `MAN`
  (largest), and `DNO` (a request stop whose `trainRamp.available` is
  `false` with explanatory notes).
- **A regression test that the raw fallback does not fire** across the
  captured fixtures — the direct, checkable inverse of §2.1's 96.2%.
- **Total-function tests retained**: the existing "never throws" property of
  `renderAccessibilityValue` must hold for `null`, `[]`, `{}`, deeply nested
  input and non-plain objects. This is the guarantee most worth keeping from
  Decision 6.
- **Sanitizer tests**: a `notes` value containing `<script>`, an
  `href="javascript:"`, and an `onerror` attribute must all render inert.
- **The existing axe-core sweep** (`e2e/accessibility.spec.ts`) covers the
  page already; the check/cross icons of §4.2 must not regress it, which the
  always-present text label ensures.
- No new e2e spec, consistent with the original spec's reasoning
  (`:599-603`).

## 9. Open questions / risks

1. **Whether to take an HTML-sanitizer dependency (§4.7).** The one
   decision here with a cost outside this feature. Option (b) avoids it and
   loses every hyperlink target. Needs a human call; recommend (a).
2. **31 of ~2,600 stations.** The stability is striking and spans the
   obvious axes of variation, but it is a 1.2% sample. A cheap hardening
   step before implementation: run the §1.1 extraction over a few hundred
   CRS codes and assert the sub-key sets match. §2.5's defibrillator case is
   the concrete warning that widening the sample changes conclusions.
3. **The sample is one point in time (2026-09-16).** `poller-stations`
   replaces `stations` wholesale each poll; nothing pins the feed's schema.
   Shape-dispatch (§4.1) is the mitigation — it degrades to the fallback
   rather than breaking — but a feed-side restructure would silently return
   the section to JSON dumps. No alerting on that is proposed here.
4. **`openingStatus`'s three values and the eight `daysOfTheWeek` tokens are
   sample-derived, not documented.** §4.3's default branches are what make
   that survivable; an exhaustive `switch` would be a latent bug.
5. **The RDM OpenAPI spec was not consulted.** `poller-stations/src/schema.rs`
   cites a "National Rail Station API OpenAPI spec (v1.0.0)" that, if it
   documents `components.schemas.Station` sub-objects, would upgrade much of
   §2.3 from "observed in 31 stations" to "documented contract". Not
   available in this sandbox; worth ten minutes from anyone with RDM
   portal access.
6. **Effort is not estimated here.** Seven pattern renderers plus a
   sanitizer is materially more than the current ~190-line module, and this
   document deliberately makes no claim about whether that is the right
   use of time next — only about what the data supports.
