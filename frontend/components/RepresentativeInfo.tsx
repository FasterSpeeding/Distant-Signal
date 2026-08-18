'use client';

import { ActionIcon, Card, Group, Text, Tooltip } from '@mantine/core';
import type { LineStatus } from '@/lib/types';

/** Shown only when at least one status carries `sampleStats` — the
 * aggregator attaches the same sample-derived stats to every status on a
 * line's report, so the first one found is representative of all of them.
 * Omitted entirely (not zeroed out) when none do.
 *
 * Marked `'use client'` purely for the `Tooltip`/`ActionIcon` info trigger
 * (same pattern as `DataFreshnessInfo` and `LineDefinitionTooltip`), even
 * though it's rendered from Server Component detail pages. */
export function RepresentativeInfo({ statuses }: { statuses: LineStatus[] }) {
  const withStats = statuses.find((status) => status.sampleStats);
  if (!withStats?.sampleStats) return null;

  const { total, delayed, cancelled, skipped, avgDelayMinutes } = withStats.sampleStats;

  return (
    <Card withBorder padding="sm">
      <Group gap="xs" wrap="nowrap">
        <Text size="sm">
          {delayed} of {total} sampled services delayed, {cancelled} cancelled, {skipped} skipping stops, avg{' '}
          {avgDelayMinutes.toFixed(1)} min late.
        </Text>
        <Tooltip
          label="These figures come from a single status on this line's report. The aggregator attaches the same sample data to every status, so this one status is representative of them all."
          multiline
          maw={280}
        >
          <ActionIcon variant="subtle" aria-label="About these sample statistics">
            {/* `@tabler/icons-react` isn't a project dependency (checked
             * package.json) — inline SVG rather than the literal "ⓘ"
             * character, which renders as a broken-looking glyph on an
             * emoji/font fallback. Same icon as `DataFreshnessInfo` and
             * `LineDefinitionTooltip`. */}
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
      </Group>
    </Card>
  );
}
