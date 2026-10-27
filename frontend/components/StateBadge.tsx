import type { ReactNode } from 'react';
import { Badge, Group, type MantineColor, type MantineSize } from '@mantine/core';

/** What a state badge says about the thing it labels. Each tone is one
 * colour from the style guide's semantic roles, so a call site picks a
 * meaning, never a colour. */
export type StateTone =
  /** Informational, neutral, unknown: "Ended", "Timetabled only". */
  | 'neutral'
  /** Good, on time, running: "Active", "On time". */
  | 'good'
  /** Minor disruption, caution, partial: "Delayed". */
  | 'caution'
  /** Running late, a skipped stop, a disruption's impact: "Not stopping". */
  | 'late'
  /** Early: "3 min early". */
  | 'early'
  /** A planned or changed arrangement, not a fault: "Planned work". */
  | 'planned'
  /** Cancelled, severe, failed: "Cancelled", "Skipped". */
  | 'critical'
  /** The visitor needs to act, or a product label: "Needs a train picked". */
  | 'action';

export const STATE_TONE_COLOR: Record<StateTone, MantineColor> = {
  neutral: 'gray',
  good: 'green',
  caution: 'yellow',
  late: 'orange',
  early: 'teal',
  planned: 'blue',
  critical: 'red',
  action: 'grape',
};

/** The one badge for a thing's state (round 2, item 3): light variant,
 * sentence case, coloured by `tone`. A line's severity is `StatusBadge`
 * (filled) instead, and AI-shaped content is `AiGeneratedBadge`; a row
 * shows at most one of each, in that order (`BadgeRow`).
 *
 * `href` makes the badge an in-page anchor (a journey's "Needs a train
 * picked" jumps to the leg). `data-state-badge` carries the tone, as a
 * test and CSS hook; other `data-*` attributes pass through.
 * `aria-hidden` hides a badge whose words a fuller sentence beside it
 * already gives a screen reader (`TrackedRowStatusBadge`'s cancellation). */
export function StateBadge({
  tone = 'neutral',
  size,
  title,
  href,
  leftSection,
  children,
  'aria-hidden': ariaHidden,
  ...data
}: {
  tone?: StateTone;
  size?: MantineSize;
  title?: string;
  href?: string;
  /** A 12px icon before the label (`ServiceModeBadge`). */
  leftSection?: ReactNode;
  children: ReactNode;
  'aria-hidden'?: boolean;
} & Record<`data-${string}`, string | undefined>) {
  const props = {
    ...data,
    color: STATE_TONE_COLOR[tone],
    variant: 'light',
    tt: 'none',
    ...(size !== undefined && { size }),
    ...(title !== undefined && { title }),
    ...(leftSection !== undefined && { leftSection }),
    ...(ariaHidden !== undefined && { 'aria-hidden': ariaHidden }),
    'data-state-badge': tone,
  } as const;
  return href === undefined ? (
    <Badge {...props}>{children}</Badge>
  ) : (
    <Badge {...props} component="a" href={href}>
      {children}
    </Badge>
  );
}

/** A row's badges, at most three, always in the order the style guide
 * fixes: the severity (`StatusBadge`), then the item's state
 * (`StateBadge`), then "AI summary" (`AiGeneratedBadge`). Each slot takes
 * one badge or nothing; the row renders nothing when all three are empty.
 * Taking slots rather than children is what makes a fourth badge, or a
 * different order, impossible to write. */
export function BadgeRow({ severity, state, ai }: { severity?: ReactNode; state?: ReactNode; ai?: ReactNode }) {
  const slots = [severity, state, ai].filter((slot) => slot !== undefined && slot !== null && slot !== false);
  if (slots.length === 0) return null;
  return (
    <Group gap={6} wrap="wrap" data-badge-row>
      {severity}
      {state}
      {ai}
    </Group>
  );
}
