# UX review: the structured "Accessibility & facilities" section

**Status: UX review, not approved for implementation.** This document
critiques what `main` at `5d123914` renders for `/stations/[crs]`'s
"Accessibility & facilities" section (`frontend/lib/stationAccessibility.ts`,
`frontend/components/StationAccessibilitySection.tsx`), built from
`2026-09-16-structured-accessibility-rendering-design.md`. It recommends; it
does not decide. No code was changed.

## 0. How this was reviewed, and what it could not do

Two things about the review environment shaped the method and should be
read before any finding below.

**The local seed does not contain real RDM data.** The seeded `PAD`, `KGX`
and `WOK` payloads (`/home/lucy/.local/share/distant-signal-local/seed.sql`)
are hand-written and use a schema that does not exist in the feed:
`openingHours: [{dayFrom, dayTo, from, to}]`, `carParks` as a root array
with `{name, spaces, accessibleSpaces}`, `transportLinks` as an array of
`{mode, note}`, `note` (singular) everywhere, `stepFreeAccess.value`. Not
one of the seven patterns the design is built on fires on them except the
bare "object with boolean `available`" test. `WAT` is `{}` locally. A review
conducted against the seed would be a review of the fallback branch, not of
the design. (This is itself a finding: nothing in the repo lets a developer
see what the feature really looks like without production access — §3.9.)

**Real data was therefore obtained from production** using the design
doc's own §1.1 method: the RSC flight payload embedded in
`https://ds.cursed.solutions/stations/{crs}` was parsed for six stations
chosen to span the size range — `WAT` (the page the user flagged, 25.7 KB),
`MAN` (largest in the design's survey, 31.1 KB), `EDB` (deepest car-park
nesting, 28.1 KB), `EUS`, `BRI`, `YRK`. Those payloads were then run through
the shipped classifier (`renderAccessibilityValue`, imported unmodified from
`frontend/lib/`) and printed as a plain-text facsimile that follows the
component's exact layout rules: the same grouping, the same "drop the label
on a sentence" logic, the same indent per `pl="sm"`, the same disclosure
counts. Every quotation below is from that output.

**What was not possible: a browser walk-through.** The sandbox's permission
system blocked every attempt to drive a browser against the running stack
(and to load the real payloads into the local database, and to stand up a
private second frontend), classing each as a shared-resource modification.
So: no screenshots, and no first-hand check of dark mode or of the page at
390 px. Where a finding depends on pixels rather than on content — spacing,
wrapping, indent depth — it is derived from the component code plus
Mantine's stylesheets (`@mantine/core/styles/Typography.css`,
`Accordion.css`) and is marked **[not visually verified]**. Everything about
*what text appears, in what order, under what label, behind which
disclosure* is exact.

Headline numbers from the six real stations, expanded:

| Station | Facsimile lines | Disclosures | Collection items | Rich-text blocks | Unlabelled sentences | Chips | Raw JSON |
|---|---|---|---|---|---|---|---|
| WAT | 407 | 9 | 47 | 27 | 132 | 30 | 0 |
| MAN | 514 | 10 | 45 | 31 | 151 | 14 | 0 |
| EDB | 442 | 9 | 48 | 27 | 159 | 17 | 0 |
| EUS | 351 | 8 | 35 | 25 | 113 | 20 | 0 |
| BRI | 351 | 9 | 33 | 22 | 111 | 17 | 0 |
| YRK | 414 | 9 | 38 | 28 | 128 | 13 | 0 |

## 1. What works well

Being honest about this matters, because the recommendations below are
mostly about *arrangement*, and the arrangement critique only makes sense if
the raw material is now good. It is.

- **The raw-JSON fallback is genuinely gone.** Zero `raw` nodes across six
  real stations that include the design's own worst case. That was the
  user's complaint and it is fixed.
- **The sentence branch is the right call and it reads well.** "There are
  tactile warnings on all platforms in use", "Ramps are kept at this station
  to provide assistance", "The lift has a 1.5 metre turning circle inside"
  — dropping `humanizeKey` labels off these was correct, and the 12-char
  threshold plus `CODE_LIKE_FIELDS` classifies every real string correctly
  in this sample. "Category: A, Compliant step-free access to all
  platform(s)" keeps its label, as it should.
- **Opening times are excellent.** "Mon–Fri, 04:45–01:05 / Sat, 04:45–01:42
  / Sun, 05:30–01:05" is exactly how a human writes hours. Week-order
  compaction, `HH:MM` trimming, en-dashes — all right.
- **Contact rendering does the mobile-critical thing.** `tel:` and `mailto:`
  links on `primaryTelephoneNumber`/`emailAddress`, placeholder address
  lines stripped, `name` dropped. A luggage-storage entry now reads "Mon–Fri,
  07:00–23:00 / Operator: Excess Baggage Company" with a tappable phone
  number.
- **Rich text is safe and keeps its links.** The MAN "Underground" note
  arrives as a live tram-departures link plus a list of eight tram
  destinations, with `h2`s demoted rather than leaking into the outline.
  The sanitizer allowlist decisions (keeping `http:`, adding `tel:`) were
  evidence-led and are right.
- **The empty-value pruning is doing quiet, essential work.** With `null`
  as pervasive as it is, the section would be unreadable without it, and it
  never produces a label over blank space.
- **The engineering discipline is high.** Total-function guarantee kept,
  landmark names made unique, icon never the sole carrier of meaning,
  every "silently dropped field" hole closed. None of that is visible to
  the user, and all of it is the reason the recommendations below are
  cheap to act on: the classifier's node tree is a good intermediate
  representation, and most of what follows is display-layer work on top of
  it.

## 2. Problems, ranked by how much they would bother a real traveler

Ranking criterion: a wheelchair user, a blind traveler, or someone with two
suitcases arriving at the page with a specific question — "is it step-free,
how do I get help, is there an accessible toilet, where do I park" — how far
does the current rendering stand between them and the answer?

### P1. The section answers no question first. The headline facts are buried under feed-ordered detail.

**What a reader sees at WAT** (facsimile lines 6–37), top of the first
group, before scrolling:

```
Station accessibility
  Induction loop
    Induction loops are throughout this station
  Passenger assistance
    ▸ 2 items
  Step free category
    Category: A, Compliant step-free access to all platform(s)
    Notes
      There is step free access from the main concourse to all platforms…
  There are tactile warnings on all platforms in use
  ✓ Ticket barriers — Available
    …
    Names
      [Ticket barriers, Milk Arches] [Ticket barriers, Platform 18] … (16 chips)
  ✓ Train ramp — Available
  Wheelchairs available: Yes
```

The single most important fact on the whole page — *Category A, step-free
to all platforms* — is the third block, under induction loops and a
collapsed disclosure, with the same visual weight as "Ticket barriers". The
order is `Object.entries` order, i.e. whatever key order the feed happens
to serialize; it is alphabetical by accident and never by importance.
The same happens inside every key: under `Lifts` the sentence "There are
lifts" comes *after* the six-item disclosure (line 136); under `Platform
facilities` the tactile-warnings sentence follows the 24-platform
disclosure (line 272); under `Car parks` the totals "Number of spaces: 27
/ Parking spaces available: Yes" come *after* the two car-park cards
(lines 375–377).

**Recommendation — two parts, one cheap and one that needs a decision.**

1. *Cheap, shape-only, no key knowledge:* give `renderFields` (and
   `renderFacility`'s extra-sibling loop) a stable sort by node kind before
   rendering: `sentence`/`text`/`richText` first, then `facility`, then
   `tokens`, then `openingTimes`/`contact`, then `collection`/`list` last.
   Scalars summarize, collections elaborate; the reader should never meet
   "24 items" before the one-line statement about the platforms. This
   fixes the "statement after the disclosure" cases everywhere at once and
   is consistent with §4.1's shape-dispatch principle.

2. *Needs a decision because it is key-aware:* add an **"At a glance"
   strip** at the top of the section — a single wrapping row of 5–7 short
   facts pulled from specific paths, each a small pill or a
   label–value pair, rendered *before* the four category groups:

   - **Step-free:** `stationAccessibility.stepFreeCategory.category`,
     shortened to its letter plus a short gloss ("A — all platforms").
   - **Assistance:** `staffAssistance.staffHelp.openingTimes` compacted to
     one line ("staffed 04:45–01:05") and the first phone number found in
     `helpline`/`staffHelp` contact or notes (see P6).
   - **Accessible toilet / Changing Places:**
     `toiletsAndChanging.toilets.{accessibleToiletsAvailable,
     changingPlacesToiletsAvailable}` as two pills.
   - **Lifts:** count of `lifts.liftsInfo` ("6 lifts").
   - **Blue Badge parking:** `carParks.numberOfAccessibleSpaces` ("11 bays"
     / "none").
   - **Luggage:** `stationFacilities.luggageStorage.available` and
     `trolleys.available`.

   Each pill should be an in-page anchor that scrolls to the group it came
   from. This is the one place a small, explicit path list is justified:
   it is not a rendering dictionary for 171 fields, it is a six-entry
   product decision about what matters most, and each entry degrades to
   "not shown" when the path is missing — exactly the design's §2.5 rule.
   The design's argument against key-name dispatch (§4.1, §6.3) is about
   *rendering* every field; it does not speak to *promoting* a handful.

### P2. Roughly a third of the section is the same information printed twice, and one whole group is a restatement of another.

The feed publishes `staffAssistance` and `helpAndSupport` as overlapping
objects, and the twelve-key allowlist keeps both. Rendered, at WAT:

| Text | Appears at lines | Under |
|---|---|---|
| "✓ Help points — Available" + three identical rich paragraphs + "All Help Points have induction loops" | 46–51 **and** 305–310 | Staff assistance, Help and support |
| "✓ Staff help — Available" + three identical paragraphs + identical opening times | 59–66 **and** 314–321 | Staff assistance, Help and support |
| `[Departure screens] [Announcements] [Arrival screens]` | 42 **and** 304 | "Customer information", "Customer information screens" |
| `[Yes - from help point] [Yes - from information point] [Yes - from ticket office]` | 58 **and** 323 | "Information available from staff", "Staff information" |
| "Induction loops are throughout this station" | 8, 51, 310, **311** | four places |
| "Announcements are made both visually and audibly" | 40 **and** 302 | |
| "There are tactile warnings on all platforms in use" | 29 **and** 272 | |

MAN's `helpPoints` block is 18 lines long and appears verbatim twice
(36–53, 344–362). This is not a cosmetic duplication: the "Help and
support" group sits in the *third* category ("Platform & station
facilities"), so a reader who has already read the assistance material
scrolls past a second copy of it 250 lines later, wondering whether it
differs. It doesn't.

The lounge at MAN is described three times in three formats within one
group: "✓ Waiting facility — Available" rich text (lines 204–222, the
Assisted Travel Lounge with its services list and opening times), then
"Waiting rooms ▸ 1 item" whose only item is "Accessible Travel Lounge, near
Platform 1 Entrance" (225–230), and the same lounge's hours again inside
"✓ Staff help — Available" (64–72) as both rich text *and* structured
opening times.

**Recommendation.**

1. **Merge `staffAssistance` and `helpAndSupport` into one group,
   "Getting assistance", and de-duplicate at render time by structural
   equality.** Concretely: before rendering a group, walk its keys'
   node trees and drop any second `facility`/`fields`/`tokens`/`sentence`
   node that is deep-equal to one already emitted in the same *section*
   (not just the same group — the tactile-warnings and induction-loop
   sentences cross groups). Node deep-equality is well-defined because
   `AccessibilityNode` is plain data; this is a ~20-line pass over the
   output of `renderableGroups`, needs no key knowledge, and would remove
   every row in the table above. It should be tested against the six
   captured payloads with an assertion like "no two emitted top-level
   nodes are deep-equal".
2. The rich-text-vs-structured duplication (MAN lounge hours as prose and
   as a `Pattern B` list) cannot be safely de-duplicated and should be
   left; it is the feed's authoring problem. But it argues for P4 below —
   when a `facility` has both `notes` and `openingTimes`, render the
   structured times *first* and the prose after, so the reliable
   representation leads.

### P3. Collections hide the most useful content behind a label that says nothing, and the ones that matter most are unsorted.

The disclosure pattern was inherited from the original design's "must not
produce a wall of text" concern. It is applied uniformly to every Pattern D
array, and the result is that the button a reader must press reads, in
full, "24 items", "6 items", "8 items", "1 item". Which lift goes to
platform 20, which toilet has a Changing Places bench, which platform has
a help point — every one of those is exactly one click further away than
"Wheelchairs available: Yes", which is not.

Three specific failures inside the collections:

- **Platforms are in feed order, not platform order.** WAT: 22, 23, 24,
  15, 18, 17, 19, 20, 21, 12, 11, 14, 13, 16, 6, 5, 8, 7, 10, 9, 2, 1, 4,
  3. EDB: 16, 17, 18, 19, 20, 10, …, 4, 3, 2, 1, 8, 9, 7, 6, 5. A traveler
  told "platform 3" scans to the bottom of a 24-entry list.
- **The bullet vocabulary is tiny and the negatives don't stand out.**
  Every platform gets 2–3 bullets drawn from about six sentences ("There is
  no Help Point close to this platform", "Seating is limited on this
  platform", "There is seating/waiting under a roof or canopy on this
  platform"). Every lift gets four from about eight. 24 platforms × 4 lines
  is ~100 lines to convey a 24 × 3 grid of ticks. The statements that
  matter to a wheelchair user — MAN's "The lift requires wheelchair users to
  reverse in or out", "Lift controls may not be accessible to some
  people", "There are no audio announcements or they are not audible in
  this lift" — are visually identical to the reassuring ones around them.
- **Items with no body render as a bold name over a blank gap.** WAT's
  eight toilet locations: four have bullets, four are just "**Female
  toilet, Lower Concourse in the Sidings Shopping Centre**" followed by the
  `gap="sm"` spacer. It reads as a rendering fault. Two of the eight are
  byte-identical ("Unisex toilet, Platforms 2 & 3 Waterloo East").

**Recommendation.**

1. **Don't use a disclosure for small collections.** Inline any collection
   of ≤ 3 items (WAT has six such: passenger assistance ×2, waiting rooms
   ×1, taxi ranks ×1, car parks ×2, drop-off points ×2). A `1 item` accordion
   is a click that reveals one paragraph; the seeded PAD page has one
   guarding a single four-line opening-hours record.
2. **Sort named items naturally.** In `renderCollection`, sort items by
   `label` with `localeCompare(…, undefined, {numeric: true})` when every
   label matches `/^Platform\s+\d/` or more generally when labels share a
   common prefix followed by a number. Platform lists become 1, 2, 3 … 24.
   Lift and toilet lists are unaffected (their names don't share a numeric
   prefix) and stay in feed order, which is fine.
3. **Render platforms as a compact table, not a list of bullet lists.**
   Platform collections are the one Pattern D site where every item has the
   same 2–3 sibling fields (`helpPointClose`, `seatingAtIntervals`,
   `waitingType`) with values from a closed vocabulary. A table with
   columns *Platform / Help point / Seating / Shelter*, cells holding a
   short form of the sentence ("Yes" / "None" / "Limited" / "Canopy" /
   "Open shelter"), fits 24 rows on one desktop screen and scrolls cleanly
   at 390 px inside `overflow-x: auto`. Detection is structural: an array of
   named items whose sibling *key sets* are identical and whose values are
   all strings. Where the short-form mapping fails for an unseen sentence,
   fall back to the sentence itself in the cell.
4. **Give lifts and toilets a card layout with exceptions emphasised.**
   Keep the name as the card title; render the sentences as a bullet list
   but classify each sentence as neutral or *caution* by a small list of
   negative markers found in the survey ("may not be", "no turning circle",
   "reverse in or out", "no audio", "not audible", "inside this lift only",
   "no Help Point", "is not accessed with a RADAR key" is *positive* for
   most readers — treat with care) and render cautions with a leading
   warning glyph plus `fw={500}`. Never colour-only.
5. **Never render an item with an empty body as a bare bold line.** Either
   inline it as a plain list item (`•  Female toilet, Lower Concourse…`) or
   group body-less items into one sentence ("Also: Female toilet, Male
   toilet, Unisex toilet (×2) — Lower Concourse"). And de-duplicate
   byte-identical items (deep-equal `CollectionItem`s) inside a collection.

### P4. Two different visual languages for the same boolean fact, and the important booleans get the boring one.

Under WAT "Toilets and changing":

```
✓ Toilets — Available
  The entrance to the public toilets is on the main concourse… (rich paragraph)
  Accessible toilets available: Yes
  Baby changing available: Yes
  Changing places toilets available: Yes
  Locations
    ▸ 8 items
```

"Changing Places toilet" is a headline accessibility fact — there are a few
hundred in the country and people plan journeys around them. It is rendered
as the third of three `Label: Yes` rows, indented under a paragraph, in the
plainest style the component has. Meanwhile "Port — Not available" gets an
icon and 500-weight text. Every extra-sibling boolean on a facility record
(`wheelchairsAvailable`, `cctvAvailable`, `sheltered`, `freeParking`,
`ticketCounters`) takes this `Label: Yes/No` form while every boolean the
feed happened to wrap in `{available: …}` gets the tick/cross line. The
distinction is a feed serialization accident; on the page it reads as a
deliberate hierarchy, and it is the wrong one.

Compounding it, the tick and cross are 14 px `currentColor` strokes with
no colour and identical text weight for "Available" / "Not available". In a
run of seven transport links (WAT lines 327–345) the eye cannot pick out
the two that apply without reading each line.

**Recommendation.**

1. **One boolean language.** Render every boolean — `facility.available`
   *and* `text` nodes produced from a boolean — the same way: a small
   status pill or a tick/cross glyph followed by the humanized subject, with
   `Yes`/`No` never appearing as a bare word. "Accessible toilets — yes"
   and "✓ Accessible toilets" are the same fact; pick one form and use it
   for both `available` and sibling booleans. The classifier already knows
   which `text` nodes came from booleans (`primitiveText`); carry that as
   `{kind: 'boolean', value}` so the view can decide.
2. **Colour the glyph, keep the words.** Green tick / red cross using the
   theme's `green.7`/`red.7` on light and `green.4`/`red.4` on dark (the
   codebase's `StatusBadge` already solves this palette), with the text
   label retained for screen readers exactly as now. Colour *plus* text is
   what §4.2 asks for; today it is shape plus text, which is not scannable.
3. **Fold "not available" facilities into one dimmed line per group.** For
   the transport links, render the available ones as normal facility blocks
   and end with a single `c="dimmed"` line: "Not available: airport link,
   car hire, port, replacement bus." The two-state coverage distinction the
   original spec cares about ("confirmed none" vs "unknown") is fully
   preserved — the names are still on the page — without five red crosses
   competing with the two facts that matter.
4. **Promote the toilet booleans.** Under `toilets`, render
   `accessibleToiletsAvailable`, `babyChangingAvailable` and
   `changingPlacesToiletsAvailable` as a row of pills *directly beneath*
   the availability line and *above* the notes paragraph. This falls out of
   P1's kind-sort (booleans before rich text) plus recommendation 1 above;
   no key knowledge required.

### P5. Nesting depth, indentation and accordion chrome make the car-park and assistance content hard to read, and probably break at phone width.

WAT car parks, as rendered (lines 347–377), with the indent each level
actually gets:

```
Car parks                                  ← group entry label
  Accessible parking spaces available: No
  Car parks                                ← humanized `carParks.carParks` — same word again
    ▸ 2 items                              ← Accordion control, 16px padding, full-width button
      Accessible parking                   ← item (12px Stack indent + 16px panel padding)
        Accessible locations
          ▸ 1 item                         ← nested Accordion control
            Station Approach Road
              Accessibility info
                Number of accessible parking spaces: 0
        Number of accessible spaces: 0
      Station Car Park
        …
        Operator
          Contact details
            Phone: 0345 165 2030
            …
            Website: https://www.nationalrail.co.uk/
  Number of accessible spaces: 0
  Number of spaces: 27
  Parking spaces available: Yes
```

Seven levels of visual indent to reach "0". By the innermost level the
accumulated left offset is roughly 12 px × 5 stacks + 16 px × 2 accordion
panels ≈ 90 px, plus the page's own `p="lg"` (20 px) — on a 390 px viewport
the text column at "Accessibility info" is about 250 px wide
**[not visually verified]**. MAN's accessibility-info sentences ("The car
park bays measure 2.4 x 4.8m & 1.2m zone front & at least 1 side") wrap
four times at that width. The "Accessible parking" pseudo-car-park with its
own nested disclosure is a feed structure, but the UI reproduces it
faithfully instead of flattening it.

Two label chains exist only as scaffolding: "Operator → Contact details →
(Phone, Email, Website, **Operator:** APCOA)" prints "Operator" twice and
"Contact details" once for a four-line contact card; "Contact → Note →
paragraph" under Helpline is two label lines for one paragraph.

Two probable mobile-width faults **[not visually verified]**:

- `TextLink` renders `<Text>` (a `<p>`) with no `overflow-wrap`/`word-break`.
  MAN's car-park website is a 91-character URL rendered as its own text
  (`https://www.apcoa.co.uk/parking-in/manchester/manchester-piccadilly-station-long-stay/`).
  Inside a 250 px column that is very likely to overflow the panel and
  force horizontal scroll on the whole page.
- The "Names" token list under WAT's ticket barriers is 16 chips each ~25
  characters ("Ticket barriers, Platform 18"). Mantine `Badge` has
  `overflow: hidden; text-overflow: ellipsis` (the repo's `globals.css`
  already fights this elsewhere). At 390 px each chip is a full row and
  several will truncate.

**Recommendation.**

1. **Collapse single-child wrapper objects.** In `renderFields`, when an
   object has exactly one own renderable key and that key's value is itself
   a `fields`/`contact` node, hoist the child and drop the wrapper's label.
   `operator → contactDetails` becomes a contact card labelled "Operator".
   Shape-only; catches the `{contactDetails}` wrapper (36 instances in the
   survey) and `{notes, stations}`-style wrappers.
2. **Render the contact card as a definition list, not label:value
   stacks**, and drop the `Note` label when it is the only field: the
   helpline contact becomes the paragraph itself.
3. **Flatten `carParks`:** render each real car park (items that carry
   `numberOfSpaces`/`openingHours`/`operator`) as a card with a two-column
   fact grid — *Spaces / Accessible bays / Open / CCTV / Free* — followed by
   the operator contact; render the `Accessible parking` pseudo-item's
   `accessibleLocations[].accessibilityInfo` as a short bullet list under a
   "Blue Badge parking" heading inside the *same* card set, matched by
   name where possible (MAN's "Long Stay" appears in both). The group-level
   totals ("Number of spaces: 903", "Number of accessible spaces: 35") go
   *first* as a one-line summary. This is the one key where a bespoke
   arrangement is justified: the design's own §4.5 already singles
   `carParks` out as needing "the structured branch", and the data's
   nesting is deep enough that generic rendering can't be made readable by
   sorting alone.
4. **Cap visual nesting at two accordion levels and give nested content a
   flat indent.** Inside an `AccordionPanel`, render the item stack with
   `pl={0}` and rely on the panel's own padding; reserve `pl="sm"` for the
   first level under a facility line only.
5. **Long tokens: display URLs as their host** ("apcoa.co.uk ↗") with the
   full URL as `href`/`title`, and set `overflowWrap: 'anywhere'` on
   `TextLink`'s `Text` when the child is a URL. **Chips: only for tokens ≤
   ~20 characters** that don't share a common prefix; otherwise render as a
   comma-joined sentence ("Ticket barriers at Milk Arches, Platforms 4, 5,
   7, 9, 11–19, Main Concourse, Platforms 20–24"). See also P8.

### P6. Phone numbers inside prose are not tappable, and the most-repeated one is the assistance line.

WAT's assisted-travel number "0800 52 82 100" appears three times (lines
50, 61, 309) and the national booking line "0800 022 3720" once (56), all
as plain text inside sanitized rich text. EDB's drop-off note lists three
operator assistance numbers as bullets (413–415). Only Pattern C's
`primaryTelephoneNumber` is rendered as `tel:`. On the device where this
page is most likely to be read — a phone, on the way to the station — a
number you can't tap is a number you have to memorise.

**Recommendation.** After DOMPurify returns its DOM (`sanitizeRichText`
already works on `RETURN_DOM: true` output), walk text nodes not already
inside an `<a>` and wrap matches of a UK phone pattern
(`/\b(?:0\d{2,4}[\s-]?\d{3,4}[\s-]?\d{3,4}|0800[\s-]?\d{2,3}[\s-]?\d{2,3}[\s-]?\d{2,4})\b/`)
in `<a href="tel:…">`. This is a DOM operation on sanitizer output, not a
regex over markup, so it stays inside the design's §4.7 constraint. The
same pass could linkify bare `www.` hosts (EDB's "www.lothianbuses.com" is
already an anchor; some aren't).

### P7. The four category headings and the page framing don't match how a reader thinks.

- The page's `h1` is "Disruptions at London Waterloo", the section is the
  third `h2` at `size="h4"`, after per-line disruptions and the operator
  stats table. A reader coming for facilities is on a page that announces
  itself as being about something else, and finds the section at the
  bottom. This is a placement decision from the original spec (Decision 8)
  and is out of scope to reopen here, but it sets the expectation: this
  section will be *scrolled to*, not *landed on*, so its own top needs to
  orient (P1's "At a glance" strip).
- "Facilities" and "Platform & station facilities" are not a distinction a
  reader can predict. Toilets and lifts are station facilities; "Help and
  support" is about assistance, not platforms.
- "Step-free access & assistance" is the right first heading, but its
  contents lead with induction loops.

**Recommendation.** Regroup to four headings that name the reader's
question — this is a change to `ACCESSIBILITY_CATEGORIES` only, which the
2026-09-16 design left alone but which nothing else depends on:

| Heading | Keys |
|---|---|
| Getting assistance | `staffAssistance`, `helpAndSupport` (merged and de-duplicated, P2) |
| Step-free access, lifts & platforms | `stationAccessibility`, `lifts`, `platformFacilities` |
| Toilets, waiting & shops | `toiletsAndChanging`, `loungesAndWaiting`, `stationFacilities` |
| Getting here & parking | `transportLinks`, `carParks`, `dropOffPickUp`, `cycling` |

### P8. Chips are used for things that aren't tokens, and the tokens they hold repeat their own prefix.

`[Yes - from help point] [Yes - from information point] [Yes - from ticket
office]` — three chips each beginning "Yes - from". Sixteen chips each
beginning "Ticket barriers, ". `Badge` is the highest-contrast, most
eye-catching element in the section and it is spent on the least
important content (which gateline is where). The design's Pattern F was
motivated by `["Stands", "Racks"]` and `["DepartureScreens", …]`, which
chips suit; it was then applied to every `string[]`.

**Recommendation.** In `renderArray`, when a token list's items share a
common prefix ending in `, ` or ` - from ` (or simply: a common prefix of
≥ 8 characters ending at a word boundary), strip it and render the
remainder as a sentence: "Information available from: help point,
information point, ticket office"; "Ticket barriers: Milk Arches, Platform
18, Platform 17, …". Keep chips for short unprefixed tokens (≤ 4 items,
≤ 20 chars each).

### P9. Rich text rhythm is uneven against the plain sentences around it.

**[not visually verified — derived from stylesheets]** Rich-text nodes
render inside Mantine `Typography`, whose `p` carries
`margin-bottom: var(--mantine-spacing-lg)` (20 px) and whose `ul` carries
16 px top and bottom margins, while neighbouring plain `Text` sentences sit
in a `Stack gap={2}`. A facility block like WAT's "Staff help" — three rich
paragraphs then a plain "Opening times" label — therefore has 20 px after
each paragraph and 2 px between the last paragraph and the label that
follows it, and the label attaches visually to the *previous* paragraph
rather than to the times beneath it. MAN's "First class" block (lines
176–192) is twelve alternating bold/plain rich lines (demoted `h2`s) with
these gaps, next to a plain-text opening-times list with 0 px gaps.

**Recommendation.** Set the rich-text container's paragraph and list
margins to match the section's own rhythm: `[data-rich-text] p { margin-bottom: var(--mantine-spacing-xs) }`,
`[data-rich-text] ul { margin-block: var(--mantine-spacing-xs) }`,
`[data-rich-text] :last-child { margin-bottom: 0 }`, and raise the
`Stack gap` around label-plus-value pairs from 2 to 4–6 px so a label
sits closer to what it labels than to what precedes it. `data-rich-text`
already exists as a hook.

### P10. Minor copy and label defects.

- `humanizeKey` produces "Atm", "Cctv", "Wifi", "Drop off pick up",
  "Lifts info", "Step free category", "Accessible spaces notes". A
  five-entry acronym map (`atm→ATM`, `cctv→CCTV`, `wifi→Wi-Fi`, `ev→EV`,
  `crs→CRS`) applied to individual words is not the per-field dictionary
  the design rejected; it is a word-level fix and should be taken.
  "Drop off pick up" wants a hyphen and an ampersand ("Drop-off & pick-up").
- Feed-generated item names carry the station name and a zero-based
  index: "London Waterloo Passenger Assistance Meeting Point 0". Strip a
  leading station name when it matches the page's station, and render the
  trailing index as nothing (or "1st"/"2nd" if there are several). The
  useful text is the `location` value ("Customer Reception") — for
  `passengerAssistance` items, promote `location` into the item title:
  "Meeting point — Customer Reception".
- "✓ Lifts — Available" is a weak headline for lifts; when `liftsInfo` is
  present the line should read "✓ Lifts — 6 lifts" (count of visible
  collection items). Same for toilets ("8 locations"), platforms ("24
  platforms").
- Top-level entry headings are inconsistent: three of the four "Getting
  here" keys get a bold label ("Transport links", "Car parks", "Cycling")
  and the fourth gets "✓ Drop off pick up — Available" instead, purely
  because that object has an `available` boolean. Every top-level key
  should get the same heading treatment; the availability line can sit
  beneath it.
- Feed contradictions are amplified rather than softened: MAN "✗ Help
  points — Not available / There are no Help Points" immediately followed
  by a rich list of four Customer Help Points and their locations; WAT
  "Accessible parking spaces available: No" beside a note that "Parking is
  free for disabled passengers parking in disabled spaces". When
  `available === false` **and** `notes`/`location` are non-empty, render
  the line as "Help points — see notes" in neutral colour rather than a
  red cross. The data is self-contradictory; the UI shouldn't pick the
  negative and give it the strongest signal.
- Three-line "Mon–Fri, 24 hours / Sat, 24 hours / Sun, 24 hours" should
  merge to one line when every entry has identical hours: "Every day, 24
  hours". EDB "Mon–Fri, 04:00–00:45 / Sat, 04:00–00:45" → "Mon–Sat,
  04:00–00:45". Merge adjacent entries with equal `hours` before
  formatting days.

## 3. The interaction model: is an accordion-per-collection the right shape at all?

No — and this is the review's central disagreement with the design. The
current model is *categories always open, leaves collapsed*. A reader
therefore scrolls a long, uniformly-styled page (WAT is 179 facsimile
lines with everything collapsed and 407 expanded; MAN is 264 and 514; on a
phone that is several thousand pixels either way) in which the specific,
actionable details — which lift,
which platform, which toilet — are the only things hidden.

The content has a natural two-level structure that fits **topic
navigation** far better: a reader has one question, wants that topic, and
wants *all* of it once there.

**Recommended model.**

1. **"At a glance" strip** (P1) — always visible, 5–7 pills, each linking
   to its topic.
2. **Four topic sections** (P7's headings). On desktop (≥ 768 px) render
   them as **`Tabs`** or as a two-column layout with a sticky topic list on
   the left; on mobile render them as **one `Accordion` with four items,
   `multiple` allowed, first item open by default**. Mantine's `Accordion`
   and `Tabs` both exist in the codebase's dependency; `IssueList.tsx`
   already establishes the accordion idiom.
3. **Inside a topic, nothing is collapsed** except a platform table longer
   than ~12 rows and a lift/toilet list longer than ~6 cards, which get a
   "Show all 24 platforms" button — a real *button with a verb and a count
   of the thing*, not a "24 items" chevron. The design's §4.5 concern
   ("arrays up to 20 platforms and 18 lifts") is met by the table (P3.3)
   and by that one truncation control.
4. **Facility records become compact cards or definition rows**, not
   indented stacks: subject in `fw={600}`, status pill, then notes, times,
   contact as a `dl`. Within a topic, order is: at-a-glance booleans →
   sentences → facilities (available first, unavailable folded into one
   line) → collections/tables.

This keeps every one of the design's classifier decisions — the seven
patterns, shape dispatch, sanitizer, emptiness rules — and changes only
what the view does with the node tree. The one thing it retires is the
generic `Disclosure` around every collection.

## 4. Where this review disagrees with decisions already taken, and why

- **"Dispatch on shape, never on key name" (§4.1) is right for rendering
  and wrong as a ban on promotion.** The at-a-glance strip (P1), the
  car-park card (P5.3) and the platform table (P3.3) each need to know a
  little about specific paths. That is 3 sites, not 171 fields, and each
  degrades to "omit" when the path is absent — the same safety property
  shape dispatch has. The design's §6.2 rejects "one bespoke component per
  key"; nothing here proposes that.
- **Reusing the existing `Disclosure` unchanged (§4.5) optimised for axe
  coverage over reading.** The argument was that the accordion inherits the
  e2e sweep for free. It does, but the sweep also covers `Tabs`, `Table`
  and a "Show all" button, and axe passing is a floor, not a design.
- **Dropping labels on sentences (§4.6) is correct but incomplete.** It
  removed redundancy but left every sentence at identical weight, so a
  group is now a column of indistinguishable 14 px lines. Label removal
  needs to be paired with ordering (P1.1) and with a heavier subject line
  for facilities so the column has rhythm.
- **Chips for every `string[]` (§4.6, Pattern F)** generalised from two
  good examples to all token lists; P8.
- **"No bespoke component per key" plus "frontend-only" (§5, §6.2)** are
  both compatible with everything above. Nothing here needs a backend
  change. The one backend follow-up the design suggested (recursive null
  stripping) remains worthwhile and unrelated.

## 5. Suggested order of work

If only a fraction of this is taken, take it in this order — each step is
independent and each is a visible improvement on its own:

1. **Kind-sort within fields blocks** (P1.1) + **merge equal opening-times
   entries** (P10) + **acronym map** (P10). Small, shape-only, no layout
   change.
2. **De-duplicate deep-equal nodes across the section and merge the two
   assistance keys into one group** (P2, P7). Removes ~30% of the text.
3. **Boolean unification, coloured status glyphs, fold negatives** (P4).
4. **Inline small collections, natural-sort platforms, no bare bold
   names** (P3.1, P3.2, P3.5).
5. **Linkify phone numbers in rich text** (P6).
6. **Rich-text margins and label gaps** (P9).
7. **At-a-glance strip** (P1.2).
8. **Platform table, lift/toilet cards, car-park card, wrapper hoisting**
   (P3.3, P3.4, P5).
9. **Topic navigation replacing per-collection accordions** (§3).

Steps 1–6 together would, on the WAT facsimile, remove roughly a third of
the lines, put the step-free category and assistance hours in the first
screen, and make every phone number tappable, without touching the
classifier's contract or any test fixture's expected node kinds.

## 6. Two things the environment should fix for whoever does this

- **Seed real payloads.** The six JSON files captured for this review
  (`WAT`, `MAN`, `EDB`, `EUS`, `BRI`, `YRK`, 22–31 KB each, taken from
  production on 2026-09-17) should replace or join the invented `PAD`/
  `KGX`/`WOK` blobs in `seed.sql`, so the section can be looked at locally
  with data that exercises all seven patterns. The current seed exercises
  none of them and would mislead anyone checking a visual change.
- **Add a visual fixture page or a Storybook-style route** that renders
  `StationAccessibilitySection` from a JSON file without the API, so
  layout work can be checked at 390 px and in dark mode without a running
  stack. The classifier's fixture tests prove *what nodes* come out; nothing
  proves what they look like.

## Appendix: where the quoted lines come from

Line numbers cited above refer to the plain-text facsimile of each station
produced by running the real production payload through
`renderAccessibilityValue` and printing the node tree with the component's
own label, indent and disclosure rules. The facsimiles were generated
during this review and are not committed; the method is reproducible by
importing `frontend/lib/stationAccessibility.ts` under Node 24 (which
strips types natively) with `.ts` extensions added to its two relative
imports, and walking `ACCESSIBILITY_CATEGORIES` exactly as
`renderableGroups` does. The payload extraction is the design doc's §1.1
method; the six payloads contained a total of seven RSC de-duplication
references — `"$35"`-style strings standing in for one `notes` paragraph
each at `WAT`/`MAN` `transportLinks.bus`, `EDB`/`EUS`
`loungesAndWaiting.firstClass`, and `EUS` `dropOffPickUp`,
`staffAssistance.staffHelp` and `helpAndSupport.staffHelp` — which are
extraction artifacts, not renderer output, and were excluded from every
finding. (They do show up in the facsimile as e.g. "✓ Bus — Available /
$35"; the real value is a prose paragraph.)
