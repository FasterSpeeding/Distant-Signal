import { describe, it, expect } from 'vitest';
import {
  ACCESSIBILITY_CATEGORIES,
  hasRenderableValue,
  isEmptyNode,
  MAX_RENDER_DEPTH,
  renderAccessibilityValue,
  type AccessibilityNode,
} from './stationAccessibility';
import {
  ACCESSIBILITY_FIXTURE_CRS,
  loadAccessibilityFixture,
  loadAllAccessibilityFixtures,
} from '@/test/fixtures/accessibility';
import type { StationAccessibilityData } from './types';

/** The survey the design is built on, run as a test suite.
 *
 * Every claim here is checkable against
 * `frontend/test/fixtures/accessibility/` -- 31 real station payloads
 * captured from production on 2026-09-16 (see that directory's loader for
 * provenance). This is the first time this feature has been tested against
 * real RDM data rather than invented values, which is §8's first bullet. */

const ALL_KEYS = ACCESSIBILITY_CATEGORIES.flatMap((category) => category.keys);

function nodesFor(data: StationAccessibilityData): AccessibilityNode[] {
  return ALL_KEYS.filter((key) => hasRenderableValue(data[key])).map((key) =>
    renderAccessibilityValue(data[key]),
  );
}

/** Every node in a rendered tree, the root included. */
function walk(node: AccessibilityNode): AccessibilityNode[] {
  const found: AccessibilityNode[] = [node];
  switch (node.kind) {
    case 'facility':
      node.parts.forEach((part) => found.push(...walk(part.node)));
      break;
    case 'contact':
    case 'fields':
      node.fields.forEach((field) => found.push(...walk(field.node)));
      break;
    case 'collection':
      node.items.forEach((item) => found.push(...walk(item.body)));
      break;
    case 'bullets':
    case 'list':
      node.items.forEach((item) => found.push(...walk(item)));
      break;
    default:
      break;
  }
  return found;
}

function allNodes(data: StationAccessibilityData): AccessibilityNode[] {
  return nodesFor(data).flatMap(walk);
}

/** Resolves a dotted path (`carParks.carParks[].openingHours`) against a
 * payload, returning every value it reaches. */
function valuesAt(value: unknown, segments: string[]): unknown[] {
  if (segments.length === 0) return [value];
  const [head, ...rest] = segments;
  const key = head.endsWith('[]') ? head.slice(0, -2) : head;
  const isArrayStep = head.endsWith('[]');
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return [];
  const next = (value as Record<string, unknown>)[key];
  if (next === null || next === undefined) return [];
  if (!isArrayStep) return valuesAt(next, rest);
  if (!Array.isArray(next)) return [];
  return next.flatMap((element) => valuesAt(element, rest));
}

function everyValueAt(path: string): { crs: string; value: unknown }[] {
  const segments = path.split('.');
  return loadAllAccessibilityFixtures().flatMap(({ crs, data }) =>
    valuesAt(data, segments).map((value) => ({ crs, value })),
  );
}

describe('the 31-station fixture set', () => {
  it('is the sample the design surveyed', () => {
    expect(ACCESSIBILITY_FIXTURE_CRS).toEqual([
      'ABD', 'BAL', 'BHM', 'BRI', 'BSK', 'BTN', 'CAR', 'CBG', 'CDF', 'DNO', 'EDB', 'EUS',
      'EXD', 'GLQ', 'HUL', 'INV', 'IPS', 'KGX', 'LDS', 'LLE', 'MAN', 'NRW', 'PMH', 'PNZ',
      'SHF', 'SKG', 'SOU', 'STP', 'TWY', 'WVH', 'YRK',
    ]);
  });

  it('carries eleven or twelve allowlisted keys per station, and nothing outside the allowlist', () => {
    for (const { crs, data } of loadAllAccessibilityFixtures()) {
      const keys = Object.keys(data);
      expect(keys.length, crs).toBeGreaterThanOrEqual(11);
      // `dropOffPickUp` is absent at 6/31 (§1.3) -- the only root-level
      // variation in the whole sample.
      expect(keys.filter((key) => !(ALL_KEYS as string[]).includes(key)), crs).toEqual([]);
    }
  });
});

/** The direct, checkable inverse of §2.1's central finding: the renderer
 * this replaces produced a collapsed "Raw data" block for 352 of 366
 * key-renders. */
describe('the raw fallback', () => {
  it('fires on nothing at all across 366 real key-renders', () => {
    const offenders: string[] = [];
    for (const { crs, data } of loadAllAccessibilityFixtures()) {
      for (const key of ALL_KEYS) {
        if (!hasRenderableValue(data[key])) continue;
        const raws = walk(renderAccessibilityValue(data[key])).filter(
          (node) => node.kind === 'raw',
        );
        if (raws.length > 0) offenders.push(`${crs}.${key}`);
      }
    }
    expect(offenders).toEqual([]);
  });

  it('is still reached by a shape none of the seven patterns describe', () => {
    // The guarantee the fallback exists for has not been deleted along with
    // the 96.2% -- see §4.9 on why a renderer that blanked instead would be
    // the worse regression.
    expect(renderAccessibilityValue(new Date()).kind).toBe('raw');
  });

  it('renders every station to something, never to a blank section', () => {
    for (const { crs, data } of loadAllAccessibilityFixtures()) {
      const rendered = nodesFor(data).filter((node) => !isEmptyNode(node));
      expect(rendered.length, crs).toBeGreaterThan(0);
    }
  });

  it('never throws on any real payload', () => {
    for (const { crs, data } of loadAllAccessibilityFixtures()) {
      expect(() => nodesFor(data), crs).not.toThrow();
    }
  });
});

describe('the depth bound, measured from the fixtures', () => {
  /** Container depth, counted exactly as §4.9 does: the key's own value is
   * 0, and every container level below it -- objects included -- adds one. */
  function deepest(value: unknown, depth: number): number {
    if (typeof value !== 'object' || value === null) return depth - 1;
    const children = Array.isArray(value) ? value : Object.values(value);
    return children.reduce<number>(
      (worst, child) => Math.max(worst, deepest(child, depth + 1)),
      depth,
    );
  }

  it('leaves exactly one level of margin over the deepest real chain', () => {
    let observed = -1;
    for (const { data } of loadAllAccessibilityFixtures()) {
      for (const key of ALL_KEYS) {
        if (!hasRenderableValue(data[key])) continue;
        observed = Math.max(observed, deepest(data[key], 0));
      }
    }
    // Seven containers -- `carParks` -> `carParks[]` -> item ->
    // `openingHours[]` -> entry -> `openPeriod[]` -> `{startTime, endTime}`
    // -- occupying depths 0 through 6.
    expect(observed).toBe(6);
    expect(MAX_RENDER_DEPTH).toBe(observed + 1);
  });

  it('reaches the innermost value of that chain -- a car park opening period', () => {
    const edb = loadAccessibilityFixture('EDB');
    const rendered = JSON.stringify(walk(renderAccessibilityValue(edb.carParks)));
    expect(rendered).toMatch(/\d\d:\d\d–\d\d:\d\d/);
  });

  it('asks the bound about the levels its pattern renderers consume, not just the ones it walks', () => {
    // Worth stating plainly, because it is the thing that makes the number
    // above mean what the design says it means. Pattern B is handed the
    // `openingHours` array and reads three more levels out of it without
    // re-entering the dispatcher, so if it did not check the bound itself,
    // `MAX_RENDER_DEPTH` would silently be measuring a shallower quantity
    // -- and this fixture set would render identically with a bound of 5.
    //
    // The off-by-one itself is caught by the synthetic 8-vs-9-container
    // case in `stationAccessibility.test.ts`; no real payload can catch it,
    // precisely because none of them comes near the bound.
    let deepestOpeningTimes = -1;
    const findOpeningTimes = (value: unknown, depth: number) => {
      if (Array.isArray(value)) {
        // `every`, matching `isOpeningTimes`'s own predicate: an array
        // where only some elements look like entries is not a Pattern B
        // array and must not be counted as one.
        if (value.length > 0 && value.every((entry) => isOpeningTimesEntry(entry))) {
          deepestOpeningTimes = Math.max(deepestOpeningTimes, depth);
        }
        value.forEach((element) => findOpeningTimes(element, depth + 1));
      } else if (typeof value === 'object' && value !== null) {
        Object.values(value).forEach((child) => findOpeningTimes(child, depth + 1));
      }
    };
    for (const { data } of loadAllAccessibilityFixtures()) {
      for (const key of ALL_KEYS) {
        if (hasRenderableValue(data[key])) findOpeningTimes(data[key], 0);
      }
    }
    // `carParks` -> `carParks[]` -> item -> `openingHours`.
    expect(deepestOpeningTimes).toBe(3);
    // Plus the entry, the `openPeriod` array and the period object it
    // consumes = container depth 6, the observed maximum, inside the bound
    // with one level to spare.
    expect(deepestOpeningTimes + 3).toBeLessThan(MAX_RENDER_DEPTH);
  });
});

function isOpeningTimesEntry(value: unknown): boolean {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    'daysOfTheWeek' in value &&
    'openingStatus' in value
  );
}

describe('real payloads land on the pattern the survey says they do', () => {
  /** The kinds a path's values render to, skipping the ones that render to
   * nothing. An empty `[]` -- `lifts.liftsInfo` at the 8 stations with no
   * lifts, `carParks.carParks` at the 6 with no car park -- is the "there
   * is nothing here" case the section drops entirely (§2.5), not a pattern
   * this table is making a claim about. */
  function kindsAt(path: string): Set<string> {
    return new Set(
      everyValueAt(path)
        .map(({ value }) => renderAccessibilityValue(value))
        .filter((node) => !isEmptyNode(node))
        .map((node) => node.kind),
    );
  }

  it.each([
    // Pattern A -- 396 exact matches plus the narrow variants.
    ['stationFacilities.wifi', 'facility'],
    ['stationAccessibility.trainRamp', 'facility'],
    ['stationAccessibility.ticketBarriers', 'facility'],
    ['toiletsAndChanging.toilets', 'facility'],
    ['loungesAndWaiting.waitingFacility', 'facility'],
    ['transportLinks.bus', 'facility'],
    ['transportLinks.taxi', 'facility'],
    // Three container keys that satisfy A's predicate and are meant to
    // (§4.8's closing note), not a misclassification.
    ['lifts', 'facility'],
    ['dropOffPickUp', 'facility'],
    ['helpAndSupport.helpPoints', 'facility'],
    // Pattern B -- including the site the feed spells `openingHours`.
    ['staffAssistance.staffHelp.openingTimes', 'openingTimes'],
    ['carParks.carParks[].openingHours', 'openingTimes'],
    // Pattern C.
    ['carParks.carParks[].operator.contactDetails', 'contact'],
    ['stationFacilities.lostProperty.operatorContactDetails', 'contact'],
    // Pattern D -- both branches.
    ['platformFacilities.platforms', 'collection'],
    ['lifts.liftsInfo', 'collection'],
    ['toiletsAndChanging.toilets.locations', 'collection'],
    ['transportLinks.taxi.taxiRanks', 'collection'],
    ['dropOffPickUp.points', 'collection'],
    ['loungesAndWaiting.waitingRooms', 'collection'],
    ['loungesAndWaiting.firstClassLounges', 'collection'],
    ['stationAccessibility.passengerAssistance', 'collection'],
    ['stationAccessibility.nearestAccessibleStations.stations', 'collection'],
    ['transportLinks.replacementBus.maps', 'collection'],
    ['carParks.carParks', 'collection'],
    ['carParks.carParks[].accessibleLocations', 'collection'],
    // Pattern E.
    ['stationAccessibility.tactilePaving', 'sentence'],
    ['platformFacilities.entranceLevels', 'sentence'],
    ['lifts.statement', 'sentence'],
    // Pattern F.
    ['staffAssistance.informationAvailableFromStaff', 'tokens'],
    ['cycling.typesOfStorage', 'tokens'],
    ['helpAndSupport.customerInformationScreens', 'tokens'],
    ['stationAccessibility.ticketBarriers.names', 'tokens'],
    // §4.9's genuinely unhandled residue -- the labelled key/value branch.
    ['cycling.spaces', 'fields'],
    ['stationAccessibility.inductionLoop', 'fields'],
    ['stationAccessibility.stepFreeCategory', 'fields'],
    ['stationAccessibility.nearestAccessibleStations', 'fields'],
    ['carParks.carParks[].charges', 'fields'],
  ])('%s renders as %s at every station that has it', (path, kind) => {
    const kinds = kindsAt(path);
    expect(kinds.size, `${path} produced ${[...kinds].join(', ')}`).toBeGreaterThan(0);
    expect([...kinds]).toEqual([kind]);
  });

  it('routes all twelve top-level keys to a pattern, never to the dump', () => {
    const kinds = new Set<string>();
    for (const { data } of loadAllAccessibilityFixtures()) {
      for (const key of ALL_KEYS) {
        if (!hasRenderableValue(data[key])) continue;
        kinds.add(renderAccessibilityValue(data[key]).kind);
      }
    }
    // `facility` (lifts, dropOffPickUp) and `fields` (the other ten
    // containers) are the only two shapes a top-level key takes.
    expect([...kinds].sort()).toEqual(['facility', 'fields']);
  });
});

describe('Pattern B, over every real entry', () => {
  it('never emits an unrecognised day token or an untrimmed time', () => {
    const allowed = new Set([
      'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun', 'Public Holidays',
    ]);
    let entries = 0;
    for (const { crs, data } of loadAllAccessibilityFixtures()) {
      for (const node of allNodes(data)) {
        if (node.kind !== 'openingTimes') continue;
        for (const entry of node.entries) {
          entries += 1;
          for (const token of entry.days.split(', ')) {
            for (const day of token.split('–')) {
              expect(allowed.has(day), `${crs}: ${entry.days}`).toBe(true);
            }
          }
          // `HH:MM:SS.mmm` must never reach the page.
          expect(entry.hours, crs).not.toMatch(/\d\d:\d\d:\d\d/);
        }
      }
    }
    // §2.3 counted 304 Pattern B entries across the sample.
    expect(entries).toBe(304);
  });

  it('finds LLE\'s self-contradicting 24-hour entries and shows both facts', () => {
    const lle = loadAccessibilityFixture('LLE');
    const conflicting = allNodes(lle).flatMap((node) =>
      node.kind === 'openingTimes'
        ? node.entries.filter((entry) => entry.hours.includes('source also lists'))
        : [],
    );
    expect(conflicting).toHaveLength(2);
    expect(conflicting[0].hours).toBe('24 hours (source also lists 06:10–12:40)');
  });
});

describe('sanitization, over every real string', () => {
  it('leaves no executable markup anywhere in any rendered tree', () => {
    for (const { crs, data } of loadAllAccessibilityFixtures()) {
      for (const node of allNodes(data)) {
        if (node.kind !== 'richText') continue;
        expect(node.html, crs).not.toMatch(/<script/i);
        expect(node.html, crs).not.toMatch(/\son[a-z]+\s*=/i);
        expect(node.html, crs).not.toMatch(/javascript:/i);
        // §4.7: a heading inside a note must not reach the page outline.
        expect(node.html, crs).not.toMatch(/<h[1-6]\b/i);
      }
    }
  });

  it('keeps the feed\'s links, including the 45 plain-http ones', () => {
    let https = 0;
    let http = 0;
    let mailto = 0;
    for (const { data } of loadAllAccessibilityFixtures()) {
      for (const node of allNodes(data)) {
        if (node.kind !== 'richText') continue;
        https += (node.html.match(/href="https:/g) ?? []).length;
        http += (node.html.match(/href="http:/g) ?? []).length;
        mailto += (node.html.match(/href="mailto:/g) ?? []).length;
      }
    }
    expect(https).toBeGreaterThan(100);
    expect(http).toBeGreaterThan(30);
    expect(mailto).toBeGreaterThan(0);
  });

  it('demotes MAN\'s three note-level h2s rather than dropping their emphasis', () => {
    const man = loadAccessibilityFixture('MAN');
    const demoted = allNodes(man).filter(
      (node) => node.kind === 'richText' && node.html.includes('<p><strong>'),
    );
    expect(demoted.length).toBeGreaterThan(0);
  });
});
