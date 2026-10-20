import { VisuallyHidden } from '@mantine/core';
import { StateBadge } from './StateBadge';
import {
  endedDescription,
  incidentState,
  upcomingDescription,
  validityEndedDescription,
  type IncidentState,
} from '@/lib/incidents';
import type { ValidityPeriod } from '@/lib/types';

/** An incident's lifecycle badge, shared by the archive rows
 * (`IncidentSearchForm`) and the detail page (`app/incidents/[id]`) so the
 * two can't drift:
 * - Active: green, as before -- a validity window covers now;
 * - Upcoming: blue, the "planned" tone -- listed and uncleared, but no
 *   validity window has started yet (2026-10-20,
 *   docs/superpowers/specs/2026-10-20-incident-validity-state-design.md);
 * - Cleared: filled gray, as before -- RDM cleared it;
 * - Ended: LIGHT gray -- the Knowledgebase feed stopped listing it without
 *   clearing it (2026-10-06,
 *   docs/superpowers/specs/2026-10-06-incident-source-removal-design.md),
 *   or every validity window has finished. Gray because it is
 *   informational ("no longer live, outcome not stated";
 *   docs/style-guide.md "Semantic roles"), light rather than filled so it
 *   doesn't read as a second "Cleared".
 *
 * WCAG 1.4.1: the word on the badge carries the state, never the colour.
 * An ended or upcoming badge also says WHEN, as a `title` tooltip for
 * pointer users and as visually hidden text for screen readers, which
 * don't announce `title` reliably (`PlatformBadge`'s cancelled badge does
 * the same). `data-incident-state` is the test hook. */
export function IncidentStateBadge({
  isCleared,
  sourceRemovedAt,
  state,
  startsAt,
  validityEndedAt,
  validityPeriods,
}: {
  isCleared: boolean;
  sourceRemovedAt?: string | null | undefined;
  state?: IncidentState | null | undefined;
  startsAt?: string | null | undefined;
  validityEndedAt?: string | null | undefined;
  validityPeriods?: readonly ValidityPeriod[] | undefined;
}) {
  const resolved = incidentState({ isCleared, sourceRemovedAt, state, validityPeriods });
  if (resolved === 'cleared') {
    return (
      <StateBadge tone="neutral" data-incident-state="cleared">
        Cleared
      </StateBadge>
    );
  }
  if (resolved === 'ended') {
    const description = sourceRemovedAt
      ? endedDescription(sourceRemovedAt)
      : validityEndedAt
        ? validityEndedDescription(validityEndedAt)
        : undefined;
    return (
      <StateBadge tone="neutral" data-incident-state="ended" {...(description !== undefined && { title: description })}>
        Ended
        {description && <VisuallyHidden>: {description}</VisuallyHidden>}
      </StateBadge>
    );
  }
  if (resolved === 'upcoming') {
    const description = startsAt ? upcomingDescription(startsAt) : undefined;
    return (
      <StateBadge
        tone="planned"
        data-incident-state="upcoming"
        {...(description !== undefined && { title: description })}
      >
        Upcoming
        {description && <VisuallyHidden>: {description}</VisuallyHidden>}
      </StateBadge>
    );
  }
  return (
    <StateBadge tone="good" data-incident-state="active">
      Active
    </StateBadge>
  );
}
