import { sanitizeRichText } from './sanitizeHtml';
import type { StationAccessibilityData } from './types';

/** Fixed display order and grouping for the twelve allowlisted keys -- see
 * docs/superpowers/specs/2026-09-12-station-accessibility-design.md
 * Decision 7, left untouched by the structured-rendering redesign
 * (2026-09-16-structured-accessibility-rendering-design.md §4.10). A
 * station page reader scans top-to-bottom for the thing they care about,
 * most-asked-about first; this is deliberately not alphabetical and not the
 * response's own key order. `StationAccessibilitySection` skips a whole
 * group when none of its keys carry a value. Every key the backend
 * allowlist (`ACCESSIBILITY_KEYS`, crates/api/src/data/reference.rs)
 * forwards must appear here exactly once, or it would be fetched and then
 * never rendered -- asserted in this module's own test. */
export const ACCESSIBILITY_CATEGORIES: {
  heading: string;
  keys: (keyof StationAccessibilityData)[];
}[] = [
  { heading: 'Step-free access & assistance', keys: ['stationAccessibility', 'staffAssistance'] },
  { heading: 'Facilities', keys: ['toiletsAndChanging', 'lifts', 'loungesAndWaiting'] },
  {
    heading: 'Platform & station facilities',
    keys: ['platformFacilities', 'stationFacilities', 'helpAndSupport'],
  },
  { heading: 'Getting here', keys: ['transportLinks', 'carParks', 'dropOffPickUp', 'cycling'] },
];

/** `stepFreeAccess` -> `Step free access`: splits on camelCase word
 * boundaries, lowercases every word, then capitalizes only the first.
 *
 * Still no per-field dictionary, but for a better reason than the original
 * spec's (which was "the field set is unverified"). The survey found 171
 * distinct field names across 31 stations, and 91% of the strings those
 * names label are already complete English sentences -- so the right move
 * is to DROP most labels (Pattern E, §4.6), not to curate one per field.
 * What is left for this function is the two jobs it is actually good at:
 * labelling the fallback branch's key/value rows, and expanding the
 * camelCase tokens Pattern F carries (`DepartureScreens`). */
export function humanizeKey(key: string): string {
  const words = key
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
    .toLowerCase()
    .split(' ')
    .filter(Boolean);
  if (words.length === 0) return key;
  return [words[0].charAt(0).toUpperCase() + words[0].slice(1), ...words.slice(1)].join(' ');
}

/** Decision 6's "`null`/`undefined` -> the key is skipped entirely" rule,
 * as a predicate the section component filters its keys through. The
 * backend already drops null-valued allowlisted keys
 * (`filter_accessibility_fields`), but the frontend does not treat that as
 * a hard guarantee -- and `null` is pervasive *below* the top level too
 * (§2.5), where this same predicate does the skipping. */
export function hasRenderableValue(value: unknown): boolean {
  return value !== null && value !== undefined;
}

// ---------------------------------------------------------------------------
// The node tree
// ---------------------------------------------------------------------------

/** What the renderer produces: one variant per confirmed shape pattern, so
 * the component only has to display, and a unit test can assert "this real
 * station payload was recognised as Pattern B" without reading markup.
 *
 * `raw` is the terminal branch and, per §4.9, should now fire on nothing in
 * the 31-station sample -- `frontend/lib/stationAccessibility.fixtures.test.ts`
 * asserts exactly that. It is kept because the patterns are confirmed
 * against 1.2% of stations at one point in time, and a renderer that blanked
 * on an unmatched shape would be a worse regression than the JSON dump this
 * replaces. */
export type AccessibilityNode =
  /** A scalar that is not display prose: a number, a boolean as Yes/No, a
   * code, a short label. Always rendered with its field's label. */
  | { kind: 'text'; text: string }
  /** Pattern E. A string the feed has already written as display copy;
   * rendered with NO label, because "Tactile paving: There are tactile
   * warnings on all platforms in use" says it twice. */
  | { kind: 'sentence'; text: string }
  /** Pattern G. Already passed through `sanitizeRichText`; safe to inject. */
  | { kind: 'richText'; html: string }
  /** Pattern F. */
  | { kind: 'tokens'; tokens: string[] }
  /** Pattern A. */
  | { kind: 'facility'; available: boolean; parts: LabelledNode[] }
  /** Pattern B. */
  | { kind: 'openingTimes'; entries: OpeningTimesEntry[] }
  /** Pattern C. */
  | { kind: 'contact'; fields: LabelledNode[] }
  /** Pattern D. */
  | { kind: 'collection'; items: CollectionItem[] }
  /** Pattern D's bullet branch: an item's sibling sentences, unlabelled. */
  | { kind: 'bullets'; items: AccessibilityNode[] }
  /** An array that matched none of B/D/F -- each element recursed. */
  | { kind: 'list'; items: AccessibilityNode[] }
  /** §4.9's fallback: an unmatched plain object as labelled key/value rows,
   * every value recursed through the same dispatcher. */
  | { kind: 'fields'; fields: LabelledNode[] }
  | { kind: 'link'; href: string; text: string; external: boolean }
  /** §4.9's last resort. */
  | { kind: 'raw'; json: string };

/** A node with an optional label. `label: undefined` is Pattern E's
 * "drop the label" answer, not an oversight. */
export interface LabelledNode {
  label?: string;
  node: AccessibilityNode;
}

/** One Pattern B line, split so tests can assert the two halves
 * independently: `Mon-Sat` and `05:00-00:45`. */
export interface OpeningTimesEntry {
  days: string;
  hours: string;
}

/** One Pattern D item. `link` carries §4.5's two small exceptions: a
 * `{name, crsCode}` station reference and a `{name, url}` map PDF, both of
 * which read as a bare unlabelled bullet under the bullet branch. */
export interface CollectionItem {
  label: string;
  link?: { href: string; external: boolean };
  body: AccessibilityNode;
}

// ---------------------------------------------------------------------------
// Scalars and shape predicates
// ---------------------------------------------------------------------------

type Primitive = string | number | boolean;

function isPrimitive(value: unknown): value is Primitive {
  return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
}

/** Deliberately stricter than "typeof object and not an array": a `Date`, a
 * `Map` or any class instance is NOT a shape this renderer claims to
 * understand, and falls to `raw` rather than being enumerated as if its own
 * properties were feed fields. Nothing out of `JSON.parse` fails this. */
function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
  const proto = Object.getPrototypeOf(value) as unknown;
  return proto === Object.prototype || proto === null;
}

function primitiveText(value: Primitive): string {
  if (typeof value === 'boolean') return value ? 'Yes' : 'No';
  return String(value);
}

/** Pattern G's detector. A `<` that starts a tag, or an HTML entity. The
 * survey's six entities are `&#160; &#39; &quot; &#163; &amp; &#233;`, all
 * of which this matches; a bare `&` followed by a space (`"07:30 - 21:30 &
 * Sunday"`, `ABD`'s `operatorName`) deliberately does NOT match, so that
 * string takes the plain-text path and React escapes it -- §4.7's
 * requirement that it "still be escaped rather than passed through a
 * sanitizer as-is".
 *
 * Detecting markup by CONTENT rather than by field name is the §4.1 rule
 * applied to Pattern G: it catches the five `operatorName`s that are a bare
 * `<a href>` without a name-based special case, and it keeps working if the
 * feed starts putting markup in a field this survey never saw. Both
 * branches are safe -- an undetected string is escaped by React, a detected
 * one is sanitized -- so a false negative degrades to literal text, never
 * to injected markup. */
export function containsMarkup(value: string): boolean {
  return /<[a-zA-Z/!]/.test(value) || /&(?:#\d+|#x[0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);/.test(value);
}

/** Fields whose value is a code rather than prose, and which therefore keep
 * their label even though they would otherwise pass `isSentence`. §4.6's
 * option 1, which the design recommends over a shape-only heuristic
 * precisely because `stepFreeCategory.category` ("B1, (refer to quick
 * reference guide)") is capitalised, spaced and 36 characters long -- it
 * defeats every length threshold while being the clearest case that needs a
 * label. `crsCode` and `postcode` are the design's own other two named
 * examples of codes; both normally reach a bespoke renderer instead
 * (§4.4/§4.5), so these entries only matter if one ever shows up somewhere
 * the survey did not see it. */
const CODE_LIKE_FIELDS = new Set(['category', 'crsCode', 'postcode']);

/** Pattern E's test: display copy, as opposed to a code or a bare label.
 *
 * Twelve characters, not the ~20 the design floats, and that difference is
 * the whole point: §4.6 measured the 20-character version and found it
 * inverted on the two cases that matter, *passing*
 * `stepFreeCategory.category` and *failing* all 31 `lifts.statement`s
 * ("There are lifts", 15 characters -- textbook self-describing prose).
 * `CODE_LIKE_FIELDS` handles the first; 12 handles the second. Measured
 * over the 31 fixtures, this pair classifies every depth-1 non-HTML string
 * correctly, the sole exception being one RSC extraction artifact (§1.3)
 * that is not a real feed value at all. */
export function isSentence(value: string): boolean {
  const text = value.trim();
  return (
    text.length >= 12 &&
    /^[A-Z£"'(]/.test(text) &&
    /\s/.test(text) &&
    /[a-z]/.test(text)
  );
}

/** Pattern B. Structural, so `carParks[].openingHours` -- which the feed
 * spells differently from the eleven `openingTimes` sites -- matches on the
 * same predicate with no path list. */
function isOpeningTimes(value: unknown[]): boolean {
  return (
    value.length > 0 &&
    value.every(
      (entry) => isPlainObject(entry) && 'daysOfTheWeek' in entry && 'openingStatus' in entry,
    )
  );
}

/** Pattern D. Tried BEFORE the object patterns (§4.1) so an array is never
 * misread as its first element, and before C/A so
 * `stationAccessibility.passengerAssistance` -- 49 items that carry both a
 * string `name` and a boolean `available` -- resolves to "several
 * distinctly-named meeting points" rather than "one facility". */
function isNamedItems(value: unknown[]): boolean {
  return (
    value.length > 0 &&
    value.every(
      (item) => isPlainObject(item) && typeof item.name === 'string' && item.name.trim() !== '',
    )
  );
}

function isTokenList(value: unknown[]): boolean {
  return value.length > 0 && value.every((item) => typeof item === 'string');
}

/** Pattern C. Keyed on the presence of `primaryTelephoneNumber`, not on its
 * being non-null: it is `null` in most contact objects, and the rest of the
 * record is still a contact record. */
function isContactDetails(value: Record<string, unknown>): boolean {
  return 'primaryTelephoneNumber' in value;
}

/** Pattern A. */
function isFacilityRecord(value: Record<string, unknown>): boolean {
  return typeof value.available === 'boolean';
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/** The last container depth this renderer will descend to, counting the
 * allowlisted key's own value as 0 and incrementing at EVERY container
 * level, objects included -- §4.9's stated precondition, and the thing the
 * old renderer got wrong by counting only array levels.
 *
 * Verified against the fixtures rather than copied from the design: the
 * deepest chain in all 31 payloads is seven containers -- `carParks` object
 * -> `carParks` array -> element -> `openingHours` array -> entry ->
 * `openPeriod` array -> `{startTime, endTime}` -- occupying depths 0
 * through 6. 7 is therefore the observed maximum plus exactly one level of
 * margin, which is what §4.9 asks for and why `depth < 7` would be the
 * wrong bound to write.
 *
 * **Every level is counted, including the ones a pattern renderer swallows
 * whole**, which needs saying because two of them do. Pattern B consumes
 * three levels below its own array (entry, `openPeriod` array, period
 * object) inside pure string formatters that never re-enter `renderAt`, and
 * Pattern D consumes one (the item object) before handing the item's own
 * fields back to the dispatcher. Both therefore check the bound for the
 * levels they are about to consume, rather than quietly reaching past it --
 * without that, `MAX_RENDER_DEPTH` would measure "levels the generic
 * dispatcher happened to walk", which is a smaller and much less meaningful
 * number than the one §4.9 reasons about. (Pattern C's `postalAddress` join
 * consumes one further level the same way; it sits at most at depth 5 in
 * the sample, inside the margin, and is left unchecked because a flat
 * object of address lines cannot recurse.)
 *
 * Termination does not depend on any of that arithmetic being right: no
 * pattern renderer recurses, so the only unbounded path is `renderAt`
 * itself, which increments on every call. */
export const MAX_RENDER_DEPTH = 7;

/** Turn one allowlisted key's value into something displayable. Never
 * throws, for any input at all -- the guarantee most worth keeping from the
 * original Decision 6 (§8). Every branch either produces a node or falls
 * through to `raw`. */
export function renderAccessibilityValue(value: unknown): AccessibilityNode {
  return renderAt(value, 0);
}

function renderAt(value: unknown, depth: number): AccessibilityNode {
  if (typeof value === 'string') return renderString(value);
  if (isPrimitive(value)) return { kind: 'text', text: primitiveText(value) };

  // Past the bound, or a shape this renderer makes no claim about
  // (`null`, `undefined`, a class instance, a function). Checked before
  // any recursion, which is what makes termination structural rather than
  // a property of the payload.
  if (depth > MAX_RENDER_DEPTH) return raw(value);

  if (Array.isArray(value)) return renderArray(value, depth);
  if (isPlainObject(value)) return renderObject(value, depth);
  return raw(value);
}

/** Patterns E and G, plus the plain-text remainder. */
function renderString(value: string): AccessibilityNode {
  const text = value.trim();
  if (text === '') return { kind: 'text', text: '' };
  if (containsMarkup(text)) return { kind: 'richText', html: sanitizeRichText(text) };
  if (isSentence(text)) return { kind: 'sentence', text };
  return { kind: 'text', text };
}

/** §4.1's precedence, array half: B, then D, then F. */
function renderArray(value: unknown[], depth: number): AccessibilityNode {
  if (isOpeningTimes(value)) return renderOpeningTimes(value, depth);
  if (isNamedItems(value)) return renderCollection(value, depth);
  if (isTokenList(value)) return { kind: 'tokens', tokens: value.map(humanizeToken) };
  return { kind: 'list', items: value.map((item) => renderAt(item, depth + 1)) };
}

/** §4.1's precedence, object half: C, then A, then the labelled fallback. */
function renderObject(value: Record<string, unknown>, depth: number): AccessibilityNode {
  if (isContactDetails(value)) return renderContact(value, depth);
  if (isFacilityRecord(value)) return renderFacility(value, depth);
  return renderFields(value, depth);
}

// ---------------------------------------------------------------------------
// Pattern A -- facility record
// ---------------------------------------------------------------------------

/** The six fields of the canonical facility record, consumed in the order
 * §4.2 lays out. Anything else the site adds (`storage` on `trainRamp`,
 * `names` on `ticketBarriers`, three booleans and `locations` on `toilets`,
 * `points` on `dropOffPickUp`, `liftsInfo`/`statement` on `lifts`) falls
 * through to the same treatment the fallback branch gives it. */
const FACILITY_FIELDS = [
  'location',
  'notes',
  'openingTimes',
  'openingHoursNotes',
  'operatorContactDetails',
] as const;

function renderFacility(value: Record<string, unknown>, depth: number): AccessibilityNode {
  const parts: LabelledNode[] = [];

  // `location` and `notes` are prose about this facility, so they carry no
  // label -- the availability line above them is the subject.
  pushUnlabelled(parts, 'location', value.location, depth);
  pushUnlabelled(parts, 'notes', value.notes, depth);
  pushLabelled(parts, 'Opening times', value.openingTimes, depth);
  pushUnlabelled(parts, 'openingHoursNotes', value.openingHoursNotes, depth);
  pushLabelled(parts, 'Contact', value.operatorContactDetails, depth);

  for (const [key, own] of Object.entries(value)) {
    if (key === 'available') continue;
    if ((FACILITY_FIELDS as readonly string[]).includes(key)) continue;
    pushField(parts, key, own, depth);
  }

  return { kind: 'facility', available: value.available === true, parts };
}

/** The unlabelled-prose slot of a facility record. A value that is not a
 * string is NOT dropped -- it falls through to the ordinary labelled
 * treatment instead. The fixed field lists in this module name fields whose
 * *usual* type has a bespoke rendering; a field arriving as some other type
 * must still reach the page, because silently losing a field is the failure
 * mode the design (§5, reason 2) calls the worst possible one for this
 * feature, and the whole point of keeping a terminal fallback. */
function pushUnlabelled(
  parts: LabelledNode[],
  key: string,
  value: unknown,
  depth: number,
): void {
  if (!hasRenderableValue(value)) return;
  if (typeof value !== 'string') {
    pushField(parts, key, value, depth);
    return;
  }
  const node = renderString(value);
  if (isEmptyNode(node)) return;
  parts.push({ node });
}

function pushLabelled(
  parts: LabelledNode[],
  label: string,
  value: unknown,
  depth: number,
): void {
  if (!hasRenderableValue(value)) return;
  const node = renderAt(value, depth + 1);
  if (isEmptyNode(node)) return;
  parts.push({ label, node });
}

// ---------------------------------------------------------------------------
// Pattern B -- opening times
// ---------------------------------------------------------------------------

const WEEK_ORDER = [
  'Monday',
  'Tuesday',
  'Wednesday',
  'Thursday',
  'Friday',
  'Saturday',
  'Sunday',
];

const WEEK_SHORT: Record<string, string> = {
  Monday: 'Mon',
  Tuesday: 'Tue',
  Wednesday: 'Wed',
  Thursday: 'Thu',
  Friday: 'Fri',
  Saturday: 'Sat',
  Sunday: 'Sun',
};

/** `Sunday,Saturday,...,Monday` -> `Mon-Sun`.
 *
 * Compaction runs over week order, never over array order: four entries in
 * the sample arrive out of order, one of them fully reversed (`BTN`), two
 * interleaved (`INV`) and one a simple swap (`CDF`), and compacting those
 * positionally produces nonsense (§4.3). Tokens that are not weekdays --
 * `Public Holidays` is the only one in the sample -- are emitted verbatim
 * after the ranges and never folded into one. */
export function formatDays(value: unknown): string {
  if (!Array.isArray(value)) return '';
  const tokens = value.filter((day): day is string => typeof day === 'string');
  const weekdays = WEEK_ORDER.filter((day) => tokens.includes(day));
  const others = [...new Set(tokens.filter((day) => !WEEK_ORDER.includes(day)))];

  const parts: string[] = [];
  let start = 0;
  while (start < weekdays.length) {
    let end = start;
    while (
      end + 1 < weekdays.length &&
      WEEK_ORDER.indexOf(weekdays[end + 1]) === WEEK_ORDER.indexOf(weekdays[end]) + 1
    ) {
      end += 1;
    }
    parts.push(
      end > start
        ? `${WEEK_SHORT[weekdays[start]]}–${WEEK_SHORT[weekdays[end]]}`
        : WEEK_SHORT[weekdays[start]],
    );
    start = end + 1;
  }
  return [...parts, ...others].join(', ');
}

/** `05:00:00.000` -> `05:00`. All 354 times in the sample are
 * `HH:MM:SS.mmm`; anything else is passed through rather than mangled,
 * since an unrecognised format is still more informative trimmed to
 * nothing. */
function formatTime(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const text = value.trim();
  if (text === '') return null;
  return /^\d{2}:\d{2}(:|$)/.test(text) ? text.slice(0, 5) : text;
}

function formatPeriods(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const periods: string[] = [];
  for (const period of value) {
    if (!isPlainObject(period)) continue;
    const start = formatTime(period.startTime);
    const end = formatTime(period.endTime);
    if (start && end) periods.push(`${start}–${end}`);
    else if (start) periods.push(`from ${start}`);
    else if (end) periods.push(`until ${end}`);
  }
  return periods;
}

/** The three `openingStatus` values are sample-derived, not documented
 * (§9.4), so this is deliberately not an exhaustive switch: an unrecognised
 * status prints verbatim next to its days, which is still readable and is
 * what stops a feed-side addition turning into a latent bug. */
export function formatHours(entry: Record<string, unknown>): string {
  const status = typeof entry.openingStatus === 'string' ? entry.openingStatus.trim() : '';
  const periods = formatPeriods(entry.openPeriod);

  if (status === '24 Hours') {
    // `LLE`'s two `staffHelp.openingTimes` entries say `24 Hours` AND carry
    // a real 06:10-12:40 period. That is the record contradicting itself;
    // showing both is honest, and picking one silently is not (§4.3).
    return periods.length > 0
      ? `24 hours (source also lists ${periods.join(', ')})`
      : '24 hours';
  }
  if (status === 'Unavailable') return 'closed';
  if (status === 'Specific Hours') {
    return periods.length > 0 ? periods.join(', ') : 'hours not published';
  }
  if (periods.length > 0) return status === '' ? periods.join(', ') : `${status} (${periods.join(', ')})`;
  return status;
}

/** `depth` is the array's own. Three more levels sit below it -- the
 * entry, its `openPeriod` array and each period object -- and the two
 * formatters above read all of them without going back through `renderAt`,
 * so this is where those levels are checked against the bound. */
function renderOpeningTimes(value: unknown[], depth: number): AccessibilityNode {
  if (depth + 3 > MAX_RENDER_DEPTH) return raw(value);
  const entries: OpeningTimesEntry[] = [];
  for (const entry of value) {
    if (!isPlainObject(entry)) continue;
    const days = formatDays(entry.daysOfTheWeek);
    const hours = formatHours(entry);
    if (days === '' && hours === '') continue;
    entries.push({ days, hours });
  }
  return { kind: 'openingTimes', entries };
}

// ---------------------------------------------------------------------------
// Pattern C -- contact details
// ---------------------------------------------------------------------------

/** Address lines are `-` placeholders at several car parks
 * (`{addressLine1: "-", addressLine2: "-"}` at `BHM`), which joined
 * verbatim reads as "-, -". A line that is nothing but dashes, dots or
 * spaces carries no address. */
function isPlaceholderLine(value: string): boolean {
  return /^[\s\-–—.,]*$/.test(value);
}

/** The address lines, in the order the feed declares them -- named
 * explicitly rather than taken from `Object.values`, so the rendered order
 * does not depend on JSON key order and a future sibling (a country, a
 * `what3words`) is not silently joined in as if it were an address line.
 * Anything not on this list falls through to `renderContact`'s labelled
 * leftovers loop. */
const POSTAL_ADDRESS_LINES = [
  'addressLine1',
  'addressLine2',
  'addressLine3',
  'addressLine4',
  'addressLine5',
  'postcode',
] as const;

function formatPostalAddress(value: Record<string, unknown>): string {
  return POSTAL_ADDRESS_LINES.map((key) => value[key])
    .filter((line): line is string => typeof line === 'string')
    .map((line) => line.trim())
    .filter((line) => line !== '' && !isPlaceholderLine(line))
    .join(', ');
}

/** A `tel:` href from display text like `0345 077 4224`. Spaces, brackets
 * and dashes are decoration; `+` is significant. Returns `null` when
 * nothing dialable is left, so the number still renders as plain text
 * rather than as a link to `tel:`. */
function telHref(value: string): string | null {
  const digits = value.replace(/[^\d+]/g, '');
  return /\d/.test(digits) ? `tel:${digits}` : null;
}

function renderContact(value: Record<string, unknown>, depth: number): AccessibilityNode {
  const fields: LabelledNode[] = [];

  // Same rule as `pushUnlabelled`: each bespoke slot below claims a field
  // only when that field has the type it renders. `unclaimed` collects the
  // rest, and the loop at the end pushes them through the ordinary labelled
  // branch, so nothing this list names can vanish by arriving as the wrong
  // type.
  //
  // `name` is claimed -- i.e. dropped (§4.4) -- but only when it is a
  // string, which is all the evidence for dropping it covers: "every `name`
  // either contains the word Details or is byte-identical to its own
  // `operatorName`" is a statement about 122 strings, and says nothing
  // about a `name` that arrives as something else.
  const claimed = new Set<string>();
  const claim = (key: string) => claimed.add(key);
  if (typeof value.name === 'string') claim('name');

  const phone = typeof value.primaryTelephoneNumber === 'string' ? value.primaryTelephoneNumber.trim() : '';
  if (typeof value.primaryTelephoneNumber === 'string') claim('primaryTelephoneNumber');
  if (phone !== '') {
    const href = telHref(phone);
    fields.push({
      label: 'Phone',
      node: href ? { kind: 'link', href, text: phone, external: false } : { kind: 'text', text: phone },
    });
  }

  const email = typeof value.emailAddress === 'string' ? value.emailAddress.trim() : '';
  if (typeof value.emailAddress === 'string') claim('emailAddress');
  if (email !== '') {
    fields.push({
      label: 'Email',
      node: email.includes('@')
        ? { kind: 'link', href: `mailto:${email}`, text: email, external: false }
        : { kind: 'text', text: email },
    });
  }

  const url = typeof value.url === 'string' ? value.url.trim() : '';
  if (typeof value.url === 'string') claim('url');
  if (url !== '') {
    fields.push({
      label: 'Website',
      node: /^https?:\/\//i.test(url)
        ? { kind: 'link', href: url, text: url, external: true }
        : { kind: 'text', text: url },
    });
  }

  if (isPlainObject(value.postalAddress)) {
    claim('postalAddress');
    const postalAddress = value.postalAddress;
    const address = formatPostalAddress(postalAddress);
    if (address !== '') fields.push({ label: 'Address', node: { kind: 'text', text: address } });
    // A sibling `POSTAL_ADDRESS_LINES` does not name -- or one it does name
    // that did not arrive as a string, and so was not joined into the line
    // above -- is shown on its own labelled row rather than dropped.
    // Joining only the known string lines would otherwise lose it
    // silently, which is the same failure this function's `claimed`
    // bookkeeping exists to prevent, one level down.
    for (const [key, own] of Object.entries(postalAddress)) {
      if ((POSTAL_ADDRESS_LINES as readonly string[]).includes(key) && typeof own === 'string') {
        continue;
      }
      pushField(fields, key, own, depth + 1);
    }
  }

  // Rich text, not plain: five `operatorName`s in the sample are a bare
  // `<a href>` (ScotRail's lost-property contact at ABD/DNO/INV, Transport
  // for Wales' at CDF/LLE) and one carries a literal `&` (§4.7).
  if (typeof value.operatorName === 'string') {
    claim('operatorName');
    const node = renderString(value.operatorName);
    if (!isEmptyNode(node)) fields.push({ label: 'Operator', node });
  }

  if (typeof value.note === 'string') {
    claim('note');
    const node = renderString(value.note);
    if (!isEmptyNode(node)) fields.push({ label: 'Note', node });
  }

  // Anything the feed adds to a contact record later -- and anything above
  // that arrived as an unexpected type -- still shows up, rather than being
  // silently dropped by a fixed field list.
  for (const [key, own] of Object.entries(value)) {
    if (claimed.has(key)) continue;
    pushField(fields, key, own, depth);
  }

  return { kind: 'contact', fields };
}

// ---------------------------------------------------------------------------
// Pattern D -- named-item collection
// ---------------------------------------------------------------------------

function renderCollection(value: unknown[], depth: number): AccessibilityNode {
  // The item objects are one level below the array and are read here rather
  // than through `renderAt`, so their level is checked here.
  if (depth + 1 > MAX_RENDER_DEPTH) return raw(value);
  const items: CollectionItem[] = [];
  for (const element of value) {
    if (!isPlainObject(element)) continue;
    items.push(renderCollectionItem(element, depth + 1));
  }
  return { kind: 'collection', items };
}

function renderCollectionItem(item: Record<string, unknown>, depth: number): CollectionItem {
  const name = String(item.name).trim();
  const siblings = Object.entries(item).filter(
    ([key, own]) => key !== 'name' && hasRenderableValue(own),
  );

  // §4.5's two "fit the bullet branch but read badly there" exceptions.
  // Both are single-sibling items whose sibling is an identifier rather
  // than a sentence, so the bullet branch would print a bare "SWA" or a
  // bare PDF URL under the name. Keyed on the field name because there is
  // nothing structural to key on -- a lone string sibling is exactly what
  // the bullet branch is for -- and the design names both explicitly.
  if (siblings.length === 1) {
    const [key, own] = siblings[0];
    if (key === 'crsCode' && typeof own === 'string' && own.trim() !== '') {
      const crs = own.trim();
      return {
        label: `${name} (${crs})`,
        link: { href: `/stations/${encodeURIComponent(crs)}`, external: false },
        body: { kind: 'bullets', items: [] },
      };
    }
    if (key === 'url' && typeof own === 'string' && /^https?:\/\//i.test(own.trim())) {
      return {
        label: name,
        link: { href: own.trim(), external: true },
        body: { kind: 'bullets', items: [] },
      };
    }
  }

  // The bullet branch: 7 of the 12 Pattern D paths (platforms, lift info,
  // toilet locations, taxi ranks, drop-off points, waiting rooms,
  // first-class lounges) carry nothing but strings, and every one of those
  // strings is already a complete English sentence -- "There is a Help
  // Point close to this platform" needs no "Help point close:" in front of
  // it (§4.5).
  const allStrings = siblings.every(([, own]) => typeof own === 'string');
  if (allStrings) {
    const bullets = siblings
      .map(([, own]) => renderString(own as string))
      .filter((node) => !isEmptyNode(node));
    return { label: name, body: { kind: 'bullets', items: bullets } };
  }

  // The structured branch: `carParks[]`, `carParks[].accessibleLocations[]`
  // and `passengerAssistance[]`. Everything in them -- the `charges` rate
  // object, the `operator` wrapper around a Pattern C record, a nested
  // `accessibilityInfo`, `passengerAssistance`'s boolean `available` --
  // comes out of the ordinary dispatcher one level down.
  return { label: name, body: renderFields(Object.fromEntries(siblings), depth) };
}

// ---------------------------------------------------------------------------
// Pattern F -- token list
// ---------------------------------------------------------------------------

/** `DepartureScreens` -> `Departure screens`, but `CCTV` -> `CCTV` and
 * `Yes - from help point` -> unchanged. `humanizeKey` lowercases
 * everything it is given, which is right for a camelCase identifier and
 * destructive for an acronym or an already-written label -- so it is
 * applied only to tokens that are a single alphabetic run containing a
 * lower-to-upper boundary, which is exactly what a camelCase identifier is
 * and what none of the sample's prose tokens are. */
function humanizeToken(value: unknown): string {
  const text = typeof value === 'string' ? value.trim() : String(value);
  return /^[A-Za-z]+$/.test(text) && /[a-z][A-Z]/.test(text) ? humanizeKey(text) : text;
}

// ---------------------------------------------------------------------------
// The fallback branch
// ---------------------------------------------------------------------------

/** §4.9's first change to the terminal branch: an unmatched plain object is
 * a labelled key/value list whose every value is recursed through the same
 * dispatcher -- NOT a raw JSON dump, and not a dump-the-whole-object-if-any
 * -value-is-nested bail either, which is what made the old renderer
 * degrade on 96.2% of key-renders.
 *
 * A field whose value renders to a Pattern E sentence loses its label
 * (§4.6); everything else keeps one. A field that renders to nothing at all
 * is dropped, for the same reason a `null` one is: a label over blank space
 * is worse than no row (§2.5, §4.10). */
function renderFields(value: Record<string, unknown>, depth: number): AccessibilityNode {
  const fields: LabelledNode[] = [];
  for (const [key, own] of Object.entries(value)) {
    pushField(fields, key, own, depth);
  }
  return { kind: 'fields', fields };
}

function pushField(fields: LabelledNode[], key: string, own: unknown, depth: number): void {
  if (!hasRenderableValue(own)) return;
  const node = renderAt(own, depth + 1);
  if (isEmptyNode(node)) return;
  if (node.kind === 'sentence') {
    if (CODE_LIKE_FIELDS.has(key)) {
      fields.push({ label: humanizeKey(key), node: { kind: 'text', text: node.text } });
      return;
    }
    fields.push({ node });
    return;
  }
  fields.push({ label: humanizeKey(key), node });
}

/** §4.9's last resort. `JSON.stringify` itself can throw (a cycle, a
 * `BigInt`) and can return `undefined` (for `undefined` and functions) --
 * neither is reachable from a `JSON.parse`d API response, but "never
 * throws" is the whole point of this module, so both are handled rather
 * than assumed away. */
function raw(value: unknown): AccessibilityNode {
  try {
    return { kind: 'raw', json: JSON.stringify(value, null, 2) ?? String(value) };
  } catch {
    return { kind: 'raw', json: String(value) };
  }
}

// ---------------------------------------------------------------------------
// Emptiness
// ---------------------------------------------------------------------------

/** True when a node would put nothing at all on the page. The section
 * component skips such a key rather than printing a label with blank space
 * under it, and treats a whole response of them as "nothing published" --
 * the same rule Decision 7 applies to category groups, applied all the way
 * down. Load-bearing given how pervasive `null` is (§2.5).
 *
 * Kept out of the render functions' return values deliberately, so a node
 * stays a faithful description of the value it was given and the decision
 * to hide is the display layer's. Terminates because the node tree is
 * finite -- `renderAt`'s depth bound guarantees that. */
export function isEmptyNode(node: AccessibilityNode): boolean {
  switch (node.kind) {
    case 'text':
    case 'sentence':
      return node.text.trim() === '';
    case 'richText':
      return isEmptyMarkup(node.html);
    case 'tokens':
      return node.tokens.every((token) => token.trim() === '');
    case 'facility':
      // Never empty: the availability line is content in itself, and
      // "Not available" is exactly the fact a reader came for.
      return false;
    case 'openingTimes':
      return node.entries.length === 0;
    case 'contact':
    case 'fields':
      return node.fields.every((field) => isEmptyNode(field.node));
    case 'collection':
      return node.items.every(
        (item) => item.label.trim() === '' && !item.link && isEmptyNode(item.body),
      );
    case 'bullets':
    case 'list':
      return node.items.every(isEmptyNode);
    case 'link':
      return node.text.trim() === '' && node.href.trim() === '';
    case 'raw':
      // The raw fallback must never be hidden: it is the only thing
      // standing between an unanticipated shape and silently dropped data.
      return node.json.trim() === '';
  }
}

/** `<p></p>` and `<p>&#160;</p>` really do occur in the feed (`BRI`'s and
 * `LDS`'s `dropOffPickUp.notes` both end with one) and put nothing on the
 * page. Markup
 * that carries a link is never empty even with no text, since the link
 * itself is the content.
 *
 * The three things stripped alongside the tags are the feed's own
 * invisible filler: the `&nbsp;`/`&#160;` entity (166 instances, §4.7's
 * inventory), a real U+00A0, and the stray U+200B zero-width space the
 * survey counted seven of (§2.6). */
function isEmptyMarkup(html: string): boolean {
  if (/<a\b/i.test(html)) return false;
  return (
    html
      .replace(/<[^>]*>/g, '')
      .replace(/&nbsp;|&#160;|&#xa0;/gi, '')
      .replace(/[\s ​]/g, '') === ''
  );
}
