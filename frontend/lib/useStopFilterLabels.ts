'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import { getStationNames } from './suggestions';
import { groupLabels, useStationGroups } from './stationGroups';
import { isGroupCode } from './stationLabel';
import type { TrainStopFilters } from './trainStopFilters';

/** Every code in the Advanced options' lists. */
function filterCodes(filters: TrainStopFilters): string[] {
  return [
    ...filters.startsAt,
    ...filters.terminatesAt,
    ...filters.previouslyCallsAt,
    ...filters.previouslyStopsAt,
    ...filters.callsAt,
    ...filters.via,
    ...filters.avoid,
    ...filters.avoidStop,
  ];
}

/** Code -> name for the Advanced options' rows (`TrainsAdvancedOptions`),
 * shared by every form that shows them (`/trains`, and a journey's "Add a
 * leg" and "Edit search"): filled as stations are picked (`addNames`), and
 * once on mount for the codes `filters` starts with (a URL's, or a leg's
 * saved filters), with the station groups' own labels. */
export function useStopFilterLabels(filters: TrainStopFilters): {
  labels: Map<string, string>;
  addNames: (names: Map<string, string>) => void;
} {
  const [stopNames, setStopNames] = useState<Map<string, string>>(new Map());
  const addNames = useCallback(
    (found: Map<string, string>) => setStopNames((current) => new Map([...current, ...found])),
    [],
  );
  useEffect(() => {
    const restored = filterCodes(filters).filter((code) => !isGroupCode(code));
    if (restored.length === 0) return;
    let cancelled = false;
    void getStationNames(restored).then((found) => {
      if (!cancelled) addNames(found);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the starting filters are read on mount only
  }, []);
  const stopGroups = useStationGroups(
    [...filters.previouslyCallsAt, ...filters.previouslyStopsAt, ...filters.callsAt, ...filters.via].some(isGroupCode),
  );
  const labels = useMemo(() => new Map([...groupLabels(stopGroups), ...stopNames]), [stopGroups, stopNames]);
  return { labels, addNames };
}
