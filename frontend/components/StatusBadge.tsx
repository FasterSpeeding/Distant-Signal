import { Badge } from '@mantine/core';
import { severityBadgeLabel, severityColor } from '@/lib/severity';

/** `data-status-badge` is the hook for the rule in `app/globals.css` that
 * opts this badge out of Mantine's `overflow: hidden` +
 * `text-overflow: ellipsis` — see that rule's comment. Kept as a plain
 * data attribute rather than a `className` so a call site can still pass
 * its own `className` without having to remember to merge this one in.
 * Sentence case like every badge (`tt="none"`, `severityBadgeLabel`). */
export function StatusBadge({ severity }: { severity: number }) {
  return (
    <Badge color={severityColor(severity)} variant="filled" tt="none" data-status-badge>
      {severityBadgeLabel(severity)}
    </Badge>
  );
}
