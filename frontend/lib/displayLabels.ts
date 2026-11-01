import type { Suggestion, TripPlanAccessLeg, TripPlanItinerary, TripPlanLeg } from './types';
import { ownValue } from './ownValue';

/** Human labels for the `category` a `LineSummary`/`CustomLineDetail`
 * carries. Three different sources feed this one string: a catalogue
 * national-rail line's own `lines/*.toml` (`commuter`/`main-line`/
 * `operator`/`regional`), a TfL line's `mode_name` verbatim
 * (`crates/api/src/routes/lines.rs`'s `to_line_summary`) for `tube`/`dlr`/
 * `overground`/`elizabeth-line`/`tram`, and the literal string `"custom"`
 * for a user-created line. None of those raw tokens is something a
 * passenger wrote or would recognise -- review §2.9's "internal codes ...
 * reach user-facing copy" finding named this field by name. Falls back to
 * the raw token itself for anything unlisted (a category this table
 * hasn't caught up with yet), same fallback contract as `stationLabel`. */
const CATEGORY_LABELS: Record<string, string> = {
  'main-line': 'Main line',
  commuter: 'Commuter',
  regional: 'Regional',
  operator: 'Operator network',
  custom: 'Custom line',
  tube: 'London Underground',
  dlr: 'DLR',
  overground: 'London Overground',
  'elizabeth-line': 'Elizabeth line',
  tram: 'London Trams',
};

export function categoryLabel(category: string): string {
  // Signal Box Audit, flib Low finding: "prototype-key lookups can render a
  // function as a label". `category` is an open-ended string (see this
  // module's own doc comment on `CATEGORY_LABELS` -- a TOML `category`, a
  // TfL `mode_name`, or `"custom"`), so a bare `CATEGORY_LABELS[category]`
  // would resolve `category === 'constructor'` to `Object.prototype
  // .constructor` (a function) rather than `undefined` -- `?? category`
  // never fires because a function is neither `null` nor `undefined`, and
  // `app/lines/[id]/page.tsx` would render that function where it expects a
  // string, printing "[object Function]"-shaped output. `ownValue` keeps
  // the lookup to CATEGORY_LABELS' own declared keys.
  return ownValue(CATEGORY_LABELS, category) ?? category;
}

/** Builds a code -> name lookup from the full TOC reference list
 * (`getAllTocs()`), for resolving an ATOC code to its operator name at
 * render time. `getAllTocs()` itself is hour-cached reference data (see
 * its own doc comment in `lib/api.ts`), so a page fetches it once and
 * passes the built lookup down rather than re-scanning the array per
 * code. */
export function tocNameLookup(tocs: Suggestion[]): Map<string, string> {
  return new Map(tocs.map((toc) => [toc.code, toc.name]));
}

/** `"London North Eastern Railway (GR)"`, or the bare code when `lookup`
 * has no match for it -- an operator whose code isn't in the current TOC
 * reference table (a poller gap, or a code retired since) degrades to the
 * code alone rather than losing the row. Same fallback contract and
 * `Name (CODE)` shape as `stationLabel`, for the same reason: the code is
 * what a reader might cross-reference elsewhere, so it's kept even once a
 * name is known. */
export function operatorLabel(code: string, lookup: ReadonlyMap<string, string>): string {
  const name = lookup.get(code);
  return name ? `${name} (${code})` : code;
}

const TRANSFER_MODE_LABELS: Record<string, string> = {
  WALK: 'Walk',
  TUBE: 'Tube',
  METRO: 'Metro',
  BUS: 'Bus',
  FERRY: 'Ferry',
  TRANSFER: 'Change',
};

/** A trip planner transfer leg's mode (the fixed-link feed's WALK, TUBE,
 * ...) as words, never the raw code. */
export function transferModeLabel(mode: string): string {
  return TRANSFER_MODE_LABELS[mode.toUpperCase()] ?? 'Change';
}

/** A transfer leg's line (`/Trips/plan`'s `line`, 2026-10-21) as people
 * write it: "Victoria line". A Tube line's TfL name is the bare name
 * ("Victoria"), so " line" is added; a name that already says it, or one
 * on another mode ("DLR"), is as given. `undefined` without a line. */
export function transferLineName(mode: string, line?: string | null): string | undefined {
  const name = line?.trim();
  if (!name) return undefined;
  return mode.toUpperCase() === 'TUBE' && !/\bline$/i.test(name) ? `${name} line` : name;
}

/** A transfer leg's mode with its line where known: "Tube (Victoria line)". */
export function transferLabel(mode: string, line?: string | null): string {
  const modeLabel = transferModeLabel(mode);
  const lineName = transferLineName(mode, line);
  return lineName ? `${modeLabel} (${lineName})` : modeLabel;
}

type TransferLeg = Extract<TripPlanLeg, { kind: 'transfer' }>;

/** Whether a `TRANSFER` link joins two different stations (Euston to
 * Victoria, 19 minutes), rather than being a change within one station
 * (the same code at both ends, or neither known). */
export function isCrossStationTransfer(leg: Pick<TransferLeg, 'mode' | 'originCrs' | 'destinationCrs'>): boolean {
  if (leg.mode.toUpperCase() !== 'TRANSFER') return false;
  return (leg.originCrs?.toUpperCase() ?? null) !== (leg.destinationCrs?.toUpperCase() ?? null);
}

/** The fixed-link mode a transfer leg is: its own, except that a `TRANSFER`
 * between two stations the plan knows is a Tube hop is `TUBE`. It is one
 * when London Underground runs it (`operator` `LT`: the pair also has a
 * Tube link) or the plan names a Tube stop at both ends (2026-11-01). */
export function transferLegMode(
  leg: Pick<TransferLeg, 'mode' | 'originCrs' | 'destinationCrs' | 'operator' | 'originStop' | 'destinationStop'>,
): string {
  if (!isCrossStationTransfer(leg)) return leg.mode;
  const tubeHop = leg.operator === 'LT' || (leg.originStop !== undefined && leg.destinationStop !== undefined);
  return tubeHop ? 'TUBE' : leg.mode;
}

/** A plan card's words for a transfer leg's mode, with its line: "Tube
 * (Victoria line)" for a Tube link or a known Tube hop, "Transfer" for any
 * other `TRANSFER` between two stations (as the saved leg says), "Change"
 * for a change within one station. */
export function transferLegLabel(
  leg: Pick<
    TransferLeg,
    'mode' | 'originCrs' | 'destinationCrs' | 'operator' | 'originStop' | 'destinationStop' | 'line'
  >,
): string {
  const mode = transferLegMode(leg);
  if (mode.toUpperCase() === 'TRANSFER' && isCrossStationTransfer(leg)) {
    const lineName = transferLineName(mode, leg.line);
    return lineName ? `Transfer (${lineName})` : 'Transfer';
  }
  return transferLabel(mode, leg.line);
}

/** The note under a `/Trips/plan` access leg (DS-135): the leg between a
 * TfL stop From or To and a National Rail station, whose minutes are an
 * estimate. Says what the estimate is from. */
export function accessLegNote(access: TripPlanAccessLeg, stopCount?: number): string {
  const km = (access.distanceMetres / 1000).toFixed(1);
  switch (access.basis) {
    case 'hub':
      return 'Estimated time to walk between the stop and the National Rail station';
    case 'line': {
      const stops = stopCount ? `, ${stopCount} ${stopCount === 1 ? 'stop' : 'stops'}` : '';
      return `Estimated time for about ${km} km along the line${stops}`;
    }
    case 'walk':
    case 'nearest':
      return `Estimated walk: the station is about ${km} km away`;
  }
}

/** An access ride's change of trains, for the leg's own line ("Tube
 * (Piccadilly line) Uxbridge to Heathrow, change at Acton Town, 45 min"),
 * so it reads, and is announced, as part of the route rather than a
 * footnote. `undefined` for a ride on one train. */
export function accessChangeClause(access: TripPlanAccessLeg): string | undefined {
  const name = access.changeAt?.name.trim();
  return name ? `change at ${name}` : undefined;
}

/** An itinerary's changes for its badge (DS-205): every change, National
 * Rail's (`changeCount`) and those within TfL access rides
 * (`accessChangeCount`), as "1 change"; and, when some are on a TfL ride,
 * what they are ("1 Tube change", "1 train change, 1 Tube change") for the
 * badge's tooltip and screen readers. */
export function changesBadge(itinerary: TripPlanItinerary): { total: number; label: string; breakdown?: string } {
  const plural = (n: number, what: string) => `${n} ${what}${n === 1 ? '' : 's'}`;
  const access = Math.max(0, itinerary.accessChangeCount ?? 0);
  const total = itinerary.changeCount + access;
  const label = plural(total, 'change');
  if (access === 0) return { total, label };
  // Each changing access ride's mode ("Tube"), in leg order.
  const byMode = new Map<string, number>();
  for (const leg of itinerary.legs) {
    if (leg.kind === 'transfer' && leg.access?.changeAt) {
      const mode = transferModeLabel(leg.mode);
      byMode.set(mode, (byMode.get(mode) ?? 0) + 1);
    }
  }
  const counted = [...byMode.values()].reduce((sum, n) => sum + n, 0);
  // The legs should name every one; if not, say only that they are TfL's.
  const accessParts =
    counted === access ? [...byMode].map(([mode, n]) => plural(n, `${mode} change`)) : [plural(access, 'TfL change')];
  const parts = [...(itinerary.changeCount > 0 ? [plural(itinerary.changeCount, 'train change')] : []), ...accessParts];
  return { total, label, breakdown: parts.join(', ') };
}

/** Where an access ride is boarded or left when that is another stop area
 * of the station ("Board at Paddington (H&C Line)."), or `undefined` when
 * it uses the plan's own stops. */
export function accessStopsNote(access: TripPlanAccessLeg): string | undefined {
  const parts = [
    access.boardStop?.name.trim() ? `Board at ${access.boardStop.name.trim()}.` : null,
    access.alightStop?.name.trim() ? `Leave at ${access.alightStop.name.trim()}.` : null,
  ].filter((part): part is string => part !== null);
  return parts.length > 0 ? parts.join(' ') : undefined;
}
