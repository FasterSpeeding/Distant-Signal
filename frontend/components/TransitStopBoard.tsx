import { Group, Stack, Text, VisuallyHidden } from '@mantine/core';
import { ServiceListNotice, ServiceRowList } from '@/components/ServiceRow';
import { TextLink } from '@/components/TextLink';
import { departureClock, SCHEDULED_CAPTION } from '@/lib/transit';
import type { TransitBoard } from '@/lib/types';
import classes from './ServiceRow.module.css';

/** A tram stop's scheduled departures (`GET .../stops/{stop}/departures`),
 * laid out like the line pages' service rows: time, destination and, on a
 * second line, the line and platform. Never live: every board says so, and
 * a row has no status, delay or cancellation (transit spec §2). A
 * replacement bus is labelled as one (Q3).
 *
 * `laterHref` is the next page's address (the board's `nextCursor`), as a
 * plain link: the page is server-rendered and needs no JavaScript. */
export function TransitStopBoard({ board, laterHref }: { board: TransitBoard; laterHref: string | null }) {
  return (
    <Stack gap="xs">
      <Text size="sm" c="dimmed">
        {SCHEDULED_CAPTION}
      </Text>
      {board.outsideHorizon ? (
        <ServiceListNotice>The timetable for this date isn&apos;t published yet.</ServiceListNotice>
      ) : board.departures.length === 0 ? (
        <ServiceListNotice>No more scheduled trams this day.</ServiceListNotice>
      ) : (
        <ServiceRowList aria-label="Scheduled departures">
          {board.departures.map((departure) => {
            const details = [departure.line?.name, departure.platform].filter(Boolean).join(' · ');
            return (
              <li key={`${departure.tripId}.${departure.platformId}`} data-trip={departure.tripId}>
                <div className={classes.row} data-details={details ? true : undefined}>
                  <span className={classes.time}>{departureClock(departure.scheduledDeparture)}</span>{' '}
                  <span className={classes.dest}>
                    <VisuallyHidden>to </VisuallyHidden>
                    {departure.destination}
                  </span>{' '}
                  <span className={`${classes.status} ${classes.muted}`}>
                    {departure.replacementBus ? 'Replacement bus' : 'Scheduled'}
                  </span>{' '}
                  {details && (
                    <>
                      <VisuallyHidden>. </VisuallyHidden>
                      <span className={classes.details}>{details}</span>
                    </>
                  )}
                </div>
              </li>
            );
          })}
        </ServiceRowList>
      )}
      {laterHref && (
        <Group>
          <TextLink href={laterHref}>Later departures</TextLink>
        </Group>
      )}
    </Stack>
  );
}
