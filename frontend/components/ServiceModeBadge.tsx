import { StateBadge } from './StateBadge';
import { serviceModeLabel, transitServiceModeLabel } from '@/lib/serviceMode';
import type { ServiceMode, TripPlanTransitServiceMode } from '@/lib/types';
import { ServiceModeIcon } from './ServiceModeIcon';

/** "Rail replacement bus" / "Bus service" / "Ferry", each with its own
 * icon (`ServiceModeIcon`). Renders nothing for a train, or when the
 * backend predates `serviceMode` -- so it can sit unconditionally beside
 * any service row. A neutral `StateBadge` (round 2): the kind of service
 * is a fact, not a brand mark, so it is gray like other neutral states;
 * the icon and the word carry the difference.
 *
 * `transitMode` instead labels a planned transit leg: "Tram", or
 * "Replacement bus" for a bus standing in for a tram. */
export function ServiceModeBadge({
  mode,
  transitMode,
}: {
  mode?: ServiceMode | null | undefined;
  transitMode?: TripPlanTransitServiceMode | undefined;
}) {
  const label = transitMode ? transitServiceModeLabel(transitMode) : serviceModeLabel(mode);
  const iconMode = transitMode ?? mode;
  if (!iconMode || !label) {
    return null;
  }
  return (
    <StateBadge tone="neutral" leftSection={<ServiceModeIcon mode={iconMode} size={12} />} data-service-mode={iconMode}>
      {label}
    </StateBadge>
  );
}
