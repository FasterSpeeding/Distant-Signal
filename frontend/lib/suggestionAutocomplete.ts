import type { AutocompleteProps } from '@mantine/core';
import type { Suggestion } from './types';
import { noMatchOptionContent, withNoMatchPlaceholder } from './autocompleteNoMatch';

/** The `data`/`filter`/`renderOption` trio every CRS/TOC-code `Autocomplete`
 * field in this app (station pickers, operator pickers) needs around a live
 * `useSuggestions(query, searchStations | searchTocs)` result, factored out
 * of what used to be five call sites' worth of hand-copied, independently
 * drifting boilerplate (`TrackTrainForm.tsx`, `TrainSearchForm.tsx`,
 * `TicketEntryForm.tsx`, `StationSearchForm.tsx`, `CustomLineForm.tsx`).
 * One of those five copies (`TrackTrainForm.tsx`'s window-mode Destination
 * field) had silently dropped `renderOption` -- nothing forced the three
 * pieces to be kept in sync by hand, so its dropdown quietly regressed to
 * showing the bare CRS code instead of `CODE — Name` like every sibling
 * field. Routing every call site through this one function is the fix for
 * the class of bug, not just that one instance: there is now exactly one
 * place this trio can be assembled, so a future field either uses it (and
 * gets all three pieces, always in sync) or has to visibly opt out.
 *
 * Two requirements this enforces for every caller, uniformly:
 *
 * 1. Display -- every option always shows `CODE — Name` (the bare code
 *    only for the synthetic "no matches" placeholder, which isn't a real
 *    suggestion). `Autocomplete` writes `data`'s `label` — not `value` —
 *    into the field on selection (confirmed by reading its source:
 *    `handleValueChange(optionsLockup[val].label)`), so `label` is kept as
 *    the bare code (what should land in the input once picked) and the
 *    friendlier `CODE — Name` text is rendered dropdown-only via
 *    `renderOption`, which doesn't affect what gets written into the
 *    field.
 * 2. Search matching -- callers already get "code, full name, or any
 *    substring of the full name, case-insensitively" for free, because
 *    `suggestions` itself comes from the shared `searchStations`/
 *    `searchTocs` server-side search every caller already goes through
 *    (`lib/suggestions.ts`) -- the matching logic lives once, server-side,
 *    not reimplemented per field. What this function guarantees on the
 *    CLIENT side is that nothing narrows those results further: `filter`
 *    is pinned to a passthrough, because Mantine's own default filter only
 *    checks `label` (the bare code per point 1 above), which would hide a
 *    correct name/substring match the server already found.
 */
export function suggestionAutocompleteProps(
  suggestions: Suggestion[],
  {
    query,
    loading,
    noMatchMessage,
  }: {
    /** The field's own current (typed) value -- gates the "no matches"
     * placeholder the same way `withNoMatchPlaceholder`'s `active` already
     * requires: only once there's something to say "no matches" about. */
    query: string;
    /** This field's own `useSuggestions` loading flag -- see `query`. */
    loading: boolean;
    /** e.g. `'No matching stations'` / `'No matching operators'` -- shown
     * both as the inert placeholder row and, via `renderOption`, as that
     * placeholder's own rendered content. */
    noMatchMessage: string;
  },
): Pick<AutocompleteProps, 'data' | 'filter' | 'renderOption'> {
  return {
    data: withNoMatchPlaceholder(
      suggestions.map((s) => ({ value: s.code, label: s.code })),
      noMatchMessage,
      { active: query.trim().length > 0 && !loading },
    ),
    // `suggestions` is already server-side filtered (the API matches the
    // search term against both the code and the full name) -- Mantine's
    // default client-side re-filter only checks `label` (the bare code, per
    // this function's own doc comment), which would hide a correct
    // name/substring match the server already found. Disable it: show
    // whatever `suggestions` already contains, unfiltered further.
    filter: ({ options }) => options,
    renderOption: ({ option }) => {
      const placeholder = noMatchOptionContent(option.value, noMatchMessage);
      if (placeholder) return placeholder;
      const match = suggestions.find((s) => s.code === option.value);
      return match ? `${match.code} — ${match.name}` : option.value;
    },
  };
}
