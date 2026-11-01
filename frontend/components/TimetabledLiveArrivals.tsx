'use client';

import { useEffect, useState } from 'react';
import { Paper, Stack, Text, VisuallyHidden } from '@mantine/core';
import { TextLink } from './TextLink';
import { formatTime } from '@/lib/dateFormat';
import { describeFailure } from '@/lib/failure';
import { arrivalsForLeg, showsLiveArrivals, vehicleNoun } from '@/lib/timetabledLeg';
import { arrivalDue, arrivalDueSpoken, tflStopHref } from '@/lib/tflStops';
import type { JourneyLegDetail, LegLiveArrivalsFilter, TflArrival, TflStopArrivals } from '@/lib/types';

/** How often the panel asks again, as the TfL stop page refreshes. */
const REFRESH_MS = 30_000;

type Load = { kind: 'loading' } | { kind: 'loaded'; arrivals: TflArrival[] } | { kind: 'failed'; message: string };

/** TfL's live predictions at a timetabled leg's boarding stop (DS-237,
 * phase C), for its line and only the trains that reach its alighting
 * stop: indicative, never the leg's own ride. Shown from an hour before
 * the planned departure until the planned arrival, asked again every 30
 * seconds; the leg's own times stay the timetable's. */
export function TimetabledLiveArrivals({ leg, filter }: { leg: JourneyLegDetail; filter: LegLiveArrivalsFilter }) {
  // Decided after mounting: the server's clock is not the visitor's.
  const [open, setOpen] = useState(false);
  const [load, setLoad] = useState<Load>({ kind: 'loading' });

  useEffect(() => {
    const check = () => setOpen(showsLiveArrivals(leg, new Date()));
    check();
    const timer = setInterval(check, REFRESH_MS);
    return () => clearInterval(timer);
  }, [leg]);

  useEffect(() => {
    if (!open) return undefined;
    let cancelled = false;
    const fetchArrivals = async () => {
      try {
        const response = await fetch(`/api/tfl/stops/${encodeURIComponent(filter.stopId)}/arrivals`);
        if (!response.ok) throw new Error(String(response.status));
        const body = (await response.json()) as TflStopArrivals;
        if (!cancelled) setLoad({ kind: 'loaded', arrivals: arrivalsForLeg(body, filter) });
      } catch {
        if (!cancelled) setLoad({ kind: 'failed', message: describeFailure('load', "TfL's live arrivals") });
      }
    };
    void fetchArrivals();
    const timer = setInterval(() => void fetchArrivals(), REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [open, filter]);

  if (!open) return null;
  const vehicles = vehicleNoun(leg.transitMode) === 'tram' ? 'trams' : 'trains';
  const stop = leg.originName ?? 'your stop';
  const destination = leg.destinationName ?? 'your stop';
  return (
    <Paper withBorder p="sm" data-live-arrivals>
      <Stack gap="xs">
        <Text size="sm" fw={600}>
          Next {vehicles} at {stop} (live from TfL)
        </Text>
        {load.kind === 'loading' && (
          <Text size="sm" c="dimmed" role="status">
            Asking TfL for its live arrivals.
          </Text>
        )}
        {load.kind === 'failed' && (
          <Text size="sm" role="status">
            {load.message}
          </Text>
        )}
        {load.kind === 'loaded' && load.arrivals.length === 0 && (
          <Text size="sm" role="status">
            TfL lists no {vehicles} for {destination} here right now.
          </Text>
        )}
        {load.kind === 'loaded' && load.arrivals.length > 0 && (
          <Stack gap={4} component="ul" m={0} p={0} aria-label={`Next ${vehicles} at ${stop}`}>
            {load.arrivals.map((arrival) => (
              <Text key={arrival.id} size="sm" component="li" data-live-arrival={arrival.id} display="flex">
                <Text span flex={1}>
                  <VisuallyHidden>To </VisuallyHidden>
                  {arrival.destination}
                </Text>
                <Text span fw={600}>
                  <span aria-hidden="true">{arrivalDue(arrival.timeToStationSecs)}</span>
                  <VisuallyHidden>, {arrivalDueSpoken(arrival.timeToStationSecs)}</VisuallyHidden>
                </Text>
                <Text span size="xs" c="dimmed" pl="sm">
                  Expected {formatTime(arrival.expectedArrival)}
                </Text>
              </Text>
            ))}
          </Stack>
        )}
        <Text size="xs" c="dimmed">
          TfL&apos;s predictions for {vehicles} that go to {destination}. Your planned times are from the timetable, so
          one of these may not be the {vehicleNoun(leg.transitMode)} in your plan. Check where it&apos;s going before
          you get on.
        </Text>
        <TextLink href={tflStopHref(filter.stopId)} underline="always" size="xs">
          Every live arrival at {stop}
        </TextLink>
      </Stack>
    </Paper>
  );
}
