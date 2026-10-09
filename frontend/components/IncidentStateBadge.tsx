import { Badge, VisuallyHidden } from '@mantine/core';
import { endedDescription, incidentState } from '@/lib/incidents';

/** An incident's lifecycle badge, shared by the archive rows
 * (`IncidentSearchForm`) and the detail page (`app/incidents/[id]`) so the
 * two can't drift:
 * - Active: green, as before;
 * - Cleared: filled gray, as before -- RDM cleared it;
 * - Ended: LIGHT gray -- the Knowledgebase feed stopped listing it without
 *   clearing it (2026-10-06,
 *   docs/superpowers/specs/2026-10-06-incident-source-removal-design.md).
 *   Gray because it is informational ("no longer live, outcome not
 *   stated"; docs/style-guide.md "Semantic roles"), light rather than filled
 *   so it doesn't read as a second "Cleared".
 *
 * WCAG 1.4.1: the word on the badge carries the state, never the colour.
 * An ended badge also says WHEN, as a `title` tooltip for pointer users
 * and as visually hidden text for screen readers, which don't announce
 * `title` reliably (`PlatformBadge`'s cancelled badge does the same).
 * `data-incident-state` is the test hook. */
export function IncidentStateBadge({
  isCleared,
  sourceRemovedAt,
}: {
  isCleared: boolean;
  sourceRemovedAt?: string | null | undefined;
}) {
  const state = incidentState({ isCleared, sourceRemovedAt });
  if (state === 'cleared') {
    return (
      <Badge tt="none" color="gray" data-incident-state="cleared">
        Cleared
      </Badge>
    );
  }
  if (state === 'ended' && sourceRemovedAt) {
    const description = endedDescription(sourceRemovedAt);
    return (
      <Badge tt="none" color="gray" variant="light" data-incident-state="ended" title={description}>
        Ended
        <VisuallyHidden>: {description}</VisuallyHidden>
      </Badge>
    );
  }
  return (
    <Badge tt="none" color="green" data-incident-state="active">
      Active
    </Badge>
  );
}
