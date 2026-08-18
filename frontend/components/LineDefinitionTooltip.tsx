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
        {/* `@tabler/icons-react` isn't a project dependency (checked
         * package.json) — inline SVG instead of the literal "ⓘ" character,
         * which renders as a broken-looking glyph on an emoji/font
         * fallback rather than a recognisable info symbol. Same icon as
         * `DataFreshnessInfo`. */}
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
