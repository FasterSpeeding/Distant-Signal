'use client';

import { ActionIcon, Stack, Text, Tooltip } from '@mantine/core';
import { InfoIcon } from './InfoIcon';
import { LastUpdated } from './LastUpdated';
import type { DataFreshness } from '@/lib/types';

function freshnessRow(label: string, timestamp: string | null) {
  if (timestamp === null) {
    return (
      <Text size="xs" c="dimmed" key={label}>
        {label}: never fetched
      </Text>
    );
  }
  return <LastUpdated key={label} timestamp={timestamp} label={`${label}:`} withTooltip={false} />;
}

/** Nav-bar info icon for the freshness of the three data sources feeding
 * the aggregator (as opposed to `LastUpdated` on each line card, which
 * shows when that line's own status was last computed). Same
 * `ActionIcon` + `Tooltip` pattern as `LineDefinitionTooltip`. Each row
 * reuses `LastUpdated` with `withTooltip={false}` — nesting a
 * `Tooltip`-wrapped element inside this outer `Tooltip`'s own `label`
 * wouldn't be hoverable (the outer tooltip closes as the pointer leaves
 * the icon), so only the outer tooltip shows on hover here. */
export function DataFreshnessInfo({ freshness }: { freshness: DataFreshness }) {
  return (
    <Tooltip
      label={
        <Stack gap={2}>
          {freshnessRow('Stations', freshness.stations)}
          {freshnessRow('TOCs', freshness.tocs)}
          {freshnessRow('Incidents', freshness.incidents)}
          {freshnessRow('TfL', freshness.tfl)}
          {freshnessRow('Schedule feed', freshness.schedule_feed)}
        </Stack>
      }
      multiline
      maw={280}
      events={{ hover: true, focus: true, touch: true }}
      // Same fix as `LineDefinitionTooltip` (open downward, not over the
      // nav), but `bottom-end` rather than `bottom`: this trigger sits in
      // the nav's right-hand group, so a centred tooltip would hang off
      // the right edge of the viewport.
      position="bottom-end"
      zIndex={400}
    >
      <ActionIcon variant="subtle" aria-label="Data freshness">
        <InfoIcon />
      </ActionIcon>
    </Tooltip>
  );
}
