import { describe, it, expect } from 'vitest';
import { suggestionAutocompleteProps } from './suggestionAutocomplete';
import { NO_MATCH_OPTION_VALUE } from './autocompleteNoMatch';
import type { Suggestion } from './types';

const STATIONS: Suggestion[] = [
  { code: 'WOK', name: 'Woking' },
  { code: 'RDG', name: 'Reading' },
];

describe('suggestionAutocompleteProps', () => {
  describe('data (requirement 1: display)', () => {
    it('keeps `label` as the bare code -- Autocomplete writes `label`, not `renderOption`, into the field on selection', () => {
      const { data } = suggestionAutocompleteProps(STATIONS, {
        query: 'wo',
        loading: false,
        noMatchMessage: 'No matching stations',
      });
      expect(data).toEqual([
        { value: 'WOK', label: 'WOK' },
        { value: 'RDG', label: 'RDG' },
      ]);
    });

    it('swaps in the no-match placeholder once a settled search has come back empty', () => {
      const { data } = suggestionAutocompleteProps([], {
        query: 'zzz',
        loading: false,
        noMatchMessage: 'No matching stations',
      });
      expect(data).toEqual([{ value: NO_MATCH_OPTION_VALUE, label: 'No matching stations', disabled: true }]);
    });

    it('does not show the placeholder while the field is empty or still loading', () => {
      expect(
        suggestionAutocompleteProps([], { query: '', loading: false, noMatchMessage: 'No matching stations' }).data,
      ).toEqual([]);
      expect(
        suggestionAutocompleteProps([], { query: 'wo', loading: true, noMatchMessage: 'No matching stations' }).data,
      ).toEqual([]);
    });
  });

  describe('renderOption (requirement 1: display)', () => {
    it('renders every real option as "CODE — Name", both code and full name together', () => {
      const { renderOption } = suggestionAutocompleteProps(STATIONS, {
        query: 'wo',
        loading: false,
        noMatchMessage: 'No matching stations',
      });
      expect(renderOption!({ option: { value: 'WOK' } })).toBe('WOK — Woking');
      expect(renderOption!({ option: { value: 'RDG' } })).toBe('RDG — Reading');
    });

    it('falls back to the bare value for a value not present in the current suggestions', () => {
      const { renderOption } = suggestionAutocompleteProps(STATIONS, {
        query: 'wo',
        loading: false,
        noMatchMessage: 'No matching stations',
      });
      expect(renderOption!({ option: { value: 'ZZZ' } })).toBe('ZZZ');
    });

    it('renders the no-match placeholder message for the sentinel value', () => {
      const { renderOption } = suggestionAutocompleteProps([], {
        query: 'zzz',
        loading: false,
        noMatchMessage: 'No matching stations',
      });
      expect(renderOption!({ option: { value: NO_MATCH_OPTION_VALUE } })).toBe('No matching stations');
    });
  });

  describe('filter (requirement 2: search matching)', () => {
    // `suggestions` already comes back server-filtered against code, full
    // name, and any substring of the name (the shared `searchStations`/
    // `searchTocs` endpoints) -- this passthrough is what stops Mantine's
    // own default filter (which only checks `label`, the bare code) from
    // re-narrowing those results and hiding a correct name/substring
    // match.
    it('passes options through unchanged, regardless of the in-flight search text', () => {
      const { filter } = suggestionAutocompleteProps(STATIONS, {
        query: 'reading',
        loading: false,
        noMatchMessage: 'No matching stations',
      });
      const options = [
        { value: 'WOK', label: 'WOK' },
        { value: 'RDG', label: 'RDG' },
      ];
      expect(filter!({ options, search: 'reading', limit: Infinity })).toBe(options);
    });
  });
});
