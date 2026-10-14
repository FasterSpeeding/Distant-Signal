import {
  PREVIEW_IMAGE_ALT,
  PREVIEW_IMAGE_CONTENT_TYPE,
  PREVIEW_IMAGE_SIZE,
  previewTitleFromMetadata,
  renderPreviewImage,
} from '@/lib/previewImage';
import { metadata } from './page';

// The link preview for /incidents: the page's own title on the brand template.
export const size = PREVIEW_IMAGE_SIZE;
export const contentType = PREVIEW_IMAGE_CONTENT_TYPE;
export const alt = PREVIEW_IMAGE_ALT;

export default function Image() {
  return renderPreviewImage(previewTitleFromMetadata(metadata));
}
