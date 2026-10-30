'use client';

import { Stack, Text, type ComboboxProps } from '@mantine/core';
import { AdvancedOptionsDisclosure, STOP_LIST_FIELDS } from './AdvancedOptionsDisclosure';
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
 * look at, which depends on the board. */
export function TrainsAdvancedOptions({
  filters,
  onChange,
  names,
  onNames,
  errors,
  opened,
  onOpenedChange,
  scope,
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
          {scope}
        </Text>
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
