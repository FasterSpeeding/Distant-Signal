'use client';

import { ActionIcon, Stack, Text, Tooltip } from '@mantine/core';
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
        </Stack>
      }
      multiline
      maw={280}
    >
      <ActionIcon variant="subtle" aria-label="Data freshness">
        ⓘ
      </ActionIcon>
    </Tooltip>
  );
}
