'use client';

import { useId } from 'react';
import Link, { useLinkStatus } from 'next/link';
import { Loader, Stack, Text } from '@mantine/core';
import { ServiceRowList } from '@/components/ServiceRow';
import { stationLabel } from '@/lib/stationLabel';
import type { NearbyStation } from '@/lib/types';
import classes from './NearbyStationList.module.css';

/** A distance from the visitor, always in kilometres to one decimal place
 * ("0.4 km", "12.3 km"), so the column reads the same down every row. */
export function formatDistanceKm(km: number): string {
  return `${km.toFixed(1)} km`;
}

/** The "Use my location" results: a titled list of rows, each one link to
 * the station's page.
 *
 * Built on `ServiceRowList` (a bordered list with a rule between rows, the
 * train search's results look) so each station reads as its own result.
 * The whole row is the link, at least 44px tall, with the same hover and
 * inset focus ring as a service row. The name and code ("Woking (WOK)",
 * `stationLabel`) sit together on the left; the distance takes its own
 * right-hand column, smaller and dimmed with tabular figures.
 *
 * Each link is named "Woking (WOK), 0.4 km away" through `aria-label`:
 * the two columns are separate boxes, so a name built from the content
 * would run them together with no pause. The label starts with the
 * visible text, so voice control still finds the link by what it shows. */
export function NearbyStationList({ stations }: { stations: NearbyStation[] }) {
  const headingId = useId();
  return (
    // Capped so on a wide screen the distance stays within a glance of the
    // name rather than at the far edge of the page.
    <Stack gap="xs" maw="32rem">
      <Text id={headingId} size="sm" fw={500}>
        Nearest stations
      </Text>
      <ServiceRowList aria-labelledby={headingId} data-nearby-stations>
        {stations.map((station) => {
          const label = stationLabel(station.code, station.name);
          const distance = formatDistanceKm(station.distanceKm);
          return (
            <li key={station.code}>
              <Link
                href={`/stations/${station.code}`}
                className={classes.row}
                aria-label={`${label}, ${distance} away`}
              >
                <span className={classes.name}>{label}</span>
                <span className={classes.distance} data-distance>
                  <PendingHint />
                  {distance}
                </span>
              </Link>
            </li>
          );
        })}
      </ServiceRowList>
    </Stack>
  );
}

/** A small spinner beside the distance while the station page loads: the
 * station route has no `loading.tsx`, so on a slow fetch this is the only
 * sign the tap registered. Always rendered and only hidden, so the row
 * doesn't shift when it appears. */
function PendingHint() {
  const { pending } = useLinkStatus();
  return (
    <Loader size={12} aria-hidden="true" className={pending ? undefined : classes.hidden} data-pending={pending} />
  );
}
