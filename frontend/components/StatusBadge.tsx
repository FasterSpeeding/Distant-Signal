import { Badge } from '@mantine/core';
import { severityColor, severityLabel } from '@/lib/severity';

export function StatusBadge({ severity }: { severity: number }) {
  return (
    <Badge color={severityColor(severity)} variant="filled">
      {severityLabel(severity)}
    </Badge>
  );
}
