import { Stack, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';
import type { TransitRailLink, TransitStationLink } from '@/lib/types';

/** On a National Rail station page: the tram stops to change to (spec
 * §12.1), each linking to its stop page. Nothing when there are none,
 * which is always the case while no tram network is served. */
export function StationTramLinks({ links }: { links: TransitStationLink[] }) {
  if (links.length === 0) return null;
  return (
    <Stack gap={2}>
      {links.map((link) => (
        <Text key={`${link.network}:${link.stopId}`} size="sm">
          {link.kind === 'interchange' ? 'Change for trams: ' : 'Trams nearby: '}
          <TextLink href={`/transit/${link.network}/stops/${link.stopId}`} underline="always" inline>
            {link.stopName}
          </TextLink>{' '}
          ({link.networkName})
        </Text>
      ))}
    </Stack>
  );
}

/** On a tram stop page: the National Rail stations to change to, each
 * linking to its station page. */
export function StopRailLinks({ links }: { links: TransitRailLink[] }) {
  if (links.length === 0) return null;
  return (
    <Stack gap={2}>
      {links.map((link) => (
        <Text key={link.crs} size="sm">
          {link.kind === 'interchange' ? 'Change for National Rail at ' : 'National Rail nearby: '}
          <TextLink href={`/stations/${link.crs}`} underline="always" inline>
            {link.stationName ?? link.crs}
          </TextLink>
        </Text>
      ))}
    </Stack>
  );
}
