import { Stack } from '@mantine/core';
import { LinkSentence } from '@/components/LinkSentence';
import type { TransitRailLink, TransitStationLink } from '@/lib/types';

/** On a National Rail station page: the tram stops to change to (spec
 * §12.1), each linking to its stop page, as link sentences ("Change for
 * trams at Piccadilly (Manchester Metrolink)."). Nothing when there are
 * none, which is always the case while no tram network is served. */
export function StationTramLinks({ links }: { links: TransitStationLink[] }) {
  if (links.length === 0) return null;
  const sentence = (kind: TransitStationLink['kind']) =>
    links
      .filter((link) => link.kind === kind)
      .map((link) => ({
        href: `/transit/${link.network}/stops/${link.stopId}`,
        label: link.stopName,
        note: link.networkName,
      }));
  return (
    <Stack gap={2}>
      <LinkSentence lead="Change for trams at " links={sentence('interchange')} />
      <LinkSentence lead="Trams nearby: " links={sentence('nearby')} />
    </Stack>
  );
}

/** On a tram stop page: the National Rail stations to change to, each
 * linking to its station page, as link sentences. */
export function StopRailLinks({ links }: { links: TransitRailLink[] }) {
  if (links.length === 0) return null;
  const sentence = (kind: TransitRailLink['kind']) =>
    links
      .filter((link) => link.kind === kind)
      .map((link) => ({ href: `/stations/${link.crs}`, label: link.stationName ?? link.crs }));
  return (
    <Stack gap={2}>
      <LinkSentence lead="Change for National Rail at " links={sentence('interchange')} />
      <LinkSentence lead="National Rail nearby: " links={sentence('nearby')} />
    </Stack>
  );
}
