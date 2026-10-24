import { Badge, VisuallyHidden } from '@mantine/core';
import { compactPlatformLabel, isPlatformChange, platformLabel } from '@/lib/platformLabel';
import type { PlatformBasis, PlatformStatus } from '@/lib/types';
import { PhoneLabel } from './PhoneLabel';

/** A small colour+text badge naming a calling point's Darwin/LDBWS
 * platform, shared between `ScheduleRow.tsx` (list-view rows) and
 * `JourneyTimeline.tsx`/`JourneyProgress.tsx` (the single-train journey
 * page) so the "planned vs current, and whether it changed" convention
 * can't drift between the two consumers.
 *
 * Renders nothing at all when `platform` is `null` -- "not known" (see
 * `JourneyStop.platform`'s own doc comment for why that's the honest
 * answer for most calling points) is a genuinely different fact from "on
 * platform X", and this component never fabricates one to fill the gap.
 *
 * WCAG 1.4.1 (colour is never the only signal): when the platform HAS
 * changed, the badge's colour (blue, vs. the neutral grey of an
 * unchanged platform) is only ever a secondary cue -- the badge's own
 * TEXT always names both the current platform and the one originally
 * planned ("Platform 9 (changed from 6)"), so the fact survives even
 * without colour vision, a screen reader, or a black-and-white printout.
 * `data-platform-changed` mirrors this codebase's other CSS-attribute
 * test hooks (`StatusRow`'s `data-status-row`) rather than relying on
 * colour or text content alone to prove the state in a test.
 *
 * Review §2.4/I20: this used to be `orange`, the exact colour a filled
 * delay badge sits next to on the same departure row ("+11 MIN") --
 * two adjacent orange badges for two unrelated facts ("where to stand"
 * vs. "how late") read at a glance as "two warnings about the same
 * thing". `blue` is the app's colour for a planned or changed
 * arrangement -- not a fault, but different from the timetable (planned
 * closures, "Planned Work"; docs/style-guide.md "Semantic roles") --
 * leaving orange/red free to mean lateness/cancellation only, the one
 * axis the rest of the app already reserves them for.
 *
 * A `platformStatus` of `cancelled` (a journey stop whose own board row
 * Darwin lists as cancelled: the platform it was allocated, but the train
 * no longer calls there) renders red -- the app's cancellation colour, as
 * in `ScheduleRow`'s "Cancelled" badge -- with the platform text struck
 * through. Neither cue is announced by a screen reader, so a visually
 * hidden " (cancelled)" completes the text: "Platform 4 (cancelled)".
 * Any other status (including one this client doesn't know yet) renders
 * as a normal platform.
 *
 * A stop with no live `platform` but a timetable `bookedPlatform` on
 * `platformBasis` `booked` (DS-127) shows that booked platform as
 * provisional: a grey outline badge with the number underlined in dots,
 * the tooltip "Booked platform, not yet shown on station boards", and a
 * visually hidden " (booked, not confirmed)" so a screen reader hears it
 * too ("Platform 4 (booked, not confirmed)"). `data-platform-basis`
 * marks it for tests.
 *
 * A live platform on `platformBasis` `confirmed` (DS-152: the Darwin
 * Push Port's confirmed flag) keeps the board style plus a check glyph,
 * with the tooltip "Platform confirmed by the operator" and a visually
 * hidden ", confirmed" ("Platform 4, confirmed"); the glyph itself is
 * hidden from screen readers. A changed platform that is also confirmed
 * stays blue, with the check.
 *
 * `compact` (under a "Platform" column header, `JourneyTimeline`) shows
 * the short form below `sm`: "4", or "9 (was 6)" for a change, so the
 * change is still in words. Screen readers keep the full label
 * (`PhoneLabel`). `data-platform-badge` opts the badge out of Mantine's
 * ellipsis (`app/globals.css`): it is never cut short, whatever its
 * width. */
/** The confirmed badge's tooltip (DS-152). */
export const CONFIRMED_PLATFORM_TOOLTIP = 'Platform confirmed by the operator';

/** The booked badge's tooltip (DS-127). */
export const BOOKED_PLATFORM_TOOLTIP = 'Booked platform, not yet shown on station boards';

export function PlatformBadge({
  platform,
  plannedPlatform,
  platformChanged,
  platformStatus = null,
  bookedPlatform = null,
  platformBasis = null,
  compact = false,
}: {
  platform: string | null;
  plannedPlatform: string | null;
  platformChanged: boolean;
  platformStatus?: PlatformStatus | null | undefined;
  bookedPlatform?: string | null | undefined;
  platformBasis?: PlatformBasis | null | undefined;
  compact?: boolean;
}) {
  if (platform === null) {
    if (platformBasis !== 'booked' || !bookedPlatform) return null;
    const full = `Platform ${bookedPlatform}`;
    return (
      <Badge
        color="gray"
        variant="outline"
        tt="none"
        data-platform-badge
        data-platform-changed={false}
        data-platform-basis="booked"
        title={BOOKED_PLATFORM_TOOLTIP}
      >
        <span className="platformBadge__booked">
          {compact ? <PhoneLabel full={full} compact={bookedPlatform} /> : full}
        </span>
        <VisuallyHidden> (booked, not confirmed)</VisuallyHidden>
      </Badge>
    );
  }

  const facts = { platform, plannedPlatform, platformChanged };
  const changed = isPlatformChange(facts);
  const fullLabel = platformLabel(facts);
  const label = compact ? <PhoneLabel full={fullLabel} compact={compactPlatformLabel(facts)} /> : fullLabel;

  if (platformStatus === 'cancelled') {
    return (
      <Badge
        color="red"
        variant="light"
        tt="none"
        data-platform-badge
        data-platform-changed={changed}
        data-platform-cancelled="true"
        title="Cancelled: the train no longer calls at this platform"
      >
        <s>{label}</s>
        <VisuallyHidden> (cancelled)</VisuallyHidden>
      </Badge>
    );
  }

  const confirmed = platformBasis === 'confirmed';
  return (
    <Badge
      color={changed ? 'blue' : 'gray'}
      variant="light"
      tt="none"
      data-platform-badge
      data-platform-changed={changed}
      data-platform-basis={confirmed ? 'confirmed' : 'board'}
      title={confirmed ? CONFIRMED_PLATFORM_TOOLTIP : undefined}
      leftSection={confirmed ? <span aria-hidden="true">✓</span> : undefined}
    >
      {label}
      {confirmed && <VisuallyHidden>, confirmed</VisuallyHidden>}
    </Badge>
  );
}
