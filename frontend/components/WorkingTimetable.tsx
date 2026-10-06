'use client';

import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Stack,
  Table,
  TableScrollContainer,
  TableTbody,
  TableTd,
  TableTh,
  TableThead,
  TableTr,
  Text,
  VisuallyHidden,
} from '@mantine/core';
import { formatWorkingTime, isPassingPoint, stopDirectionLabels, workingTimeAccessibleLabel } from '@/lib/stopTimes';
import type { JourneyStop } from '@/lib/types';

/** Same flush, chrome-free disclosure styling as
 * `StationAccessibilitySection.tsx`'s `Disclosure`. */
const DISCLOSURE_STYLES = {
  item: { border: 'none', backgroundColor: 'transparent' },
  control: { padding: 0, paddingBlock: 'var(--mantine-spacing-xs)' },
  chevron: { marginInlineEnd: 'var(--mantine-spacing-xs)' },
  panel: { paddingInlineStart: 0 },
} as const;

/** A WTT time with its half-minute: `20:50½` on screen, "20:50 and a half"
 * to a screen reader. Empty for no time. */
function WorkingTime({ value }: { value: string | null | undefined }) {
  if (!value) return null;
  return (
    <>
      <span aria-hidden="true">{formatWorkingTime(value)}</span>
      <VisuallyHidden>{workingTimeAccessibleLabel(value)}</VisuallyHidden>
    </>
  );
}

/** A location's label in the detailed view: its name -- a station's, or for
 * a junction, signal or other timing point the `tiploc_locations` name the
 * API now sends ("Marylebone 10 Signal") -- else its CRS, else its bare
 * TIPLOC (a TIPLOC no source can name), which here, unlike the passenger
 * view, still belongs. */
export function locationLabel(stop: JourneyStop): string {
  return stop.name ?? stop.crs ?? stop.tiploc ?? 'Unknown location';
}

/** The train page's optional "Detailed (working timetable)" view: every
 * location the train is timed at -- stops AND passing points -- with the
 * working-timetable (WTT) times signallers run to, half-minutes included.
 * The passenger view above it shows public times only; this is the
 * Realtime Trains-style detailed view the user asked for. See
 * docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md.
 *
 * Collapsed by default, as an `Accordion` with `keepMounted={false}` -- the
 * same disclosure `StationAccessibilitySection.tsx` uses, for the same
 * reasons (a real `aria-expanded` button; the hidden table is unmounted, not
 * merely clipped). Renders nothing when no stop carries any working time
 * (a schedule stored before these existed, until the next publish). */
export function WorkingTimetable({ stops }: { stops: JourneyStop[] }) {
  const rows = stops.filter(
    (stop) => stop.workingArrival != null || stop.workingDeparture != null || stop.workingPass != null,
  );
  if (rows.length === 0) return null;

  return (
    <Accordion chevronPosition="left" keepMounted={false} styles={DISCLOSURE_STYLES}>
      <AccordionItem value="working-timetable">
        <AccordionControl>Detailed (working timetable)</AccordionControl>
        <AccordionPanel>
          <Stack gap="xs">
            <Text size="sm" c="dimmed">
              Working timetable times, as used by signallers: ½ means half a minute past. Passing points are places the
              train runs through without stopping. Times elsewhere on this page are the public timetable&apos;s.
            </Text>
            <TableScrollContainer minWidth={420}>
              <Table verticalSpacing={4} horizontalSpacing="sm" aria-label="Working timetable">
                <TableThead>
                  <TableTr>
                    <TableTh>Location</TableTh>
                    <TableTh>Arrive</TableTh>
                    <TableTh>Depart</TableTh>
                    <TableTh>Pass</TableTh>
                    <TableTh>Notes</TableTh>
                  </TableTr>
                </TableThead>
                <TableTbody>
                  {rows.map((stop, index) => {
                    const passing = isPassingPoint(stop);
                    const notes = passing ? ['Passing point'] : stopDirectionLabels(stop);
                    return (
                      <TableTr key={`${stop.tiploc ?? stop.crs ?? 'unknown'}-${index}`}>
                        <TableTd>
                          <Text size="sm" c={passing ? 'dimmed' : undefined} fs={passing ? 'italic' : undefined}>
                            {locationLabel(stop)}
                          </Text>
                        </TableTd>
                        <TableTd>
                          <Text size="sm">
                            <WorkingTime value={stop.workingArrival} />
                          </Text>
                        </TableTd>
                        <TableTd>
                          <Text size="sm">
                            <WorkingTime value={stop.workingDeparture} />
                          </Text>
                        </TableTd>
                        <TableTd>
                          <Text size="sm" c="dimmed">
                            <WorkingTime value={stop.workingPass} />
                          </Text>
                        </TableTd>
                        <TableTd>
                          <Text size="sm" c="dimmed">
                            {notes.join(', ')}
                          </Text>
                        </TableTd>
                      </TableTr>
                    );
                  })}
                </TableTbody>
              </Table>
            </TableScrollContainer>
          </Stack>
        </AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}
