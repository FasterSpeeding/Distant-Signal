import type { Suggestion } from './types';

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
  return CATEGORY_LABELS[category] ?? category;
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
export function operatorLabel(code: string, lookup: Map<string, string>): string {
  const name = lookup.get(code);
  return name ? `${name} (${code})` : code;
}
