import {
  PREVIEW_IMAGE_ALT,
  PREVIEW_IMAGE_CONTENT_TYPE,
  PREVIEW_IMAGE_SIZE,
  previewTitleFromMetadata,
  renderPreviewImage,
} from '@/lib/previewImage';
import { generateMetadata } from './page';

// The link preview for /lines/[id]: the title the page's own `generateMetadata`
// resolves (the line name), so the two can't disagree, and an unknown line
// 404s here exactly as the page does.
export const size = PREVIEW_IMAGE_SIZE;
export const contentType = PREVIEW_IMAGE_CONTENT_TYPE;
export const alt = PREVIEW_IMAGE_ALT;

export default async function Image({ params }: { params: Promise<{ id: string }> }) {
  return renderPreviewImage(previewTitleFromMetadata(await generateMetadata({ params })));
}
