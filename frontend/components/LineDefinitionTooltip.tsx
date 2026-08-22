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
      // only direction with room. That positional change is the actual
      // fix; the tooltip is already portalled to <body> and the nav
      // creates no stacking context, so it was never losing a z-index
      // fight. `zIndex` is still bumped one level above Mantine's default
      // popover z-index (300) to Mantine's "overlay" elevation (400) —
      // a deliberate, if largely belt-and-braces, stacking-order bump.
      position="bottom"
      zIndex={400}
    >
      <ActionIcon variant="subtle" aria-label="How this line is defined">
        <InfoIcon />
      </ActionIcon>
    </Tooltip>
  );
}
