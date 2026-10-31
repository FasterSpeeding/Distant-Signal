import {
  ApiNotFoundError,
  getPreviewTransitNetwork,
  getPreviewTransitSituation,
  getPreviewTransitStops,
} from '@/lib/api';
import { LIVE_PREVIEW_CACHE_CONTROL } from '@/lib/livePreview';
import { renderStatusPreviewImage } from '@/lib/previewImage';
import { TRANSIT_ID } from '@/lib/transit';
import { transitIncidentPreviewCard } from '@/lib/transitIncidentPreview';

// A transit disruption's link-preview image, listed by the page's
// `generateMetadata` as `/transit/{network}/incidents/{id}/card.png?v=...`:
// the same pattern, caching and renderer as `/incidents/[id]/card.png`
// (see that route's comment). `?v=` is never read here.
export const revalidate = 60; // LIVE_PREVIEW_REVALIDATE_SECONDS: segment config must be a literal

export function generateStaticParams(): { network: string; id: string }[] {
  return [];
}

function notFoundResponse(): Response {
  return new Response('Not found', { status: 404, headers: { 'Cache-Control': LIVE_PREVIEW_CACHE_CONTROL } });
}

export async function GET(
  _request: Request,
  { params }: { params: Promise<{ network: string; id: string }> },
): Promise<Response> {
  const { network, id } = await params;
  if (!TRANSIT_ID.test(network)) return notFoundResponse();
  let situation;
  let detail;
  try {
    [situation, detail] = await Promise.all([
      getPreviewTransitSituation(network, id),
      getPreviewTransitNetwork(network),
    ]);
  } catch (err) {
    if (err instanceof ApiNotFoundError) return notFoundResponse();
    throw err;
  }
  // Names degrade to none, as on the page.
  const stops = situation.stops.length > 0 ? await getPreviewTransitStops(network).catch(() => []) : [];
  const card = transitIncidentPreviewCard(situation, {
    networkName: detail.name,
    stopNames: new Map(stops.map((stop) => [stop.id, stop.name])),
    now: Date.now(),
  });
  return renderStatusPreviewImage(card, { headers: { 'Cache-Control': LIVE_PREVIEW_CACHE_CONTROL } });
}
