/** `"London Kings Cross (KGX)"`, or the bare code when no name resolved.
 *
 * `Name (CRS)` rather than name-only: this is already what
 * `app/stations/[crs]/page.tsx` renders as its heading and what
 * `app/page.tsx` renders for pinned stations, and the code is what a
 * reader cross-references against a ticket or a departure board. See
 * docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F3.
 *
 * `name` is `null` whenever the backend's `LEFT JOIN stations` found no
 * reference row for the code, so every caller needs this fallback and
 * none of them should hand-roll it. */
export function stationLabel(crs: string, name: string | null | undefined): string {
  return name ? `${name} (${crs})` : crs;
}

/** What to render in place of a station when there isn't one to name. A
 * tracked train's `pinOriginCrs` is genuinely `null` for a subscription
 * created by `train_uid` alone, before any schedule data has been matched
 * to the shared train -- see `TrackedTrainState.pinOriginCrs` in
 * `lib/types.ts`. */
export const UNKNOWN_STATION_LABEL = 'Unknown station';

/** `"A (AAA) → B (BBB)"`, or just the origin when there is no destination
 * (a pre-match pin genuinely has none -- see
 * `2026-09-01-tracked-trains-home-page-design.md` Decision 1).
 *
 * When BOTH ends have a code, the two ends are always formatted the same
 * way as each other: full `Name (CODE)` when both names resolved, bare
 * codes on both ends otherwise. Both ends run through the exact same
 * `stationLabel` the autocomplete's own results use -- there's no separate
 * lookup for either side -- so a name being null here means the same
 * thing on either end: no reference row for that code. Previously the two
 * ends were resolved independently, so a route with only one name known
 * rendered as e.g. "London Kings Cross (KGX) → EDB" -- a real code paired
 * with a resolved name right next to a bare code, which reads as a data
 * error rather than a degraded lookup (review §2.9). Falling back to a
 * bare code is right; mixing the two forms in one string is not. */
export function routeLabel(
  originCrs: string | null | undefined,
  originName: string | null | undefined,
  destinationCrs: string | null | undefined,
  destinationName: string | null | undefined,
): string {
  // `originCrs` is nullable as of the shared-train-identity change: an
  // NR-primary subscription whose train has no schedule match yet has no
  // origin at all, not merely no origin *name*. This is a different kind
  // of "unknown" from a resolvable-but-unresolved name (there is no code
  // to fall back to, bare or otherwise), so it keeps its own placeholder
  // rather than joining the both-ends-consistent branch below.
  if (!originCrs) {
    if (!destinationCrs) return UNKNOWN_STATION_LABEL;
    return `${UNKNOWN_STATION_LABEL} → ${stationLabel(destinationCrs, destinationName)}`;
  }
  if (!destinationCrs) return stationLabel(originCrs, originName);
  return originName && destinationName
    ? `${stationLabel(originCrs, originName)} → ${stationLabel(destinationCrs, destinationName)}`
    : `${originCrs} → ${destinationCrs}`;
}

/** `"EUS — London Euston"`, or the bare code when no name resolved. The
 * code-first counterpart to `stationLabel` above (which puts the name
 * first, `Name (CODE)`): this is the shape every CRS/TOC `Autocomplete`
 * dropdown in this app already renders a suggestion as
 * (`suggestionAutocompleteProps` in `lib/suggestionAutocomplete.ts`, and
 * `AllLinesTable.tsx`'s own operator picker), so a PERMANENT summary built
 * from a code+name pair that visitor just picked from one of those pickers
 * -- e.g. `PlanTripFlow`'s segment/itinerary summaries -- uses this
 * code-first shape instead of `stationLabel`'s name-first one, to read as
 * the same kind of label the picker itself already showed, not a
 * different convention. `GET /Trips/plan` (unlike every other
 * station-bearing response in this app) has no `*Name` sibling field of
 * its own to resolve this server-side, which is why callers need to
 * resolve names themselves (see `lib/suggestions.ts`'s `getStationNames`)
 * and hand the result through here. */
export function codeStationLabel(crs: string, name: string | null | undefined): string {
  return name ? `${crs} — ${name}` : crs;
}

/** Code-first two-sided join, mirroring `routeLabel`'s own "never mix a
 * resolved name with a bare code on the other end" rule (see its doc
 * comment) -- a route with only one name known must not render as e.g.
 * "EUS — London Euston → MKC" (a resolved name right next to a bare code
 * reads as a data error, not a degraded lookup). Built from
 * `codeStationLabel` rather than `stationLabel` for the same reason that
 * function exists: to match the code-first shape of the picker the
 * visitor used to choose these stations.
 *
 * `unknownLabel` defaults to `'?'` -- `ItineraryOption`'s own established
 * placeholder for a `TripPlanLeg` whose CRS is genuinely `null` (a TIPLOC
 * with no CRS mapping), NOT `routeLabel`'s `UNKNOWN_STATION_LABEL` (that
 * placeholder is `TrackedTrainState`'s own pre-match-pin convention, a
 * different caller with a different null-shaped reason). A caller whose
 * ends are never nullable (e.g. `TripPlanSegment.originCrs`/
 * `destinationCrs`) never reaches this branch at all. */
export function codeRouteLabel(
  originCrs: string | null,
  originName: string | null | undefined,
  destinationCrs: string | null,
  destinationName: string | null | undefined,
  unknownLabel = '?',
): string {
  if (!originCrs || !destinationCrs) {
    return `${originCrs ?? unknownLabel} → ${destinationCrs ?? unknownLabel}`;
  }
  return originName && destinationName
    ? `${codeStationLabel(originCrs, originName)} → ${codeStationLabel(destinationCrs, destinationName)}`
    : `${originCrs} → ${destinationCrs}`;
}
