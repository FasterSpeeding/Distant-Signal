'use client';

import { NativeSelect, Stack } from '@mantine/core';
import { AdvancedOptionsDisclosure, STOP_LIST_FIELDS } from './AdvancedOptionsDisclosure';
import { StationListPicker } from './StationListPicker';
import { searchPlannerLocations } from '@/lib/suggestions';
import { withGroupSuggestions } from '@/lib/stationGroups';
import { codeStationLabel, normalizeLocationCode } from '@/lib/stationLabel';
import {
  DEFAULT_MAX_CHANGES,
  MAX_AVOIDED,
  MAX_CHANGES_LIMIT,
  MAX_VIAS,
  countAdvancedOptions,
  type TripPlanAdvancedOptions,
} from '@/lib/tripPlan';

export type AdvancedOptionErrors = Partial<Record<'viaCrs' | 'avoidCrs' | 'avoidStopCrs' | 'avoidChangeCrs', string>>;

type ListField = keyof AdvancedOptionErrors;

/** The planner's client-side checks on the advanced options, mirroring the
 * API's own 400s (`check_avoid_conflicts`, `check_via_conflicts` in
 * `crates/api/src/routes/trips.rs`) so the visitor sees them on the field,
 * before searching. */
export function advancedOptionErrors(
  options: TripPlanAdvancedOptions,
  {
    originCrs,
    destinationCrs,
    waypointCrs,
    names,
  }: { originCrs: string; destinationCrs: string; waypointCrs: string[]; names: Map<string, string> },
): AdvancedOptionErrors {
  const origin = normalizeLocationCode(originCrs);
  const destination = normalizeLocationCode(destinationCrs);
  const waypoints = waypointCrs.map(normalizeLocationCode);
  const label = (code: string) => codeStationLabel(code, names.get(code));
  const errors: AdvancedOptionErrors = {};

  const lists: [ListField, string[] | undefined][] = [
    ['avoidCrs', options.avoidCrs],
    ['avoidStopCrs', options.avoidStopCrs],
    ['avoidChangeCrs', options.avoidChangeCrs],
  ];
  for (const [field, codes] of lists) {
    for (const code of codes ?? []) {
      const role =
        code === origin
          ? 'where you start'
          : code === destination
            ? 'where you finish'
            : waypoints.includes(code)
              ? 'a stop you call at'
              : null;
      if (role) {
        errors[field] = `${label(code)} is ${role}, so it can't be avoided.`;
        break;
      }
    }
  }

  const vias = options.viaCrs ?? [];
  for (const [index, via] of vias.entries()) {
    if (via === origin || via === destination) {
      errors.viaCrs = `${label(via)} is ${via === origin ? 'where you start' : 'where you finish'}: every journey passes it already.`;
      break;
    }
    if ((options.avoidCrs ?? []).includes(via)) {
      errors.viaCrs = `${label(via)} is also in "Avoid completely": a journey can't both pass through it and avoid it.`;
      break;
    }
    if (index > 0 && vias[index - 1] === via) {
      errors.viaCrs = `${label(via)} is listed twice in a row: list it once.`;
      break;
    }
  }
  return errors;
}

/** One line saying which advanced options are set, for the collapsed
 * section: e.g. "Pass through STA · Max 1 change". */
export function advancedOptionsSummary(options: TripPlanAdvancedOptions, names = new Map<string, string>()): string {
  const parts: string[] = [];
  const list = (codes: string[] | undefined) => (codes ?? []).map((code) => names.get(code) ?? code).join(', ');
  if (options.viaCrs?.length) parts.push(`Pass through ${list(options.viaCrs)}`);
  if (options.avoidCrs?.length) parts.push(`Avoid ${list(options.avoidCrs)}`);
  if (options.avoidStopCrs?.length) parts.push(`Don't stop at ${list(options.avoidStopCrs)}`);
  if (options.avoidChangeCrs?.length) parts.push(`Don't change at ${list(options.avoidChangeCrs)}`);
  if (options.maxChanges !== undefined) {
    parts.push(`Max ${options.maxChanges} ${options.maxChanges === 1 ? 'change' : 'changes'}`);
  }
  return parts.join(' · ');
}

/** "Pass through" also offers the station groups
 * (`GET /Trips/station-groups`), e.g. "Any of the London Terminals (18
 * stations)", sent as `group:LON`. One function for the module's life:
 * `useSuggestions` refetches whenever its search changes. */
const searchVias = withGroupSuggestions(searchPlannerLocations);

const MAX_CHANGES_OPTIONS = [
  { value: '', label: `Default (${DEFAULT_MAX_CHANGES})` },
  ...Array.from({ length: MAX_CHANGES_LIMIT + 1 }, (_, n) => ({
    value: String(n),
    label: n === 0 ? '0 (direct trains only)' : String(n),
  })),
];

/** The planner form's "Advanced options": pass-through vias, the three
 * avoid lists and the change cap (`GET /Trips/plan`'s `via`, `avoid`,
 * `avoidStop`, `avoidChange`, `maxChanges`; docs/api-changelog.md,
 * 2026-09-29 and 2026-10-06).
 *
 * Collapsed by default (`AdvancedOptionsDisclosure`, shared with the
 * `/trains` boards' `TrainsAdvancedOptions`, as are the three station
 * lists' wording, `STOP_LIST_FIELDS`). The parent controls whether it is
 * open, so it can open it on an error inside.
 *
 * Every list, vias included, takes bus stops and ferry terminals (`tiploc:`
 * codes) as well as stations, like From and To: the API normalizes `via`
 * with the same location codes as the avoid lists. */
export function PlanTripAdvancedOptions({
  options,
  onChange,
  names,
  onNames,
  errors,
  opened,
  onOpenedChange,
  results,
}: {
  options: TripPlanAdvancedOptions;
  onChange: (options: TripPlanAdvancedOptions) => void;
  names: Map<string, string>;
  onNames: (names: Map<string, string>) => void;
  errors: AdvancedOptionErrors;
  opened: boolean;
  onOpenedChange: (opened: boolean) => void;
  results: 'fastest' | 'options';
}) {
  return (
    <AdvancedOptionsDisclosure
      count={countAdvancedOptions(options)}
      problems={Object.keys(errors).length}
      summary={advancedOptionsSummary(options, names)}
      opened={opened}
      onOpenedChange={onOpenedChange}
    >
      <Stack gap="lg">
        <StationListPicker
          {...STOP_LIST_FIELDS.passThrough}
          values={options.viaCrs ?? []}
          onChange={(viaCrs) => onChange({ ...options, viaCrs })}
          names={names}
          onNames={onNames}
          search={searchVias}
          max={MAX_VIAS}
          ordered
          allowStops
          allowGroups
          error={errors.viaCrs}
        />
        <StationListPicker
          {...STOP_LIST_FIELDS.avoid}
          values={options.avoidCrs ?? []}
          onChange={(avoidCrs) => onChange({ ...options, avoidCrs })}
          names={names}
          onNames={onNames}
          search={searchPlannerLocations}
          max={MAX_AVOIDED}
          allowStops
          error={errors.avoidCrs}
        />
        <StationListPicker
          {...STOP_LIST_FIELDS.avoidStop}
          values={options.avoidStopCrs ?? []}
          onChange={(avoidStopCrs) => onChange({ ...options, avoidStopCrs })}
          names={names}
          onNames={onNames}
          search={searchPlannerLocations}
          max={MAX_AVOIDED}
          allowStops
          error={errors.avoidStopCrs}
        />
        <StationListPicker
          label="Don't change at"
          description="Never get on, get off or change here; staying on a train that stops there is fine."
          values={options.avoidChangeCrs ?? []}
          onChange={(avoidChangeCrs) => onChange({ ...options, avoidChangeCrs })}
          names={names}
          onNames={onNames}
          search={searchPlannerLocations}
          max={MAX_AVOIDED}
          allowStops
          error={errors.avoidChangeCrs}
          itemNoun="no-change station"
        />
        <NativeSelect
          label="Most changes"
          description={
            results === 'options'
              ? 'Compare options never shows a route with more changes than this.'
              : 'Fastest always shows the fastest route, and says if it needs more changes than this. Compare options never exceeds it.'
          }
          data={MAX_CHANGES_OPTIONS}
          value={options.maxChanges === undefined ? '' : String(options.maxChanges)}
          onChange={(event) => {
            const value = event.currentTarget.value;
            onChange({ ...options, maxChanges: value === '' ? undefined : Number(value) });
          }}
        />
      </Stack>
    </AdvancedOptionsDisclosure>
  );
}
