import { ApiNotFoundError, getPreviewIncident, getPreviewStationName, getPreviewTocs } from '@/lib/api';
import { tocNameLookup } from '@/lib/displayLabels';
import { incidentPreviewCard, incidentStatusCard, MAX_PREVIEW_STATIONS } from '@/lib/incidentPreview';
import { LIVE_PREVIEW_CACHE_CONTROL } from '@/lib/livePreview';
import { renderStatusPreviewImage } from '@/lib/previewImage';

// An incident's link-preview image, listed by the page's `generateMetadata`
// as `/incidents/{id}/card.png?v={digest}` (`lib/livePreview.ts`). Not an
// `opengraph-image.tsx`: Next lists a file-convention image itself, at a
// URL the page can't version, and runs its `generateImageMetadata` again on
// every image request. `?v=` is never read here: the image always shows the
// incident as it is now, and its words come only from the api.
//
// Cheap to unfurl many times: every read is a preview read (`lib/api.ts`,
// "Reads for link-preview images"), the incident's for a minute and
// the names for an hour, and the PNG itself is regenerated at most once a
// minute (`revalidate`, with no params prerendered at build time). The
// `.png` path lets the CDN in front cache it too.
export const revalidate = 60; // LIVE_PREVIEW_REVALIDATE_SECONDS: segment config must be a literal

export function generateStaticParams(): { id: string }[] {
  return [];
}

export async function GET(_request: Request, { params }: { params: Promise<{ id: string }> }): Promise<Response> {
  const { id } = await params;
  let incident;
  try {
    incident = await getPreviewIncident(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return new Response('Not found', { status: 404, headers: { 'Cache-Control': LIVE_PREVIEW_CACHE_CONTROL } });
    }
    throw err;
  }
  // Both degrade to codes, as on the page.
  const stationCodes = incident.affectedStations.slice(0, MAX_PREVIEW_STATIONS);
  const [tocs, stationNames] = await Promise.all([
    getPreviewTocs().catch(() => []),
    Promise.all(stationCodes.map((crs) => getPreviewStationName(crs).catch(() => null))),
  ]);
  const card = incidentPreviewCard(incident, {
    operatorNames: tocNameLookup(tocs),
    stationNames: new Map(stationCodes.map((crs, i) => [crs, stationNames[i] ?? null])),
    now: Date.now(),
  });
  return renderStatusPreviewImage(incidentStatusCard(card), {
    headers: { 'Cache-Control': LIVE_PREVIEW_CACHE_CONTROL },
  });
}
