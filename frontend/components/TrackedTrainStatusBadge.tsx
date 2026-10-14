import { Group } from '@mantine/core';
import { StateBadge } from './StateBadge';
import { ServiceModeBadge } from './ServiceModeBadge';
import { isTimetableOnly } from '@/lib/serviceMode';
import { DelayBadge } from './DelayBadge';
import type { ServiceModeFields } from '@/lib/types';

// Short, human badge words -- the single copy every page that renders a
// tracked train's status badge shares. Previously duplicated verbatim
// across `app/page.tsx`, `app/track/mine/page.tsx` and (as a raw
// `train.status` string literal, with no words at all) `app/groups/[id]/
// page.tsx`'s shared-train card -- three independent places that could
// silently drift from each other over one enum's wording. See review
// §2.9's "shared display-label layer" recommendation. Falls back to the
// raw token itself for anything unlisted, so an unexpected value never
// disappears from the badge.
const STATUS_LABELS: Record<string, string> = {
  pending: 'Pending match',
  schedule_matched: 'Matched to schedule',
  unresolved: 'Unmatched',
  awaiting_activation: 'Not yet started',
  en_route: 'En route',
  completed: 'Completed',
  cancelled: 'Cancelled',
};

/** Structural rather than `TrackedTrainListItem`: a `GroupTrain`/
 * `SharedGroupTrain`'s `resolutionStatus`/`status` are plain `string`s on
 * the wire rather than the own-list's narrowed unions, and both shapes
 * satisfy this since the branching below already treats every value as an
 * opaque token (`STATUS_LABELS` falls back to the raw string for anything
 * unlisted). Renders nothing else -- callers place it, typically in a
 * `StatusRow`'s `trailing` slot. */
export function TrackedTrainStatusBadge({
  train,
}: {
  train: { resolutionStatus: string; status: string | null; delayMinutes: number | null } & ServiceModeFields;
}) {
  // A tracked bus or ferry never moves past "matched to schedule": say what
  // it is and that it is timetabled only, instead of a status that reads
  // as stuck.
  if (isTimetableOnly(train)) {
    return (
      <Group gap={6} wrap="nowrap">
        <ServiceModeBadge mode={train.serviceMode} />
        <StateBadge tone="neutral">Timetabled only</StateBadge>
      </Group>
    );
  }
  // pending/unresolved show the resolution status itself -- no journey
  // status exists yet for either. Once resolved, the journey status plus a
  // delay badge takes over. No "active only" filter and no attempt to
  // distinguish a genuinely-finished journey from one that's merely gone
  // quiet -- per Decision 1/Finding 1 of the tracked-train design spec, the
  // backend can't honestly support that distinction today.
  if (train.resolutionStatus !== 'resolved') {
    return (
      <StateBadge tone={train.resolutionStatus === 'unresolved' ? 'critical' : 'neutral'}>
        {STATUS_LABELS[train.resolutionStatus] ?? train.resolutionStatus}
      </StateBadge>
    );
  }
  return (
    <Group gap={6} wrap="nowrap">
      {train.status && (
        <StateBadge tone={train.status === 'cancelled' ? 'critical' : 'neutral'}>
          {STATUS_LABELS[train.status] ?? train.status}
        </StateBadge>
      )}
      {train.delayMinutes !== null && <DelayBadge delayMinutes={train.delayMinutes} />}
    </Group>
  );
}
