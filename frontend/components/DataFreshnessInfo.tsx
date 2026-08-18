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
        {/* `@tabler/icons-react` isn't a project dependency (checked
         * package.json) — inline SVG instead of the literal "ⓘ" character,
         * which renders as a broken-looking glyph on an emoji/font
         * fallback rather than a recognisable info symbol. */}
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="16" x2="12" y2="12" />
          <line x1="12" y1="8" x2="12.01" y2="8" />
        </svg>
      </ActionIcon>
    </Tooltip>
  );
}
