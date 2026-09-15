import { describe, it, expect } from 'vitest';
import {
  ACCESSIBILITY_CATEGORIES,
  hasRenderableValue,
  humanizeKey,
  isEmptyRenderable,
  renderAccessibilityValue,
} from './stationAccessibility';

describe('humanizeKey', () => {
  it('splits camelCase into title-cased words', () => {
    expect(humanizeKey('stepFreeAccess')).toBe('Step free access');
    expect(humanizeKey('lifts')).toBe('Lifts');
    expect(humanizeKey('helpAndSupport')).toBe('Help and support');
  });

  it('splits an acronym run from the word that follows it', () => {
    expect(humanizeKey('hasWCFacilities')).toBe('Has wc facilities');
  });

  // Unverified upstream field names (design spec Correction 2) can be
  // anything at all; a wrong or ugly label is an acceptable degradation,
  // an exception is not.
  it('returns the key unchanged rather than throwing on a key with no word boundaries', () => {
    expect(humanizeKey('')).toBe('');
    expect(humanizeKey('_')).toBe('_');
  });
});

describe('hasRenderableValue', () => {
  it('treats null and undefined as absent, everything else as present', () => {
    expect(hasRenderableValue(null)).toBe(false);
    expect(hasRenderableValue(undefined)).toBe(false);
    expect(hasRenderableValue(false)).toBe(true);
    expect(hasRenderableValue(0)).toBe(true);
    expect(hasRenderableValue('')).toBe(true);
    expect(hasRenderableValue({})).toBe(true);
  });
});

describe('isEmptyRenderable', () => {
  it('flags the shapes that would put nothing on the page', () => {
    expect(isEmptyRenderable(renderAccessibilityValue({}))).toBe(true);
    expect(isEmptyRenderable(renderAccessibilityValue([]))).toBe(true);
    expect(isEmptyRenderable(renderAccessibilityValue({ notes: null }))).toBe(true);
    expect(isEmptyRenderable({ kind: 'text', text: '   ' })).toBe(true);
  });

  it('does not flag a value that genuinely renders something', () => {
    expect(isEmptyRenderable(renderAccessibilityValue(false))).toBe(false);
    expect(isEmptyRenderable(renderAccessibilityValue({ stepFree: true }))).toBe(false);
    expect(isEmptyRenderable(renderAccessibilityValue([{ a: 1 }]))).toBe(false);
    expect(isEmptyRenderable(renderAccessibilityValue({ a: { b: { c: 1 } } }))).toBe(false);
  });
});

describe('renderAccessibilityValue', () => {
  it('renders a string primitive as text', () => {
    expect(renderAccessibilityValue('Available 06:00-23:00')).toEqual({
      kind: 'text',
      text: 'Available 06:00-23:00',
    });
  });

  it('renders a number primitive as text', () => {
    expect(renderAccessibilityValue(2)).toEqual({ kind: 'text', text: '2' });
  });

  it('renders booleans as Yes/No, not "true"/"false"', () => {
    expect(renderAccessibilityValue(true)).toEqual({ kind: 'text', text: 'Yes' });
    expect(renderAccessibilityValue(false)).toEqual({ kind: 'text', text: 'No' });
  });

  it('renders an array of primitives as comma-joined text', () => {
    expect(renderAccessibilityValue(['Bus', 'Underground', 'Taxi'])).toEqual({
      kind: 'text',
      text: 'Bus, Underground, Taxi',
    });
  });

  it('renders an empty array as an items list of zero, not as empty text', () => {
    expect(renderAccessibilityValue([])).toEqual({ kind: 'items', count: 0, items: [] });
  });

  it('renders an array of objects as an items list, not inlined', () => {
    const result = renderAccessibilityValue([{ spaces: 120 }, { spaces: 40 }]);
    expect(result.kind).toBe('items');
    if (result.kind === 'items') {
      expect(result.count).toBe(2);
      expect(result.items).toHaveLength(2);
      expect(result.items[0]).toEqual({ kind: 'rows', rows: [{ label: 'Spaces', value: '120' }] });
    }
  });

  it('renders a mixed-type array as an items list too', () => {
    const result = renderAccessibilityValue([{ spaces: 120 }, 'overflow']);
    expect(result.kind).toBe('items');
    if (result.kind === 'items') {
      expect(result.count).toBe(2);
      expect(result.items[1]).toEqual({ kind: 'text', text: 'overflow' });
    }
  });

  it('renders a shallow plain object as one label/value row per own key, keys humanized', () => {
    expect(renderAccessibilityValue({ stepFree: true, notes: 'Ramp available' })).toEqual({
      kind: 'rows',
      rows: [
        { label: 'Step free', value: 'Yes' },
        { label: 'Notes', value: 'Ramp available' },
      ],
    });
  });

  it('renders a shallow object whose own value is an array of primitives as a joined row', () => {
    expect(renderAccessibilityValue({ operators: ['GWR', 'Avanti'] })).toEqual({
      kind: 'rows',
      rows: [{ label: 'Operators', value: 'GWR, Avanti' }],
    });
  });

  it('drops a null-valued own key inside a shallow object rather than rendering a blank row', () => {
    expect(renderAccessibilityValue({ stepFree: true, notes: null })).toEqual({
      kind: 'rows',
      rows: [{ label: 'Step free', value: 'Yes' }],
    });
  });

  it('falls back to raw JSON for an object nested more than one level deep, rather than throwing', () => {
    const deeplyNested = { level1: { level2: { level3: 'too deep' } } };
    const result = renderAccessibilityValue(deeplyNested);
    expect(result.kind).toBe('raw');
    if (result.kind === 'raw') {
      expect(JSON.parse(result.json)).toEqual(deeplyNested);
    }
  });

  it('falls back to raw JSON for the whole object when any one own value is a nested object', () => {
    // Partial rendering would silently drop `nested` from the page; the
    // whole value degrades together so nothing goes missing unannounced.
    const mixed = { stepFree: true, nested: { a: 1 } };
    const result = renderAccessibilityValue(mixed);
    expect(result.kind).toBe('raw');
    if (result.kind === 'raw') expect(JSON.parse(result.json)).toEqual(mixed);
  });

  it('falls back to raw JSON for a deliberately malformed array-of-arrays-of-objects shape, never throwing', () => {
    const malformed = [[{ a: 1 }], [{ b: 2 }]];
    expect(() => renderAccessibilityValue(malformed)).not.toThrow();
    const result = renderAccessibilityValue(malformed);
    // Each element of the outer array is itself an array, which is one
    // level deeper than the single extra level of recursion the renderer
    // allows -- so each item degrades to raw JSON rather than recursing
    // without bound or throwing.
    expect(result.kind).toBe('items');
    if (result.kind === 'items') {
      expect(result.items[0].kind).toBe('raw');
      if (result.items[0].kind === 'raw') expect(JSON.parse(result.items[0].json)).toEqual([{ a: 1 }]);
    }
  });

  it('falls back to raw JSON for an object nested inside an array item, one level past the limit', () => {
    const result = renderAccessibilityValue([{ tariff: { daily: '£5' } }]);
    expect(result.kind).toBe('items');
    if (result.kind === 'items') expect(result.items[0].kind).toBe('raw');
  });

  it('never throws and never recurses without bound on a pathological input', () => {
    // A self-referential object cannot come out of JSON.parse, but the
    // "never crashes on unknown shape" claim (design spec Correction 2 /
    // Decision 6) is absolute -- JSON.stringify throws on a cycle, so the
    // raw fallback has to survive that too.
    const cyclic: Record<string, unknown> = { self: null };
    cyclic.self = cyclic;
    expect(() => renderAccessibilityValue(cyclic)).not.toThrow();
    expect(renderAccessibilityValue(cyclic).kind).toBe('raw');
  });

  it('degrades null and undefined to raw rather than throwing, even though callers should skip them first', () => {
    expect(() => renderAccessibilityValue(null)).not.toThrow();
    expect(() => renderAccessibilityValue(undefined)).not.toThrow();
    expect(renderAccessibilityValue(null).kind).toBe('raw');
    expect(renderAccessibilityValue(undefined).kind).toBe('raw');
  });
});

describe('ACCESSIBILITY_CATEGORIES', () => {
  it('covers exactly the twelve allowlisted keys, once each, in the spec-defined group order', () => {
    const allKeys = ACCESSIBILITY_CATEGORIES.flatMap((c) => c.keys);
    expect(allKeys).toEqual([
      'stationAccessibility',
      'staffAssistance',
      'toiletsAndChanging',
      'lifts',
      'loungesAndWaiting',
      'platformFacilities',
      'stationFacilities',
      'helpAndSupport',
      'transportLinks',
      'carParks',
      'dropOffPickUp',
      'cycling',
    ]);
    expect(ACCESSIBILITY_CATEGORIES.map((c) => c.heading)).toEqual([
      'Step-free access & assistance',
      'Facilities',
      'Platform & station facilities',
      'Getting here',
    ]);
  });

  // The backend's own ACCESSIBILITY_KEYS const
  // (crates/api/src/data/reference.rs) is asserted against the same twelve
  // names in its own Rust test. This is the frontend half of that pairing:
  // a key added to the wire allowlist but not to a category here would be
  // fetched and then silently never rendered.
  it('matches the backend allowlist as a set, so no forwarded key goes unrendered', () => {
    const backendAllowlist = [
      'stationAccessibility',
      'staffAssistance',
      'toiletsAndChanging',
      'lifts',
      'transportLinks',
      'cycling',
      'carParks',
      'dropOffPickUp',
      'platformFacilities',
      'stationFacilities',
      'helpAndSupport',
      'loungesAndWaiting',
    ];
    expect([...ACCESSIBILITY_CATEGORIES.flatMap((c) => c.keys)].sort()).toEqual(
      [...backendAllowlist].sort(),
    );
  });
});
