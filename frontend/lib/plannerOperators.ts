import type { ComboboxItem, ComboboxParsedItem, OptionsFilter } from '@mantine/core';
import type { PlannerOperatorCatalogue, TripPlanResponse } from './types';

/** The planner's operator filters (`GET /Trips/plan`'s `operator` and
 * `avoidOperator`, docs/api-changelog.md 2026-10-31): their modes, the
 * picker's choices, and how the page words a filter. The design is
 * docs/superpowers/specs/2026-10-31-planner-operator-filtering-design.md
 * (§4, "On /plan"). An entry is an ATOC code (`VT`), a DS operator with no
 * ATOC code (`ds:DLR`) or a named group (`group:GTR`). */

/** London Underground: the Tube, every way it can be ridden (§5.1). */
export const UNDERGROUND = 'LT';

/** `GET /Trips/plan`'s cap on one operator list. */
export const MAX_OPERATORS = 8;

/** "Only these", "Avoid these" or "Prefer" (Compare options only: an
 * extra option on only these operators, labelled; spec §5.4). */
export type OperatorMode = 'only' | 'avoid' | 'prefer';

export const OPERATOR_GROUP_PREFIX = 'group:';
export const DS_OPERATOR_PREFIX = 'ds:';

const ATOC = /^[A-Za-z0-9]{2}$/;
const DS = /^ds:[A-Za-z_]{2,20}$/i;
const GROUP = /^group:[A-Za-z0-9]{2,12}$/i;

/** One entry as `GET /Trips/plan` takes it, normalized (`vt` -> `VT`,
 * `GROUP:gtr` -> `group:GTR`); `null` for anything else. */
export function operatorEntry(raw: string): string | null {
  const entry = raw.trim();
  if (ATOC.test(entry)) return entry.toUpperCase();
  if (DS.test(entry)) return `${DS_OPERATOR_PREFIX}${entry.slice(DS_OPERATOR_PREFIX.length).toUpperCase()}`;
  if (GROUP.test(entry)) return `${OPERATOR_GROUP_PREFIX}${entry.slice(OPERATOR_GROUP_PREFIX.length).toUpperCase()}`;
  return null;
}

/** A comma-separated list (a `/plan` URL's), normalized, deduplicated and
 * capped; malformed entries dropped. */
export function operatorList(raw: string | undefined): string[] {
  const entries: string[] = [];
  for (const part of (raw ?? '').split(',')) {
    const entry = operatorEntry(part);
    if (entry && !entries.includes(entry)) entries.push(entry);
  }
  return entries.slice(0, MAX_OPERATORS);
}

/** Names the picker shows instead of the code's own, where the code
 * covers more than its name says (§4.2). */
const LABEL_NAMES: Record<string, string> = {
  LM: 'LNR & WMR (West Midlands Railway and London Northwestern Railway)',
  LE: 'Greater Anglia (includes Stansted Express)',
  LT: 'London Underground (the Tube, and its trains on National Rail lines)',
};

/** Other words a visitor might type for a code (search text only, never a
 * value of its own, so nothing picked can match no train). */
const ALIASES: Record<string, string> = {
  LM: 'West Midlands Railway London Northwestern Railway LNR WMR',
  LE: 'Stansted Express',
  LT: 'Tube Underground',
  XR: 'Crossrail',
  TW: 'Metro',
  GR: 'London North Eastern Railway',
};

/** One picker option: `search` is what typing matches. */
export interface OperatorChoice extends ComboboxItem {
  search: string;
}

/** The picker's options: the groups first, then every operator running in
 * the published window by name ("VT · Avanti West Coast", an unnamed code
 * as itself), then the DS operators ("DLR"). */
export function operatorChoices(catalogue: PlannerOperatorCatalogue): { group: string; items: OperatorChoice[] }[] {
  const groups = catalogue.groups.map((group) => ({
    value: `${OPERATOR_GROUP_PREFIX}${group.code}`,
    label: group.name,
    search: `${group.code} ${group.name}`,
  }));
  const operators = catalogue.operators
    .map((operator) => {
      const name = LABEL_NAMES[operator.code] ?? operator.name;
      return {
        value: operator.code,
        label: name ? `${operator.code} · ${name}` : operator.code,
        search: `${operator.code} ${name ?? ''} ${ALIASES[operator.code] ?? ''}`,
        sortKey: name ?? operator.code,
        named: name !== null,
      };
    })
    .sort((a, b) => (a.named === b.named ? a.sortKey.localeCompare(b.sortKey) : a.named ? -1 : 1))
    .map(({ sortKey: _sortKey, named: _named, ...choice }) => choice);
  const other = catalogue.dsOperators.map((operator) => ({
    value: operator.code,
    label: operator.name,
    search: operator.name,
  }));
  return [
    { group: 'Groups', items: groups },
    { group: 'Operators', items: operators },
    { group: 'Trams and light rail', items: other },
  ].filter((section) => section.items.length > 0);
}

/** The picker's filter: every word typed must appear in an option's label
 * or search text (aliases included), case-insensitively. */
export function operatorSearchFilter(choices: { group: string; items: OperatorChoice[] }[]): OptionsFilter {
  const searchText = new Map<string, string>();
  for (const section of choices) {
    for (const item of section.items) searchText.set(item.value, `${item.label} ${item.search}`.toLowerCase());
  }
  return ({ options, search }) => {
    const words = search.toLowerCase().split(/\s+/).filter(Boolean);
    const matches = (item: ComboboxItem) => {
      const text = searchText.get(item.value) ?? item.label.toLowerCase();
      return words.every((word) => text.includes(word));
    };
    const result: ComboboxParsedItem[] = [];
    for (const option of options) {
      if ('group' in option) {
        const items = option.items.filter(matches);
        if (items.length > 0) result.push({ group: option.group, items });
      } else if (matches(option)) {
        result.push(option);
      }
    }
    return result;
  };
}

/** What a person calls an entry: a group's name, an operator's name (the
 * catalogue's, else its code), London Underground for `LT`. */
export function operatorName(entry: string, catalogue: PlannerOperatorCatalogue | null | undefined): string {
  if (entry === UNDERGROUND) return 'London Underground';
  if (entry.startsWith(OPERATOR_GROUP_PREFIX)) {
    const code = entry.slice(OPERATOR_GROUP_PREFIX.length);
    return catalogue?.groups.find((group) => group.code === code)?.name.replace(/ \(.*\)$/, '') ?? code;
  }
  if (entry.startsWith(DS_OPERATOR_PREFIX)) {
    return catalogue?.dsOperators.find((operator) => operator.code === entry)?.name ?? entry;
  }
  return catalogue?.operators.find((operator) => operator.code === entry)?.name ?? entry;
}

/** "Avanti West Coast", "Avanti West Coast and LNER", "A, B and C";
 * `or` joins with "or". */
export function joinNames(names: readonly string[], conjunction: 'and' | 'or' = 'and'): string {
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} ${conjunction} ${names[names.length - 1]}`;
}

/** A Compare options `operatorOnly` label: "LNER only", "GTR only",
 * "Avanti West Coast or LNER only". `entries` are the request's own (a
 * group named as a group), else the label's codes. */
export function operatorOnlyLabel(
  codes: readonly string[],
  nameOf: (entry: string) => string,
  entries?: readonly string[],
): string {
  return `${joinNames((entries?.length ? entries : codes).map(nameOf), 'or')} only`;
}

/** What a Prefer search's outcome says when it added nothing visible;
 * `null` when it added or labelled an option. */
export function preferOutcomeNote(outcome: string | undefined, names: string): string | null {
  switch (outcome) {
    case 'tooSlow':
      return `The best route using only ${names} is much slower than these, so it isn't shown.`;
    case 'none':
      return `No route uses only ${names} from this time.`;
    default:
      return null;
  }
}

/** The collapsed Advanced options' summary part: "Only Avanti West Coast,
 * LNER" or "Avoid GTR"; `null` without a filter. */
export function operatorFilterSummary(
  mode: OperatorMode | undefined,
  entries: readonly string[] | undefined,
  nameOf: (entry: string) => string,
): string | null {
  if (!mode || !entries?.length) return null;
  const word = mode === 'only' ? 'Only' : mode === 'avoid' ? 'Avoid' : 'Prefer';
  return `${word} ${entries.map(nameOf).join(', ')}`;
}

/** The line above filtered results, from what the plan applied (§4.4);
 * `null` when no filter applied. `entries` are the request's own (groups
 * named as groups), else the response's expanded codes. */
export function operatorFilterBanner(
  plan: Pick<TripPlanResponse, 'operator' | 'avoidOperator'>,
  nameOf: (entry: string) => string,
  entries?: readonly string[],
): string | null {
  const only = plan.operator ?? [];
  const avoid = plan.avoidOperator ?? [];
  if (only.length > 0) {
    const names = joinNames((entries?.length ? entries : only).map(nameOf));
    return only.includes(UNDERGROUND)
      ? `Showing routes using only ${names}.`
      : `Showing routes using only ${names}. Walks between stations are included; the Tube is not.`;
  }
  if (avoid.length > 0) {
    const names = joinNames((entries?.length ? entries : avoid).map(nameOf));
    return avoid.includes(UNDERGROUND)
      ? `Showing routes that avoid ${names}, including the Tube. London crossings are by other trains or on foot.`
      : `Showing routes that avoid ${names}.`;
  }
  return null;
}
