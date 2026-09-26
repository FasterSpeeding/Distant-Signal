'use client';

import { useId, useState } from 'react';
import dayjs from 'dayjs';
import {
  Autocomplete,
  Button,
  Group,
  SegmentedControl,
  Stack,
  Text,
  TextInput,
  ActionIcon,
  VisuallyHidden,
} from '@mantine/core';
import { DateInput, TimeInput } from '@mantine/dates';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { withNoMatchPlaceholder, noMatchOptionContent } from '@/lib/autocompleteNoMatch';
import type { TripPlanQuery } from '@/lib/tripPlan';

/** `@tabler/icons-react` isn't a project dependency (checked package.json,
 * matching `components/InfoIcon.tsx`/`KebabIcon.tsx`'s own reasoning) --
 * inline SVG instead, same 16px stroke-icon convention `InfoIcon.tsx`
 * already uses. Decorative on purpose: both live inside a labelled
 * `Button`/`ActionIcon`, which already carries the accessible name. */
function PlusIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

function XIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

/** The input side of "Plan a route for me" (design spec §5.3) -- collects
 * origin, destination, ordered optional waypoints, a date, an optional
 * earliest-departure time, and a fastest/options preference, then hands a
 * ready-to-fetch [`TripPlanQuery`] to its caller (`PlanTripFlow`, Task 5).
 * Mirrors `TrackTrainForm.tsx`'s own station-autocomplete and
 * `SegmentedControl` conventions rather than reinventing them -- see this
 * task's own Step 1.
 *
 * `searching` (final-review fix I2, later closed by the whole-branch
 * review's own deferred double-submit-guard finding): `GET /Trips/plan`
 * can take several seconds (a full day of schedule connections, run
 * through pathfinding), so the submit button needs real in-flight
 * feedback -- `PlanTripFlow` (the only caller) passes its own `searching`
 * state through, and this component reflects it in both the button's
 * label AND, now, its `disabled` state. `PlanTripFlow`'s own monotonic
 * request-id guard already made a second, overlapping search harmless
 * data-correctness-wise (a stale response can never clobber a newer one),
 * but left the button clickable while a search was in flight, so rapid
 * re-clicking still fired one real `GET /Trips/plan` per click -- wasted
 * backend pathfinding work, not a correctness bug, but real work all the
 * same. Disabling on `searching` too closes that gap the same way
 * `TrainSearchForm.tsx`'s own `canSearch` (gated on `!searching`) and
 * `JourneyLegCandidates.tsx`'s per-row `disabled={picking !== null}`
 * button already do for their own in-flight actions: a second click while
 * one is outstanding is a genuine no-op, not a state update the
 * request-id guard just discards after the backend has already done the
 * work. */
export function PlanTripForm({
  onSubmit,
  searching = false,
}: {
  onSubmit: (query: TripPlanQuery) => void;
  searching?: boolean;
}) {
  const [originCrs, setOriginCrs] = useState('');
  const [destinationCrs, setDestinationCrs] = useState('');
  const [waypoints, setWaypoints] = useState<string[]>([]);
  // `string | null` ("YYYY-MM-DD"), not `Date | null`: `@mantine/dates`
  // 9.5.2's `DateInput`/`DatePickerInput` both take/emit
  // `DateStringValue` (a plain `"YYYY-MM-DD"` string), not a `Date` --
  // confirmed by `tsc` rejecting a `Date`-typed `onChange` handler outright,
  // and matching every other date field in this codebase (e.g.
  // `TrackTrainForm.tsx`'s own `windowServiceDate`). This also happens to
  // be exactly `TripPlanQuery.date`'s own shape, so `handleSubmit` below
  // needs no `.toISOString()` conversion (which, since that reads UTC,
  // could roll to the wrong calendar day near midnight in a non-UTC
  // timezone anyway).
  const [date, setDate] = useState<string | null>(() => dayjs().format('YYYY-MM-DD'));
  // Defaults to "now" (`'HH:MM'`, the same value contract `TimeFilterInput`'s
  // own `onChange` and every other `TimeInput` field in this codebase
  // already use), not `''` -- computed once via lazy `useState` initializer,
  // same pattern as `date` immediately above and as `TrackTrainForm.tsx`'s
  // own `scheduledDeparture`/`windowServiceDate` defaults. Before this fix,
  // an empty `departAfter` was omitted from the query entirely
  // (`buildTripPlanQuery`'s `if (query.departAfter)` guard), which
  // `GET /Trips/plan` (`crates/api/src/routes/trips.rs`) then defaults
  // server-side to `NaiveTime::MIN` -- so a visitor who opened the planner
  // and immediately hit "Find routes" without touching this field silently
  // got an itinerary search from midnight, not from now, on every visit.
  // Still genuinely optional: the field stays a plain, uncontrolled-feeling
  // `TimeInput` a visitor can clear (or retype) to search from any other
  // time, or from the start of the day -- this only changes what it shows
  // before anyone has touched it. Deliberately browser-local time via
  // `dayjs()`, matching `date` above and every other "now" default in this
  // app (`TrackTrainForm.tsx`) -- this is an ordinary "what time do you want
  // to depart" form field a visitor fills in for themselves, not a rail-day
  // boundary computation, so there is no reason to force it to Europe/London
  // specifically the way rail-day-boundary logic elsewhere in this codebase
  // does.
  const [departAfter, setDepartAfter] = useState(() => dayjs().format('HH:mm'));
  const [results, setResults] = useState<'fastest' | 'options'>('fastest');
  const resultsLabelId = useId();

  const { suggestions: originSuggestions, loading: originSuggestionsLoading } = useSuggestions(
    originCrs,
    searchStations,
  );
  const { suggestions: destinationSuggestions, loading: destinationSuggestionsLoading } = useSuggestions(
    destinationCrs,
    searchStations,
  );

  function addWaypoint() {
    setWaypoints(current => [...current, '']);
  }

  function updateWaypoint(index: number, value: string) {
    setWaypoints(current => current.map((existing, i) => (i === index ? value : existing)));
  }

  function removeWaypoint(index: number) {
    setWaypoints(current => current.filter((_, i) => i !== index));
  }

  function handleSubmit() {
    if (!originCrs.trim() || !destinationCrs.trim() || !date) return;
    onSubmit({
      originCrs: originCrs.trim(),
      destinationCrs: destinationCrs.trim(),
      waypointCrs: waypoints,
      date,
      departAfter: departAfter || undefined,
      results,
    });
  }

  const canSubmit = originCrs.trim().length > 0 && destinationCrs.trim().length > 0 && date !== null;

  return (
    <Stack gap="md">
      <Autocomplete
        label="From"
        placeholder="Station name or CRS code"
        value={originCrs}
        onChange={setOriginCrs}
        // See this task's Step 1 -- mirrors `TrackTrainForm.tsx`'s own
        // Origin `Autocomplete` (debounced `searchStations` via
        // `useSuggestions`, plus `withNoMatchPlaceholder`/
        // `noMatchOptionContent` for the "no matches" option -- see
        // `lib/autocompleteNoMatch.ts` for why `Autocomplete` needs that
        // workaround instead of a `nothingFoundMessage` prop).
        data={withNoMatchPlaceholder(
          originSuggestions.map((s) => ({ value: s.code, label: s.code })),
          'No matching stations',
          { active: originCrs.trim().length > 0 && !originSuggestionsLoading },
        )}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const placeholder = noMatchOptionContent(option.value, 'No matching stations');
          if (placeholder) return placeholder;
          const match = originSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
      />
      <Autocomplete
        label="To"
        placeholder="Station name or CRS code"
        value={destinationCrs}
        onChange={setDestinationCrs}
        data={withNoMatchPlaceholder(
          destinationSuggestions.map((s) => ({ value: s.code, label: s.code })),
          'No matching stations',
          { active: destinationCrs.trim().length > 0 && !destinationSuggestionsLoading },
        )}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const placeholder = noMatchOptionContent(option.value, 'No matching stations');
          if (placeholder) return placeholder;
          const match = destinationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
      />
      {waypoints.map((waypoint, index) => (
        <Group key={index} gap="xs">
          <TextInput
            style={{ flex: 1 }}
            label={index === 0 ? 'Via (optional, in order)' : undefined}
            placeholder="Station name or CRS code"
            value={waypoint}
            onChange={event => updateWaypoint(index, event.currentTarget.value)}
          />
          <ActionIcon color="red" variant="subtle" mt={index === 0 ? 24 : 0} onClick={() => removeWaypoint(index)} aria-label="Remove this waypoint">
            <XIcon />
          </ActionIcon>
        </Group>
      ))}
      <Button variant="subtle" leftSection={<PlusIcon />} onClick={addWaypoint} style={{ alignSelf: 'flex-start' }}>
        Add a waypoint
      </Button>
      <DateInput label="Date" value={date} onChange={setDate} minDate={new Date()} />
      <TimeInput label="Depart after (optional)" value={departAfter} onChange={event => setDepartAfter(event.currentTarget.value)} />
      {/* I3: mirrors `TrackTrainForm.tsx`'s own `modeLabelId` +
          `aria-labelledby` fix for its mode toggle (2026-09-22 UX review) --
          without a visible `Text` label wired as the name, a screen reader
          announced this as an unnamed "radiogroup". */}
      <Text id={resultsLabelId} size="xs" fw={600} c="dimmed">
        Results
      </Text>
      <SegmentedControl
        aria-labelledby={resultsLabelId}
        value={results}
        onChange={value => setResults(value as 'fastest' | 'options')}
        data={[
          { label: 'Fastest', value: 'fastest' },
          { label: 'Compare options', value: 'options' },
        ]}
      />
      {/* Same `VisuallyHidden`/`aria-live="polite"` swap announcement as
          `TrackTrainForm.tsx`'s own mode toggle -- this preference doesn't
          change anything visible until the NEXT search, but a screen
          reader user should still hear that the choice registered. */}
      <VisuallyHidden role="status" aria-live="polite">
        {results === 'options' ? 'Will compare route options.' : 'Will show the fastest route only.'}
      </VisuallyHidden>
      <Button disabled={!canSubmit || searching} onClick={handleSubmit}>
        {searching ? 'Searching…' : 'Find routes'}
      </Button>
    </Stack>
  );
}
