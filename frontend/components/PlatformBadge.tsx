import type { ReactNode } from 'react';
import { Badge, VisuallyHidden } from '@mantine/core';
import {
  platformDisplay,
  platformTooltip,
  spokenPlatformLabel,
  type PlatformDisplay,
  type PlatformFacts,
} from '@/lib/platformLabel';
import { statusBadgeColors } from '@/lib/severity';
import { PhoneLabel } from './PhoneLabel';

/** The app's one platform badge: every National Rail platform on screen
 * (departure rows, the calling-point table, the progress line) is this
 * component, so booked, live, confirmed, changed and cancelled look the
 * same everywhere. `platformDisplay` (lib/platformLabel.ts) picks the
 * state from the api's fields; docs/style-guide.md, "PlatformBadge", has
 * the table.
 *
 * Three cues, each carried by more than colour (WCAG 1.4.1):
 *
 * - How sure: an outline badge with the number underlined in dots and
 *   "(booked)" for a booked platform; a light badge for one a live board
 *   shows; a filled badge once it is confirmed.
 * - Changed: blue, the app's colour for a changed arrangement (never
 *   orange, which a delay badge beside it uses for lateness): the new
 *   number first, then the old one in lighter type, struck through and
 *   named in words: "Platform 9 was ~~6~~".
 * - Cancelled: red, the whole label struck through.
 *
 * A screen reader hears `spokenPlatformLabel` at every width ("Platform 9
 * (changed from 6)"), never a bare struck-through number: that drawing is
 * `aria-hidden`, and the words the look stands for are visually hidden
 * beside it. `title` repeats the state for a mouse user and adds nothing
 * the badge doesn't already show or say.
 *
 * Renders nothing when no platform is known: "not known" is a different
 * fact from "on platform X", and this never fabricates one.
 *
 * `compact` (under a "Platform" column header, `JourneyTimeline`) drops
 * the word "Platform" and "(booked)" below `sm`: "4", "9 was ~~6~~". The
 * spoken words are the same. `data-platform-badge` opts the badge out of
 * Mantine's ellipsis (`app/globals.css`); `data-platform-state` names the
 * state for tests and styles. */
export function PlatformBadge({ compact = false, ...facts }: PlatformFacts & { compact?: boolean }) {
  const display = platformDisplay(facts);
  if (display === null) return null;
  const { color, variant } = BADGE_LOOK[display.state];
  const confirmed = (display.state === 'live' || display.state === 'changed') && display.confirmed;
  const suffix = spokenSuffix(display);
  // A confirmed platform is a filled pill, painted from the same measured
  // fill and label tokens as `StatusBadge` (`--ds-status-badge-*`, white on
  // gray 7/8 or blue 9/8), not Mantine's filled shade with `autoContrast`,
  // whose black-on-gray and black-on-blue labels read poorly at badge size.
  const filled = confirmed ? statusBadgeColors(display.state === 'changed' ? 'planned' : 'informational') : null;
  return (
    <Badge
      color={filled?.fill ?? color}
      {...(filled ? { c: filled.text } : {})}
      variant={filled ? 'filled' : variant}
      tt="none"
      data-platform-badge
      data-platform-state={display.state}
      data-platform-confirmed={confirmed || undefined}
      data-platform-changed={display.state === 'changed'}
      data-platform-cancelled={display.state === 'cancelled' || undefined}
      title={platformTooltip(display)}
    >
      {compact ? <PhoneLabel full={fullLabel(display)} compact={shortLabel(display)} /> : fullLabel(display)}
      {suffix && <VisuallyHidden>{suffix}</VisuallyHidden>}
    </Badge>
  );
}

const BADGE_LOOK: Record<PlatformDisplay['state'], { color: string; variant: 'outline' | 'light' }> = {
  booked: { color: 'gray', variant: 'outline' },
  live: { color: 'gray', variant: 'light' },
  changed: { color: 'blue', variant: 'light' },
  cancelled: { color: 'red', variant: 'light' },
};

/** The words on screen from `sm` up, and, with `spokenSuffix`, what a
 * screen reader hears at every width: together they read as
 * `spokenPlatformLabel`. */
function fullLabel(display: PlatformDisplay): ReactNode {
  const name = `Platform ${display.platform}`;
  switch (display.state) {
    case 'booked':
      return (
        <>
          <span className="platformBadge__booked">{name}</span>
          <span aria-hidden="true"> (booked)</span>
        </>
      );
    case 'cancelled':
      return <s>{name}</s>;
    case 'changed':
      return (
        <>
          <span aria-hidden="true">
            <span className="platformBadge__current">Platform {display.platform}</span>{' '}
            <WasPlatform previous={display.previous} />
          </span>
          <VisuallyHidden>{spokenPlatformLabel(display)}</VisuallyHidden>
        </>
      );
    case 'live':
      return name;
  }
}

/** A changed platform's superseded one, after the new number: "was 6",
 * lighter than the badge's bold, the old number struck through. The word
 * carries the meaning on its own: a strike-through alone is easy to miss
 * at badge size, and all but vanishes through a "4", whose crossbar it
 * overlaps, so "~~4~~ 5" read as "4 5". Dimmed by weight, not opacity, so
 * it keeps the badge's measured contrast. */
function WasPlatform({ previous }: { previous: string }) {
  return (
    <span className="platformBadge__was">
      was <s className="platformBadge__previous">{previous}</s>
    </span>
  );
}

/** The phone form under a "Platform" column header: the same look without
 * the word "Platform" or "(booked)". `PhoneLabel` hides it from screen
 * readers. */
function shortLabel(display: PlatformDisplay): ReactNode {
  switch (display.state) {
    case 'booked':
      return <span className="platformBadge__booked">{display.platform}</span>;
    case 'cancelled':
      return <s>{display.platform}</s>;
    case 'changed':
      return (
        <>
          {display.platform} <WasPlatform previous={display.previous} />
        </>
      );
    case 'live':
      return display.platform;
  }
}

/** The spoken words `fullLabel` leaves out, visually hidden at every
 * width: "(booked, not confirmed)", "(confirmed)", "(cancelled)". */
function spokenSuffix(display: PlatformDisplay): string | null {
  switch (display.state) {
    case 'booked':
      return ' (booked, not confirmed)';
    case 'cancelled':
      return ' (cancelled)';
    case 'changed':
      return null;
    case 'live':
      return display.confirmed ? ' (confirmed)' : null;
  }
}
