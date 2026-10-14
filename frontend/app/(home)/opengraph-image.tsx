import {
  PREVIEW_IMAGE_ALT,
  PREVIEW_IMAGE_CONTENT_TYPE,
  PREVIEW_IMAGE_SIZE,
  renderPreviewImage,
} from '@/lib/previewImage';

// The home page's link preview: the wordmark and the strapline, no page
// title. `public/opengraph-image.png` is a saved copy of this image, the
// fallback for routes with no preview of their own.
export const size = PREVIEW_IMAGE_SIZE;
export const contentType = PREVIEW_IMAGE_CONTENT_TYPE;
export const alt = PREVIEW_IMAGE_ALT;

export default function Image() {
  return renderPreviewImage(null);
}
