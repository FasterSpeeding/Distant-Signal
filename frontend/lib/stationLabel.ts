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

/** `"A (AAA) → B (BBB)"`, or just the origin when there is no destination
 * (a pre-match pin genuinely has none -- see
 * `2026-09-01-tracked-trains-home-page-design.md` Decision 1). */
export function routeLabel(
  originCrs: string,
  originName: string | null | undefined,
  destinationCrs: string | null | undefined,
  destinationName: string | null | undefined,
): string {
  const origin = stationLabel(originCrs, originName);
  if (!destinationCrs) return origin;
  return `${origin} → ${stationLabel(destinationCrs, destinationName)}`;
}
