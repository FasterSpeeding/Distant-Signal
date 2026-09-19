/** Works around a gap `nothingFoundMessage` can't reach: Mantine's
 * `Select`/`MultiSelect` accept a `nothingFoundMessage` prop that renders
 * into `Combobox.Empty` whenever their options list is empty (see
 * `components/IncidentSearchForm.tsx`'s own `noOptionsFound` for why that
 * matters -- an unstyled `Combobox.Empty` has no ARIA role, so a plain
 * string there fails axe's `aria-required-children`). `Autocomplete` does
 * NOT expose that prop at all (`@mantine/core` 9.5.2,
 * `node_modules/@mantine/core/esm/components/Autocomplete/Autocomplete.mjs`):
 * it hardcodes `hiddenWhenEmpty: true` on its internal `OptionsDropdown`
 * and never forwards a `nothingFoundMessage` of its own -- confirmed by
 * `tsc` rejecting the prop outright, not just by reading the source. So
 * every CRS/operator-code `Autocomplete` in this app (station and
 * operator search across `TrackTrainForm`, `TrainSearchForm`,
 * `TicketEntryForm`, `StationSearchForm`, `CustomLineForm`) hits the exact
 * same "open combobox, zero-child listbox" gap as the Select/MultiSelect
 * fields did, but can't take the same fix.
 *
 * The workaround is the options list itself, which IS fully
 * caller-controlled: `OptionsDropdown` only hides the dropdown when its
 * *parsed data* is empty (`isEmptyComboboxData`), so swapping in one
 * `disabled: true` placeholder item -- instead of an empty array -- makes
 * the listbox non-empty from Mantine's own point of view, and
 * `ComboboxOption` always renders `role="option"` regardless of
 * `disabled` (`ComboboxOption.mjs`), satisfying `aria-required-children`
 * the same way a real option would. `disabled: true` keeps
 * `ComboboxOption`'s own click handler a no-op
 * (`if (!disabled) { onOptionSubmit } else preventDefault()`), so the
 * placeholder can't actually be picked.
 *
 * One real gap remains, noted rather than silently accepted: this
 * `Autocomplete`/`OptionsDropdown` version doesn't forward a custom
 * `data` field through to `aria-disabled` on the option itself (only
 * `value`/`disabled`/a few `data-*` attributes make that trip -- see
 * `OptionsDropdown.mjs`'s own `Option` helper), unlike
 * `noOptionsFound`'s explicit `aria-disabled="true"` for the
 * Select/MultiSelect fix. A screen reader hears this as an ordinary
 * (if inert) option, not an explicitly disabled one -- a smaller gap than
 * the `aria-required-children` failure this closes, and not one
 * `Autocomplete`'s public API has a way to close in this version. */
export const NO_MATCH_OPTION_VALUE = '__no-match__';

export interface AutocompleteOptionLike {
  value: string;
  label: string;
}

/** `realOptions` unchanged whenever it has anything in it; a single inert
 * placeholder standing in for "nothing found" otherwise. Pass the result
 * straight through as `Autocomplete`'s `data` -- it's already shaped as
 * `{ value, label }[]`, no group support needed here since none of this
 * app's CRS/operator-code fields ever group their options. */
export function withNoMatchPlaceholder(
  realOptions: AutocompleteOptionLike[],
  message: string,
): AutocompleteOptionLike[] {
  if (realOptions.length > 0) {
    return realOptions;
  }
  return [{ value: NO_MATCH_OPTION_VALUE, label: message }];
}

/** For a call site's own `renderOption`, which otherwise runs its normal
 * "look the code up in the live suggestions list" logic, finds nothing for
 * the synthetic placeholder `withNoMatchPlaceholder` swaps in, and falls
 * back to printing the raw sentinel value. Returns `message` (the exact
 * same string passed to `withNoMatchPlaceholder` for this field) when
 * `value` is that placeholder, `null` otherwise -- deliberately taking
 * `message` as a parameter rather than reading it off `option.label`,
 * since `Autocomplete`'s own `renderOption` types its `option` as
 * `ComboboxGenericItem` (`value`/`disabled` only, no `label`). */
export function noMatchOptionContent(value: string, message: string): string | null {
  return value === NO_MATCH_OPTION_VALUE ? message : null;
}
