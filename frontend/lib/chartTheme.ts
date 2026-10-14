import { formatDate, formatShortDate, formatTime } from '@/lib/dateFormat';
import type { TrendGranularity } from '@/lib/history';

/** Shared settings for every chart on the site (`TrendsCharts`, used by
 * the line, operator and network history pages). Charts read these rather
 * than picking their own colours, curves or number formats, so they all
 * look and read the same. See docs/style-guide.md, "Charts". */

/** Series colours: CSS variables from `app/globals.css`, so each one has a
 * light and a dark shade that clears 3:1 against the page. Chart series
 * are data encodings, but they borrow the semantic hues so a reader
 * doesn't have to relearn them: late is orange, cancelled red, skipped
 * stops yellow, and a plain count is the brand grape. */
export const CHART_COLORS = {
  count: 'var(--ds-chart-count)',
  delay: 'var(--ds-chart-delay)',
  cancellation: 'var(--ds-chart-cancellation)',
  skip: 'var(--ds-chart-skip)',
} as const;

/** Straight segments between buckets. Each point is one bucket's measured
 * value, so a smoothed curve would draw values nobody measured between
 * them. A step was ruled out because the axis is a category axis that
 * puts each value on its bucket's tick, and a step would push it half a
 * bucket sideways. */
export const CHART_CURVE = 'linear' as const;

/** The shading behind buckets with too little data to plot. */
export const CHART_GAP_FILL = 'var(--mantine-color-gray-5)';
export const CHART_GAP_OPACITY = 0.15;

const countFormat = new Intl.NumberFormat('en-GB', { maximumFractionDigits: 0 });
const minutesFormat = new Intl.NumberFormat('en-GB', { maximumFractionDigits: 1 });
const percentFormat = new Intl.NumberFormat('en-GB', { maximumFractionDigits: 1 });

/** "80,000": a whole count with thousands separators. */
export function formatCount(value: number): string {
  return countFormat.format(value);
}

/** "12.5%" from a 0-1 proportion. Up to one decimal, so the axis reads
 * "10%", not "10.0%". */
export function formatRate(value: number): string {
  return `${percentFormat.format(value * 100)}%`;
}

/** "2.5 min", "1,200 min". */
export function formatMinutes(value: number): string {
  return `${minutesFormat.format(value)} min`;
}

/** An x-axis tick for one bucket: "15 Sep" for a day, "14:30" within a
 * day. The bucket key stays the raw, unique category value; only the
 * label is formatted. */
export function bucketTickLabel(granularity: TrendGranularity, bucketKey: string): string {
  return granularity === 'day' ? formatShortDate(bucketKey) : formatTime(bucketKey);
}

/** The tooltip heading for one bucket: "15 Sep 2026" for a day, and
 * "15 Sep, 14:30" within a day, since a rolling 24-hour window can show
 * the same clock time twice. */
export function bucketTooltipLabel(granularity: TrendGranularity, bucketKey: string): string {
  return granularity === 'day' ? formatDate(bucketKey) : `${formatShortDate(bucketKey)}, ${formatTime(bucketKey)}`;
}
