import {
  PREVIEW_IMAGE_ALT,
  PREVIEW_IMAGE_CONTENT_TYPE,
  PREVIEW_IMAGE_SIZE,
  previewTitleFromMetadata,
  renderPreviewImage,
} from '@/lib/previewImage';
import { previewImageAlt } from '@/lib/pageMetadata';
import { generateMetadata } from './page';

// The link preview for /lines/[id]: the title the page's own `generateMetadata`
// resolves (the line name), so the two can't disagree, and an unknown line
// 404s here exactly as the page does.
export const size = PREVIEW_IMAGE_SIZE;
export const contentType = PREVIEW_IMAGE_CONTENT_TYPE;
// Rendered per request, as before this route had `generateImageMetadata`:
// with it, Next prerenders the image route, and the page's metadata reads
// the session cookie (`lib/api.ts`), which a static render can't.
export const dynamic = 'force-dynamic';

/** One image whose alt text names the line ("Northern on Distant
 * Signal"), served at `/lines/{id}/opengraph-image/card`. An unknown or
 * unreachable line falls back to the generic alt; the page itself 404s or
 * retries as usual. */
export async function generateImageMetadata({ params }: { params: { id: string } }) {
  let alt = PREVIEW_IMAGE_ALT;
  try {
    const title = previewTitleFromMetadata(await generateMetadata({ params: Promise.resolve({ id: params.id }) }));
    if (title) alt = previewImageAlt(title);
  } catch {
    // Unknown line (`notFound()`) or a failed lookup: keep the generic alt.
  }
  return [{ id: 'card', alt, size, contentType }];
}

export default async function Image({ params }: { params: Promise<{ id: string }> }) {
  return renderPreviewImage(previewTitleFromMetadata(await generateMetadata({ params })));
}
