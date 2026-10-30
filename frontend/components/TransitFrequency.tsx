import { SimpleGrid, Stack, Table, TableTbody, TableTd, TableTh, TableTr, Text } from '@mantine/core';
import { SectionTitle } from '@/components/SectionTitle';
import { SCHEDULED_CAPTION } from '@/lib/transitStops';
import { FREQUENCY_BAND_LABELS, frequencyText } from '@/lib/transitModes';
import type { TransitDirection, TransitLineFrequency } from '@/lib/types';

/** How often a line runs today (the TfL transit design §8.3): for each
 * direction (headed by `headings`, "Towards Brixton"), its first and last
 * departure from its busiest stop, and how often it leaves there in each
 * part of the day ("Every 2 to 4 minutes"). From the timetable, so it says
 * nothing about delays. */
export function TransitFrequency({
  frequency,
  headings,
}: {
  frequency: TransitLineFrequency;
  headings: Partial<Record<TransitDirection, string>>;
}) {
  if (frequency.outsideHorizon || frequency.directions.length === 0) return null;
  return (
    <Stack gap="sm">
      <SectionTitle>How often it runs today</SectionTitle>
      <Text size="sm" c="dimmed">
        {SCHEDULED_CAPTION}
      </Text>
      <SimpleGrid cols={{ base: 1, sm: frequency.directions.length > 1 ? 2 : 1 }} spacing="lg">
        {frequency.directions.map((direction) => (
          <Stack key={direction.direction} gap="xs">
            <SectionTitle order={3}>{headings[direction.direction] ?? direction.station.name}</SectionTitle>
            <Text size="sm">
              From {direction.station.name}: first {direction.first}, last {direction.last}
            </Text>
            <Table fz="sm" withRowBorders={false} verticalSpacing={4} maw={480}>
              <TableTbody>
                {direction.bands.map((band) => (
                  <TableTr key={band.band}>
                    <TableTh scope="row" fw={500}>
                      {FREQUENCY_BAND_LABELS[band.band]}{' '}
                      <Text span size="xs" c="dimmed">
                        {band.from} to {band.to}
                      </Text>
                    </TableTh>
                    <TableTd>{frequencyText(band)}</TableTd>
                  </TableTr>
                ))}
              </TableTbody>
            </Table>
          </Stack>
        ))}
      </SimpleGrid>
    </Stack>
  );
}
