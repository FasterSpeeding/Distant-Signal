import { Fragment } from 'react';
import { Stack, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';
import { type DiagramSpur, type DiagramStop, type LineDiagram, spurLabel } from '@/lib/transitLineDiagram';
import { transitStopHref } from '@/lib/transitStops';
import classes from './TransitLineDiagram.module.css';

/** A line's stops drawn once, as a route strip (`buildLineDiagram`): the
 * main run top to bottom, each branch an indented strip under the stop
 * where it splits or joins, named ("Branch to Mill Hill East", "Via
 * Waterloo, rejoining at Euston"). Trains run both ways along it, so there
 * is no second list for the other direction.
 *
 * Markup: an ordered list of stops, each a link to the stop's page; a
 * branch is a group (named by its visible label) holding its own ordered
 * list, nested in the stop's item. The strip itself is CSS decoration.
 * Stations only a few trips call at, too few to draw, are listed after it. */
export function TransitLineDiagram({
  network,
  diagram,
  colour,
  label,
  idPrefix,
}: {
  network: string;
  diagram: LineDiagram;
  /** The line's colour, for the strip. */
  colour: string;
  /** The main list's accessible name ("Victoria stops, Brixton to Walthamstow Central"). */
  label: string;
  /** Unique on the page: the branch labels' ids start with it. */
  idPrefix: string;
}) {
  return (
    <Stack gap="sm" __vars={{ '--line-colour': colour }}>
      <Strip network={network} stops={diagram.stops} label={label} idPath={idPrefix} />
      {diagram.separate.map((spur, index) => (
        <Spur key={index} network={network} spur={spur} idPath={`${idPrefix}-s${index}`} />
      ))}
      {diagram.others.length > 0 && (
        <Text size="sm" c="dimmed">
          Some trips also call at{' '}
          {diagram.others.map((station, index, all) => (
            <Fragment key={station.id}>
              <TextLink href={transitStopHref(network, station.id)} inline underline="always">
                {station.name}
              </TextLink>
              {index < all.length - 2 ? ', ' : index === all.length - 2 ? ' and ' : '.'}
            </Fragment>
          ))}
        </Text>
      )}
    </Stack>
  );
}

function Strip({
  network,
  stops,
  label,
  idPath,
}: {
  network: string;
  stops: readonly DiagramStop[];
  label?: string;
  idPath: string;
}) {
  return (
    <ol className={classes.strip} aria-label={label}>
      {stops.map((stop, index) => (
        // A loop line passes a stop twice: the index keeps the keys apart.
        <li key={`${stop.station.id}-${index}`} className={classes.stop} data-terminus={stop.terminus || undefined}>
          <TextLink href={transitStopHref(network, stop.station.id)}>
            {stop.terminus ? (
              <Text span fw={600} inherit>
                {stop.station.name}
              </Text>
            ) : (
              stop.station.name
            )}
          </TextLink>
          {stop.spurs.map((spur, spurIndex) => (
            <Spur key={spurIndex} network={network} spur={spur} idPath={`${idPath}-${index}-${spurIndex}`} />
          ))}
        </li>
      ))}
    </ol>
  );
}

function Spur({ network, spur, idPath }: { network: string; spur: DiagramSpur; idPath: string }) {
  const labelId = `${idPath}-label`;
  return (
    <div className={classes.spur} role="group" aria-labelledby={labelId} data-spur={spur.kind}>
      <Text id={labelId} size="sm" c="dimmed" fs="italic" className={classes.spurLabel}>
        {spurLabel(spur)}
      </Text>
      <Strip network={network} stops={spur.stops} idPath={idPath} />
    </div>
  );
}
