import {
  PREVIEW_IMAGE_ALT,
  PREVIEW_IMAGE_CONTENT_TYPE,
  PREVIEW_IMAGE_SIZE,
  previewTitleFromMetadata,
  renderPreviewImage,
} from '@/lib/previewImage';
import { previewImageAlt } from '@/lib/pageMetadata';
import { generateMetadata } from './page';

// The link preview for /stations/[crs]: the title the page's own `generateMetadata`
// resolves ("Woking (WOK)"), so the two can't disagree, and an unknown station
// 404s here exactly as the page does.
export const size = PREVIEW_IMAGE_SIZE;
export const contentType = PREVIEW_IMAGE_CONTENT_TYPE;

/** One image whose alt text names the station ("Woking (WOK) on Distant
 * Signal"). Next serves it at `/stations/WOK/opengraph-image/card`. The
 * code is upper-cased first, so the page's case redirect never fires here;
 * an unknown or unreachable station falls back to the generic alt, and the
 * page itself 404s or retries as usual. */
export async function generateImageMetadata({ params }: { params: { crs: string } }) {
  let alt = PREVIEW_IMAGE_ALT;
  try {
    const title = previewTitleFromMetadata(
      await generateMetadata({ params: Promise.resolve({ crs: params.crs.toUpperCase() }) }),
    );
    if (title) alt = previewImageAlt(title);
  } catch {
    // Unknown station (`notFound()`) or a failed lookup: keep the generic alt.
  }
  return [{ id: 'card', alt, size, contentType }];
}

export default async function Image({ params }: { params: Promise<{ crs: string }> }) {
  return renderPreviewImage(previewTitleFromMetadata(await generateMetadata({ params })));
}
