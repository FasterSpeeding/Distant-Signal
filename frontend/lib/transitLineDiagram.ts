import type { TransitLineBranch, TransitLineDetail, TransitStationRef } from './types';

/** A line's stops drawn once (a route strip), not once per direction and
 * branch: the main run of stations, with every other run of stations the
 * line's trips make hung off the stop where it leaves or joins it.
 *
 * - `to`: a branch that leaves at `at` and ends elsewhere ("Branch to Mill
 *   Hill East", at Finchley Central).
 * - `from`: a branch that starts elsewhere and joins at `at` ("Branch from
 *   Battersea Power Station", at Kennington).
 * - `via`: another way between `at` and `rejoins` ("Via Waterloo", from
 *   Kennington, rejoining at Euston); back to a stop already passed is a
 *   loop, the same shape.
 * - `separate`: a run that touches no other (rare; listed after the rest).
 *
 * The runs come from the line's branches (each direction's, the other way
 * reversed, busiest first), else from its directions' station lists. A
 * branch's stations nest too: Mill Hill East hangs off the High Barnet
 * branch. Stations only the directions' merged lists name (trips too few to
 * be a branch) are `others`, listed, never drawn as if adjacent. */
export interface LineDiagram {
  stops: DiagramStop[];
  separate: DiagramSpur[];
  others: TransitStationRef[];
  /** The ends of the main run: "Hammersmith" and "Upminster". */
  from: TransitStationRef | null;
  to: TransitStationRef | null;
}

export interface DiagramStop {
  station: TransitStationRef;
  /** The first or last stop of the main run or of a branch. */
  terminus: boolean;
  spurs: DiagramSpur[];
}

export interface DiagramSpur {
  kind: 'to' | 'from' | 'via' | 'separate';
  stops: DiagramStop[];
  /** `via`: where it rejoins; `from`: where it joins. */
  rejoins: TransitStationRef | null;
}

type Path = readonly TransitStationRef[];

/** Runs of stations, busiest first: each branch outbound as given, each
 * inbound one reversed (so all read the same way), by share; a line with no
 * branches gives its directions' lists, the inbound one reversed. */
function pathsOf(line: Pick<TransitLineDetail, 'directions' | 'branches'>): Path[] {
  const branches: readonly TransitLineBranch[] = line.branches ?? [];
  if (branches.length > 0) {
    const byShare = (a: TransitLineBranch, b: TransitLineBranch) => b.sharePercent - a.sharePercent;
    return [
      ...branches.filter((b) => b.direction === 'outbound').sort(byShare),
      ...branches.filter((b) => b.direction === 'inbound').sort(byShare),
    ].map((b) => (b.direction === 'inbound' ? [...b.stations].reverse() : b.stations));
  }
  const outbound = line.directions.filter((d) => d.direction === 'outbound');
  const inbound = line.directions.filter((d) => d.direction === 'inbound');
  // A line run only "inbound" still reads its own way round.
  if (outbound.length === 0) return inbound.map((d) => d.stations);
  return [...outbound.map((d) => d.stations), ...inbound.map((d) => [...d.stations].reverse())];
}

export function buildLineDiagram(line: Pick<TransitLineDetail, 'directions' | 'branches'>): LineDiagram {
  const paths = pathsOf(line).filter((path) => path.length > 0);
  const main = paths[0] ?? [];
  const placed = new Map<string, DiagramStop>();
  const toStop = (station: TransitStationRef): DiagramStop => {
    const stop: DiagramStop = { station, terminus: false, spurs: [] };
    if (!placed.has(station.id)) placed.set(station.id, stop);
    return stop;
  };
  const stops = main.map(toStop);
  markEnds(stops, true, true);
  const separate: DiagramSpur[] = [];

  for (const path of paths.slice(1)) {
    // Runs of stations not yet drawn, each with the placed stops either
    // side of it (if any). A station twice in one run (a loop) ends it.
    let run: TransitStationRef[] = [];
    let before: DiagramStop | undefined;
    let previous: TransitStationRef | undefined;
    const flush = (after: DiagramStop | undefined) => {
      if (run.length === 0) return;
      const spurStops = run.map(toStop);
      const kind: DiagramSpur['kind'] = before ? (after ? 'via' : 'to') : after ? 'from' : 'separate';
      markEnds(spurStops, kind === 'from' || kind === 'separate', kind === 'to' || kind === 'separate');
      const spur: DiagramSpur = {
        kind,
        stops: spurStops,
        rejoins: (kind === 'via' || kind === 'from') && after ? after.station : null,
      };
      const anchor = kind === 'from' ? after : before;
      if (anchor) anchor.spurs.push(spur);
      else separate.push(spur);
      run = [];
    };
    for (const station of path) {
      const at = placed.get(station.id);
      if (at) {
        flush(at);
        before = at;
      } else if (run.some((s) => s.id === station.id)) {
        flush(undefined);
        before = placed.get(station.id);
      } else {
        if (run.length === 0) before = previous ? placed.get(previous.id) : undefined;
        run.push(station);
      }
      previous = station;
    }
    flush(undefined);
  }

  const others: TransitStationRef[] = [];
  for (const direction of line.directions) {
    for (const station of direction.stations) {
      if (placed.has(station.id) || others.some((o) => o.id === station.id)) continue;
      others.push(station);
    }
  }

  return { stops, separate, others, from: main[0] ?? null, to: main[main.length - 1] ?? null };
}

/** Marks a run's first and/or last stop as an end of the line. */
function markEnds(stops: readonly DiagramStop[], first: boolean, last: boolean): void {
  const head = stops[0];
  const tail = stops[stops.length - 1];
  if (first && head) head.terminus = true;
  if (last && tail) tail.terminus = true;
}

/** "Branch to Mill Hill East", "Branch from Battersea Power Station,
 * joining at Kennington", "Via
 * Waterloo, rejoining at Euston": what a spur is, for its label. */
export function spurLabel(spur: DiagramSpur): string {
  const first = spur.stops[0]?.station.name ?? '';
  const last = spur.stops[spur.stops.length - 1]?.station.name ?? '';
  switch (spur.kind) {
    case 'to':
      return `Branch to ${last}`;
    case 'from':
      return `Branch from ${first}${spur.rejoins ? `, joining at ${spur.rejoins.name}` : ''}`;
    case 'via':
      return `Via ${first}${spur.rejoins ? `, rejoining at ${spur.rejoins.name}` : ''}`;
    case 'separate':
      return first === last ? `Also ${first}` : `Also ${first} to ${last}`;
  }
}

/** How many stops the diagram draws, each once. */
export function diagramStopCount(diagram: LineDiagram): number {
  const count = (stops: readonly DiagramStop[]): number =>
    stops.reduce((sum, stop) => sum + 1 + stop.spurs.reduce((s, spur) => s + count(spur.stops), 0), 0);
  return count(diagram.stops) + diagram.separate.reduce((s, spur) => s + count(spur.stops), 0);
}
