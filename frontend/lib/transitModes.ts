import type { TransitFrequencyBand, TransitLine, TransitStationRef } from './types';

/** What a network with several modes runs, for its card on `/transit`
 * (the TfL transit design §8.1). `null` for a single-mode network, whose
 * name says it. */
export function transitNetworkSummary(networkId: string): string | null {
  return networkId === 'tfl' ? 'Underground, DLR and London Trams' : null;
}

/** A line mode's heading on its network's page. */
const MODE_LABELS: Readonly<Record<string, string>> = {
  tube: 'Underground',
  dlr: 'DLR',
  tram: 'Trams',
};

/** A mode as running text names it: "Tube", "DLR", "tram". */
const MODE_NOUNS: Readonly<Record<string, string>> = {
  tube: 'Tube',
  dlr: 'DLR',
  tram: 'tram',
};

/** What a network's lines run, for running text and descriptions: "tram"
 * for Metrolink and NET, "Tube, DLR and tram" for TfL. Each line's own
 * mode, else the network's, in the order they first appear. `null` when a
 * mode has no word here, so the copy stays mode-neutral rather than guess. */
export function networkModesPhrase(network: {
  mode: string;
  lines: readonly Pick<TransitLine, 'mode'>[];
}): string | null {
  const modes = [...new Set(network.lines.map((line) => line.mode || network.mode))];
  const nouns = (modes.length > 0 ? modes : [network.mode]).map((mode) => MODE_NOUNS[mode]);
  const words = nouns.filter((noun) => noun !== undefined);
  const last = words.at(-1);
  if (words.length !== nouns.length || last === undefined) return null;
  return words.length === 1 ? last : `${words.slice(0, -1).join(', ')} and ${last}`;
}

/** The lines grouped by mode, in the order their modes first appear (the
 * api's line order). One group, with no label, when every line shares a
 * mode or the api sends none: a single-mode network keeps one list. */
export function groupLinesByMode<T extends Pick<TransitLine, 'mode'>>(
  lines: readonly T[],
): { mode: string | null; label: string | null; lines: T[] }[] {
  const modes = [...new Set(lines.map((line) => line.mode ?? ''))];
  if (modes.length <= 1) return [{ mode: modes[0] || null, label: null, lines: [...lines] }];
  return modes.map((mode) => ({
    mode: mode || null,
    label: MODE_LABELS[mode] ?? 'Other lines',
    lines: lines.filter((line) => (line.mode ?? '') === mode),
  }));
}

/** A band of the day, as a heading. */
export const FREQUENCY_BAND_LABELS: Readonly<Record<TransitFrequencyBand['band'], string>> = {
  early: 'Early morning',
  'morning-peak': 'Morning peak',
  daytime: 'Daytime',
  'evening-peak': 'Evening peak',
  evening: 'Evening',
  late: 'Late evening',
};

/** How often a band runs: "Every 2 to 4 minutes", "Every 5 minutes", or
 * the departure count when there are too few to have a gap. */
export function frequencyText(
  band: Pick<TransitFrequencyBand, 'departures' | 'minGapMinutes' | 'maxGapMinutes'>,
): string {
  const { minGapMinutes: min, maxGapMinutes: max } = band;
  if (min === null || max === null) return band.departures === 1 ? 'One departure' : `${band.departures} departures`;
  const unit = (n: number) => (n === 1 ? 'minute' : 'minutes');
  if (min === max) return `Every ${min} ${unit(min)}`;
  return `Every ${min} to ${max} minutes`;
}

/** A direction's heading: towards its headsign, else its last station. */
export function directionHeading(direction: {
  direction: 'outbound' | 'inbound';
  headsign: string | null;
  stations: readonly TransitStationRef[];
}): string {
  const towards = direction.headsign ?? direction.stations.at(-1)?.name;
  if (towards) return `Towards ${towards}`;
  return direction.direction === 'outbound' ? 'One way' : 'The other way';
}

/** "Towards Edgware, High Barnet or Mill Hill East": where a direction's
 * branches go, each end once in branch order (at most three, then "and
 * more"). */
export function branchesHeading(branches: readonly { stations: readonly TransitStationRef[] }[]): string {
  const ends = [...new Set(branches.map((branch) => branch.stations.at(-1)?.name).filter((name) => !!name))];
  if (ends.length === 0) return 'Branches';
  const shown = ends.slice(0, 3);
  const more = ends.length > shown.length;
  const list =
    shown.length === 1
      ? shown[0]
      : more
        ? `${shown.join(', ')} and more`
        : `${shown.slice(0, -1).join(', ')} or ${shown.at(-1)}`;
  return `Towards ${list}`;
}

/** Whether a line runs often enough that its timetable shows an hour at a
 * time (the Underground and the DLR) rather than three. */
export function isHighFrequencyMode(mode: string | null | undefined): boolean {
  return mode === 'tube' || mode === 'dlr';
}
