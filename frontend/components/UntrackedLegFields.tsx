'use client';

import { Autocomplete, Group, Select, Stack, Text, TextInput } from '@mantine/core';
import { DateInput } from '@mantine/dates';
import dayjs from 'dayjs';
import { useCallback } from 'react';
import { TimeFilterInput } from './TimeFilterInput';
import { PICKER_DATE_FORMAT } from '@/lib/dateFormat';
import { addCalendarDays, LONDON_TZ } from '@/lib/londonWallClock';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { searchTflStops } from '@/lib/suggestions';
import { TFL_STOPS_ATTRIBUTION } from '@/lib/tflAttribution';
import { hasTflStops, TRANSIT_MODES } from '@/lib/untrackedLeg';
import { useSuggestions } from '@/lib/useSuggestions';
import type { TransitMode, UntrackedLegRequest } from '@/lib/types';

/** What the untracked-leg form holds: a mode, optional line and places,
 * a London date and two London wall-clock times (`'HH:MM'`). */
export interface UntrackedLegDraft {
  transitMode: TransitMode;
  lineName: string;
  originName: string;
  destinationName: string;
  /** The TfL stop each place was picked from (Tube, DLR and tram), or
   * `null` for typed text. Editing the text drops it. */
  originStopId: string | null;
  destinationStopId: string | null;
  date: string; // 'YYYY-MM-DD'
  leaves: string; // 'HH:MM' or ''
  arrives: string; // 'HH:MM' or ''
  /** A time field holds a half-typed value (see `TimeFilterInput`). */
  incomplete: { leaves: boolean; arrives: boolean };
}

/** A fresh draft, seeded from the previous leg when there is one: its
 * arrival date and time (the same carry-over the train modes get) and its
 * destination as this leg's start. */
export function newUntrackedLegDraft(seed: { date: string; leaves?: string; originName?: string }): UntrackedLegDraft {
  return {
    transitMode: 'tube',
    lineName: '',
    originName: seed.originName ?? '',
    destinationName: '',
    originStopId: null,
    destinationStopId: null,
    date: seed.date,
    leaves: seed.leaves ?? '',
    arrives: '',
    incomplete: { leaves: false, arrives: false },
  };
}

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;
const TIME_PATTERN = /^\d{2}:\d{2}$/;

function isValidDate(value: string): boolean {
  if (!DATE_PATTERN.test(value)) return false;
  const parsed = dayjs(value);
  return parsed.isValid() && parsed.format('YYYY-MM-DD') === value;
}

/** Whether the arrival time is earlier in the day than the departure, so
 * the leg runs past midnight and arrives the next day. */
export function untrackedDraftArrivesNextDay(draft: UntrackedLegDraft): boolean {
  return TIME_PATTERN.test(draft.leaves) && TIME_PATTERN.test(draft.arrives) && draft.arrives < draft.leaves;
}

/** The request body for a complete draft, or `null` while it isn't one.
 * Times are London wall-clock times on the draft's date, sent as UTC
 * instants; an arrival earlier than the departure is the next day. Blank
 * text is left out. */
export function untrackedDraftRequest(draft: UntrackedLegDraft): UntrackedLegRequest | null {
  if (!isValidDate(draft.date) || !TIME_PATTERN.test(draft.leaves) || !TIME_PATTERN.test(draft.arrives)) {
    return null;
  }
  if (draft.incomplete.leaves || draft.incomplete.arrives) return null;
  const arrivalDate = untrackedDraftArrivesNextDay(draft) ? addCalendarDays(draft.date, 1) : draft.date;
  const departure = dayjs.tz(`${draft.date} ${draft.leaves}:00`, LONDON_TZ);
  const arrival = dayjs.tz(`${arrivalDate} ${draft.arrives}:00`, LONDON_TZ);
  if (!departure.isValid() || !arrival.isValid()) return null;
  const text = (value: string) => {
    const trimmed = value.trim();
    return trimmed === '' ? undefined : trimmed;
  };
  const lineName = text(draft.lineName);
  const originName = text(draft.originName);
  const destinationName = text(draft.destinationName);
  // A stop id only with its place, and only for a mode with TfL stops.
  const stops = hasTflStops(draft.transitMode);
  const originStopId = stops && originName ? draft.originStopId : null;
  const destinationStopId = stops && destinationName ? draft.destinationStopId : null;
  return {
    mode: 'untracked',
    transitMode: draft.transitMode,
    ...(lineName ? { lineName } : {}),
    ...(originName ? { originName } : {}),
    ...(originStopId ? { originStopId } : {}),
    ...(destinationName ? { destinationName } : {}),
    ...(destinationStopId ? { destinationStopId } : {}),
    plannedDeparture: departure.toISOString(),
    plannedArrival: arrival.toISOString(),
  };
}

/** One place of an untracked leg whose mode has TfL stops: an
 * `Autocomplete` over that mode's stops (`GET /tfl/stops`, the site's
 * `useSuggestions` + `suggestionAutocompleteProps` pattern). Picking a stop
 * writes its name into the field and reports its id; typing reports the
 * text with no id, so free text still works
 * (docs/superpowers/specs/2026-10-20-tfl-stop-lookup-design.md §6). */
function StopPlaceField({
  label,
  placeholder,
  mode,
  value,
  onType,
  onPick,
}: {
  label: string;
  placeholder: string;
  mode: TransitMode;
  value: string;
  /** The field's text changed (typed, or written by a pick just after
   * `onPick`). */
  onType: (text: string) => void;
  /** A stop was picked: its name and id. */
  onPick: (name: string, stopId: string) => void;
}) {
  const search = useCallback((q: string, signal: AbortSignal) => searchTflStops(q, [mode], signal), [mode]);
  const { suggestions, loading } = useSuggestions(value, search);
  return (
    <Autocomplete
      label={label}
      placeholder={placeholder}
      value={value}
      maxLength={100}
      onChange={onType}
      onOptionSubmit={(id) => {
        const stop = suggestions.find((suggestion) => suggestion.code === id);
        if (stop) onPick(stop.name, stop.code);
      }}
      {...suggestionAutocompleteProps(suggestions, {
        query: value,
        loading,
        noMatchMessage: 'No matching stops. You can still type a place.',
        fill: 'name',
      })}
    />
  );
}

/** The fields of an untracked leg (a Tube ride, a bus, a walk; spec
 * docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
 * §2): how, line, from and to, date, leaves and arrives. For Tube, DLR and
 * tram, from and to suggest TfL's stops (`StopPlaceField`) and keep the
 * picked stop's id; otherwise they are free text. Shared by "Add a leg" and the "Tube, bus or
 * walk" start on `/journeys/new`, which own the draft and submit
 * `untrackedDraftRequest(draft)`. */
export function UntrackedLegFields({
  draft,
  onChange,
  priorArrivalTime,
}: {
  draft: UntrackedLegDraft;
  /** Takes an updater, so two fields changing in one event (a time and
   * its half-typed flag) both land. Pass a `useState` setter. */
  onChange: (update: (draft: UntrackedLegDraft) => UntrackedLegDraft) => void;
  /** The previous leg's arrival (`'HH:MM'`), said under "Leaves" when known. */
  priorArrivalTime?: string | null | undefined;
}) {
  const set = (patch: Partial<UntrackedLegDraft>) => {
    onChange((current) => ({ ...current, ...patch }));
  };
  // Mantine's `Autocomplete` writes a picked option's label into the field
  // (`onChange`) right after `onOptionSubmit`, so a text change that leaves
  // the name as it is keeps the stop; any real edit drops it.
  const typeOrigin = (text: string) => {
    onChange((current) => ({
      ...current,
      originName: text,
      originStopId: text === current.originName ? current.originStopId : null,
    }));
  };
  const typeDestination = (text: string) => {
    onChange((current) => ({
      ...current,
      destinationName: text,
      destinationStopId: text === current.destinationName ? current.destinationStopId : null,
    }));
  };
  const setIncomplete = (field: 'leaves' | 'arrives', incomplete: boolean) => {
    onChange((current) => ({ ...current, incomplete: { ...current.incomplete, [field]: incomplete } }));
  };
  return (
    <Stack gap="sm">
      <Text size="sm" c="dimmed">
        For a journey we have no live data for. It shows as planned, then in progress between these times.
      </Text>
      <Select
        label="How"
        data={TRANSIT_MODES.map(({ value, label }) => ({ value, label }))}
        value={draft.transitMode}
        onChange={(value) => {
          const mode = TRANSIT_MODES.find((entry) => entry.value === value);
          // Stops are per mode: a new mode keeps the text, not the ids.
          if (mode) set({ transitMode: mode.value, originStopId: null, destinationStopId: null });
        }}
        allowDeselect={false}
      />
      <TextInput
        label="Line (optional)"
        placeholder="e.g. Victoria line"
        value={draft.lineName}
        maxLength={60}
        onChange={(event) => set({ lineName: event.currentTarget.value })}
      />
      <Group grow align="flex-start">
        {hasTflStops(draft.transitMode) ? (
          <>
            <StopPlaceField
              label="From (optional)"
              placeholder="e.g. King's Cross St Pancras"
              mode={draft.transitMode}
              value={draft.originName}
              onType={typeOrigin}
              onPick={(originName, originStopId) => set({ originName, originStopId })}
            />
            <StopPlaceField
              label="To (optional)"
              placeholder="e.g. Victoria"
              mode={draft.transitMode}
              value={draft.destinationName}
              onType={typeDestination}
              onPick={(destinationName, destinationStopId) => set({ destinationName, destinationStopId })}
            />
          </>
        ) : (
          <>
            <TextInput
              label="From (optional)"
              placeholder="e.g. King's Cross St Pancras"
              value={draft.originName}
              maxLength={100}
              onChange={(event) => set({ originName: event.currentTarget.value, originStopId: null })}
            />
            <TextInput
              label="To (optional)"
              placeholder="e.g. Victoria"
              value={draft.destinationName}
              maxLength={100}
              onChange={(event) => set({ destinationName: event.currentTarget.value, destinationStopId: null })}
            />
          </>
        )}
      </Group>
      {/* TfL's terms, where its stop data is offered (see
          `OpenDataAttribution.tsx`). */}
      {hasTflStops(draft.transitMode) && (
        <Text size="xs" c="dimmed">
          Stop names: {TFL_STOPS_ATTRIBUTION}
        </Text>
      )}
      <DateInput
        label="Date"
        valueFormat={PICKER_DATE_FORMAT}
        value={draft.date || null}
        onChange={(value) => set({ date: value ?? '' })}
      />
      <Group grow align="flex-start">
        <TimeFilterInput
          clockHint
          label="Leaves"
          name="leaves"
          description={priorArrivalTime ? `Your last leg arrives at ${priorArrivalTime}.` : 'UK time.'}
          value={draft.leaves}
          onChange={(leaves) => set({ leaves })}
          onIncompleteChange={(incomplete) => setIncomplete('leaves', incomplete)}
          error={null}
        />
        <TimeFilterInput
          label="Arrives"
          name="arrives"
          description={untrackedDraftArrivesNextDay(draft) ? 'The next day.' : 'UK time.'}
          value={draft.arrives}
          onChange={(arrives) => set({ arrives })}
          onIncompleteChange={(incomplete) => setIncomplete('arrives', incomplete)}
          error={null}
        />
      </Group>
    </Stack>
  );
}
