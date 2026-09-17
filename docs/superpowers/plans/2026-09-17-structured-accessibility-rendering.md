# Plan: Structured Rendering for Station Accessibility & Facilities

Implements `docs/superpowers/specs/2026-09-16-structured-accessibility-rendering-design.md`.
Frontend only: `crates/api`'s accessibility route stays a key-name-filtered
passthrough (design §5), no migration, no wire-type change.

## Global constraints

1. **Dispatch on shape, never on the twelve key names.** §4.1's precedence is
   fixed: B (opening times) → D (named items) → F (token list) for arrays;
   C (contact) → A (facility) → labelled-fields fallback for objects. Two
   key-name exceptions are explicitly sanctioned by the spec and nowhere
   else: §4.5's `crsCode`/`url` single-sibling collection items, and §4.6
   option 1's code-like field deny-list.
2. **Total function.** `renderAccessibilityValue` must not throw for any
   input — `null`, `[]`, `{}`, a cycle, a class instance, arbitrary depth.
   This is the guarantee worth keeping from the original Decision 6.
3. **Three coverage states preserved** (`unavailable` / nothing-published /
   content), plus `renderableGroups`' empty-skipping (§4.10, §2.5).
4. **No new headings.** The section owns one `h2`; every pattern renders as
   text, lists, or the existing `Accordion` disclosure, so the just-merged
   full-ruleset axe sweep (`e90cb78f`) keeps passing unchanged.

## Phase 1 — fixtures

Copy all 31 surveyed payloads into `frontend/test/fixtures/accessibility/`
as compact JSON (~580 KB), one file per CRS, plus a small typed loader
carrying provenance. Precedent for real-API fixtures of this size:
`crates/poller-tfl/tests/fixtures/`. Every claim the tests make is then
checkable against the same evidence the design was derived from.

## Phase 2 — `lib/sanitizeHtml.ts`: `sanitizeRichText`

Reuse the existing `isomorphic-dompurify` dependency and the existing
module (§9.1 resolved in favour of option (a); the "should we take a
sanitizer dependency" cost is already sunk — `sanitizeDescription` has
shipped since the incident-detail work).

- Allowlist: `p br strong b em i u ul ol li a` plus `h1`–`h6` (admitted only
  so they can be demoted rather than flattened).
- `ALLOWED_ATTR: ['href']`, `ALLOWED_URI_REGEXP` restricted to
  `https: http: mailto: tel:` — `http:` is 45 of the sample's 193 anchors
  (§2.4), so omitting it silently drops 23% of links.
- Headings are demoted to `<p><strong>` by DOM surgery on the sanitizer's
  own `RETURN_DOM` output — not by a regex over markup. §4.7 wants `h2`
  gone from the outline; `h4` under an `h2` would be a *skipped* level and
  an axe `heading-order` failure, so `strong` (the spec's other option) is
  the correct one.
- The module's existing `afterSanitizeAttributes` hook keeps forcing
  `target="_blank" rel="noopener"` on every surviving anchor.

## Phase 3 — `lib/stationAccessibility.ts`: shape dispatch

Replace `RenderableValue` with an `AccessibilityNode` union — one variant
per pattern, so a unit test can assert "this real payload matched Pattern
B" rather than inspecting markup:

| Node | Pattern | Detect | Render |
|---|---|---|---|
| `facility` | A | plain object with boolean `available` | availability line + location/notes (rich) + `openingTimes` (B) + `openingHoursNotes` + `operatorContactDetails` (C) + extra siblings |
| `openingTimes` | B | array of objects with `daysOfTheWeek` **and** `openingStatus` | per entry: week-order-sorted, range-compacted days + hours |
| `contact` | C | plain object with a `primaryTelephoneNumber` key | `tel:`/`mailto:`/url links, joined postal address, `operatorName` rich, `note` rich; `name` dropped (§4.4) |
| `collection` | D | array whose every element is an object with a string `name` | bullet branch (all other values string/null) or structured branch (recurse) |
| `sentence` | E | string, ≥12 chars, capitalised, multi-word, not deny-listed | unlabelled prose |
| `tokens` | F | non-empty `string[]` | chips, `humanizeKey` only on camelCase tokens |
| `richText` | G | string containing a tag or an entity | sanitized HTML |
| `fields` | fallback | any other plain object | labelled key/value rows, each value recursed |
| `raw` | terminal | non-plain object, or past the depth bound | collapsed `JSON.stringify` |

Details the real data forces:

- **Days**: sort into week order before compacting — four sample entries
  arrive out of order, one fully reversed (§4.3). `Public Holidays` is a
  trailing token, never folded into a range.
- **`24 Hours` with a real `openPeriod`** (2 entries at `LLE`) shows both,
  never silently picks one.
- **Unknown `openingStatus`** prints verbatim; no exhaustive switch (§9.4).
- **Depth bound `depth <= 7`.** Verified against the data, not copied: the
  deepest container chain really is seven containers
  (`carParks` → `carParks[]` → item → `openingHours[]` → entry →
  `openPeriod[]` → `{startTime,endTime}`), occupying 0-based depths 0-6, so
  7 is exactly one level of margin. Every container level increments
  `depth`, object levels included — §4.9's precondition.
- **Pattern E deny-list**: `category`, `crsCode`, `postcode`. §4.6 shows the
  bare length heuristic is inverted on `stepFreeCategory.category` (a code
  that passes) and `lifts.statement` (prose that fails); a 12-character
  threshold fixes the second and the deny-list fixes the first. Measured
  over the sample: every depth-1 non-HTML string is classified correctly,
  the single exception being an RSC extraction artifact (§1.3).

## Phase 4 — `components/StationAccessibilitySection.tsx`

One view per node kind. Unchanged: the section `h2`, the three states, the
group order, `Disclosure` (`Accordion` + `keepMounted={false}` + the
`qualifier` that keeps `role="region"` landmark names unique).

New markup rules that axe cannot enforce and unit tests therefore must
(§8): the availability icon is `aria-hidden` and always accompanied by the
words "Available"/"Not available"; sanitized rich text renders inside
`TypographyStylesProvider` with the `data-rich-text` anchor-colour hook
that `DisruptionDetail` already uses; bullets are a real `<ul>`.

## Phase 5 — tests

- `lib/stationAccessibility.test.ts`: per-pattern predicate and formatting
  tests driven by real fragments; the retained total-function tests.
- `lib/stationAccessibility.fixtures.test.ts`: the sweep over all 31 real
  payloads — no `raw` node anywhere (the checkable inverse of §2.1's
  96.2%), every named path lands on its expected pattern, nothing throws,
  no `<script>`/`javascript:`/`on*` survives anywhere in the output.
- `lib/sanitizeHtml.test.ts`: `<script>`, `javascript:` href, `onerror`,
  heading demotion, `http:` and `mailto:` survival.
- `components/StationAccessibilitySection.test.tsx`: the existing
  three-state and landmark-name tests, re-pointed at structured output,
  plus the icon-not-alone and no-injected-heading assertions.
