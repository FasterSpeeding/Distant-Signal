import { describe, expect, it } from 'vitest';
import { buildSitemap, isExcludedPath, LEGAL_PATHS, SITEMAP_URL_LIMIT, sitemapPaths, STATIC_PATHS } from './sitemap';
import type { LineSummary } from './types';

const line = (id: string, source: LineSummary['source']): LineSummary => ({
  id,
  name: id,
  category: 'x',
  operators: [],
  source,
});

const SOURCES = {
  stationCodes: ['EUS', 'kgx'],
  tflStopIds: ['940GZZLUEUS'],
  lines: [line('wcml-north', 'catalogue'), line('tfl-northern', 'tfl'), line('custom-mine', 'custom')],
  transit: [{ id: 'metrolink', lineIds: ['blue'], stopIds: ['9400ZZMAPIC'] }],
  legalPublished: true,
};

describe('sitemapPaths', () => {
  const paths = sitemapPaths(SOURCES);

  it('lists the static pages, the transit index and the published legal pages', () => {
    for (const path of [...STATIC_PATHS, '/transit', ...LEGAL_PATHS]) expect(paths).toContain(path);
  });

  it('lists the Trains section, but never a search in it', () => {
    expect(paths).toContain('/trains');
    expect(isExcludedPath('/trains?station=RDG')).toBe(true);
  });

  it('lists every line, with a timetable only for a National Rail catalogue line', () => {
    expect(paths).toContain('/lines/wcml-north');
    expect(paths).toContain('/lines/wcml-north/timetable');
    expect(paths).toContain('/lines/tfl-northern');
    expect(paths).not.toContain('/lines/tfl-northern/timetable');
    expect(paths.some((path) => path.includes('custom-mine'))).toBe(false);
  });

  it('lists every station by its upper-case code, and every TfL stop', () => {
    expect(paths).toContain('/stations/EUS');
    expect(paths).toContain('/stations/KGX');
    expect(paths).not.toContain('/stations/kgx');
    expect(paths).toContain('/tfl/stops/940GZZLUEUS');
  });

  it("lists each served tram network's lines, timetables and stops", () => {
    expect(paths).toEqual(
      expect.arrayContaining([
        '/transit/metrolink',
        '/transit/metrolink/lines/blue',
        '/transit/metrolink/lines/blue/timetable',
        '/transit/metrolink/stops/9400ZZMAPIC',
      ]),
    );
  });

  it('leaves out the transit index and unpublished legal pages', () => {
    const bare = sitemapPaths({ ...SOURCES, transit: [], legalPublished: false });
    expect(bare).not.toContain('/transit');
    for (const path of LEGAL_PATHS) expect(bare).not.toContain(path);
  });

  it('never lists a train, an incident, a history page or a query string', () => {
    for (const path of paths) expect(isExcludedPath(path)).toBe(false);
    expect(paths.some((path) => path.startsWith('/train/'))).toBe(false);
    expect(paths.some((path) => /^\/incidents\/./.test(path))).toBe(false);
    expect(paths.some((path) => path.includes('/history'))).toBe(false);
  });

  it('lists each path once', () => {
    const twice = sitemapPaths({ ...SOURCES, stationCodes: ['EUS', 'eus'] });
    expect(twice.filter((path) => path === '/stations/EUS')).toHaveLength(1);
  });
});

describe('isExcludedPath', () => {
  it.each([
    '/train/C12345/2026-10-24',
    '/incidents/abc',
    '/lines/wcml-north/history',
    '/operators/VT/history',
    '/network/history',
    '/stations?q=x',
  ])('excludes %s', (path) => {
    expect(isExcludedPath(path)).toBe(true);
  });

  it.each(['/incidents', '/lines/wcml-north', '/trains', '/stations/EUS'])('keeps %s', (path) => {
    expect(isExcludedPath(path)).toBe(false);
  });
});

describe('buildSitemap', () => {
  it('makes every URL absolute and sets no lastModified', () => {
    const entries = buildSitemap({ ...SOURCES, origin: 'https://ds.example' });
    expect(entries[0]).toEqual({ url: 'https://ds.example/' });
    expect(entries.every((entry) => entry.url.startsWith('https://ds.example/'))).toBe(true);
    expect(entries.some((entry) => 'lastModified' in entry)).toBe(false);
  });

  it('stays within one file', () => {
    const stationCodes = Array.from({ length: SITEMAP_URL_LIMIT + 10 }, (_, i) => `S${i}`);
    expect(buildSitemap({ ...SOURCES, stationCodes, origin: 'https://ds.example' })).toHaveLength(SITEMAP_URL_LIMIT);
  });
});
