/** Where a station's departures are: the train search ("Find a train",
 * `/trains`) pre-filled with that station. The station page shows no
 * departures of its own and links here instead. Upper case, as the
 * search's own URLs are. */
export function stationDeparturesHref(crs: string): string {
  return `/trains?station=${encodeURIComponent(crs.toUpperCase())}`;
}
