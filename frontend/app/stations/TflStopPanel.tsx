'use client';

import { useEffect, useState } from 'react';
import { Badge, Group, Paper, Stack, Text, Title } from '@mantine/core';
import { TextLink } from '@/components/TextLink';
import { getStationNames, railCrsForStop } from '@/lib/suggestions';
import { stationLabel } from '@/lib/stationLabel';
import type { TflStop } from '@/lib/types';

const MODE_LABELS: Record<string, string> = { tube: 'Underground', dlr: 'DLR', tram: 'Tram' };

/** A TfL line id that `/lines/tfl-{id}` can name (`/lines/[id]`'s own id
 * shape). */
const LINE_ID = /^[a-z0-9-]+$/;

/** What `/stations` shows for a picked TfL Tube, DLR or tram stop (DS-134).
 * There is no TfL stop page and no board for one, so this says so, links
 * each line's status page (`/lines/tfl-{line}`), and links the National
 * Rail station the stop serves when it has one (`railCrsForStop`: Euston's
 * Tube stop is Euston). Uxbridge has none, so it shows its lines alone. */
export function TflStopPanel({ stop }: { stop: TflStop }) {
  const [rail, setRail] = useState<{ stopId: string; crs: string; name: string | null } | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    railCrsForStop(stop.id, controller.signal)
      .then(async (crs) => {
        if (!crs) return;
        const names = await getStationNames([crs], controller.signal);
        setRail({ stopId: stop.id, crs, name: names.get(crs) ?? null });
      })
      .catch(() => {
        // No link is the honest fallback: the lines still show.
      });
    return () => controller.abort();
  }, [stop.id]);

  const lines = stop.lines.filter((line) => LINE_ID.test(line.id));
  const modes = stop.modes.map((mode) => MODE_LABELS[mode] ?? mode);
  const railHere = rail?.stopId === stop.id ? rail : null;

  return (
    <Paper withBorder p="md" maw="32rem" data-tfl-stop={stop.id}>
      <Stack gap="xs">
        <Title order={2} size="h4">
          {stop.name}
        </Title>
        <Group gap="xs">
          {modes.map((mode) => (
            <Badge key={mode} variant="light" tt="none">
              {mode}
            </Badge>
          ))}
        </Group>
        <Text size="sm">
          This is a TfL stop, not a National Rail station, so there is no departure board for it here.
        </Text>
        {lines.length > 0 && (
          <Text size="sm">
            Line status:{' '}
            {lines.map((line, index) => (
              <span key={line.id}>
                {index > 0 && ', '}
                <TextLink href={`/lines/tfl-${line.id}`} underline="always" inline>
                  {line.name}
                </TextLink>
              </span>
            ))}
          </Text>
        )}
        {railHere && (
          <Text size="sm">
            National Rail station here:{' '}
            <TextLink href={`/stations/${railHere.crs}`} underline="always" inline>
              {stationLabel(railHere.crs, railHere.name)}
            </TextLink>
          </Text>
        )}
      </Stack>
    </Paper>
  );
}
