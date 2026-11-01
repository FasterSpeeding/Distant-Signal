/* What kind of station each end of a walk or transfer is, so the two ends
 * never read alike (DS-239). A walk from a National Rail station to its
 * Tube station reads "Vauxhall to Vauxhall" without it; with it, "Vauxhall
 * (National Rail) to Vauxhall (Tube)". The type is words in the text, never
 * an icon alone, so a screen reader says it too. Same bracket style as a
 * planner stop ("Old Street (Tube)", docs/style-guide.md). */

/** One end's kind: a National Rail station, or a TfL (or other transit)
 * stop with its modes (`tube`, `dlr`, `tram`, ...). `undefined` when not
 * known: such an end is never tagged. */
export type PlaceKind = { kind: 'rail' } | { kind: 'stop'; modes: readonly string[] };

export const RAIL_PLACE: PlaceKind = { kind: 'rail' };

/** The words for a stop's mode: TfL's `tfl_stops` modes, and TfL's own
 * mode names for the networks it doesn't list there yet. */
const STOP_MODE_LABELS: Partial<Record<string, string>> = {
  tube: 'Tube',
  dlr: 'DLR',
  tram: 'tram',
  overground: 'Overground',
  'elizabeth-line': 'Elizabeth line',
  metrolink: 'Metrolink',
};

/** What a stop's tag says: "Tube", "DLR", "Tube, DLR" for a stop of both. */
export function stopModesLabel(modes: readonly string[]): string | undefined {
  const labels = [...new Set(modes.map((mode) => STOP_MODE_LABELS[mode.toLowerCase()] ?? mode))];
  return labels.length > 0 ? labels.join(', ') : undefined;
}

/** The modes a stop id says, by its NaPTAN prefix: a TfL stop area
 * (`940GZZLU...` Tube, `940GZZDL...` DLR, `940GZZCR...` tram) or the TfL
 * transit network's spelling of it (`9400ZZLU...`), and a Metrolink
 * station (`9400ZZMA...`). `undefined` for anything else. */
export function stopModesFromId(id: string | null | undefined): string[] | undefined {
  const prefix = /^940[0G]ZZ(LU|DL|CR|MA)/i.exec(id?.trim() ?? '')?.[1]?.toUpperCase();
  const modes: Partial<Record<string, string>> = { LU: 'tube', DL: 'dlr', CR: 'tram', MA: 'metrolink' };
  const mode = prefix ? modes[prefix] : undefined;
  return mode ? [mode] : undefined;
}

/** A stop end's kind: its own modes, else its id's. */
export function stopPlaceKind(stop: { id?: string | null; modes?: readonly string[] | null }): PlaceKind | undefined {
  const modes = stop.modes && stop.modes.length > 0 ? stop.modes : stopModesFromId(stop.id);
  return modes ? { kind: 'stop', modes } : undefined;
}

/** What an end's tag says: "National Rail", or its stop's modes. */
export function placeKindLabel(kind: PlaceKind): string | undefined {
  return kind.kind === 'rail' ? 'National Rail' : stopModesLabel(kind.modes);
}

/** A name to compare: lower case, letters and digits only, without
 * "London" in front or "station" behind ("London Euston" and "Euston
 * Underground Station" both read "euston"). */
function comparable(name: string): string {
  return name
    .toLowerCase()
    .replace(/\b(underground|dlr|rail|railway|tram)?\s*(station|stop)\s*$/, '')
    .replace(/^london\s+/, '')
    .replace(/[^a-z0-9]/g, '');
}

/** Whether two place names would read as the same place. */
export function placeNamesAlike(a: string, b: string): boolean {
  const left = comparable(a);
  return left.length > 0 && left === comparable(b);
}

/** Whether a name already says what it is ("Vauxhall (Tube)"). */
function hasTag(name: string): boolean {
  return /\([^)]*\)\s*$/.test(name);
}

function sameKind(a: PlaceKind, b: PlaceKind): boolean {
  if (a.kind === 'rail' || b.kind === 'rail') return a.kind === b.kind;
  return stopModesLabel(a.modes) === stopModesLabel(b.modes);
}

export interface PlaceEnd {
  name: string;
  kind?: PlaceKind | undefined;
}

/** A walk's or transfer's two place names, each end's kind added where it
 * tells them apart:
 *
 * - a stop end, whenever the two ends are different kinds of station or
 *   their names read alike ("London Euston to Euston (Tube)", "Canning
 *   Town (DLR) to Canning Town (Tube)");
 * - a National Rail end, only when the names read alike ("Vauxhall
 *   (National Rail) to Vauxhall (Tube)").
 *
 * Two ends of one kind with different names ("Bank to Monument", "London
 * Euston to London Victoria") are as given, as is an end whose kind isn't
 * known or whose name already has a tag. `alwaysTagStops`: tag every stop
 * end, as the planner names its stops ("Uxbridge (Tube)"). `onlyWhenAlike`:
 * tag only names that read alike, for a ride (a Tube ride from a stop to a
 * National Rail station is plainly a Tube ride). */
export function labelPlaceEnds(
  origin: PlaceEnd,
  destination: PlaceEnd,
  options: { alwaysTagStops?: boolean; onlyWhenAlike?: boolean } = {},
): [string, string] {
  const alike = placeNamesAlike(origin.name, destination.name);
  const mixed =
    options.onlyWhenAlike !== true &&
    origin.kind !== undefined &&
    destination.kind !== undefined &&
    !sameKind(origin.kind, destination.kind);
  const tagStops = alike || mixed || options.alwaysTagStops === true;
  const label = (end: PlaceEnd): string => {
    if (!end.kind || hasTag(end.name)) return end.name;
    if (end.kind.kind === 'rail' ? !alike : !tagStops) return end.name;
    const tag = placeKindLabel(end.kind);
    return tag ? `${end.name} (${tag})` : end.name;
  };
  return [label(origin), label(destination)];
}
