import { Badge } from '@mantine/core';

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
 * changed, the badge's colour (orange, vs. the neutral grey of an
 * unchanged platform) is only ever a secondary cue -- the badge's own
 * TEXT always names both the current platform and the one originally
 * planned ("Platform 9 (changed from 6)"), so the fact survives even
 * without colour vision, a screen reader, or a black-and-white printout.
 * `data-platform-changed` mirrors this codebase's other CSS-attribute
 * test hooks (`StatusRow`'s `data-status-row`) rather than relying on
 * colour or text content alone to prove the state in a test. */
export function PlatformBadge({
  platform,
  plannedPlatform,
  platformChanged,
}: {
  platform: string | null;
  plannedPlatform: string | null;
  platformChanged: boolean;
}) {
  if (platform === null) return null;

  const changed = platformChanged && plannedPlatform !== null;
  const label = changed ? `Platform ${platform} (changed from ${plannedPlatform})` : `Platform ${platform}`;

  return (
    <Badge color={changed ? 'orange' : 'gray'} variant="light" tt="none" data-platform-changed={changed}>
      {label}
    </Badge>
  );
}
