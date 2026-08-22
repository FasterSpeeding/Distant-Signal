import { Badge } from '@mantine/core';
import { severityColor, severityLabel } from '@/lib/severity';

/** `data-status-badge` is the hook for the rule in `app/globals.css` that
 * opts this badge out of Mantine's `overflow: hidden` +
 * `text-overflow: ellipsis` — see that rule's comment. Kept as a plain
 * data attribute rather than a `className` so a call site can still pass
 * its own `className` without having to remember to merge this one in. */
export function StatusBadge({ severity }: { severity: number }) {
  return (
    <Badge color={severityColor(severity)} variant="filled" data-status-badge>
      {severityLabel(severity)}
    </Badge>
  );
}
