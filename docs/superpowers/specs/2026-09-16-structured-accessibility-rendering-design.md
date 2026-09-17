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

### 1.2 Sample — 31 stations, surveyed 2026-09-16

Chosen before looking at any results, to span termini and request stops, all
four nations, and many operators. Every one returned `200` with
`coverage: 'present'`:

`ABD` `BAL` `BHM` `BRI` `BSK` `BTN` `CAR` `CBG` `CDF` `DNO` `EDB` `EUS`
`EXD` `GLQ` `HUL` `INV` `IPS` `KGX` `LDS` `LLE` `MAN` `NRW` `PMH` `PNZ`
`SHF` `SKG` `SOU` `STP` `TWY` `WVH` `YRK`

That deliberately includes the extremes: `MAN` (Manchester Piccadilly, the
largest payload), `DNO` (Dunrobin Castle, a seasonal request stop) and `BAL`
(Balham, the smallest). Sizes in §2.6.

### 1.3 Honest limits of this method

- **22 of 6,996 strings (0.31%) came back as RSC de-duplication references**
  (`"$2b"`, `"$35"`, …) rather than their literal text. These are an
  artifact of reading the flight payload, **not** real feed values. They
  affect a handful of quoted *values*; they do not affect any *shape* claim
  in this document, since a reference stands in for a string either way.
- 31 stations is a sample, not the ~2,600-station population. Every
  frequency below ("31/31", "396 instances") describes this sample. §2.5
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

Below the top level there is **zero** key-set variation between a London
terminus and a Highland request stop: variation between stations is
entirely in *values* and in `null`-vs-populated. The one exception is at
the root, where `dropOffPickUp` is absent at 6/31 (§1.3) — which is also
why the sample has two root key-sets rather than one.

The whole sample contains **46 distinct object signatures** (44 interior
plus those two root key-sets) and **480 distinct JSON paths**, counting
array elements collapsed to a single `[]` path — a finite, enumerable
schema, not an open-ended blob.

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
with extra siblings (`trainRamp` adds the scalar `storage`;
`ticketBarriers` adds the array `names`; `toilets` adds three booleans and
the array `locations`).

A narrower two-field variant `{available, notes}` occurs 155 times — five of
the seven `transportLinks.*` sub-keys (`airport`, `bus`, `carHire`, `port`,
`underground`), 31 each. The other two extend it: `replacementBus` is
`{available, maps, notes}` (31×) and `taxi` is `{available, notes,
taxiRanks}` (29×, null at 2).

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

**Pattern D — Named-item collection (877 objects across 12 array paths).**
An array whose every element is an object carrying a string `name` plus
sibling descriptive fields. The twelve paths are
`platformFacilities.platforms` (259 items),
`toiletsAndChanging.toilets.locations` (195), `lifts.liftsInfo` (111),
`transportLinks.taxi.taxiRanks`, `dropOffPickUp.points`,
`loungesAndWaiting.waitingRooms`, `loungesAndWaiting.firstClassLounges`,
`stationAccessibility.passengerAssistance`,
`stationAccessibility.nearestAccessibleStations.stations`,
`transportLinks.replacementBus.maps`, `carParks.carParks` and
`carParks.carParks[].accessibleLocations`.

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

Three of the twelve are exceptions and need labelled rendering:
`carParks.carParks[]` (items mix numbers, a `charges` object of ten rate
strings, and an `operator`), `carParks.carParks[].accessibleLocations[]`
(carries a nested `accessibilityInfo` object) and
`stationAccessibility.passengerAssistance[]` (carries a boolean
`available`). See §4.5.

**Pattern E — Sentence-valued scalar.** Of 372 non-HTML scalar strings at
depth 1, **340 (91%)** are full sentences beginning with a capital and
reading as display copy: `tactilePaving: "There are tactile warnings on all
platforms in use"`, `platformFacilities.entranceLevels: "The platforms are
level with the Main Entrance of the station"`, `announcements:
"Announcements are made both visually and audibly"`.
(`lifts.statement: "There are lifts"` is the same kind of prose but is
*not* among the 340 — it is too short to pass the length test §4.6
discusses, which is precisely that test's problem.) The feed has already
done the prose. The current
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
would keep the section unreadable.

The complete tag inventory for the sample — eight tags, nothing else,
counted as elements: `p` (1,069), `a[href]` (193), `li` (136), `strong`
(134), `ul` (53), `em` (37), `h2` (9), `u` (4). Note there is **no `br` and
no `ol`**.

The 193 anchors' URL schemes are `https:` (143), **`http:` (45)** and
`mailto:` (5). There is **no `tel:` link anywhere** in the sample — phone
numbers appear as plain text inside `note`/`notes`, or in
`primaryTelephoneNumber`. Any `href` allowlist must therefore include
`http:`, or it silently drops 23% of the links.

Entities, six in total: `&#160;` (166), `&#39;` (60), `&quot;` (50),
`&#163;` (30 — a pound sign, inside car-park rate text), `&amp;` (28),
`&#233;` (1).

Handling this is a first-class requirement, not a polish item — see
Decision 4.7.

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
object as compact JSON is **9,065 characters (min, `BAL`), 20,667 (median,
`BSK`), 29,919 (max, `MAN`)** — UTF-8 byte counts are a handful higher
(9,069 / 20,681 / 29,925). The gap is entirely non-ASCII punctuation and
currency: across the sample, `£` ×108, `’` ×14, `“`/`”` ×3 each, `–` ×4,
and — worth knowing before rendering — a zero-width space (U+200B) ×7,
which will show up as an invisible artifact. The one `é` exists only as the
ASCII entity `&#233;` and costs nothing on the wire.

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
including at the 396 Pattern-A sites, which sit two levels below the
payload root across 18 different paths and which a key-name switch would
have to enumerate one by one.

This keeps the renderer's contract close to what it is today (a total
function from `unknown` to something displayable) while making its output
useful. It also means new RDM fields that reuse an existing shape render
correctly with no code change — the property that makes the passthrough
worth keeping (§5).

**Dispatch precedence matters, and one real case needs it.** The predicates
are not mutually exclusive, so they must be tried in a fixed order:
B (opening-times array) → D (named-item array) → C (contact details) →
A (facility record) → F (token list) → E (sentence) → primitives → fallback.
Array patterns are tested before object patterns, so an array is never
misread as its first element.

The one overlap in the real data is
`stationAccessibility.passengerAssistance` (49 items across the sample): it
is an array of objects carrying **both** a string `name` and a boolean
`available`, so it satisfies D's array predicate and its items satisfy A's.
Ordering resolves it to D — the right answer, since these are several
distinctly-named meeting points, not one facility. Their `available` field
is then rendered by D's structured branch (§4.5) as a normal boolean line.

**Evidence status: confirmed.** Patterns A–G below are each backed by the
counts in §2.3, and the overlap above was found by exhaustively testing the
predicates against all 31 payloads rather than assumed absent.

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
- Any *extra* scalar siblings (`storage`, the three extra `toilets` booleans) as
  Pattern E / boolean lines.
- Any extra array siblings (`names`, `locations`) via Pattern F / D.

The `{available, notes}` two-field variant (all of `transportLinks.*`) is
the same renderer with everything else absent, giving e.g. "Bus —
Available" plus a note.

The icon must not be the only carrier of meaning: the text label
("Available" / "Not available") is always present, so the section stays
legible to screen readers. Note this is a design rule, not something the
existing axe-core sweep would enforce — see §8.

### 4.3 Pattern B — Opening times

*Detect:* an array whose elements are objects with `daysOfTheWeek` and
`openingStatus`.

*Render:* one line per entry — the entry's own day set, compacted, then its
hours. The day set is whatever the entry carries, never assumed to be the
full week: `24 Hours` entries in the sample include day sets of exactly
`["Saturday"]` (26×) and `["Sunday"]` (27×), and `Unavailable` entries
include `["Public Holidays"]` (3×) and `["Saturday","Sunday"]` (5×).

- `Specific Hours` → `Mon–Sat, 05:00–00:45`, times trimmed from
  `HH:MM:SS.mmm` to `HH:MM`. All 175 such entries carry exactly one
  `openPeriod`.
- `24 Hours` → `Sat, 24 hours`.
- `Unavailable` → `Public Holidays, closed`.

Two traps the real data sets:

- **Sort before compacting.** `daysOfTheWeek` is *not* in canonical order.
  Four entries arrive out of order, one of them fully reversed
  (`Sunday,Saturday,…,Monday` at `BTN`), two interleaved
  (`Monday,Tuesday,Thursday,Friday,Saturday,Wednesday` at `INV`, twice) and
  one a simple swap (`CDF`). Range compaction
  over array order would produce nonsense. Compact over the seven weekday
  tokens sorted into week order; `Public Holidays` (§2.3) is emitted as its
  own trailing token and never folded into a range.
- **`24 Hours` does not always mean `openPeriod` is absent.** 106 such
  entries have an empty array and 7 have `null`, but **2 carry a real
  period** — `LLE`'s `staffAssistance.staffHelp.openingTimes` and
  `helpAndSupport.staffHelp.openingTimes` both say `24 Hours` with
  `openPeriod: [{startTime: "06:10:00.000", endTime: "12:40:00.000"}]`.
  Rendering "24 hours" over that would assert something the record itself
  contradicts. Where both are present, show both — `Mon–Fri, 24 hours
  (source also lists 06:10–12:40)` or similar — rather than silently
  picking one. This is upstream data disagreeing with itself; the UI should
  not resolve it by deletion.

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
`operatorName` as plain text, `note` as rich text.

**Drop `name`.** The 122 contact objects carry 114 distinct names and, on
inspection, **not one of them contains information unavailable elsewhere in
the same object**. Almost all are boilerplate restating the context
("Basingstoke Help Line Contact Details", "Edinburgh Car Park 1 Contact
Details"). The one that looks like a genuine exception — `"GREGGS BAKERY"`,
the refreshments tenant at `GLQ` — turns out to duplicate its own sibling
verbatim:

```json
{ "emailAddress": null, "name": "GREGGS BAKERY",
  "note": "<p>U6 Queen Street Station</p><p>North Hanover Street</p><p>G1 2AF</p>",
  "operatorName": "GREGGS BAKERY", "primaryTelephoneNumber": null }
```

That generalises: across all 122 contact objects, every `name` either
contains the word "Details" or is byte-identical to its own `operatorName`.
Since `operatorName` is rendered anyway, dropping `name` outright loses
nothing on this sample and needs no string heuristic at all. If the feed
later puts something unique in `name`, the loss is a duplicated label, not
data.

### 4.5 Pattern D — Named-item collection

*Detect:* an array of objects that all carry a string `name`.

*Render:* a list, one block per item, `name` as the block's label. Then:

- **If every other own value is a string or null** (7 of the 12 paths —
  platforms, lift info, toilet locations, taxi ranks, drop-off points,
  waiting rooms, first-class lounges): render those values as a plain bullet
  list with **no field labels**, because they are already complete sentences
  (§2.3, Pattern E). "Platform 3" followed by "There is a Help Point close
  to this platform" and "Seating is limited on this platform" needs nothing
  else.
- **Otherwise** — 3 of the 12 paths:
  - `carParks.carParks[]` (61 items): scalars as labelled rows, `charges`
    as a rate table skipping null rates, `operator` and `openingHours` via
    Patterns C and B, `accessibleLocations` via Pattern D again.
  - `carParks.carParks[].accessibleLocations[]` (39): carries a nested
    `accessibilityInfo` object of ten scalars, so it cannot take the bullet
    branch — recurse, rendering `accessibilityInfo`'s sentence-valued
    fields as the item's bullets.
  - `stationAccessibility.passengerAssistance[]` (49): carries a boolean
    `available` alongside its strings (see §4.1's precedence note), so it
    takes the structured branch and renders `available` as a normal
    availability line.
- **Two paths fit the bullet branch but read badly there**, and need a
  small exception: `stationAccessibility.nearestAccessibleStations.stations[]`
  (whose only non-`name` field is `crsCode`, e.g. a bare "SWA" bullet under
  "Swansea" — render as "Swansea (SWA)", ideally linking to that station's
  own page) and `transportLinks.replacementBus.maps[]` (whose only
  non-`name` field is a PDF `url` — render the `name` as the link text).

Collections stay collapsed behind the existing `Disclosure` when long — the
sample has arrays up to 20 platforms and 18 lifts, so Decision 6's original
"must not produce a wall of text" concern remains valid and its `Accordion`
mechanism (`StationAccessibilitySection.tsx:46-58`) should be reused
unchanged. The control's item count must keep counting *visible* items, as
it does today.

### 4.6 Patterns E and F — sentences and token lists

**Pattern E (sentence-valued scalar):** render the value as a plain sentence
with **no humanized key label**. "Tactile paving: There are tactile warnings
on all platforms in use" becomes "There are tactile warnings on all
platforms in use".

The obvious heuristic — longer than ~20 characters, starts with a capital,
contains a space — is measured at 340/372 = 91% of depth-1 plain scalars
(§2.3), but **testing it against the real data shows it does not do what it
looks like it does, and it should not be adopted as stated**:

- It *passes* `stationAccessibility.stepFreeCategory.category: "B1, (refer
  to quick reference guide)"` (36 characters, capitalised, spaced), which is
  a code needing a label — the very case a length threshold is supposed to
  catch. (That string sits at depth 2, so it is not itself part of the 372
  the 91% is measured over; it is the clearest example of the failure mode,
  not a counterexample to the statistic.)
- The only depth-1 strings it *fails* are 32: the 31 instances of
  `lifts.statement` (`"There are lifts"` / `"There are no lifts"`), which
  are textbook self-describing sentences that should be unlabelled, plus one
  RSC extraction artifact (§1.3).

So the heuristic is close to exactly inverted on the two cases that matter.
Two honest ways forward, both of which this document leaves open:

1. **Label by field, not by shape** — a small deny-list of the fields that
   are codes rather than prose keeps a label; everything else drops it. In
   this sample there are **zero** code-like strings at depth 1, so the list
   needs exactly one entry to cover the failure case above:
   `stationAccessibility.stepFreeCategory.category`. (Codes do occur deeper
   — `crsCode`, `postcode`, the `charges.*` rates — but §4.4 and §4.5
   already give each of those its own rendering, so they never reach this
   rule.) Unlike the per-field dictionary rejected in §6, which would need
   an entry for each of the sample's **171 distinct field names**, that is
   trivially maintainable.
2. **Keep every label** for depth-1 scalars and take the redundancy, gaining
   the benefit only inside Pattern D items (§4.5), where the sentence
   finding is unambiguous and no counterexample exists.

**Evidence status: the 91% is measured; the heuristic built on it is
refuted** by the two cases above. Recommend option 1, decided against the
rendered page rather than from the statistic.

**Pattern F (token list):** render a `string[]` as chips rather than today's
comma-join. Values that are camelCase tokens
(`"DepartureScreens"`) get `humanizeKey` applied — which is exactly what
that function is good at, and a better use for it than labelling sentences.

### 4.7 Pattern G — rich text

Every `location`, `notes`, `note` and `*Notes` field must be treated as
HTML, not as text (§2.4) — and so must **`operatorName`**, which §4.4
otherwise renders as plain text: five of its values are a bare `<a href>`
(ScotRail's lost-property contact at `ABD`, `DNO` and `INV`; Transport for
Wales' at `CDF` and `LLE`). Those 5 plus the 703 in the note/location
fields account for all 708 HTML-bearing strings exactly. A sixth
`operatorName` carries a literal `&` ("Monday - Saturday 07:30 - 21:30 &
Sunday 09:00 - 21:00", `ABD`) — not markup, but it must still be escaped
rather than passed through a sanitizer as-is. Two viable options:

**(a) Sanitize and render (recommended).** Strip to an allowlist of ten
tags — the eight §2.4 observed (`p`, `a`, `strong`, `em`, `ul`, `li`, `u`,
`h2`) plus `br` and `ol`, which do not occur today but are innocuous, and
whose absence from an allowlist would silently destroy formatting if the
feed started using them. Restrict `href` to `https:`, `http:` and `mailto:`
— the three schemes that actually occur. Adding `tel:` is harmless and
future-proof; omitting `http:` would drop 45 real links. Then render inside
Mantine's `TypographyStylesProvider`.

This preserves the structure and, importantly, the hyperlinks: several
`notes` values carry an operator's assistance phone number or booking URL
only as an `<a href>`.

One further detail the observed inventory dictates. `h2` must be
**demoted** (to a
`strong`, or to `h4` under the section's own `h2`), not passed through: the
page renders an `h1` (`page.tsx:242`) and this section an `h2`
(`StationAccessibilitySection.tsx:159-161`), so a stray `h2` from inside a
note would land in the outline as a sibling of the section heading. All
nine `h2`s in the sample sit inside just three notes, all at `MAN`
(`helpAndSupport.helpPoints.notes`, `staffAssistance.helpPoints.notes` and
`loungesAndWaiting.waitingFacility.notes`), but that is three notes
presenting themselves as page-level sections. Note this is a correctness
argument, not one the test suite enforces: axe's `heading-order` flags
*skipped* levels, and an `h2` following an `h2` is not a skip, so
`frontend/e2e/accessibility.spec.ts` would pass either way (see §8).

**(b) Strip to text.** Decode all six entities, convert `</p>` and `</li>`
to line breaks, drop every other tag. No new dependency and no XSS surface,
but silently destroys every link's target — all 193 of them.

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
| `stationAccessibility` | A (`trainRamp`, `ticketBarriers`), D (`passengerAssistance`, `nearestAccessibleStations.stations`), E (`tactilePaving`), B, F (`names`), booleans, plus 3 unmatched objects (`inductionLoop`, `stepFreeCategory`, `nearestAccessibleStations`) | 31/31 |
| `staffAssistance` | A (`helpline`, `staffHelp`, `helpPoints`), B, C, E, F | 31/31 |
| `toiletsAndChanging` | A (`toilets`, `showers`) + three booleans, D (`locations`) | 31/31 |
| `lifts` | E (`statement`), boolean, D (`liftsInfo`) | 31/31 |
| `loungesAndWaiting` | A ×3, B, D (`waitingRooms`, `firstClassLounges`), E, boolean | 31/31 |
| `platformFacilities` | E (`entranceLevels`, `tactileWarnings`), number, D (`platforms`) | 31/31 |
| `stationFacilities` | A ×11, C, booleans | 31/31 |
| `helpAndSupport` | A (`staffHelp`, `helpPoints`), E ×5, F ×2 | 31/31 |
| `transportLinks` | A-narrow `{available, notes}` ×5, D (`taxiRanks`, `replacementBus.maps`) | 31/31 |
| `carParks` | D (structured branch), B, C, numbers, booleans, plus 4 unmatched objects (`charges`, `operator`, `accessibilityInfo`, `postalAddress`) | 31/31 |
| `dropOffPickUp` | A-ish (`available`/`location`/`notes`), D (`points`) | 25/25 |
| `cycling` | booleans, E, F (`typesOfStorage`), 1 unmatched object (`spaces`) | 31/31 |

**This table is a reader's index of what each key contains — it is not the
dispatch mechanism.** Rendering is driven purely by §4.1's shape predicates;
nothing switches on these key names. Three container keys listed above
(`lifts`, `dropOffPickUp`, and `staffAssistance.helpPoints`/
`helpAndSupport.helpPoints`) themselves satisfy Pattern A's "object with a
boolean `available`" predicate and are rendered by it, with their extra
siblings handled by §4.2's extra-sibling rules — which is the intended
behaviour, not a misclassification.

No key requires a bespoke component. Three Pattern-D arrays need its
structured branch (§4.5), and eight interior object paths match no pattern
at all and fall to §4.9's labelled key/value branch.

### 4.9 The fallback stays — but narrower and better-dressed

Keep a terminal fallback. The patterns above are confirmed against 31 of
~2,600 stations and one point in time; the feed can add fields, and
`ACCESSIBILITY_KEYS` can gain keys. A renderer that throws or blanks on an
unmatched shape would be a worse regression than the status quo.

**Patterns A–G do not cover everything, and this document should not
pretend otherwise.** Exhaustively testing the predicates against all 31
payloads leaves **226 interior object instances across 8 paths** matching
none of them:

| Path | Instances | Shape |
|---|---|---|
| `carParks.carParks[].accessibleLocations[].accessibilityInfo` | 39 | 10 scalars |
| `carParks.carParks[].charges` | 36 | 10 rate strings |
| `carParks.carParks[].operator` | 36 | `{contactDetails}` wrapper |
| `cycling.spaces` | 31 | `{notes, numberOfSpaces}` |
| `stationAccessibility.inductionLoop` | 31 | `{provision, ticketCounters}` |
| `stationAccessibility.stepFreeCategory` | 31 | `{category, levelAccess, notes}` |
| `carParks.carParks[].operator.contactDetails.postalAddress` | 21 | address lines |
| `stationAccessibility.nearestAccessibleStations` | 1 | `{notes, stations}` |

Note `carParks[].operator` is the *parent* of a Pattern C object, not one
itself.

Four of those eight paths already have bespoke handling elsewhere in this
document — `charges` (36) is §4.5's rate table, `operator` (36) is routed to
Pattern C, `accessibilityInfo` (39) is recursed as an item's bullets, and
`postalAddress` (21) is joined into one line by §4.4 — so 132 of the 226
never reach the terminal branch. The genuinely unhandled residue is **94
instances across 4 paths**: `cycling.spaces` (31),
`stationAccessibility.inductionLoop` (31),
`stationAccessibility.stepFreeCategory` (31, core step-free content
including HTML notes) and `nearestAccessibleStations` (1).

So the terminal branch does real work, and the two changes to it are:

1. **It should not be raw JSON.** An unmatched object renders as a labelled
   key/value list (today's `renderShallowObject` behaviour) with each value
   recursed through the same dispatcher — which handles all 226 above
   correctly, since every one is a plain object of scalars, a known pattern,
   or another plain object. Only a value that defeats *that* — a non-plain
   object, or recursion past the depth bound — falls back to collapsed
   `JSON.stringify`. Raw JSON remains the last resort because for a
   genuinely unanticipated shape it is honest and lossless, which the
   original Decision 6 got right.
2. **Raw `JSON.stringify` should then fire on nothing in the sample.** That
   is a narrower claim than "no value is unmatched" — 94 instances are
   unmatched — but it is the one the data supports, because every unmatched
   instance is a plain object of scalars, a known pattern, or another plain
   object, all of which the labelled branch renders correctly. (The single
   `nearestAccessibleStations` is the mixed case: `{notes, stations}`, whose
   `stations` recurses into Pattern D.)

The existing depth limit should be raised, not removed. The deepest real
chain is **seven containers counting the key's own value** — `carParks`
object → `carParks` array → element → `openingHours` array → element →
`openPeriod` array → `{startTime, endTime}` — at `EDB`, `INV`, `KGX` and
`NRW`; nothing in the sample is deeper. The
`operator → contactDetails → postalAddress` chain is only six under the
same convention, and picking the wrong one matters: a bound of 6 would
**truncate car-park opening periods**. Use **8**, which clears the observed
maximum by one while keeping the "terminates by construction" guarantee.

**Stated in the code's own units, to remove the off-by-one this paragraph
is warning about:** `renderAt`'s `depth` parameter is 0-based at the key's
value (`frontend/lib/stationAccessibility.ts:161`, entered via
`renderAt(value, 0)`), so the seven-container chain above occupies `depth`
0 through 6 and its innermost container sits at `depth` 6. Permitting the
one level of margin means allowing `depth` 7, so the bound to implement is
**`depth <= 7`** (equivalently `depth < 8`). Note `depth < 7` would permit
exactly the observed maximum with no margin at all — which is the sort of
off-by-one that truncates a car park's opening period the first time the
feed nests one level further.

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

1. **Global Constraint 7 points against it — though less decisively than
   it first appears, and this document will not overstate it.** Verbatim
   (`docs/superpowers/plans/01-poller-microservices.md:40-43`):

   > 7. **JSONB passthrough for accessibility data.** Don't hand-model every
   >    Stations-JSON accessibility sub-field as a typed Rust struct field;
   >    store the sub-object as `serde_json::Value` / Postgres `JSONB`,
   >    matching the existing `station_samples.departures JSONB` precedent.

   Read strictly, GC7's operative verb is **store**, its precedent is a
   storage one, and it lives in a plan about poller microservices — so on
   its own it binds the ingest and storage layers, not necessarily a
   read-time IR in `crates/api`. What extends it to the API is the
   station-accessibility spec's own non-goal, "No decomposition of the JSONB
   into typed Rust structs. Global Constraint 7 stands"
   (`2026-09-12-station-accessibility-design.md:538-539`) — but that is a
   non-goal in the very document this one partly supersedes, so it is
   weaker authority than a standalone rule.

   Net: GC7 makes a backend IR a decision that must be taken deliberately
   and argued, not one that can be slipped in. It does not by itself settle
   the question. Reasons 2–4 do, and would carry the recommendation even if
   this one were struck entirely.
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
  key-name-agnostic, so it stays inside Global Constraint 7. Measured over
  the sample it saves **14.2%** across all 31 payloads (597,158 → 512,393
  characters), about 2.9 KB off the median station — weight that currently
  ships to every browser through the RSC flight payload. Real but modest;
  worth doing as a cheap follow-up, not worth arguing about. The frontend
  already drops nulls at render time
  (`StationAccessibilitySection.tsx:73-77`), so this changes no output.
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
   *depth-1 non-HTML* scalars are self-describing sentences, so the correct
   move is removing labels, not curating one per each of the sample's 171
   distinct field names.
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
- **The existing axe-core sweep is a weaker safety net than it sounds.**
  `frontend/e2e/accessibility.spec.ts` does sweep
  `/stations/${REAL_STATION_CRS}` (default `PAD`), but only with five rules
  — `color-contrast`, `landmark-one-main`, `region`, `heading-order`,
  `page-has-heading-one`. None of those can catch an icon-carrying-meaning
  regression, so §4.2's text-label rule needs its own unit assertion rather
  than relying on this sweep. Nor would `heading-order` catch §4.7's stray
  `h2`: the page's outline is `h1` → `h2`, and an injected `h2` is a sibling,
  not a skipped level. Both rules in §4.2 and §4.7 need their own unit
  tests; this sweep is not the safety net for either.
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
5. **The RDM OpenAPI spec was not consulted.**
   `crates/poller-stations/src/schema.rs:3-6` already cites a "National Rail
   Station API OpenAPI spec (v1.0.0, `paths./stations`,
   `components.schemas.Station`)" as the source for its field names, so that
   document demonstrably exists and describes `Station`. Whether it also
   specifies the *sub-object* shapes in §2.3 is unknown — if it does, much
   of this survey upgrades from "observed in 31 stations" to "documented
   contract". Not available in this sandbox; worth ten minutes from anyone
   with RDM portal access.
6. **Effort is not estimated here.** Seven pattern renderers plus a
   sanitizer is materially more than the current ~190-line module, and this
   document deliberately makes no claim about whether that is the right
   use of time next — only about what the data supports.
