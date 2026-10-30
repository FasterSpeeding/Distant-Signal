import { describe, expect, it } from 'vitest';
import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { apiErrorStatusFor, isDigest, ROUTE_TEMPLATES, routeTemplateFor } from './usageEvents';

/** Every `page.tsx` under `app/` as a route template (`(home)` adds no
 * segment). */
function pageTemplates(dir: string, prefix: string, out: Set<string>) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (prefix === '' && entry.name === 'api') continue;
      const next = entry.name.startsWith('(') ? prefix : `${prefix}/${entry.name}`;
      pageTemplates(join(dir, entry.name), next, out);
    } else if (entry.name === 'page.tsx') {
      out.add(prefix === '' ? '/' : prefix);
    }
  }
}

describe('ROUTE_TEMPLATES', () => {
  it("lists exactly the app's pages, plus /offline and /_not-found", () => {
    const pages = new Set<string>(['/offline', '/_not-found']);
    pageTemplates('app', '', pages);
    expect([...ROUTE_TEMPLATES].sort()).toEqual([...pages].sort());
  });
});

describe('routeTemplateFor', () => {
  it.each([
    ['/', '/'],
    ['/lines', '/lines'],
    ['/lines/northern', '/lines/[id]'],
    ['/lines/northern/', '/lines/[id]'],
    ['/lines/northern/history', '/lines/[id]/history'],
    ['/lines/new', '/lines/new'],
    ['/trains', '/trains'],
    ['/trains?station=RDG&board=arrivals', '/trains'],
    ['/journeys/42', '/journeys/[id]'],
    ['/journeys/templates', '/journeys/templates'],
    ['/journeys/templates/7', '/journeys/templates/[id]'],
    ['/journeys/shared/abc', '/journeys/shared/[token]'],
    ['/train/by-id/12', '/train/by-id/[trackingId]'],
    ['/train/C12345/2026-10-30', '/train/[uid]/[date]'],
    ['/groups/join/tok', '/groups/join/[token]'],
    ['/groups/new', '/groups/new'],
    ['/stations/KGX?tab=arrivals', '/stations/[crs]'],
    ['/transit/metrolink/lines/blue/timetable', '/transit/[network]/lines/[line]/timetable'],
    ['/no/such/page', '/_not-found'],
    ['/offline', '/_not-found'],
  ])('%s is %s', (pathname, template) => {
    expect(routeTemplateFor(pathname)).toBe(template);
  });

  it('never returns the pathname itself for a real id', () => {
    expect(ROUTE_TEMPLATES).toContain(routeTemplateFor('/stations/KGX'));
    expect(routeTemplateFor('/stations/KGX')).not.toContain('KGX');
  });
});

describe('apiErrorStatusFor', () => {
  it('classes a status', () => {
    expect(apiErrorStatusFor(undefined)).toBe('network');
    expect(apiErrorStatusFor(404)).toBe('4xx');
    expect(apiErrorStatusFor(503)).toBe('5xx');
    expect(apiErrorStatusFor(200)).toBeNull();
    expect(apiErrorStatusFor(0)).toBeNull();
  });
});

describe('isDigest', () => {
  it('accepts a Next digest and nothing like a message', () => {
    expect(isDigest('2914387@E1')).toBe(true);
    expect(isDigest('TypeError: x is null')).toBe(false);
    expect(isDigest('')).toBe(false);
    expect(isDigest('1'.repeat(65))).toBe(false);
  });
});
