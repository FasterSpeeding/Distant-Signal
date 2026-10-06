import { Badge } from '@mantine/core';
import { serviceModeLabel } from '@/lib/serviceMode';
import type { ServiceMode } from '@/lib/types';
import { ServiceModeIcon } from './ServiceModeIcon';

/** "Rail replacement bus" / "Bus service" / "Ferry", each with its own
 * icon (`ServiceModeIcon`). Renders nothing for a train, or when the
 * backend predates `serviceMode` -- so it can sit unconditionally beside
 * any service row. `tt="none"` like the app's other badges (Mantine's
 * uppercase 11px default is hard to read). */
export function ServiceModeBadge({ mode }: { mode: ServiceMode | null | undefined }) {
  const label = serviceModeLabel(mode);
  if (!mode || !label) {
    return null;
  }
  return (
    <Badge
      color={mode === 'ferry' ? 'cyan' : 'grape'}
      variant="light"
      tt="none"
      leftSection={<ServiceModeIcon mode={mode} size={12} />}
      data-service-mode={mode}
    >
      {label}
    </Badge>
  );
}
