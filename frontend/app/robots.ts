import type { MetadataRoute } from 'next';
import { buildRobots } from '@/lib/robots';
import { getConfiguredSiteOrigin } from '@/lib/siteOrigin';

// Render per request, not at `next build`. Without this, Next prerenders
// /robots.txt at build time. The image build doesn't pass
// `NEXT_PUBLIC_SITE_URL` (the chart sets it on the running pod), so a
// prerendered file would never carry the configured origin.
export const dynamic = 'force-dynamic';

/** `/robots.txt`. The rules live in `lib/robots.ts`. */
export default function robots(): MetadataRoute.Robots {
  return buildRobots({ origin: getConfiguredSiteOrigin() });
}
