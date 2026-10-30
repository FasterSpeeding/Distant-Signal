import { formatTime, formatWeekdayShortDate, londonDayKey } from './dateFormat';
import type { PreviewImage } from './pageMetadata';
import type { SeverityGroup } from './severity';

/** Link-preview cards that show live data (an incident, a line, a
 * station): what they have in common. Each page maps its own data to a
 * `StatusPreviewCard` (`lib/incidentPreview.ts`, `lib/linePreview.ts`,
 * `lib/stationPreview.ts`), `renderStatusPreviewImage` draws it, and the
 * page lists the image itself with a `?v=` digest of what it shows.
 *
 * Chat apps cache a preview image by URL, often for hours, and some never
 * fetch it again. So:
 * - the image says when its data is from, and that it is a snapshot;
 * - the page's `og:image` carries `?v=`, a digest of the state the card
 *   shows (never of a timestamp), so a changed state is a new URL and a
 *   link shared afterwards gets a fresh card, while an unchanged one keeps
 *   its cached image;
 * - the image route reads anonymously through Next's data cache for
 *   `LIVE_PREVIEW_REVALIDATE_SECONDS` and answers with
 *   `LIVE_PREVIEW_CACHE_CONTROL`, so a burst of unfurls costs one api call
 *   per minute per page and the CDN can serve the rest;
 * - `og:description` stays evergreen: the live part is in the image only. */

/** How long an image route's api reads are shared, in seconds. */
export const LIVE_PREVIEW_REVALIDATE_SECONDS = 60;

/** A minute fresh, then served stale while one request fetches the new
 * state. A changed state also gets a new `?v=` on the page, so a link
 * shared after the change never waits for this. */
export const LIVE_PREVIEW_CACHE_CONTROL = `public, max-age=${LIVE_PREVIEW_REVALIDATE_SECONDS}, s-maxage=${LIVE_PREVIEW_REVALIDATE_SECONDS}, stale-while-revalidate=300`;

/** A title this long always shows whole on the card (two or three lines
 * at `cardTitleFontSize`); a longer one may be clamped. */
export const TITLE_FITS_LENGTH = 90;

/** What `renderStatusPreviewImage` draws. */
export interface StatusPreviewCard {
  /** The coloured band, in the severity pill's colours: `label` carries
   * the state in words, the colour only repeats it. `aside` is its time
   * ("Updated Fri 30 Oct, 13:58", "As of 14:03"). */
  band: { tone: SeverityGroup; label: string; aside: string };
  /** The page's subject, clamped to three lines. */
  title: string;
  /** The first fact, a little larger: where an incident is, what is wrong
   * on a line or at a station. */
  lead: string | null;
  /** Lines, as chips: those that fit, then "and N more". */
  chips: { names: string[]; more: number };
  /** Facts, one line each, cut short with an ellipsis. Empty ones are
   * left out by the mapper, never drawn blank. */
  rows: string[];
}

/** An 8-hex-digit digest (FNV-1a, 32-bit) of `value` as JSON, for `?v=`.
 * A collision only costs a stale cached image. */
export function previewDigest(value: unknown): string {
  const input = JSON.stringify(value);
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

/** The page's own preview image, listed explicitly in its metadata. */
export function livePreviewImage(path: string, digest: string, alt: string): PreviewImage {
  return { url: `${path}?v=${digest}`, alt, width: 1200, height: 630, type: 'image/png' };
}

/** "Fri 30 Oct, 13:50" in UK time. */
export function previewWhen(value: string): string {
  return `${formatWeekdayShortDate(value)}, ${formatTime(value)}`;
}

/** "Fri 30 Oct, 12:20 to 12:56", or both dates when the days differ. */
export function previewSpan(from: string, to: string): string {
  return londonDayKey(from) === londonDayKey(to)
    ? `${previewWhen(from)} to ${formatTime(to)}`
    : `${previewWhen(from)} to ${previewWhen(to)}`;
}

/** "A", "A and B", "A, B and C", "A, B, C and 2 more". */
export function listPhrase(items: readonly string[], max = 3): string {
  if (items.length <= 1) return items[0] ?? '';
  if (items.length > max) return `${items.slice(0, max).join(', ')} and ${items.length - max} more`;
  return `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
}

/** Whitespace collapsed, ends trimmed. */
export function tidy(text: string): string {
  return text.replace(/\s+/g, ' ').trim();
}

/** Plain text from a status reason that may carry HTML (Knowledgebase
 * text): tags dropped, the few entities it uses decoded, whitespace
 * collapsed. The image renderer draws text, never markup. */
export function plainText(text: string): string {
  return tidy(
    text
      .replace(/<[^>]*>/g, ' ')
      .replace(/&nbsp;/g, ' ')
      .replace(/&#39;|&apos;/g, "'")
      .replace(/&quot;/g, '"')
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&amp;/g, '&'),
  );
}
