import { StateBadge } from './StateBadge';
import { serviceModeLabel } from '@/lib/serviceMode';
import type { ServiceMode } from '@/lib/types';
import { ServiceModeIcon } from './ServiceModeIcon';

/** "Rail replacement bus" / "Bus service" / "Ferry", each with its own
 * icon (`ServiceModeIcon`). Renders nothing for a train, or when the
 * backend predates `serviceMode` -- so it can sit unconditionally beside
 * any service row. A neutral `StateBadge` (round 2): the kind of service
 * is a fact, not a brand mark, so it is gray like other neutral states;
 * the icon and the word carry the difference. */
export function ServiceModeBadge({ mode }: { mode: ServiceMode | null | undefined }) {
  const label = serviceModeLabel(mode);
  if (!mode || !label) {
    return null;
  }
  return (
    <StateBadge tone="neutral" leftSection={<ServiceModeIcon mode={mode} size={12} />} data-service-mode={mode}>
      {label}
    </StateBadge>
  );
}
