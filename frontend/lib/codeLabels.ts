import { groupLabels, loadStationGroups } from './stationGroups';
import {
  isGroupCode,
  isTflStopCode,
  isTiplocCode,
  normalizeLocationCode,
  parseGroupCode,
  stationLabel,
  tflStopPlaceName,
} from './stationLabel';
import { getStationNames, searchTocs } from './suggestions';
import type { Suggestion, TflStop } from './types';

/** Code -> display label for one kind of code lookup field (stations,
 * operators), kept for the page's life so a picked or resolved code never
 * shows bare again (`components/CodeLookupField.tsx`). Filled from every
 * suggestion a field has listed, and by `resolve` for a code nothing has
 * listed yet (a deep link, a prefill), which looks each code up once. */
export interface CodeLabelCache {
  get: (code: string) => string | undefined;
  /** Remembers each suggestion's label (`labelFor`). */
  remember: (suggestions: Suggestion[]) => void;
  set: (code: string, label: string) => void;
  /** Looks `code` up unless it is known or was already looked up. */
  resolve: (code: string) => void;
  subscribe: (listener: () => void) => () => void;
  /** Bumped on every change, for `useSyncExternalStore`. */
  version: () => number;
  /** For tests: forget every label and lookup. */
  reset: () => void;
}

function key(code: string): string {
  return code.trim().toUpperCase();
}

export function createCodeLabelCache({
  labelFor,
  lookUp,
}: {
  /** What the field shows once `suggestion` is picked. */
  labelFor: (suggestion: Suggestion) => string;
  /** Labels for codes no suggestion has listed; a code it can't find is
   * left out, so the field keeps showing it bare. */
  lookUp: (code: string) => Promise<string | null>;
}): CodeLabelCache {
  const labels = new Map<string, string>();
  const attempted = new Set<string>();
  const listeners = new Set<() => void>();
  let version = 0;

  function changed() {
    version += 1;
    for (const listener of listeners) listener();
  }

  function store(code: string, label: string): boolean {
    const k = key(code);
    if (!k || labels.get(k) === label) return false;
    labels.set(k, label);
    return true;
  }

  return {
    get: (code) => labels.get(key(code)),
    remember(suggestions) {
      let any = false;
      for (const suggestion of suggestions) any = store(suggestion.code, labelFor(suggestion)) || any;
      if (any) changed();
    },
    set(code, label) {
      if (store(code, label)) changed();
    },
    resolve(code) {
      const k = key(code);
      if (!k || labels.has(k) || attempted.has(k)) return;
      attempted.add(k);
      void lookUp(code.trim())
        .catch(() => null)
        .then((label) => {
          if (label && store(code, label)) changed();
        });
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    version: () => version,
    reset() {
      labels.clear();
      attempted.clear();
      changed();
    },
  };
}

/** What a station field shows for a picked station: "London Euston (EUS)"
 * (docs/style-guide.md's `Name (CODE)`). A bus stop, ferry terminal, TfL
 * stop, station group or `/stations` place shows its name only, which
 * already says what it is ("Keswick (bus)", "Old Street (Tube)"): its code
 * means nothing to a reader. */
export function stationFieldLabel(code: string, name: string | null | undefined): string {
  if (!name) return code;
  if (isTiplocCode(code) || isGroupCode(code) || isTflStopCode(code)) return name;
  return stationLabel(code, name);
}

function stationSuggestionLabel(suggestion: Suggestion): string {
  return suggestion.place ? suggestion.name : stationFieldLabel(suggestion.code, suggestion.name);
}

async function lookUpStationLabel(code: string): Promise<string | null> {
  if (isGroupCode(code)) {
    const groups = await loadStationGroups();
    return groupLabels(groups).get(normalizeLocationCode(code)) ?? null;
  }
  if (isTflStopCode(code)) {
    const response = await fetch(`/api/tfl/stops/${encodeURIComponent(code.toUpperCase())}`);
    if (!response.ok) return null;
    return tflStopPlaceName((await response.json()) as TflStop);
  }
  // `/stations`' place keys (`tfl:940G...`) only ever come from a pick.
  if (code.includes(':') && !isTiplocCode(code)) return null;
  const names = await getStationNames([code]);
  const name = names.get(code);
  return name ? stationFieldLabel(isTiplocCode(code) ? code : code.toUpperCase(), name) : null;
}

/** Every station, stop and group field's labels (`components/StationField.tsx`). */
export const stationLabels = createCodeLabelCache({
  labelFor: stationSuggestionLabel,
  lookUp: lookUpStationLabel,
});

/** "Avanti West Coast (VT)", or the bare code when no name resolved. */
export function operatorFieldLabel(code: string, name: string | null | undefined): string {
  return name ? `${name} (${code})` : code;
}

async function lookUpOperatorLabel(code: string): Promise<string | null> {
  const results = await searchTocs(code);
  const match = results.find((s) => s.code.toUpperCase() === code.toUpperCase());
  return match ? operatorFieldLabel(match.code, match.name) : null;
}

/** Every operator field's labels (`components/OperatorField.tsx`). */
export const operatorLabels = createCodeLabelCache({
  labelFor: (suggestion) => operatorFieldLabel(suggestion.code, suggestion.name),
  lookUp: lookUpOperatorLabel,
});

const TYPED_CRS = /^[A-Za-z]{3}$/;
const TYPED_TOC = /^[A-Za-z0-9]{2}$/;

/** A typed 3-letter station code, upper-cased; `null` for anything else. */
export function typedStationCode(text: string): string | null {
  const trimmed = text.trim();
  return TYPED_CRS.test(trimmed) ? trimmed.toUpperCase() : null;
}

/** Any code the journey planner takes, as `GET /Trips/plan` takes it: a
 * station code, a `tiploc:` stop, a `group:` and, for From and To, a TfL
 * stop id. */
export function typedPlannerCode(text: string, { tflStops }: { tflStops: boolean }): string | null {
  const trimmed = text.trim();
  if (TYPED_CRS.test(trimmed)) return trimmed.toUpperCase();
  if (isTiplocCode(trimmed) && trimmed.length > 'tiploc:'.length) return normalizeLocationCode(trimmed);
  if (isGroupCode(trimmed)) return parseGroupCode(trimmed);
  if (tflStops && isTflStopCode(trimmed)) return trimmed.toUpperCase();
  return null;
}

/** A typed 2-character operator code, upper-cased; `null` otherwise. */
export function typedOperatorCode(text: string): string | null {
  const trimmed = text.trim();
  return TYPED_TOC.test(trimmed) ? trimmed.toUpperCase() : null;
}

/** For tests: forget every field's labels. */
export function resetCodeLabels(): void {
  stationLabels.reset();
  operatorLabels.reset();
}
