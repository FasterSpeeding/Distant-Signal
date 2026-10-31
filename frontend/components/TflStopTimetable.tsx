import { Group, Stack, Table, TableTbody, TableTd, TableTh, TableTr, Text } from '@mantine/core';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { TransitStopBoard } from '@/components/TransitStopBoard';
import { formatDate } from '@/lib/dateFormat';
import type { TransitBoard, TransitStopFirstLast } from '@/lib/types';

/** A TfL stop's timetable, on its `/tfl/stops/{id}` page under the live
 * arrivals (the TfL transit design §8.4): its first and last departures
 * today per line and way, and its next scheduled departures, with the
 * links to later ones and the next day. Scheduled, so it answers "is
 * there a train at 23:40?" rather than what is coming now. */
export function TflStopTimetable({
  firstLast,
  dated,
  board,
  laterHref,
  nextDayHref,
}: {
  firstLast: readonly TransitStopFirstLast[];
  /** The page asked for a day (`?date=`): the first and last are that
   * day's, the board's, not today's. */
  dated: boolean;
  board: TransitBoard;
  laterHref: string | null;
  nextDayHref: string;
}) {
  return (
    <Stack gap="sm" component="section" aria-labelledby="timetable">
      <SectionTitle id="timetable">Timetable</SectionTitle>
      {firstLast.length > 0 && (
        <Stack gap={4}>
          <SectionTitle order={3}>
            {dated ? `First and last on ${formatDate(board.date)}` : 'First and last today'}
          </SectionTitle>
          <Table fz="sm" withRowBorders={false} verticalSpacing={4} maw={560}>
            <TableTbody>
              {firstLast.map((row) => (
                <TableTr key={`${row.line.id}.${row.direction}`}>
                  <TableTh scope="row" fw={500}>
                    {row.line.name}
                    {row.towards ? ` towards ${row.towards}` : ''}
                  </TableTh>
                  <TableTd>
                    First {row.first}, last {row.last}
                  </TableTd>
                </TableTr>
              ))}
            </TableTbody>
          </Table>
        </Stack>
      )}
      <Stack gap={4}>
        <SectionTitle order={3}>Scheduled departures</SectionTitle>
        <Text size="sm">{formatDate(board.date)}</Text>
        <TransitStopBoard board={board} laterHref={laterHref} />
        <Group>
          <TextLink href={nextDayHref}>Next day</TextLink>
        </Group>
      </Stack>
    </Stack>
  );
}
