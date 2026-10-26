import type { MetadataRoute } from 'next';
import { connection } from 'next/server';
import {
  getPublicLines,
  getStationCodes,
  getTflStopIds,
  getTransitNetwork,
  getTransitNetworks,
  getTransitStops,
} from '@/lib/api';
import { legalPagesPublished } from '@/lib/legal';
import { createLogger } from '@/lib/logger';
import { buildSitemap, SITEMAP_REVALIDATE_SECONDS, SITEMAP_URL_LIMIT, type TransitSitemapNetwork } from '@/lib/sitemap';
import { getSiteOrigin } from '@/lib/siteOrigin';

const log = createLogger('app/sitemap');

/** One list, or `[]` (logged) when its source fails: a sitemap missing one
 * kind of page for a while beats no sitemap. */
async function orEmpty<T>(what: string, load: () => Promise<T[]>): Promise<T[]> {
  try {
    return await load();
  } catch (err) {
    log.warn('Sitemap source failed; leaving it out.', { source: what, error: err });
    return [];
  }
}

/** The served tram networks' lines and stops (`api.transitNetworks`; none
 * by default). */
async function transitNetworks(): Promise<TransitSitemapNetwork[]> {
  const networks = await getTransitNetworks();
  return Promise.all(
    networks.map(async (network) => ({
      id: network.id,
      lineIds: await orEmpty(`transit lines ${network.id}`, async () =>
        (await getTransitNetwork(network.id)).lines.map((line) => line.lineId),
      ),
      stopIds: await orEmpty(`transit stops ${network.id}`, async () =>
        (await getTransitStops(network.id)).map((stop) => stop.id),
      ),
    })),
  );
}

/** `/sitemap.xml`: every public page worth indexing (`lib/sitemap.ts`).
 *
 * Rendered per request, not at `next build` (as `app/robots.ts`): the
 * image build has neither the api nor `NEXT_PUBLIC_SITE_URL`. `connection()`
 * opts in to that without `dynamic = 'force-dynamic'`, which would also
 * turn off the data cache: each list below is a cached fetch kept for
 * `SITEMAP_REVALIDATE_SECONDS`, so a crawler re-reading the sitemap costs
 * the api nothing between refreshes. */
export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  await connection();
  const [origin, stationCodes, tflStopIds, lines, transit] = await Promise.all([
    getSiteOrigin(),
    orEmpty('stations', () => getStationCodes(SITEMAP_REVALIDATE_SECONDS)),
    orEmpty('tfl stops', () => getTflStopIds(SITEMAP_REVALIDATE_SECONDS)),
    orEmpty('lines', () => getPublicLines(SITEMAP_REVALIDATE_SECONDS)),
    orEmpty('transit', transitNetworks),
  ]);
  const entries = buildSitemap({
    origin,
    stationCodes,
    tflStopIds,
    lines,
    transit,
    legalPublished: legalPagesPublished(),
  });
  if (entries.length >= SITEMAP_URL_LIMIT) {
    log.warn('Sitemap hit the 50,000 URL limit; split it with generateSitemaps.', { count: entries.length });
  }
  return entries;
}
