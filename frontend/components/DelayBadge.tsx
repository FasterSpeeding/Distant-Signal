import { Badge } from '@mantine/core';
import { delayLabel, type DelayTone } from '@/lib/serviceStatus';

/** Each delay tone's light-badge colour. Every light-variant text colour
 * is pinned in `app/globals.css` to >= 4.5:1 on its own tint. */
const COLOR: Record<DelayTone, string> = { late: 'orange', early: 'teal', onTime: 'green' };

/** The app's delay badge, worded by `lib/serviceStatus.ts`: "12 min late"
 * orange, "3 min early" teal, "On time" green -- a different word for each
 * state, never colour alone. `tt="none"`: Mantine's default 11px uppercase
 * measured borderline for contrast, and these are phrases with units. */
export function DelayBadge({ delayMinutes, provisional = false }: { delayMinutes: number; provisional?: boolean }) {
  const delay = delayLabel(delayMinutes, provisional);
  return (
    <Badge color={COLOR[delay.tone]} variant="light" tt="none">
      {delay.text}
    </Badge>
  );
}
