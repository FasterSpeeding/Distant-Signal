'use client';

import { useState } from 'react';
import { Autocomplete, Button, Group, Stack, Text } from '@mantine/core';
import { DateInput } from '@mantine/dates';
import { TimeFilterInput } from './TimeFilterInput';
import { PICKER_DATE_FORMAT } from '@/lib/dateFormat';
import { legSearchDraftFromLeg, legSearchFromDraft, type LegSearch, type LegSearchDraft } from '@/lib/legSearch';
import { resolveStationCode, suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import type { JourneyLegDetail } from '@/lib/types';

type TimeField = 'departAfter' | 'departBefore' | 'arriveAfter' | 'arriveBefore';

/** "Edit search" on a train leg of a journey: the leg's own window search
 * (its stations, service date, and earliest and latest departure and
 * arrival), opened with everything the leg was searched with
 * (`legSearchDraftFromLeg`), or with `initial` when the traveller has
 * already edited it once. "Search" hands the complete search to
 * `onSearch`, which lists its trains for THIS leg (`JourneyLegCandidates`'s
 * `search`); nothing is saved until a train is picked.
 *
 * The same fields and rules as the "Search a time window" mode of "Add a
 * leg" (`AddJourneyLegButton`): station search by code or name, at least
 * one time, no earliest later than its latest. A window search has no via
 * or avoid stations, and the leg stores nothing else, so there is nothing
 * else to carry over. */
export function LegSearchForm({
  leg,
  initial = null,
  onSearch,
  onCancel,
}: {
  leg: JourneyLegDetail;
  initial?: LegSearch | null;
  onSearch: (search: LegSearch) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState<LegSearchDraft>(() =>
    initial
      ? {
          origin: initial.originCrs,
          destination: initial.destinationCrs,
          serviceDate: initial.serviceDate,
          departAfter: initial.departAfter ?? '',
          departBefore: initial.departBefore ?? '',
          arriveAfter: initial.arriveAfter ?? '',
          arriveBefore: initial.arriveBefore ?? '',
        }
      : legSearchDraftFromLeg(leg),
  );
  const [incomplete, setIncomplete] = useState<Record<TimeField, boolean>>({
    departAfter: false,
    departBefore: false,
    arriveAfter: false,
    arriveBefore: false,
  });
  const [touched, setTouched] = useState({ origin: false, destination: false });
  const { suggestions: originSuggestions, loading: originLoading } = useSuggestions(draft.origin, searchStations);
  const { suggestions: destinationSuggestions, loading: destinationLoading } = useSuggestions(
    draft.destination,
    searchStations,
  );
  const origin = resolveStationCode(draft.origin, originSuggestions);
  const destination = resolveStationCode(draft.destination, destinationSuggestions);
  const search = Object.values(incomplete).some(Boolean) ? null : legSearchFromDraft(draft, { origin, destination });

  const set = (patch: Partial<LegSearchDraft>) => setDraft((current) => ({ ...current, ...patch }));
  const stationError = (field: 'origin' | 'destination', code: string | null, loading: boolean) =>
    touched[field] && draft[field].trim() !== '' && code === null && !loading
      ? 'Must be a 3-letter station code'
      : null;
  const time = (field: TimeField, label: string, description: string, clockHint = false) => (
    <TimeFilterInput
      clockHint={clockHint}
      label={label}
      name={label.replace(' (optional)', '').toLowerCase()}
      description={description}
      value={draft[field]}
      onChange={(value) => set({ [field]: value })}
      onIncompleteChange={(value) => setIncomplete((current) => ({ ...current, [field]: value }))}
      error={null}
    />
  );

  return (
    <Stack gap="sm">
      <Group grow align="flex-start">
        <Autocomplete
          label="From station"
          placeholder="e.g. Woking or WOK"
          value={draft.origin}
          onChange={(value) => set({ origin: value })}
          onBlur={() => setTouched((current) => ({ ...current, origin: true }))}
          {...suggestionAutocompleteProps(originSuggestions, {
            query: draft.origin,
            loading: originLoading,
            noMatchMessage: 'No matching stations',
          })}
          error={stationError('origin', origin, originLoading)}
        />
        <Autocomplete
          label="To station"
          placeholder="e.g. London Waterloo or WAT"
          value={draft.destination}
          onChange={(value) => set({ destination: value })}
          onBlur={() => setTouched((current) => ({ ...current, destination: true }))}
          {...suggestionAutocompleteProps(destinationSuggestions, {
            query: draft.destination,
            loading: destinationLoading,
            noMatchMessage: 'No matching stations',
          })}
          error={stationError('destination', destination, destinationLoading)}
        />
      </Group>
      <DateInput
        label="Service date"
        valueFormat={PICKER_DATE_FORMAT}
        value={draft.serviceDate || null}
        onChange={(value) => set({ serviceDate: value ?? '' })}
      />
      <Text size="sm">At least one of the four times below is required.</Text>
      <Group grow align="flex-start">
        {time(
          'departAfter',
          'Earliest departure (optional)',
          `Only trains leaving ${origin ?? 'the origin'} at or after this time.`,
          true,
        )}
        {time(
          'departBefore',
          'Latest departure (optional)',
          `Only trains leaving ${origin ?? 'the origin'} at or before this time.`,
        )}
      </Group>
      <Group grow align="flex-start">
        {time(
          'arriveAfter',
          'Earliest arrival (optional)',
          `Only trains reaching ${destination ?? 'the destination'} at or after this time.`,
        )}
        {time(
          'arriveBefore',
          'Latest arrival (optional)',
          `Only trains reaching ${destination ?? 'the destination'} at or before this time.`,
        )}
      </Group>
      <Group gap="xs">
        <Button size="xs" disabled={search === null} onClick={() => search && onSearch(search)}>
          Search
        </Button>
        <Button size="xs" variant="default" onClick={onCancel}>
          Cancel
        </Button>
      </Group>
    </Stack>
  );
}
