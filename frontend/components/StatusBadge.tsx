import { Badge } from '@mantine/core';
import { severityBadgeLabel, severityGroup, statusBadgeColors } from '@/lib/severity';

/** `data-status-badge` is the hook for the rule in `app/globals.css` that
 * opts this badge out of Mantine's `overflow: hidden` +
 * `text-overflow: ellipsis` — see that rule's comment. Kept as a plain
 * data attribute rather than a `className` so a call site can still pass
 * its own `className` without having to remember to merge this one in.
 * Sentence case like every badge (`tt="none"`, `severityBadgeLabel`).
 *
 * Fill and label colour come from the group's `--ds-status-badge-*` tokens
 * (`statusBadgeColors`), never from `autoContrast`. The label is 12px
 * (`fz="xs"`) rather than Mantine's 11px: this pill is the one status
 * people read from across a list. `data-severity-group` names the group,
 * as a test hook. */
export function StatusBadge({ severity }: { severity: number }) {
  const group = severityGroup(severity);
  const { fill, text } = statusBadgeColors(group);
  return (
    <Badge color={fill} c={text} variant="filled" tt="none" fz="xs" data-status-badge data-severity-group={group}>
      {severityBadgeLabel(severity)}
    </Badge>
  );
}
