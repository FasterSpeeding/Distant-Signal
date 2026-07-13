'use client';

import { ActionIcon, Stack, Text, Tooltip } from '@mantine/core';

/** Purely presentational — the detail page (a Server Component) fetches
 * the definition and passes it in as props, rather than this component
 * fetching it itself, so no client-side proxy round-trip is needed just
 * to show a tooltip. */
export function LineDefinitionTooltip({ stations, operators }: { stations: string[]; operators: string[] }) {
  return (
    <Tooltip
      label={
        <Stack gap={2}>
          <Text size="xs">Stations: {stations.join(', ')}</Text>
          <Text size="xs">Operators: {operators.join(', ')}</Text>
        </Stack>
      }
      multiline
      maw={320}
    >
      <ActionIcon variant="subtle" aria-label="How this line is defined">
        ⓘ
      </ActionIcon>
    </Tooltip>
  );
}
