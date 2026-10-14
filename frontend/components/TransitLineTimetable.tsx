import {
  ScrollArea,
  Stack,
  Table,
  TableTbody,
  TableTd,
  TableTh,
  TableThead,
  TableTr,
  Text,
  VisuallyHidden,
} from '@mantine/core';
import { ServiceListNotice } from '@/components/ServiceRow';
import { SCHEDULED_CAPTION } from '@/lib/transit';
import type { TransitLineTimetable as Timetable } from '@/lib/types';

/** A tram line's timetable for one direction and time window: stops down
 * the side, one column per tram, its scheduled time at each stop (an en
 * dash where it doesn't call). Scrolls sideways in its own container, so
 * the page never does. */
export function TransitLineTimetable({ timetable }: { timetable: Timetable }) {
  if (timetable.outsideHorizon) {
    return <ServiceListNotice>The timetable for this date isn&apos;t published yet.</ServiceListNotice>;
  }
  if (timetable.trips.length === 0) {
    return <ServiceListNotice>No scheduled trams in this window.</ServiceListNotice>;
  }
  return (
    <Stack gap="xs">
      <Text size="sm" c="dimmed">
        {SCHEDULED_CAPTION}
      </Text>
      <ScrollArea type="auto" offsetScrollbars>
        <Table striped withTableBorder withColumnBorders fz="sm" ff="monospace" miw="max-content">
          <TableThead>
            <TableTr>
              <TableTh scope="col">Stop</TableTh>
              {timetable.trips.map((trip, i) => (
                <TableTh key={trip.tripId} scope="col">
                  <VisuallyHidden>Tram </VisuallyHidden>
                  {i + 1}
                </TableTh>
              ))}
            </TableTr>
          </TableThead>
          <TableTbody>
            {timetable.stations.map((station, row) => (
              <TableTr key={station.id}>
                <TableTh scope="row" ff="text">
                  {station.name}
                </TableTh>
                {timetable.trips.map((trip) => {
                  const time = trip.times[row] ?? null;
                  return (
                    <TableTd key={trip.tripId}>
                      {time ?? (
                        <>
                          <span aria-hidden="true">–</span>
                          <VisuallyHidden>Doesn&apos;t call</VisuallyHidden>
                        </>
                      )}
                    </TableTd>
                  );
                })}
              </TableTr>
            ))}
          </TableTbody>
        </Table>
      </ScrollArea>
    </Stack>
  );
}
