'use client';

import { useId, useMemo } from 'react';
import { Button, Group, MultiSelect, NativeSelect, SegmentedControl, Stack, Text } from '@mantine/core';
import {
  MAX_OPERATORS,
  UNDERGROUND,
  operatorChoices,
  operatorFilterSummary,
  operatorSearchFilter,
  type OperatorMode,
} from '@/lib/plannerOperators';
import type { PlannerOperatorCatalogue } from '@/lib/types';
import { noOptionsFound } from './noOptionsFound';
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
 * section: e.g. "Pass through STA · Max 1 change · Only Avanti West Coast".
 * `operatorName` names an operator entry (`lib/plannerOperators.ts`). */
export function advancedOptionsSummary(
  options: TripPlanAdvancedOptions,
  names = new Map<string, string>(),
  operatorName: (entry: string) => string = (entry) => entry,
): string {
  const parts: string[] = [];
  const list = (codes: string[] | undefined) => (codes ?? []).map((code) => names.get(code) ?? code).join(', ');
  if (options.viaCrs?.length) parts.push(`Pass through ${list(options.viaCrs)}`);
  if (options.avoidCrs?.length) parts.push(`Avoid ${list(options.avoidCrs)}`);
  if (options.avoidStopCrs?.length) parts.push(`Don't stop at ${list(options.avoidStopCrs)}`);
  if (options.avoidChangeCrs?.length) parts.push(`Don't change at ${list(options.avoidChangeCrs)}`);
  if (options.maxChanges !== undefined) {
    parts.push(`Max ${options.maxChanges} ${options.maxChanges === 1 ? 'change' : 'changes'}`);
  }
  const operators = operatorFilterSummary(options.operatorMode, options.operators, operatorName);
  if (operators) parts.push(operators);
  return parts.join(' · ');
}

/** The operator field's mode choices: "Any" is no filter. */
function operatorModes(results: 'fastest' | 'options') {
  return [
    { label: 'Any', value: 'any' },
    { label: 'Only these', value: 'only' },
    { label: 'Avoid these', value: 'avoid' },
    // Prefer adds a labelled option to Compare options; Fastest has one
    // route only.
    { label: 'Prefer', value: 'prefer', disabled: results === 'fastest' },
  ];
}

/** What each mode's picker means (§4.2 of the operator filtering spec). */
const OPERATOR_DESCRIPTIONS: Record<OperatorMode, string> = {
  only: "Only routes on these operators' trains, buses, ferries, links and connecting rides. Walks between stations are always allowed. Add London Underground to cross London by Tube.",
  avoid: "Never use these operators' trains, buses, ferries, links or connecting rides.",
  prefer: 'Compare options adds the best route using only these operators, labelled.',
};

/** "Operators" (2026-10-31): Any, Only these or Avoid these, and the
 * operators and groups (`GET /Trips/operators`). The picks stay while the
 * mode changes; "Any" hides them and sends nothing. Without a catalogue
 * (the read failed) the field says it can't offer operators. */
export function OperatorFilterField({
  mode,
  operators,
  onChange,
  catalogue,
  operatorName,
  results = 'options',
}: {
  mode: OperatorMode | undefined;
  operators: string[];
  onChange: (mode: OperatorMode | undefined, operators: string[]) => void;
  catalogue: PlannerOperatorCatalogue | null | undefined;
  operatorName: (entry: string) => string;
  /** Prefer needs Compare options. */
  results?: 'fastest' | 'options';
}) {
  const labelId = useId();
  const descriptionId = useId();
  const choices = useMemo(() => (catalogue ? operatorChoices(catalogue) : []), [catalogue]);
  const filter = useMemo(() => operatorSearchFilter(choices), [choices]);
  // A restored pick the catalogue doesn't list (a stale URL) still shows,
  // under its code.
  const data = useMemo(() => {
    const listed = new Set(choices.flatMap((section) => section.items.map((item) => item.value)));
    const extra = operators.filter((entry) => !listed.has(entry));
    return [
      ...choices.map((section) => ({
        group: section.group,
        items: section.items.map(({ value, label }) => ({ value, label })),
      })),
      ...(extra.length > 0
        ? [{ group: 'Other', items: extra.map((entry) => ({ value: entry, label: operatorName(entry) })) }]
        : []),
    ];
  }, [choices, operators, operatorName]);
  return (
    <Stack gap={6} data-operator-field>
      <Text id={labelId} size="sm" fw={500}>
        Operators
      </Text>
      <SegmentedControl
        aria-labelledby={labelId}
        aria-describedby={mode ? descriptionId : undefined}
        value={mode ?? 'any'}
        onChange={(value) => onChange(value === 'any' ? undefined : (value as OperatorMode), operators)}
        data={operatorModes(results)}
      />
      {results === 'fastest' && (
        <Text size="xs" c="dimmed">
          Prefer needs Compare options.
        </Text>
      )}
      {mode && (
        <>
          <Text id={descriptionId} size="xs" c="dimmed">
            {OPERATOR_DESCRIPTIONS[mode]}
          </Text>
          {catalogue ? (
            <MultiSelect
              aria-labelledby={labelId}
              aria-describedby={descriptionId}
              placeholder={operators.length > 0 ? undefined : 'Search operators'}
              data={data}
              value={operators}
              onChange={(next) => onChange(mode, next)}
              filter={filter}
              maxValues={MAX_OPERATORS}
              searchable
              clearable
              hidePickedOptions
              nothingFoundMessage={noOptionsFound('No matching operators')}
              clearButtonProps={{ 'aria-label': 'Clear operator filter', className: 'iconHitArea24' }}
            />
          ) : (
            <Text size="sm" c="dimmed">
              Operators couldn&apos;t be loaded. Reload the page to choose them.
            </Text>
          )}
          {mode === 'only' && operators.length > 0 && !operators.includes(UNDERGROUND) && (
            <Group>
              <Button
                variant="subtle"
                size="compact-sm"
                onClick={() => onChange(mode, [...operators, UNDERGROUND])}
                disabled={operators.length >= MAX_OPERATORS}
              >
                Add London Underground
              </Button>
            </Group>
          )}
        </>
      )}
    </Stack>
  );
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
 * avoid lists, the operator filter and the change cap (`GET /Trips/plan`'s
 * `via`, `avoid`, `avoidStop`, `avoidChange`, `operator`/`avoidOperator`,
 * `maxChanges`; docs/api-changelog.md, 2026-09-29, 2026-10-06 and
 * 2026-10-31).
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
  operatorCatalogue,
  operatorName = (entry) => entry,
}: {
  options: TripPlanAdvancedOptions;
  onChange: (options: TripPlanAdvancedOptions) => void;
  names: Map<string, string>;
  onNames: (names: Map<string, string>) => void;
  errors: AdvancedOptionErrors;
  opened: boolean;
  onOpenedChange: (opened: boolean) => void;
  results: 'fastest' | 'options';
  /** `GET /Trips/operators`, read by the page; `null` when it failed. */
  operatorCatalogue?: PlannerOperatorCatalogue | null | undefined;
  /** Names an operator entry for the summary and the field. */
  operatorName?: ((entry: string) => string) | undefined;
}) {
  return (
    <AdvancedOptionsDisclosure
      count={countAdvancedOptions(options)}
      problems={Object.keys(errors).length}
      summary={advancedOptionsSummary(options, names, operatorName)}
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
        <OperatorFilterField
          mode={options.operatorMode}
          operators={options.operators ?? []}
          onChange={(operatorMode, operators) => onChange({ ...options, operatorMode, operators })}
          catalogue={operatorCatalogue}
          operatorName={operatorName}
          results={results}
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
