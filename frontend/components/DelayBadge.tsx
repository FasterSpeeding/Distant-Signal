import { Badge } from '@mantine/core';
import { delayLabel, type DelayTone } from '@/lib/serviceStatus';

/** Each delay tone's colour: the app's one delay-to-colour table. The
 * badge below is its main user; `JourneyProgress`'s stop markers read it
 * too, so a late stop is the same orange as a late badge. Every
 * light-variant text colour is pinned in `app/globals.css` to >= 4.5:1 on
 * its own tint. */
export const DELAY_TONE_COLOR: Record<DelayTone, 'orange' | 'teal' | 'green'> = {
  late: 'orange',
  early: 'teal',
  onTime: 'green',
};

/** The app's one delay badge (schedule rows, timelines, tracked trains,
 * the train page), worded by `lib/serviceStatus.ts`: "12 min late"
 * orange, "3 min early" teal, "On time" green -- a different word for each
 * state, never colour alone. `tt="none"`: Mantine's default 11px uppercase
 * measured borderline for contrast, and these are phrases with units. */
export function DelayBadge({ delayMinutes, provisional = false }: { delayMinutes: number; provisional?: boolean }) {
  const delay = delayLabel(delayMinutes, provisional);
  return (
    <Badge color={DELAY_TONE_COLOR[delay.tone]} variant="light" tt="none">
      {delay.text}
    </Badge>
  );
}
