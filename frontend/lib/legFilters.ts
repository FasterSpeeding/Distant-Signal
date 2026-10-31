import { operatorChoices } from './trainSearch';
import {
  NO_STOP_FILTERS,
  STOP_FILTER_PARAMS,
  countStopFilters,
  parseStopFilterParams,
  stopFilterEntries,
  stopFilterErrors,
  stopFiltersSummary,
  type StopFilterErrors,
  type TrainStopFilters,
} from './trainStopFilters';
import type { LegSearchFilters, TrainSearchOperator } from './types';

/** A journey window leg's search filters as its forms hold them (DS-215):
 * the Trains section's Between stations filters, the Operator field's
 * codes and the Advanced options' lists (`TrainStopFilters`). "Add a leg"
 * and a leg's "Edit search" save them with the leg's window, and its
 * candidates list only the trains they allow
 * (`GET /Journeys/{id}/legs/{legId}/candidates`). */
export interface LegFilters {
  /** ATOC codes, any of which. */
  operators: string[];
  stops: TrainStopFilters;
}

export const NO_LEG_FILTERS: LegFilters = { operators: [], stops: NO_STOP_FILTERS };

/** The api's `searchFilters` (or an edited search's own) as the form's
 * filters, read as tolerantly as a `/trains` URL is
 * (`parseStopFilterParams`). */
export function legFiltersFromApi(api: LegSearchFilters | null | undefined): LegFilters {
  if (!api) return NO_LEG_FILTERS;
  const params = new URLSearchParams();
  for (const [param, field] of STOP_FILTER_PARAMS) {
    const value = api[field];
    if (value) params.set(param, value);
  }
  const operators = (api.operator ?? '')
    .split(',')
    .map((code) => code.trim().toUpperCase())
    .filter((code) => code.length > 0);
  return { operators: [...new Set(operators)], stops: parseStopFilterParams(params) };
}

/** The form's filters as the api's `filters` (`LegSearchFilters`), each
 * list normalized as the `/trains` search sends it; `null` when none is
 * set. */
export function legFiltersToApi(filters: LegFilters): LegSearchFilters | null {
  const api: LegSearchFilters = {};
  if (filters.operators.length > 0) api.operator = filters.operators.join(',');
  const fieldOf = new Map<string, keyof TrainStopFilters>(STOP_FILTER_PARAMS);
  for (const [param, value] of stopFilterEntries(filters.stops)) {
    const field = fieldOf.get(param);
    if (field) api[field] = value;
  }
  return Object.keys(api).length > 0 ? api : null;
}

/** One line naming the set filters: "Operator SW · Call at Reading". */
export function legFiltersSummary(filters: LegFilters, names = new Map<string, string>()): string {
  const stops = stopFiltersSummary(filters.stops, names);
  const operators =
    filters.operators.length > 0
      ? `${filters.operators.length === 1 ? 'Operator' : 'Operators'} ${filters.operators.join(', ')}`
      : '';
  return [operators, stops].filter((part) => part.length > 0).join(' · ');
}

/** How many filters are set: the operators count as one, as on `/trains`. */
export function countLegFilters(filters: LegFilters): number {
  return (filters.operators.length > 0 ? 1 : 0) + countStopFilters(filters.stops);
}

/** The Advanced options' errors for a leg from `origin` to `destination`
 * (codes, or `null` while not yet stations): the Between stations view's
 * own checks (`stopFilterErrors`), the leg's stations being where the
 * traveller gets on and off. */
export function legFilterErrors(
  filters: LegFilters,
  {
    origin,
    destination,
    names = new Map<string, string>(),
  }: { origin: string | null; destination: string | null; names?: Map<string, string> },
): StopFilterErrors {
  return stopFilterErrors(filters.stops, {
    stations: [
      [origin ?? '', 'where you get on'],
      [destination ?? '', 'where you get off'],
    ],
    names,
    between: true,
  });
}

/** `GET /public/trains/search/operators`' list as the Operator field's
 * options, the way the `/trains` page builds them: by name, then the
 * codes the timetable carries with no name, by code. */
export function operatorChoicesFromList(operators: readonly TrainSearchOperator[]): { value: string; label: string }[] {
  const names: Record<string, string> = {};
  const unnamed: string[] = [];
  for (const { code, name } of operators) {
    if (name === null) {
      unnamed.push(code);
    } else {
      names[code] = name;
    }
  }
  return operatorChoices(names, unnamed);
}
