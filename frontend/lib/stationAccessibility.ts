import type { StationAccessibilityData } from './types';

/** Fixed display order and grouping for the twelve allowlisted keys -- see
 * docs/superpowers/specs/2026-09-12-station-accessibility-design.md
 * Decision 7. A station page reader scans top-to-bottom for the thing they
 * care about, most-asked-about first; this is deliberately not alphabetical
 * and not the response's own key order. `StationAccessibilitySection` skips
 * a whole group when none of its keys carry a value. Every key the backend
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
 * boundaries, lowercases every word, then capitalizes only the first --
 * deliberately no hardcoded per-field dictionary, since the field set is
 * unverified against a real payload (design spec Correction 2). A wrong or
 * ugly label from an unanticipated key is an acceptable, non-crashing
 * degradation (Decision 6). */
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
 * a hard guarantee -- defense in depth against a payload shape this
 * codebase has never verified. */
export function hasRenderableValue(value: unknown): boolean {
  return value !== null && value !== undefined;
}

/** True when a rendered value would put nothing at all on the page -- an
 * empty object, an empty array, an object whose every own value was null,
 * or an array of any of those. The section component skips such a key
 * rather than printing a label with blank space under it, and treats a
 * whole response of them as "nothing published": the same "don't invent a
 * row for data that isn't there" rule Decision 7 applies to category
 * groups, applied one and two levels down.
 *
 * Kept out of `renderAccessibilityValue` deliberately, so that function
 * stays a faithful description of the value it was given and the decision
 * to hide is the display layer's. Recursion terminates because
 * `renderAccessibilityValue`'s depth limit means an `'items'` entry's
 * children are never themselves `'items'`. */
export function isEmptyRenderable(value: RenderableValue): boolean {
  switch (value.kind) {
    case 'text':
      return value.text.trim() === '';
    case 'rows':
      return value.rows.every((row) => row.value.trim() === '');
    case 'items':
      return value.items.every(isEmptyRenderable);
    case 'raw':
      return value.json.trim() === '';
  }
}

export type RenderableValue =
  | { kind: 'text'; text: string }
  | { kind: 'rows'; rows: { label: string; value: string }[] }
  | { kind: 'items'; count: number; items: RenderableValue[] }
  | { kind: 'raw'; json: string };

type Primitive = string | number | boolean;

function isPrimitive(value: unknown): value is Primitive {
  return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function primitiveText(value: Primitive): string {
  if (typeof value === 'boolean') return value ? 'Yes' : 'No';
  return String(value);
}

/** The terminal branch every other rule falls through to. `JSON.stringify`
 * itself can throw (a cycle, a `BigInt`) and can return `undefined` (for
 * `undefined` and functions) -- neither is reachable from a `JSON.parse`d
 * API response, but "never throws" is the whole point of this module
 * (design spec Correction 2), so both are handled rather than assumed
 * away. */
function raw(value: unknown): RenderableValue {
  try {
    return { kind: 'raw', json: JSON.stringify(value, null, 2) ?? String(value) };
  } catch {
    return { kind: 'raw', json: String(value) };
  }
}

/** Renders a plain object as one label/value row per own key -- the
 * "shallow object" branch. Any own value that is itself an object, or an
 * array containing anything but primitives, is deeper than this branch
 * covers: the **whole** object then degrades to raw JSON rather than
 * rendering the shallow half and silently dropping the rest.
 *
 * An own value that renders to no text at all (`''`, `[]`) is dropped for
 * the same reason a `null` one is: a label followed by blank space is
 * worse than no row. */
function renderShallowObject(value: Record<string, unknown>): RenderableValue {
  const rows: { label: string; value: string }[] = [];
  for (const [key, own] of Object.entries(value)) {
    if (!hasRenderableValue(own)) continue;
    let text: string;
    if (isPrimitive(own)) {
      text = primitiveText(own);
    } else if (Array.isArray(own) && own.every(isPrimitive)) {
      text = own.map(primitiveText).join(', ');
    } else {
      return raw(value);
    }
    if (text.trim() === '') continue;
    rows.push({ label: humanizeKey(key), value: text });
  }
  return { kind: 'rows', rows };
}

/** The one place this feature decides how to display a value of genuinely
 * unknown shape -- see design spec Decision 6. Never throws: every branch
 * either produces a renderable result or falls through to the raw-JSON
 * fallback.
 *
 * Depth-limited, in the literal sense the spec's "each item independently
 * recursed one level" asks for: `depth` 0 is a whole allowlisted key's
 * value, `depth` 1 is one element of an array of non-primitives, and there
 * is no `depth` 2 -- an array element that is itself an array, or an object
 * with a nested object inside it, degrades to raw JSON instead of
 * recursing further. That bound is what makes "never produces a wall of
 * text" and "terminates on any input" true by construction rather than by
 * trusting the upstream payload's depth.
 *
 * Takes no `depth` argument of its own: `renderAt` below carries it, so a
 * caller writing `array.map(renderAccessibilityValue)` cannot accidentally
 * pass the array index as a depth. */
export function renderAccessibilityValue(value: unknown): RenderableValue {
  return renderAt(value, 0);
}

function renderAt(value: unknown, depth: number): RenderableValue {
  if (isPrimitive(value)) {
    return { kind: 'text', text: primitiveText(value) };
  }

  if (Array.isArray(value)) {
    // At depth 1 an array is one level past the limit -- an array of
    // arrays is exactly the malformed shape the raw fallback exists for.
    if (depth > 0) return raw(value);
    // Checked before the all-primitives test, which is vacuously true for
    // `[]` and would otherwise render an empty list as empty text.
    if (value.length === 0) return { kind: 'items', count: 0, items: [] };
    if (value.every(isPrimitive)) {
      return { kind: 'text', text: value.map(primitiveText).join(', ') };
    }
    return {
      kind: 'items',
      count: value.length,
      items: value.map((item) => renderAt(item, depth + 1)),
    };
  }

  if (isPlainObject(value)) {
    return renderShallowObject(value);
  }

  return raw(value);
}
