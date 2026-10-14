import { ImageResponse } from 'next/og';
import type { Metadata } from 'next';
import { signalMarkSvg } from './brand';
import { SITE_NAME } from './pageMetadata';

/** The link-preview (Open Graph) image template (round 2, item 9): the
 * page's title on the brand background, under the wordmark. A route opts in
 * with an `opengraph-image.tsx` that re-exports `size`, `contentType` and
 * `alt` from here and renders `renderPreviewImage(title)`; Next then lists
 * that image instead of the site-wide `public/opengraph-image.png`, which is
 * this template's no-title form (`renderPreviewImage(null)`), saved.
 *
 * Only for titles the app itself computes: a fixed page name, or the title
 * a route's own `generateMetadata` resolves (`previewTitleFromMetadata`).
 * Never a query parameter, so nobody can mint a branded image saying
 * whatever they like.
 *
 * The type is next/og's bundled Geist Regular: the site loads no web fonts
 * and the image renderer can't use the visitor's system stack. */

export const PREVIEW_IMAGE_SIZE = { width: 1200, height: 630 };
export const PREVIEW_IMAGE_CONTENT_TYPE = 'image/png';
export const PREVIEW_IMAGE_ALT = `${SITE_NAME}: live UK rail status`;

/** Shown under the site name when a preview has no page title. */
const SITE_STRAPLINE = 'Live UK rail status';

/** Longer titles drop a size so two lines still fit under the wordmark. */
export function previewTitleFontSize(title: string): number {
  if (title.length <= 24) return 88;
  if (title.length <= 48) return 68;
  return 52;
}

/** The title a route's `generateMetadata` resolved, as the tab shows it
 * without the site name: the preview already carries the wordmark. */
export function previewTitleFromMetadata(metadata: Metadata): string | null {
  const { title } = metadata;
  if (typeof title === 'string') return title;
  if (title && typeof title === 'object' && 'absolute' in title && typeof title.absolute === 'string') {
    return title.absolute;
  }
  return null;
}

/** The post and arm without their tile, which would vanish on the grape
 * background, cropped to their own bounds in the icon's 32x32 box. */
const BARE_MARK_VIEWBOX = { x: 13, y: 5, width: 16, height: 22 };

function bareSignalMarkDataUri(): string {
  const { x, y, width, height } = BARE_MARK_VIEWBOX;
  const svg = signalMarkSvg({ tile: false }).replace(/viewBox="[^"]*"/, `viewBox="${x} ${y} ${width} ${height}"`);
  return `data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`;
}

/** The bare mark at `height` px, keeping the cropped box's aspect ratio. */
function Mark({ height }: { height: number }) {
  const width = Math.round((height * BARE_MARK_VIEWBOX.width) / BARE_MARK_VIEWBOX.height);
  // eslint-disable-next-line @next/next/no-img-element -- rendered to a PNG by next/og, not a page
  return <img src={bareSignalMarkDataUri()} width={width} height={height} alt="" />;
}

/** Grape 7 to grape 9, the same family as the favicon's tile. */
const BACKGROUND = 'linear-gradient(135deg, #ae3ec9 0%, #862e9c 100%)';
const TEXT = '#ffffff';

export function renderPreviewImage(title: string | null): ImageResponse {
  if (title === null) {
    // The site's own card: the wordmark large, the strapline under it.
    return new ImageResponse(
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 48,
          backgroundImage: BACKGROUND,
          color: TEXT,
        }}
      >
        <Mark height={240} />
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div style={{ fontSize: 112, lineHeight: 1, letterSpacing: -2 }}>{SITE_NAME}</div>
          <div style={{ fontSize: 44, opacity: 0.92 }}>{SITE_STRAPLINE}</div>
        </div>
      </div>,
      PREVIEW_IMAGE_SIZE,
    );
  }
  return new ImageResponse(
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'space-between',
        padding: '72px 88px',
        backgroundImage: BACKGROUND,
        color: TEXT,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 24 }}>
        <Mark height={88} />
        <div style={{ fontSize: 52, letterSpacing: -0.5 }}>{SITE_NAME}</div>
      </div>
      <div style={{ display: 'flex', fontSize: previewTitleFontSize(title), lineHeight: 1.1, letterSpacing: -1 }}>
        {title}
      </div>
    </div>,
    PREVIEW_IMAGE_SIZE,
  );
}
