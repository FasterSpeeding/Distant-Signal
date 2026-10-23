import { Paper, SimpleGrid, Stack, Text, VisuallyHidden } from '@mantine/core';
import { SectionTitle } from '@/components/SectionTitle';
import { formatTime } from '@/lib/dateFormat';
import { arrivalDue, arrivalDueSpoken } from '@/lib/tflStops';
import type { TflArrivalGroup, TflStopArrivals } from '@/lib/types';
import classes from './TflArrivalsBoard.module.css';

/** "Platform 2" for TfL's bare platform number (DLR, tram), else TfL's
 * own words ("Westbound - Platform 1"). */
export function platformLabel(platformName: string): string {
  return /^\d+[A-Za-z]?$/.test(platformName) ? `Platform ${platformName}` : platformName;
}

/** A group's heading: "Piccadilly, Westbound - Platform 1", or the line
 * alone when TfL names no platform. */
export function arrivalGroupTitle(group: Pick<TflArrivalGroup, 'lineName' | 'platformName'>): string {
  const platform = platformLabel(group.platformName.trim());
  return platform ? `${group.lineName}, ${platform}` : group.lineName;
}

/** A TfL stop's live arrivals (`GET /public/tfl/stops/{id}/arrivals`):
 * one list per line and platform, each train's destination and how long
 * until it arrives, with its expected time and where it is now under it.
 * Server-rendered; the root layout's `AutoRefresh` re-renders it every
 * 30 s, and the page shows when TfL was last asked. */
export function TflArrivalsBoard({ arrivals }: { arrivals: TflStopArrivals }) {
  const groups = arrivals.groups.filter((group) => group.arrivals.length > 0);
  if (groups.length === 0) {
    return (
      <Paper withBorder p="md" role="status">
        <Text size="sm" c="dimmed">
          No arrivals are predicted here right now.
        </Text>
      </Paper>
    );
  }
  return (
    <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md" verticalSpacing="lg">
      {groups.map((group) => {
        const headingId = `arrivals-${group.lineId}-${group.platformName}`.replace(/[^A-Za-z0-9-]+/g, '-');
        return (
          <Stack key={`${group.lineId}|${group.platformName}`} gap="xs" component="section" aria-labelledby={headingId}>
            <SectionTitle order={3} id={headingId}>
              {arrivalGroupTitle(group)}
            </SectionTitle>
            <ul className={classes.list} aria-labelledby={headingId}>
              {group.arrivals.map((arrival) => {
                const details = [
                  <span key="clock" className={classes.clock}>
                    Expected {formatTime(arrival.expectedArrival)}
                  </span>,
                ];
                if (arrival.currentLocation) {
                  details.push(<span key="where">{` · ${arrival.currentLocation}`}</span>);
                }
                return (
                  <li key={arrival.id} className={classes.row} data-arrival={arrival.id}>
                    <span className={classes.dest}>
                      <VisuallyHidden>To </VisuallyHidden>
                      {arrival.destination}
                    </span>
                    <span className={classes.due}>
                      <span aria-hidden="true">{arrivalDue(arrival.timeToStationSecs)}</span>
                      <VisuallyHidden>, {arrivalDueSpoken(arrival.timeToStationSecs)}.</VisuallyHidden>
                    </span>
                    <span className={classes.details} data-arrival-details>
                      {details}
                    </span>
                  </li>
                );
              })}
            </ul>
          </Stack>
        );
      })}
    </SimpleGrid>
  );
}
