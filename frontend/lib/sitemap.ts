import type { MetadataRoute } from 'next';
import type { LineSummary } from './types';

/** How long the sitemap's lists stay in Next's data cache: six hours.
 * Stations, stops and lines change a few times a year, and the sitemap is
 * all a crawler fetches from these lists. */
export const SITEMAP_REVALIDATE_SECONDS = 6 * 60 * 60;

/** The most URLs one sitemap file may list (sitemaps.org). Today's total is
 * a few thousand; past this, split it with `generateSitemaps`. */
export const SITEMAP_URL_LIMIT = 50_000;

/** The public pages that need no list: the home page, the indexes, the
 * planner and the Trains section (its bare page: a search's query string
 * is never listed, `isExcludedPath`). `/transit` and the legal pages are
 * added only while they are served (`buildSitemap`). */
export const STATIC_PATHS: readonly string[] = [
  '/',
  '/trains',
  '/status',
  '/lines',
  '/stations',
  '/operators',
  '/incidents',
  '/plan',
  '/attribution',
];

/** The legal pages (`lib/legal.ts`), listed only while they are published,
 * since an unpublished one is `noindex`. */
export const LEGAL_PATHS: readonly string[] = ['/terms', '/privacy', '/cookies', '/accessibility', '/contact'];

/** Pages that must never be listed: short-lived or range views, all
 * `noindex, follow` (a train on one day, one incident, history), and every
 * private route. Checked on every URL as a backstop. */
export function isExcludedPath(path: string): boolean {
  return (
    path.startsWith('/train/') ||
    /^\/incidents\/[^/]+/.test(path) ||
    /^\/(lines|operators)\/[^/]+\/history(\/|$)/.test(path) ||
    path.startsWith('/network/history') ||
    path.includes('?')
  );
}

export interface TransitSitemapNetwork {
  /** The path segment (`metrolink`). */
  id: string;
  /** Each line's path segment (`blue`). */
  lineIds: string[];
  /** Each stop's ATCO code (`9400ZZMAPIC`). */
  stopIds: string[];
}

export interface SitemapSources {
  /** `https://host`, no trailing slash. */
  origin: string;
  stationCodes: string[];
  tflStopIds: string[];
  lines: LineSummary[];
  transit: TransitSitemapNetwork[];
  legalPublished: boolean;
}

const seg = encodeURIComponent;

/** Every public page path the sources name, in a stable order. */
export function sitemapPaths({
  stationCodes,
  tflStopIds,
  lines,
  transit,
  legalPublished,
}: Omit<SitemapSources, 'origin'>): string[] {
  const paths: string[] = [...STATIC_PATHS];
  if (transit.length > 0) paths.push('/transit');
  if (legalPublished) paths.push(...LEGAL_PATHS);
  for (const line of lines) {
    // Anonymous `/public/lines` has no custom lines; this keeps it so.
    if (line.source === 'custom') continue;
    paths.push(`/lines/${seg(line.id)}`);
    // Only a catalogue National Rail line has a published timetable (the
    // line page shows no trains panel for a TfL line).
    if (line.source === 'catalogue' && !line.id.startsWith('tfl-')) paths.push(`/lines/${seg(line.id)}/timetable`);
  }
  for (const crs of stationCodes) paths.push(`/stations/${seg(crs.toUpperCase())}`);
  for (const id of tflStopIds) paths.push(`/tfl/stops/${seg(id)}`);
  for (const network of transit) {
    const base = `/transit/${seg(network.id)}`;
    paths.push(base);
    for (const line of network.lineIds) {
      paths.push(`${base}/lines/${seg(line)}`, `${base}/lines/${seg(line)}/timetable`);
    }
    for (const stop of network.stopIds) paths.push(`${base}/stops/${seg(stop)}`);
  }
  return [...new Set(paths)].filter((path) => !isExcludedPath(path));
}

/** The sitemap. No `lastModified`: every listed page is live (status,
 * departures, arrivals), so no date would be meaningful, and a made-up one
 * teaches crawlers to ignore it. No `changeFrequency` or `priority` either:
 * Google ignores both. */
export function buildSitemap(sources: SitemapSources): MetadataRoute.Sitemap {
  return sitemapPaths(sources)
    .slice(0, SITEMAP_URL_LIMIT)
    .map((path) => ({ url: `${sources.origin}${path}` }));
}
