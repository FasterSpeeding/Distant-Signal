'use client';

import { Accordion, AccordionControl, AccordionItem, AccordionPanel, Stack, Text } from '@mantine/core';

/** Collapsed-by-default "Scheduled departures" section for the station page
 * (`frontend/app/stations/[crs]/page.tsx`), listing CIF-schedule-derived
 * departures for `crs` via the existing, unchanged
 * `GET /public/trains/search?station=<crs>` route -- see
 * docs/superpowers/specs/2026-09-12-station-timetable-design.md.
 *
 * `keepMounted={false}` on the panel matches `IssueList.tsx`'s own
 * documented reasoning verbatim: Mantine v9 keeps a collapsed panel's
 * content mounted (via the Activity API) purely visually hidden by
 * default, which `screen.queryByText` (and a screen reader in "not
 * visible" mode, inconsistently) can still find. `keepMounted={false}`
 * makes "collapsed by default" actually mean "not rendered" until first
 * expanded.
 *
 * One `AccordionItem`, not `multiple`: there is only one section here,
 * unlike `IssueList.tsx`'s per-status accordion. */
export function StationTimetable({ crs }: { crs: string }) {
  return (
    <Accordion keepMounted={false}>
      <AccordionItem value="scheduled-departures">
        <AccordionControl>Scheduled departures</AccordionControl>
        <AccordionPanel>
          <Stack gap="xs">
            <Text size="sm" c="dimmed">
              Loading scheduled departures…
            </Text>
          </Stack>
        </AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}
