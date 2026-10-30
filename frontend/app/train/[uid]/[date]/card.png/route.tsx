import { ApiNotFoundError, getPreviewTrain } from '@/lib/api';
import { LIVE_PREVIEW_CACHE_CONTROL } from '@/lib/livePreview';
import { renderStatusPreviewImage } from '@/lib/previewImage';
import { trainPreviewCard } from '@/lib/trainPreview';

// A train's link-preview image, listed by the page's `generateMetadata` as
// `/train/{uid}/{date}/card.png?v={digest}` (`lib/livePreview.ts`, and the
// incident card's route for why it isn't an `opengraph-image.tsx`). `?v=`
// is never read here: the image always shows the train as it is now.
//
// One preview read (`lib/api.ts`, "Reads for link-preview images"), shared
// for a minute: the public train, whose api route reads only the shared
// train rows. Nothing about anyone who tracks it (a custom name, tickets)
// is in that answer, so nothing private can reach the card. The
// session-only `/train/by-id/...` pages have no card: they keep the
// site-wide image.
export const revalidate = 60; // LIVE_PREVIEW_REVALIDATE_SECONDS: segment config must be a literal

export function generateStaticParams(): { uid: string; date: string }[] {
  return [];
}

function notFound(): Response {
  return new Response('Not found', { status: 404, headers: { 'Cache-Control': LIVE_PREVIEW_CACHE_CONTROL } });
}

export async function GET(
  _request: Request,
  { params }: { params: Promise<{ uid: string; date: string }> },
): Promise<Response> {
  const { uid, date } = await params;
  // The page's own date check, and a UID's shape: never build a request
  // from anything else.
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || !/^[A-Za-z0-9]{1,16}$/.test(uid)) return notFound();

  let train;
  try {
    train = await getPreviewTrain(uid, date);
  } catch (err) {
    if (err instanceof ApiNotFoundError) return notFound();
    throw err;
  }
  return renderStatusPreviewImage(trainPreviewCard(train, Date.now()), {
    headers: { 'Cache-Control': LIVE_PREVIEW_CACHE_CONTROL },
  });
}
