import { ImageResponse } from 'next/og';
import type { Metadata } from 'next';
import { type MarkBox, signalMarkBounds, signalMarkSvg } from './brand';
import { type PreviewBandTone, type StatusPreviewCard, TITLE_FITS_LENGTH } from './livePreview';
import { SITE_NAME } from './pageMetadata';

/** The link-preview (Open Graph) image template (round 2, item 9): the
 * page's title on the brand background, under the wordmark. A route opts in
 * with an `opengraph-image.tsx` that re-exports `size`, `contentType` and
 * `alt` from here and renders `renderPreviewImage(title)`; Next then lists
 * that image instead of the site-wide `public/opengraph-image.png`, which
 * is this template's no-title form (`renderPreviewImage(null)`), saved. The
 * route's page passes `OWN_PREVIEW_IMAGE` to `pageMetadata`, or the
 * explicit site-wide image would win.
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
 * background, cropped to their own bounds in the icon's 32x32 box plus an
 * even half-unit margin so no anti-aliased edge touches the box. The bounds
 * come from the shape itself (`signalMarkBounds`): a hand-picked box
 * (y from 5) once sliced the top off the raised arm's fishtail tip. */
const MARK_MARGIN = 0.5;
export const BARE_MARK_VIEWBOX: MarkBox = (() => {
  const { x, y, width, height } = signalMarkBounds();
  return {
    x: x - MARK_MARGIN,
    y: y - MARK_MARGIN,
    width: width + 2 * MARK_MARGIN,
    height: height + 2 * MARK_MARGIN,
  };
})();

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

/** The light scheme's `StatusBadge` pairs (`--ds-status-badge-<group>-fill`
 * and `-text` in app/globals.css, resolved to hex: the image renderer has
 * no CSS variables). `previewImage.test.tsx` checks them against the CSS,
 * so the band can't drift from the pills, which clear 4.5:1 and APCA Lc 75.
 * A train's delay takes `DelayBadge`'s hue instead: the light pill's own
 * text colour (`--mantine-color-<hue>-light-color`) as the fill, under
 * white, 6.5:1 or more. */
export const STATUS_BAND_COLORS: Record<PreviewBandTone, { fill: string; text: string }> = {
  good: { fill: '#287f39', text: '#ffffff' },
  informational: { fill: '#495057', text: '#ffffff' },
  planned: { fill: '#1864ab', text: '#ffffff' },
  mild: { fill: '#fcc419', text: '#000000' },
  severe: { fill: '#c92a2a', text: '#ffffff' },
  late: { fill: '#a5370b', text: '#ffffff' },
  early: { fill: '#076b4c', text: '#ffffff' },
};

/** Titles step down so three lines fit; past that the title is clamped. */
export function cardTitleFontSize(title: string): number {
  if (title.length <= 50) return 60;
  if (title.length <= TITLE_FITS_LENGTH) return 52;
  return 44;
}

/** The line chips' row: the card's width less its padding. */
const CHIP_ROW_WIDTH = 1200 - 2 * 64;
const CHIP_MAX_WIDTH = 520;
const CHIP_GAP = 12;
/** Room kept for "and 12 more". */
const CHIP_MORE_WIDTH = 150;

/** A chip's width estimated from its name: Geist at 26px averages under
 * 14px a character; plus padding and border. Long names cap at
 * `CHIP_MAX_WIDTH` and end in an ellipsis. */
function chipWidth(name: string): number {
  return Math.min(CHIP_MAX_WIDTH, 40 + 14 * name.length);
}

/** The line chips that fit on one row at full size, and how many more to
 * count instead: squeezing a third chip to "Northe..." says nothing. The
 * first chip always shows. */
export function fitLineChips(names: readonly string[], more: number): { names: string[]; more: number } {
  const total = names.length + more;
  const shown: string[] = [];
  let used = 0;
  for (const name of names) {
    const width = chipWidth(name) + (shown.length > 0 ? CHIP_GAP : 0);
    const reserve = shown.length + 1 < total ? CHIP_MORE_WIDTH : 0;
    if (shown.length > 0 && used + width + reserve > CHIP_ROW_WIDTH) break;
    shown.push(name);
    used += width;
  }
  return { names: shown, more: total - shown.length };
}

/** One line of text that ends in an ellipsis rather than wrapping. */
const SINGLE_LINE = {
  display: 'block',
  overflow: 'hidden',
  whiteSpace: 'nowrap',
  textOverflow: 'ellipsis',
} as const;

/** A live link preview (an incident, a line, a station, a train;
 * `lib/livePreview.ts`):
 * the state as a coloured band with the words on it and its time, then the
 * brand template's wordmark, the title, a lead line, line chips and the
 * other facts. The mapper leaves out what it has nothing to say for, so
 * nothing is drawn blank. */
export function renderStatusPreviewImage(card: StatusPreviewCard, init: ResponseInit = {}): ImageResponse {
  const band = STATUS_BAND_COLORS[card.band.tone];
  const chips = fitLineChips(card.chips.names, card.chips.more);
  return new ImageResponse(
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        backgroundImage: BACKGROUND,
        color: TEXT,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 32,
          height: 96,
          padding: '0 64px',
          backgroundColor: band.fill,
          color: band.text,
        }}
      >
        <div style={{ ...SINGLE_LINE, fontSize: 44, letterSpacing: -0.5 }}>{card.band.label}</div>
        <div style={{ display: 'flex', flexShrink: 0, fontSize: 28 }}>{card.band.aside}</div>
      </div>
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          flexGrow: 1,
          padding: '36px 64px 40px',
          gap: 24,
        }}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 18 }}>
            <Mark height={52} />
            <div style={{ fontSize: 34, letterSpacing: -0.5 }}>{SITE_NAME}</div>
          </div>
          <div
            style={{
              display: 'block',
              overflow: 'hidden',
              lineClamp: 3,
              fontSize: cardTitleFontSize(card.title),
              lineHeight: 1.12,
              letterSpacing: -1,
            }}
          >
            {card.title}
          </div>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12, fontSize: 28 }}>
          {card.lead && <div style={{ ...SINGLE_LINE, fontSize: 32 }}>{card.lead}</div>}
          {chips.names.length > 0 && (
            <div style={{ display: 'flex', alignItems: 'center', gap: CHIP_GAP, overflow: 'hidden' }}>
              {chips.names.map((name) => (
                <div
                  key={name}
                  style={{
                    ...SINGLE_LINE,
                    flexShrink: 0,
                    maxWidth: CHIP_MAX_WIDTH,
                    padding: '4px 18px',
                    borderRadius: 999,
                    border: '2px solid rgba(255, 255, 255, 0.7)',
                    backgroundColor: 'rgba(255, 255, 255, 0.14)',
                    fontSize: 26,
                  }}
                >
                  {name}
                </div>
              ))}
              {chips.more > 0 && (
                <div style={{ display: 'flex', flexShrink: 0, fontSize: 26 }}>{`and ${chips.more} more`}</div>
              )}
            </div>
          )}
          {card.rows.map((row) => (
            <div key={row} style={{ ...SINGLE_LINE, opacity: 0.94 }}>
              {row}
            </div>
          ))}
        </div>
      </div>
    </div>,
    { ...PREVIEW_IMAGE_SIZE, ...init },
  );
}
