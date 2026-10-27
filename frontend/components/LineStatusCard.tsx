import { Text } from '@mantine/core';
import { StatusBadge } from './StatusBadge';
import { AiGeneratedBadge, ENRICHED_INCIDENT_SHORT_NOTE, isEnricherInfluenced } from './AiGeneratedBadge';
import { LastUpdated } from './LastUpdated';
import { UpcomingDisruptions } from './UpcomingDisruptions';
import { runsNoTrains, worstStatus } from '@/lib/severity';
import { representativeStatus, formatSampleSummary } from '@/lib/sampleStats';
import type { LineStatusReport } from '@/lib/types';
import { StatusCard } from './StatusCard';
import { BadgeRow } from './StateBadge';

/** `showUpdated={false}` where the page states one "Updated" time for
 * every card (`/status`). */
export function LineStatusCard({ report, showUpdated = true }: { report: LineStatusReport; showUpdated?: boolean }) {
  const worst = worstStatus(report);
  const representative = representativeStatus(report.lineStatuses);
  return (
    <StatusCard
      href={`/lines/${report.id}`}
      title={report.name}
      badge={<StatusBadge severity={worst.statusSeverity} />}
      reason={worst.reason}
    >
      {/* LEG-16: the worst status's severity and reason may have been
          shaped by the incident enricher's LLM. */}
      {'disruption' in worst && isEnricherInfluenced(worst.disruption.source) && (
        <BadgeRow ai={<AiGeneratedBadge note={ENRICHED_INCIDENT_SHORT_NOTE} />} />
      )}
      <Text size="xs" c="dimmed">
        {runsNoTrains(worst.statusSeverity) ? 'No trains running' : formatSampleSummary(representative)}
      </Text>
      {/* A note, not a status: the badge above never reflects it. */}
      <UpcomingDisruptions upcoming={report.upcoming} compact />
      {showUpdated && <LastUpdated timestamp={report.computedAt} />}
    </StatusCard>
  );
}
