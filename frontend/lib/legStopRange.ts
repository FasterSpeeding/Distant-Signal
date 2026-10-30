import type { JourneyStop } from './types';

/** The stops of one journey leg within its train's calling pattern: the
 * indices (inclusive) of the stop the traveller boards at and the stop
 * they alight at. */
export interface LegStopRange {
  start: number;
  end: number;
}

/** Where the train is relative to a leg: not yet at the boarding stop,
 * somewhere on the leg, or at or past the alighting stop. */
export type LegPhase = 'before' | 'on' | 'after';

function crsMatch(stop: JourneyStop, crs: string): boolean {
  return stop.crs?.toUpperCase() === crs.toUpperCase();
}

function indicesOf(stops: JourneyStop[], crs: string | null | undefined): number[] {
  if (!crs) return [];
  const found: number[] = [];
  stops.forEach((stop, index) => {
    if (crsMatch(stop, crs)) found.push(index);
  });
  return found;
}

/** The part of `stops` a journey leg travels over, from its board stop
 * (`boardCrs`) to its alight stop (`alightCrs`), or `null` to show the
 * whole calling pattern as before.
 *
 * - A train that calls at a station twice (a loop): the alight stop is
 *   the first call there with a board call before it, and the board stop
 *   is the last call before that, so the range is the shortest ride that
 *   fits the leg. A circular train boarded and left at its own origin
 *   covers the whole loop.
 * - A board or alight stop missing from the live calling pattern (a
 *   diversion, or a leg with no CRS): that end falls back to the train's
 *   own origin or terminus, so the range is never blank.
 * - Neither stop found, or only one stop left: `null`, the whole pattern.
 *
 * Cancelled stops stay in the range; the progress diagram already draws
 * them as cancelled. */
export function legStopRange(
  stops: JourneyStop[],
  boardCrs: string | null | undefined,
  alightCrs: string | null | undefined,
): LegStopRange | null {
  if (stops.length === 0) return null;
  const boards = indicesOf(stops, boardCrs);
  const alights = indicesOf(stops, alightCrs);
  const last = stops.length - 1;

  let range: LegStopRange | null = null;
  if (boards.length > 0 && alights.length > 0) {
    for (const end of alights) {
      const start = boards.filter((index) => index < end).at(-1);
      if (start !== undefined) {
        range = { start, end };
        break;
      }
    }
    // The alight stop only comes before the board stop: the alight end
    // isn't on this train after boarding, so run to the terminus.
    range ??= { start: boards[0] ?? 0, end: last };
  } else if (boards.length > 0) {
    range = { start: boards[0] ?? 0, end: last };
  } else if (alights.length > 0) {
    range = { start: 0, end: alights[0] ?? last };
  }

  if (!range || range.end <= range.start) return null;
  if (range.start === 0 && range.end === last) return null;
  return range;
}

/** Maps the train's last confirmed stop (an index into the FULL calling
 * pattern, `-1` for none) onto a leg's range: before the board stop the
 * leg hasn't started (`-1`), at or past the alight stop it is complete
 * (the alight stop, the range's last index), and in between it is the
 * same stop, renumbered from the board stop. */
export function legProgressIndex(fullIndex: number, range: LegStopRange): { index: number; phase: LegPhase } {
  if (fullIndex < range.start) return { index: -1, phase: 'before' };
  if (fullIndex >= range.end) return { index: range.end - range.start, phase: 'after' };
  return { index: fullIndex - range.start, phase: 'on' };
}
