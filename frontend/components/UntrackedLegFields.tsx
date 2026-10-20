'use client';

import { Group, Select, Stack, Text, TextInput } from '@mantine/core';
import { DateInput } from '@mantine/dates';
import dayjs from 'dayjs';
import { TimeFilterInput } from './TimeFilterInput';
import { PICKER_DATE_FORMAT } from '@/lib/dateFormat';
import { addCalendarDays, LONDON_TZ } from '@/lib/londonWallClock';
import { TRANSIT_MODES } from '@/lib/untrackedLeg';
import type { TransitMode, UntrackedLegRequest } from '@/lib/types';

/** What the untracked-leg form holds: a mode, optional line and places,
 * a London date and two London wall-clock times (`'HH:MM'`). */
export interface UntrackedLegDraft {
  transitMode: TransitMode;
  lineName: string;
  originName: string;
  destinationName: string;
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
  return {
    mode: 'untracked',
    transitMode: draft.transitMode,
    ...(lineName ? { lineName } : {}),
    ...(originName ? { originName } : {}),
    ...(destinationName ? { destinationName } : {}),
    plannedDeparture: departure.toISOString(),
    plannedArrival: arrival.toISOString(),
  };
}

/** The fields of an untracked leg (a Tube ride, a bus, a walk; spec
 * docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
 * §2): how, line, from and to (free text: there is no TfL stop lookup),
 * date, leaves and arrives. Shared by "Add a leg" and the "Tube, bus or
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
          if (mode) set({ transitMode: mode.value });
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
        <TextInput
          label="From (optional)"
          placeholder="e.g. King's Cross St Pancras"
          value={draft.originName}
          maxLength={100}
          onChange={(event) => set({ originName: event.currentTarget.value })}
        />
        <TextInput
          label="To (optional)"
          placeholder="e.g. Victoria"
          value={draft.destinationName}
          maxLength={100}
          onChange={(event) => set({ destinationName: event.currentTarget.value })}
        />
      </Group>
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
