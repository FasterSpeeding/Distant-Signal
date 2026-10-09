'use client';

import { useEffect, useId, useMemo, useState } from 'react';
import { londonToday, nowInLondon } from '@/lib/londonWallClock';
import { Autocomplete, Button, Group, SegmentedControl, Stack, Text, ActionIcon, VisuallyHidden } from '@mantine/core';
import { DateInput, TimeInput } from '@mantine/dates';
import { getStationNames, searchPlannerLocations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { groupLabels, useStationGroups, withGroupSuggestions } from '@/lib/stationGroups';
import { isGroupCode, normalizeLocationCode } from '@/lib/stationLabel';
import type { Suggestion, TrainSearchDates } from '@/lib/types';
import { searchDateBounds } from '@/lib/searchDates';
import type { TripPlanAdvancedOptions, TripPlanQuery } from '@/lib/tripPlan';
import type { PlanFormInitial } from '@/lib/tripPlanUrl';
import { PlanTripAdvancedOptions, advancedOptionErrors } from './PlanTripAdvancedOptions';

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

/** One "Call at" stop: a station, bus stop or ferry terminal, or a station
 * group (2026-10-07: "Any of the London Terminals (18 stations)", sent as
 * `group:LON`, a stop at ANY member), picked from suggestions or typed as a
 * code. */
function WaypointField({
  value,
  onChange,
  search,
  first,
  groupLabel,
}: {
  value: string;
  onChange: (value: string) => void;
  search: (q: string, signal: AbortSignal) => Promise<Suggestion[]>;
  first: boolean;
  /** The label of the group `value` names, if it is one (the field shows
   * the code). */
  groupLabel: string | undefined;
}) {
  const { suggestions, loading } = useSuggestions(value, search);
  const help = first
    ? 'The train must stop at each of these. A group such as “Any of the London Terminals” means a stop at any one of its stations. To go through a station without stopping, use “Pass through” in Advanced options.'
    : null;
  const shown = isGroupCode(value) && groupLabel !== undefined ? groupLabel : null;
  const parts = [help, shown].filter((part): part is string => part !== null);
  return (
    <Autocomplete
      style={{ flex: 1 }}
      label={first ? 'Call at (optional, in order)' : undefined}
      description={parts.length > 0 ? parts.join(' ') : undefined}
      placeholder="Station name or code"
      value={value}
      onChange={onChange}
      {...suggestionAutocompleteProps(suggestions, {
        query: value,
        loading,
        noMatchMessage: 'No matching stations',
      })}
    />
  );
}

/** "Call at" also offers the station groups (`GET /Trips/station-groups`).
 * One function for the module's life: `useSuggestions` refetches whenever
 * its search changes. */
const searchWaypoints = withGroupSuggestions(searchPlannerLocations);

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
  initialOriginCrs = '',
  initial = {},
  searchDates,
}: {
  onSubmit: (query: TripPlanQuery) => void;
  searching?: boolean;
  /** The From field's starting value; only read on mount. */
  initialOriginCrs?: string | undefined;
  /** A restored search (`/plan`'s own query string, `lib/tripPlanUrl.ts`);
   * only read on mount. Its `originCrs` wins over `initialOriginCrs`. */
  initial?: PlanFormInitial | undefined;
  /** `GET /public/trains/search/dates`, read by the page (`/plan` or
   * `/journeys/new`) on the server. Its `to` is the date picker's last day,
   * as on `/trains`; `null` (the read failed) falls back to a week ahead
   * (`lib/searchDates.ts`). Absent (a caller that doesn't read the range),
   * the picker has no upper bound. */
  searchDates?: TrainSearchDates | null | undefined;
}) {
  const maxDate = searchDates === undefined ? undefined : searchDateBounds(searchDates, londonToday()).maxDate;
  const [originCrs, setOriginCrs] = useState(initial.originCrs ?? initialOriginCrs);
  const [destinationCrs, setDestinationCrs] = useState(initial.destinationCrs ?? '');
  const [waypoints, setWaypoints] = useState<string[]>(initial.waypointCrs ?? []);
  const [advanced, setAdvanced] = useState<TripPlanAdvancedOptions>(() => ({
    viaCrs: initial.viaCrs,
    avoidCrs: initial.avoidCrs,
    avoidStopCrs: initial.avoidStopCrs,
    avoidChangeCrs: initial.avoidChangeCrs,
    maxChanges: initial.maxChanges,
  }));
  const [advancedOpened, setAdvancedOpened] = useState(false);
  // Code -> name for the advanced options' rows: filled as stations are
  // picked, and once on mount for codes restored from the URL.
  const [names, setNames] = useState<Map<string, string>>(new Map());
  const advancedErrorId = useId();
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
  // A restored date in the past is dropped: the picker can't show it
  // (`minDate`), and a shared link from last week means "this trip", today.
  // So is one past `maxDate`, which the picker can't show either.
  const [date, setDate] = useState<string | null>(() =>
    initial.date && initial.date >= londonToday() && (maxDate === undefined || initial.date <= maxDate)
      ? initial.date
      : nowInLondon().format('YYYY-MM-DD'),
  );
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
  // before anyone has touched it. Europe/London wall clock via
  // `nowInLondon()`, like `date` above and `TrackTrainForm.tsx` (FE-4):
  // `GET /Trips/plan` reads both fields as London wall-clock values, so a
  // browser-zone default would search hours away (or on the wrong rail
  // day) for any visitor whose device isn't on UK time.
  const [departAfter, setDepartAfter] = useState(() => initial.departAfter ?? nowInLondon().format('HH:mm'));
  const [results, setResults] = useState<'fastest' | 'options'>(initial.results ?? 'fastest');
  const resultsLabelId = useId();

  // The two ends may also be a bus stop or ferry terminal (a `tiploc:` code,
  // shown as "Keswick (bus)"): `searchPlannerLocations`.
  const { suggestions: originSuggestions, loading: originSuggestionsLoading } = useSuggestions(
    originCrs,
    searchPlannerLocations,
  );
  const { suggestions: destinationSuggestions, loading: destinationSuggestionsLoading } = useSuggestions(
    destinationCrs,
    searchPlannerLocations,
  );

  // Names for the advanced options restored from the URL (read once, like
  // every other `initial` field).
  useEffect(() => {
    const restored = [
      ...(initial.viaCrs ?? []),
      ...(initial.avoidCrs ?? []),
      ...(initial.avoidStopCrs ?? []),
      ...(initial.avoidChangeCrs ?? []),
    ].filter((code) => !isGroupCode(code)); // a group is labelled from `groups`
    if (restored.length === 0) return;
    let cancelled = false;
    void getStationNames(restored).then((found) => {
      if (!cancelled) addNames(found);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `initial` is read on mount only
  }, []);

  function addNames(found: Map<string, string>) {
    setNames((current) => new Map([...current, ...found]));
  }

  // With the groups' labels, for the advanced options' rows and summary
  // (loaded once a group is listed, e.g. restored from the URL).
  const groups = useStationGroups([...waypoints, ...(advanced.viaCrs ?? [])].some(isGroupCode));
  const labels = useMemo(() => new Map([...groupLabels(groups), ...names]), [groups, names]);

  const errors = advancedOptionErrors(advanced, {
    originCrs,
    destinationCrs,
    waypointCrs: waypoints,
    names: labels,
  });
  const firstError = Object.values(errors)[0];

  function addWaypoint() {
    setWaypoints((current) => [...current, '']);
  }

  function updateWaypoint(index: number, value: string) {
    setWaypoints((current) => current.map((existing, i) => (i === index ? value : existing)));
  }

  function removeWaypoint(index: number) {
    setWaypoints((current) => current.filter((_, i) => i !== index));
  }

  function handleSubmit() {
    if (!originCrs.trim() || !destinationCrs.trim() || !date) return;
    if (firstError) {
      setAdvancedOpened(true);
      return;
    }
    onSubmit({
      originCrs: originCrs.trim(),
      destinationCrs: destinationCrs.trim(),
      waypointCrs: waypoints,
      date,
      departAfter: departAfter || undefined,
      results,
      ...(advanced.viaCrs?.length ? { viaCrs: advanced.viaCrs } : {}),
      ...(advanced.avoidCrs?.length ? { avoidCrs: advanced.avoidCrs } : {}),
      ...(advanced.avoidStopCrs?.length ? { avoidStopCrs: advanced.avoidStopCrs } : {}),
      ...(advanced.avoidChangeCrs?.length ? { avoidChangeCrs: advanced.avoidChangeCrs } : {}),
      ...(advanced.maxChanges !== undefined ? { maxChanges: advanced.maxChanges } : {}),
    });
  }

  const canSubmit = originCrs.trim().length > 0 && destinationCrs.trim().length > 0 && date !== null;

  return (
    <Stack gap="md">
      <Autocomplete
        label="From"
        placeholder="Station name or code"
        value={originCrs}
        onChange={setOriginCrs}
        // Mirrors `TrackTrainForm.tsx`'s own Origin `Autocomplete`: the
        // shared `data`/`filter`/`renderOption` trio every CRS/TOC-code
        // `Autocomplete` field in this app needs (see
        // `lib/suggestionAutocomplete.ts`'s own doc comment for why this
        // is factored out rather than hand-rolled per field -- this field
        // used to carry its own copy of the same three pieces, one of the
        // "five call sites' worth of hand-copied, independently drifting
        // boilerplate" that helper was meant to end).
        {...suggestionAutocompleteProps(originSuggestions, {
          query: originCrs,
          loading: originSuggestionsLoading,
          noMatchMessage: 'No matching stations',
        })}
      />
      <Autocomplete
        label="To"
        placeholder="Station name or code"
        value={destinationCrs}
        onChange={setDestinationCrs}
        {...suggestionAutocompleteProps(destinationSuggestions, {
          query: destinationCrs,
          loading: destinationSuggestionsLoading,
          noMatchMessage: 'No matching stations',
        })}
      />
      {waypoints.map((waypoint, index) => (
        <Group key={index} gap="xs">
          <WaypointField
            value={waypoint}
            onChange={(value) => updateWaypoint(index, value)}
            search={searchWaypoints}
            first={index === 0}
            groupLabel={labels.get(normalizeLocationCode(waypoint))}
          />
          <ActionIcon
            color="red"
            variant="subtle"
            mt={index === 0 ? 24 : 0}
            onClick={() => removeWaypoint(index)}
            aria-label="Remove this waypoint"
          >
            <XIcon />
          </ActionIcon>
        </Group>
      ))}
      <Button variant="subtle" leftSection={<PlusIcon />} onClick={addWaypoint} style={{ alignSelf: 'flex-start' }}>
        Add a stop to call at
      </Button>
      {/* London's today, not `new Date()` (the browser's): a visitor ahead
          of UK time near midnight could otherwise not pick London's today,
          the very day `date` defaults to. */}
      <DateInput
        label="Date"
        value={date}
        onChange={setDate}
        minDate={londonToday()}
        {...(maxDate !== undefined && { maxDate })}
      />
      <TimeInput
        label="Depart after (optional)"
        value={departAfter}
        onChange={(event) => setDepartAfter(event.currentTarget.value)}
      />
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
        onChange={(value) => setResults(value)}
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
      <PlanTripAdvancedOptions
        options={advanced}
        onChange={setAdvanced}
        names={labels}
        onNames={addNames}
        errors={errors}
        opened={advancedOpened}
        onOpenedChange={setAdvancedOpened}
        results={results}
      />
      {/* The advanced options' first problem, repeated next to the button it
          blocks (and named by it via `aria-describedby`): the section may be
          collapsed, e.g. after From was changed to a station already listed
          as a via. The field itself carries the same message. */}
      {firstError && (
        <Text id={advancedErrorId} size="sm" c="var(--ds-color-error-text)">
          Check Advanced options: {firstError}
        </Text>
      )}
      <Button
        // Not disabled by an advanced-option problem: a click opens the
        // section on it, where a disabled button would just go quiet.
        disabled={!canSubmit || searching}
        onClick={handleSubmit}
        aria-describedby={firstError ? advancedErrorId : undefined}
      >
        {searching ? 'Searching…' : 'Find routes'}
      </Button>
    </Stack>
  );
}
