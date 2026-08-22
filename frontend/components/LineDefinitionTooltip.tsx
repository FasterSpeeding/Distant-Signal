'use client';

import { ActionIcon, Stack, Text, Tooltip } from '@mantine/core';
import { InfoIcon } from './InfoIcon';

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
      events={{ hover: true, focus: true, touch: true }}
      // The trigger sits immediately below the nav, so Mantine's default
      // `position="top"` opened the tooltip straight over the header (and
      // over the whole nav at 390px). Downward, into the content, is the
      // only direction with room. `zIndex` is stated rather than left to
      // the default so the intent is explicit — the tooltip is already
      // portalled to <body> and the nav creates no stacking context, so
      // this is belt-and-braces, not the actual fix.
      position="bottom"
      zIndex={400}
    >
      <ActionIcon variant="subtle" aria-label="How this line is defined">
        <InfoIcon />
      </ActionIcon>
    </Tooltip>
  );
}
