'use client';

import { Stack, Text, type ComboboxProps } from '@mantine/core';
import { AdvancedOptionsDisclosure, STOP_LIST_FIELDS } from './AdvancedOptionsDisclosure';
import { StationField } from './StationField';
import { StationListPicker } from './StationListPicker';
import { searchPlannerLocations } from '@/lib/suggestions';
import { withGroupSuggestions } from '@/lib/stationGroups';
import {
  MAX_AVOIDED,
  MAX_CALLS_AT,
  MAX_VIAS,
  countStopFilters,
  stopFiltersSummary,
  type StopFilterErrors,
  type TrainStopFilters,
} from '@/lib/trainStopFilters';

/** "Call at" and "Pass through" also offer the station groups, as the
 * planner's do. One function for the module's life: `useSuggestions`
 * refetches whenever its search changes. */
const searchWithGroups = withGroupSuggestions(searchPlannerLocations);

/** The `/trains` station and between-stations views' "Advanced options":
 * Call at, Pass through, Avoid completely and Don't stop at
 * (`calls_at`, `via`, `avoid`, `avoid_stop`; docs/api-changelog.md,
 * 2026-10-30). The planner's own lists, in the same disclosure and with
 * the same pickers, labels, help text and caps (`AdvancedOptionsDisclosure`,
 * `STOP_LIST_FIELDS`); "Call at", which the planner keeps on its main
 * form, opens the list here. `scope` says which part of the train they
 * look at, which depends on the board.
 *
 * First, under `earlierScope`, the options for finding one particular
 * train by where it has already been (DS-213; never in the planner):
 * Starts at (`origin`, one station, exactly where it starts), then
 * Previously calls at (`previously_calls_at`, where it starts counts) and
 * Previously stops at (`previously_stops_at`, it doesn't). They look at
 * the route before the station, the rest after it, so they get their own
 * line. The "after" group opens with Terminates at (`terminates_at`, one
 * station, exactly where it ends), Starts at's counterpart.
 *
 * On the arrivals board (`arrivals`) every list looks before the station:
 * there is one line, and "Previously calls at", which would mean just
 * what "Call at" does, is left out unless it holds stations (a link from
 * the departures board), so a filter in force is never hidden. Starts at
 * and Terminates at mean the same on every board, so they stay. */
export function TrainsAdvancedOptions({
  filters,
  onChange,
  names,
  onNames,
  errors,
  opened,
  onOpenedChange,
  scope,
  earlierScope,
  arrivals = false,
  comboboxProps,
}: {
  filters: TrainStopFilters;
  onChange: (filters: TrainStopFilters) => void;
  names: Map<string, string>;
  onNames: (names: Map<string, string>) => void;
  errors: StopFilterErrors;
  opened: boolean;
  onOpenedChange: (opened: boolean) => void;
  /** e.g. "These look at the train's route after Reading." */
  scope: string;
  /** e.g. "To find a particular train: where it has been before
   * Reading." Not shown on the arrivals board, where `scope` covers every
   * list. */
  earlierScope: string;
  /** The arrivals board: see above. */
  arrivals?: boolean | undefined;
  /** Each picker's `comboboxProps` (see `StationListPicker`). */
  comboboxProps?: ComboboxProps | undefined;
}) {
  return (
    <AdvancedOptionsDisclosure
      count={countStopFilters(filters)}
      problems={Object.keys(errors).length}
      summary={stopFiltersSummary(filters, names)}
      opened={opened}
      onOpenedChange={onOpenedChange}
    >
      <Stack gap="lg">
        <Text size="sm" c="dimmed">
          {arrivals ? scope : earlierScope}
        </Text>
        <StationField
          label="Starts at"
          placeholder="e.g. Paddington or PAD"
          description="Only trains that start their journey at this station."
          value={filters.startsAt[0] ?? ''}
          onChange={(value) => onChange({ ...filters, startsAt: value.trim() ? [value] : [] })}
          comboboxProps={comboboxProps}
          error={errors.startsAt}
        />
        {(!arrivals || filters.previouslyCallsAt.length > 0) && (
          <StationListPicker
            label="Previously calls at (in order)"
            description={
              arrivals
                ? 'On the arrivals board this is the same as "Call at": the train must stop at each of these, in this order, where it starts included.'
                : 'The train must already have stopped at each of these, in this order. Where it starts counts.'
            }
            values={filters.previouslyCallsAt}
            onChange={(previouslyCallsAt) => onChange({ ...filters, previouslyCallsAt })}
            names={names}
            onNames={onNames}
            search={searchWithGroups}
            max={MAX_CALLS_AT}
            ordered
            allowStops
            allowGroups
            comboboxProps={comboboxProps}
            error={errors.previouslyCallsAt}
            itemNoun="previous call"
          />
        )}
        <StationListPicker
          label="Previously stops at (in order)"
          description={
            arrivals
              ? 'The train must stop at each of these, in this order, after it started. Unlike "Call at", where it starts doesn\'t count.'
              : "The train must already have stopped at each of these, in this order, after it started. Where it starts doesn't count."
          }
          values={filters.previouslyStopsAt}
          onChange={(previouslyStopsAt) => onChange({ ...filters, previouslyStopsAt })}
          names={names}
          onNames={onNames}
          search={searchWithGroups}
          max={MAX_CALLS_AT}
          ordered
          allowStops
          allowGroups
          comboboxProps={comboboxProps}
          error={errors.previouslyStopsAt}
          itemNoun="previous stop"
        />
        {!arrivals && (
          <Text size="sm" c="dimmed">
            {scope}
          </Text>
        )}
        <StationField
          label="Terminates at"
          placeholder="e.g. Paddington or PAD"
          description="Only trains that end their journey at this station."
          value={filters.terminatesAt[0] ?? ''}
          onChange={(value) => onChange({ ...filters, terminatesAt: value.trim() ? [value] : [] })}
          comboboxProps={comboboxProps}
          error={errors.terminatesAt}
        />
        <StationListPicker
          label="Call at (in order)"
          description='The train must stop at each of these, in this order. A group such as "Any of the London Terminals" means a stop at any one of its stations.'
          values={filters.callsAt}
          onChange={(callsAt) => onChange({ ...filters, callsAt })}
          names={names}
          onNames={onNames}
          search={searchWithGroups}
          max={MAX_CALLS_AT}
          ordered
          allowStops
          allowGroups
          comboboxProps={comboboxProps}
          error={errors.callsAt}
          itemNoun="call-at station"
        />
        <StationListPicker
          {...STOP_LIST_FIELDS.passThrough}
          values={filters.via}
          onChange={(via) => onChange({ ...filters, via })}
          names={names}
          onNames={onNames}
          search={searchWithGroups}
          max={MAX_VIAS}
          ordered
          allowStops
          allowGroups
          comboboxProps={comboboxProps}
          error={errors.via}
        />
        <StationListPicker
          {...STOP_LIST_FIELDS.avoid}
          values={filters.avoid}
          onChange={(avoid) => onChange({ ...filters, avoid })}
          names={names}
          onNames={onNames}
          search={searchPlannerLocations}
          max={MAX_AVOIDED}
          allowStops
          comboboxProps={comboboxProps}
          error={errors.avoid}
        />
        <StationListPicker
          {...STOP_LIST_FIELDS.avoidStop}
          values={filters.avoidStop}
          onChange={(avoidStop) => onChange({ ...filters, avoidStop })}
          names={names}
          onNames={onNames}
          search={searchPlannerLocations}
          max={MAX_AVOIDED}
          allowStops
          comboboxProps={comboboxProps}
          error={errors.avoidStop}
        />
      </Stack>
    </AdvancedOptionsDisclosure>
  );
}
