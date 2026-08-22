import { Box, Text } from '@mantine/core';

/** Attribution for the third-party open data this app republishes.
 *
 * TfL publishes its Unified API data under a modified Open Government
 * Licence v2.0 whose attribution clause is a condition of use, not a
 * courtesy: "Powered by TfL Open Data" has to appear wherever the data is
 * presented. The wording is fixed — do not paraphrase it.
 *
 * The licence also asks for Ordnance Survey and Geomni attributions where
 * the data used is derived from theirs. That applies to TfL's *geographic*
 * data — StopPoint coordinates, maps, route geometry — and this app ingests
 * none of it: v1 is line status only (see
 * `docs/superpowers/plans/2026-08-22-tfl-line-status-integration.md`). If
 * stop-level TfL data is ever added, those two lines have to be added here
 * with it.
 *
 * A plain Server Component with no interactivity, rendered once by the root
 * layout so it is on every page. */
export function OpenDataAttribution() {
  return (
    <Box
      component="footer"
      p="md"
      style={{ borderTop: '1px solid var(--mantine-color-default-border)' }}
    >
      <Text size="xs" c="dimmed">
        Powered by TfL Open Data
      </Text>
    </Box>
  );
}
